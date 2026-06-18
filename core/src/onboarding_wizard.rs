//! Onboarding (`start setup` and the `setup vault|llm|model` sub-flows) expressed
//! as registered wizards on the shared `wizard` engine, replacing the bespoke
//! `try_execute_onboarding` state machine.
//!
//! The steps are generic over an [`OnboardingHost`] capability trait so the *same*
//! step values run on every host: the desktop `AppState` (with a real folder
//! picker via `WizardHost::perform_native`) and a core/CLI [`CoreOnboardingCtx`]
//! (which degrades the picker gracefully). The four entry points are four `Wizard`
//! registrations that share those step values; each wizard's `seed` sets its flow,
//! pre-fills from effective config, and its `finalize` writes the right config
//! section — see docs/command-contexts.md §4-§5 and docs/config.md for the
//! dispatch route, seed invariants, and config keys.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use runebound_models::output::{
    OutputDoc, command_ref, doc, heading, list, paragraph_text, paragraph_with_inlines, text_node,
};
use runebound_models::{CommandResponse, OutputSegment, OutputSegmentKind};
use wizard::prompt::choice_lines;
use wizard::{
    CommandResult, NativeAction, Wizard, WizardChoice, WizardData, WizardHost, WizardRegistry,
    WizardSession, WizardStep, WizardTransition,
};

use crate::command::{
    OLLAMA_SETUP_TIMEOUT_SECONDS, config_vault_path_string, expand_tilde_path,
    normalize_ollama_input, probe_ollama_models, validate_vault_path_for_onboarding,
};
use crate::config::{load_effective, required_issues, save_config};
use crate::health;
use crate::session::OnboardingFlow;
use crate::vault::Vault;
use anyhow::{Result, anyhow, bail};

// ---------------------------------------------------------------------------
// Host capability trait
// ---------------------------------------------------------------------------

/// Marker for a host that can run the onboarding wizard. It adds nothing beyond
/// [`WizardHost`] (config I/O is global, no longer host-scoped); the native folder
/// picker is provided through `WizardHost::perform_native` (overridden on the
/// desktop, degraded by the default elsewhere).
pub trait OnboardingHost: WizardHost + 'static {}

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// The onboarding wizard's accumulator — the per-flow answers. The cursor/history
/// are engine-owned in `WizardSession`. `flow` selects per-section transition and
/// save behavior; `notice` is a transient one-shot shown on the next prompt.
#[derive(Debug, Clone, Default)]
struct OnboardingData {
    flow: OnboardingFlow,
    vault_path: String,
    ollama_base_url: String,
    ollama_models: Vec<String>,
    selected_model: String,
    /// Connection summary from the last probe, shown on the model step.
    probe_detail: String,
    /// One-shot notice (e.g. an invalid menu choice) rendered on the next prompt.
    notice: Option<String>,
}

fn data(d: &WizardData) -> &OnboardingData {
    d.downcast_ref::<OnboardingData>().expect("onboarding data")
}

fn data_mut(d: &mut WizardData) -> &mut OnboardingData {
    d.downcast_mut::<OnboardingData>().expect("onboarding data")
}

// ---------------------------------------------------------------------------
// Shared prompt helpers
// ---------------------------------------------------------------------------

/// Prepend the one-shot `notice` (if any) to a step's doc as the first block.
fn with_notice(d: &OnboardingData, document: OutputDoc) -> OutputDoc {
    let Some(notice) = &d.notice else {
        return document;
    };
    let mut blocks = vec![paragraph_text(notice.clone())];
    blocks.extend(document.blocks);
    OutputDoc { blocks }
}

/// Build a `CommandResponse` carrying an `output_doc` + plain-text fallback, with
/// no client event. Used by the wizards' `finalize` hand-offs.
fn ok_response(output: String, document: OutputDoc) -> CommandResponse {
    CommandResponse {
        ok: true,
        output: output.clone(),
        error: None,
        exit_code: 0,
        segments: vec![OutputSegment {
            kind: OutputSegmentKind::Text,
            text: output,
            command_ref: None,
        }],
        output_doc: Some(document),
        client_event: None,
        wizard: None,
    }
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

/// "vault_menu": pick the dialog picker, type a path, or keep the current vault.
struct VaultMenuStep;

fn vault_menu_choices(d: &OnboardingData) -> Vec<WizardChoice> {
    let mut choices = vec![
        WizardChoice::new("1: Select a vault with the dialog picker", "1")
            .with_help("Open the native folder picker (desktop only)"),
        WizardChoice::new("2: Type the path to the vault", "2")
            .with_help("Enter the vault path as text"),
    ];
    if !d.vault_path.trim().is_empty() {
        choices.push(
            WizardChoice::new("3: Continue (keep current vault)", "3")
                .with_help("Keep the current vault and move on"),
        );
    }
    choices
}

fn vault_menu_doc(d: &OnboardingData) -> OutputDoc {
    let mut document = doc().with_block(heading(2, "Vault setup"));
    if !d.vault_path.trim().is_empty() {
        document = document.with_block(paragraph_text(format!("current vault: {}", d.vault_path)));
    }
    with_notice(d, document.with_block(choice_lines(&vault_menu_choices(d))))
}

#[async_trait]
impl<H: OnboardingHost> WizardStep<H> for VaultMenuStep {
    fn id(&self) -> &'static str {
        "vault_menu"
    }
    fn summary(&self) -> &'static str {
        "Choose your vault: 1 picker, 2 type a path, 3 keep current."
    }
    fn prompt(&self, d: &WizardData) -> OutputDoc {
        vault_menu_doc(data(d))
    }
    fn choices(&self, d: &WizardData) -> Vec<WizardChoice> {
        vault_menu_choices(data(d))
    }
    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _host: &H,
    ) -> Result<WizardTransition, String> {
        data_mut(d).notice = None;
        match input.trim() {
            "1" => Ok(WizardTransition::Native(NativeAction::PickFolder {
                resubmit_to: "vault_path",
            })),
            "2" => Ok(WizardTransition::Next),
            "3" | "continue" if !data(d).vault_path.trim().is_empty() => {
                Ok(vault_done(data(d).flow))
            }
            other => {
                data_mut(d).notice = Some(format!("invalid choice: {other}"));
                Ok(WizardTransition::Stay)
            }
        }
    }
}

/// "vault_path": free-text vault path entry (also the picker's resubmit target).
struct VaultPathStep;

#[async_trait]
impl<H: OnboardingHost> WizardStep<H> for VaultPathStep {
    fn id(&self) -> &'static str {
        "vault_path"
    }
    fn summary(&self) -> &'static str {
        "Type the path to your vault and press Enter."
    }
    fn prompt(&self, d: &WizardData) -> OutputDoc {
        with_notice(
            data(d),
            doc()
                .with_block(heading(3, "Vault path"))
                .with_block(list(vec![
                    vec![text_node("Enter the path to your vault and press Enter.")],
                    vec![text_node("Example: /path/to/your/Obsidian/Vault")],
                ])),
        )
    }
    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _host: &H,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(WizardTransition::Stay);
        }
        let expanded = expand_tilde_path(trimmed);
        validate_vault_path_for_onboarding(&expanded).map_err(|e| e.to_string())?;
        data_mut(d).vault_path = expanded.display().to_string();
        Ok(vault_done(data(d).flow))
    }
}

/// After a vault is chosen: Vault flow saves + exits; others advance to Ollama.
fn vault_done(flow: OnboardingFlow) -> WizardTransition {
    match flow {
        OnboardingFlow::Vault => WizardTransition::Complete,
        _ => WizardTransition::Goto("ollama_menu"),
    }
}

/// "ollama_menu": configure a new server, or continue with the current one
/// (which probes it and advances to the model step).
struct OllamaMenuStep;

fn ollama_menu_choices(d: &OnboardingData) -> Vec<WizardChoice> {
    vec![
        WizardChoice::new("1: Configure a new server", "1")
            .with_help("Type a different Ollama URL"),
        WizardChoice::new(format!("2: Continue with {}", d.ollama_base_url), "2")
            .with_help("Probe the current server and pick a model"),
    ]
}

#[async_trait]
impl<H: OnboardingHost> WizardStep<H> for OllamaMenuStep {
    fn id(&self) -> &'static str {
        "ollama_menu"
    }
    fn summary(&self) -> &'static str {
        "Choose the Ollama server: 1 configure a new URL, 2 keep the current one."
    }
    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("checking Ollama")
    }
    fn prompt(&self, d: &WizardData) -> OutputDoc {
        let d = data(d);
        with_notice(
            d,
            doc()
                .with_block(heading(2, "Step 2: Ollama server"))
                .with_block(paragraph_text(format!(
                    "current server: {}",
                    d.ollama_base_url
                )))
                .with_block(choice_lines(&ollama_menu_choices(d))),
        )
    }
    fn choices(&self, d: &WizardData) -> Vec<WizardChoice> {
        ollama_menu_choices(data(d))
    }
    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _host: &H,
    ) -> Result<WizardTransition, String> {
        data_mut(d).notice = None;
        match input.trim() {
            "1" => Ok(WizardTransition::Next),
            "2" | "continue" => {
                probe_into(data_mut(d)).await?;
                Ok(WizardTransition::Goto("model"))
            }
            other => {
                data_mut(d).notice = Some(format!("invalid choice: {other}"));
                Ok(WizardTransition::Stay)
            }
        }
    }
}

/// "ollama_url": free-text Ollama URL entry; probes it then advances to model.
struct OllamaUrlStep;

#[async_trait]
impl<H: OnboardingHost> WizardStep<H> for OllamaUrlStep {
    fn id(&self) -> &'static str {
        "ollama_url"
    }
    fn summary(&self) -> &'static str {
        "Type your Ollama URL and press Enter."
    }
    fn awaiting_llm_label(&self) -> Option<&'static str> {
        Some("checking Ollama")
    }
    fn prompt(&self, d: &WizardData) -> OutputDoc {
        with_notice(
            data(d),
            doc()
                .with_block(heading(3, "Ollama server"))
                .with_block(list(vec![
                    vec![text_node("Enter your Ollama URL and press Enter.")],
                    vec![text_node("Example: http://127.0.0.1:11434")],
                ])),
        )
    }
    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _host: &H,
    ) -> Result<WizardTransition, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(WizardTransition::Stay);
        }
        let normalized = normalize_ollama_input(trimmed);
        health::validate_ollama_url(&normalized).map_err(|e| e.to_string())?;
        data_mut(d).ollama_base_url = normalized;
        probe_into(data_mut(d)).await?;
        Ok(WizardTransition::Goto("model"))
    }
}

/// Probe the data's `ollama_base_url`, storing the detail + model list and
/// defaulting the selected model when none is set yet.
async fn probe_into(d: &mut OnboardingData) -> std::result::Result<(), String> {
    let normalized = normalize_ollama_input(&d.ollama_base_url);
    health::validate_ollama_url(&normalized).map_err(|e| e.to_string())?;
    let (detail, models) = probe_ollama_models(&normalized, OLLAMA_SETUP_TIMEOUT_SECONDS)
        .await
        .map_err(|e| e.to_string())?;
    d.ollama_base_url = normalized;
    d.ollama_models = models;
    d.probe_detail = detail;
    if d.selected_model.trim().is_empty() && !d.ollama_models.is_empty() {
        d.selected_model = d.ollama_models[0].clone();
    }
    Ok(())
}

/// "model": pick a model by number or name, or keep the current one.
struct ModelStep;

fn model_choices(d: &OnboardingData) -> Vec<WizardChoice> {
    let mut choices: Vec<WizardChoice> = d
        .ollama_models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            WizardChoice::new(format!("{}: {}", index + 1, model), (index + 1).to_string())
        })
        .collect();
    if matches!(d.flow, OnboardingFlow::Llm | OnboardingFlow::Model)
        && !d.selected_model.trim().is_empty()
    {
        choices.push(
            WizardChoice::new("continue", "continue")
                .with_help(format!("Keep {}", d.selected_model)),
        );
    }
    choices
}

#[async_trait]
impl<H: OnboardingHost> WizardStep<H> for ModelStep {
    fn id(&self) -> &'static str {
        "model"
    }
    fn summary(&self) -> &'static str {
        "Pick a model by number or name, or type continue to keep the current one."
    }
    fn prompt(&self, d: &WizardData) -> OutputDoc {
        let d = data(d);
        let mut blocks = vec![
            heading(2, "Step 3: Model"),
            paragraph_text(d.probe_detail.clone()),
            paragraph_text("Enter a model name and press Enter.".to_string()),
            paragraph_text("Or enter a model number from the list below.".to_string()),
        ];
        if matches!(d.flow, OnboardingFlow::Llm | OnboardingFlow::Model)
            && !d.selected_model.trim().is_empty()
        {
            blocks.push(paragraph_text(format!(
                "Or type continue to keep {}.",
                d.selected_model
            )));
        }
        if d.ollama_models.is_empty() {
            blocks.push(paragraph_text("(no models returned)".to_string()));
        } else {
            blocks.push(choice_lines(&model_choices(d)));
        }
        with_notice(d, OutputDoc { blocks })
    }
    fn choices(&self, d: &WizardData) -> Vec<WizardChoice> {
        model_choices(data(d))
    }
    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _host: &H,
    ) -> Result<WizardTransition, String> {
        data_mut(d).notice = None;
        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("continue") {
            if data(d).selected_model.trim().is_empty() {
                return Err("no model is selected. choose a model first.".to_string());
            }
            return Ok(WizardTransition::Next);
        }
        if trimmed.is_empty() {
            return Ok(WizardTransition::Stay);
        }
        if let Ok(index) = trimmed.parse::<usize>() {
            let models = &data(d).ollama_models;
            if index >= 1 && index <= models.len() {
                let selected = models[index - 1].clone();
                data_mut(d).selected_model = selected;
            } else {
                return Err(format!("model index out of range: {trimmed}"));
            }
        } else {
            data_mut(d).selected_model = trimmed.to_string();
        }
        Ok(WizardTransition::Next)
    }
}

/// "save": confirm and write the full config (Full flow only).
struct SaveStep;

#[async_trait]
impl<H: OnboardingHost> WizardStep<H> for SaveStep {
    fn id(&self) -> &'static str {
        "save"
    }
    fn summary(&self) -> &'static str {
        "Type save to write your config and finish."
    }
    fn prompt(&self, d: &WizardData) -> OutputDoc {
        let d = data(d);
        with_notice(
            d,
            doc()
                .with_block(paragraph_text(format!(
                    "model set to: {}",
                    d.selected_model
                )))
                .with_block(heading(2, "Step 4: Save config"))
                .with_block(paragraph_with_inlines(vec![
                    text_node("Type "),
                    command_ref("save", "save"),
                    text_node(" to finish."),
                ])),
        )
    }
    fn choices(&self, _d: &WizardData) -> Vec<WizardChoice> {
        vec![WizardChoice::new("save", "save").with_help("Write the config and finish setup")]
    }
    async fn accept(
        &self,
        input: &str,
        d: &mut WizardData,
        _host: &H,
    ) -> Result<WizardTransition, String> {
        data_mut(d).notice = None;
        if input.trim().eq_ignore_ascii_case("save") {
            Ok(WizardTransition::Complete)
        } else {
            Ok(WizardTransition::Stay)
        }
    }
}

// ---------------------------------------------------------------------------
// finalize / config-write logic (ported from the bespoke save_* sections)
// ---------------------------------------------------------------------------

/// The full save: vault + ollama + (optional) model, ensure vault structure,
/// init the database, run quick health checks. Mirrors the former `save` block.
async fn finalize_full(d: &OnboardingData) -> Result<CommandResponse> {
    if d.vault_path.trim().is_empty() {
        bail!("vault path is missing. run set vault <path>.");
    }
    if d.ollama_base_url.trim().is_empty() {
        bail!("ollama URL is missing. run set ollama <url>.");
    }
    let missing_model = d.selected_model.trim().is_empty();

    let loaded = load_effective()?;
    let mut config = loaded.effective;
    config.vault.path = Some(PathBuf::from(&d.vault_path));
    config.ollama.base_url = d.ollama_base_url.clone();
    if !missing_model {
        config.ollama.model = Some(d.selected_model.clone());
    }

    let issues = required_issues(&config);
    if !issues.is_empty() {
        bail!("missing required config:\n- {}", issues.join("\n- "));
    }

    let config_path = save_config(&config)?;
    let vault_path = config
        .vault
        .path
        .clone()
        .ok_or_else(|| anyhow!("vault.path is not configured"))?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure()?;
    let db = crate::db::init_database().await?;

    let report = health::run_quick_checks(&config).await;
    let warnings: Vec<String> = report
        .items
        .into_iter()
        .filter(|item| !item.ok)
        .map(|item| format!("{}: {}", item.name, item.detail))
        .collect();

    let mut lines = vec![
        "## Onboarding complete".to_string(),
        format!("config saved: {}", config_path.display()),
        format!("vault ready: {}", vault.root().display()),
        format!("database ready: {}", db.path.display()),
    ];
    let mut document = doc()
        .with_block(heading(2, "Onboarding complete"))
        .with_block(list(vec![
            vec![text_node(format!(
                "config saved: {}",
                config_path.display()
            ))],
            vec![text_node(format!(
                "vault ready: {}",
                vault.root().display()
            ))],
            vec![text_node(format!("database ready: {}", db.path.display()))],
        ]));
    if missing_model {
        lines.push("ollama model not set; run `start setup` later to choose a model if you plan to use AI generation.".to_string());
        document = document.with_block(paragraph_with_inlines(vec![
            text_node("ollama model not set; run "),
            command_ref("start setup", "start setup"),
            text_node(" later to choose a model if you plan to use AI generation."),
        ]));
    }
    if !warnings.is_empty() {
        lines.push("setup warnings:".to_string());
        lines.extend(warnings.iter().map(|warning| format!("- {warning}")));
        document = document
            .with_block(heading(3, "Setup warnings"))
            .with_block(list(
                warnings
                    .iter()
                    .map(|warning| vec![text_node(warning.clone())])
                    .collect(),
            ));
    }

    Ok(ok_response(lines.join("\n"), document))
}

/// Save only the vault section. Mirrors the former `save_vault_section`.
async fn finalize_vault(d: &OnboardingData) -> Result<CommandResponse> {
    if d.vault_path.trim().is_empty() {
        bail!("vault path is missing.");
    }
    let loaded = load_effective()?;
    let mut config = loaded.effective;
    config.vault.path = Some(PathBuf::from(&d.vault_path));

    let config_path = save_config(&config)?;
    let vault_path = config
        .vault
        .path
        .clone()
        .ok_or_else(|| anyhow!("vault.path is not configured"))?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure()?;

    let lines = [
        "## Vault updated".to_string(),
        format!("config saved: {}", config_path.display()),
        format!("vault ready: {}", vault.root().display()),
    ];
    let document = doc()
        .with_block(heading(2, "Vault updated"))
        .with_block(list(vec![
            vec![text_node(format!(
                "config saved: {}",
                config_path.display()
            ))],
            vec![text_node(format!(
                "vault ready: {}",
                vault.root().display()
            ))],
        ]));
    Ok(ok_response(lines.join("\n"), document))
}

/// Save the ollama server + (optional) model. Mirrors `save_llm_section`.
async fn finalize_llm(d: &OnboardingData) -> Result<CommandResponse> {
    if d.ollama_base_url.trim().is_empty() {
        bail!("ollama URL is missing. run set ollama <url>.");
    }
    let loaded = load_effective()?;
    let mut config = loaded.effective;
    config.ollama.base_url = d.ollama_base_url.clone();
    let missing_model = d.selected_model.trim().is_empty();
    if !missing_model {
        config.ollama.model = Some(d.selected_model.clone());
    }
    let config_path = save_config(&config)?;

    let mut lines = vec![
        "## LLM updated".to_string(),
        format!("config saved: {}", config_path.display()),
        format!("ollama: {}", config.ollama.base_url),
    ];
    let mut rows = vec![
        vec![text_node(format!(
            "config saved: {}",
            config_path.display()
        ))],
        vec![text_node(format!("ollama: {}", config.ollama.base_url))],
    ];
    let mut model_note = None;
    if missing_model {
        lines.push("model not set; run `setup llm` later to choose one.".to_string());
        model_note = Some(paragraph_with_inlines(vec![
            text_node("model not set; run "),
            command_ref("setup llm", "setup llm"),
            text_node(" later to choose one."),
        ]));
    } else {
        lines.push(format!("model: {}", d.selected_model));
        rows.push(vec![text_node(format!("model: {}", d.selected_model))]);
    }

    let mut document = doc()
        .with_block(heading(2, "LLM updated"))
        .with_block(list(rows));
    if let Some(note) = model_note {
        document = document.with_block(note);
    }
    Ok(ok_response(lines.join("\n"), document))
}

/// Save only the selected model. Mirrors `save_model_section`.
async fn finalize_model(d: &OnboardingData) -> Result<CommandResponse> {
    if d.selected_model.trim().is_empty() {
        bail!("no model is selected. choose a model first.");
    }
    let loaded = load_effective()?;
    let mut config = loaded.effective;
    config.ollama.model = Some(d.selected_model.clone());
    let config_path = save_config(&config)?;

    let lines = [
        "## Model updated".to_string(),
        format!("config saved: {}", config_path.display()),
        format!("model: {}", d.selected_model),
    ];
    let document = doc()
        .with_block(heading(2, "Model updated"))
        .with_block(list(vec![
            vec![text_node(format!(
                "config saved: {}",
                config_path.display()
            ))],
            vec![text_node(format!("model: {}", d.selected_model))],
        ]));
    Ok(ok_response(lines.join("\n"), document))
}

// ---------------------------------------------------------------------------
// Wizards (four registrations sharing the step values)
// ---------------------------------------------------------------------------

/// Seed effective config into a fresh accumulator for `flow`. Seeds
/// `ollama_base_url` *unconditionally* from effective config (the documented
/// "continue with 127.0.0.1" invariant — see docs/command-contexts.md §5). The
/// Model flow starts at the model step, so it probes the configured server here.
async fn seed_data(flow: OnboardingFlow) -> Result<OnboardingData> {
    let loaded = load_effective()?;
    let mut d = OnboardingData {
        flow,
        ollama_base_url: loaded.effective.ollama.base_url.clone(),
        ..Default::default()
    };
    if let Some(path) = config_vault_path_string(&loaded.effective) {
        d.vault_path = path;
    }
    if let Some(model) = &loaded.effective.ollama.model {
        d.selected_model = model.clone();
    }
    if flow == OnboardingFlow::Model {
        if d.ollama_base_url.trim().is_empty() {
            bail!("no Ollama server is configured. run setup llm first.");
        }
        probe_into(&mut d).await.map_err(|e| anyhow!("{e}"))?;
    }
    Ok(d)
}

macro_rules! onboarding_wizard {
    ($name:ident, $id:literal, $title:literal, $flow:expr, [$($step:expr),+ $(,)?], $finalize:ident) => {
        struct $name<H> {
            steps: Vec<Arc<dyn WizardStep<H>>>,
        }
        impl<H: OnboardingHost> $name<H> {
            fn new() -> Self {
                Self {
                    steps: vec![$(Arc::new($step)),+],
                }
            }
        }
        #[async_trait]
        impl<H: OnboardingHost> Wizard<H> for $name<H> {
            fn id(&self) -> &'static str {
                $id
            }
            fn title(&self) -> &'static str {
                $title
            }
            fn steps(&self) -> &[Arc<dyn WizardStep<H>>] {
                &self.steps
            }
            async fn seed(&self, _host: &H) -> std::result::Result<WizardData, String> {
                let acc = seed_data($flow).await.map_err(|e| e.to_string())?;
                Ok(WizardData::new(acc))
            }
            async fn finalize(&self, _host: &H, d: &WizardData) -> CommandResult {
                let response = $finalize(data(d)).await.map_err(|e| e.to_string())?;
                Ok(Some(response))
            }
        }
    };
}

onboarding_wizard!(
    SetupFullWizard,
    "setup",
    "Setup",
    OnboardingFlow::Full,
    [
        VaultMenuStep,
        VaultPathStep,
        OllamaMenuStep,
        OllamaUrlStep,
        ModelStep,
        SaveStep
    ],
    finalize_full
);
onboarding_wizard!(
    SetupVaultWizard,
    "setup-vault",
    "Vault setup",
    OnboardingFlow::Vault,
    [VaultMenuStep, VaultPathStep],
    finalize_vault
);
onboarding_wizard!(
    SetupLlmWizard,
    "setup-llm",
    "LLM setup",
    OnboardingFlow::Llm,
    [OllamaMenuStep, OllamaUrlStep, ModelStep],
    finalize_llm
);
onboarding_wizard!(
    SetupModelWizard,
    "setup-model",
    "Model setup",
    OnboardingFlow::Model,
    [ModelStep],
    finalize_model
);

/// Map an entry command to the wizard it launches, or `None` if it is not an
/// onboarding launcher (e.g. `setup verbosity`, `setup help`, which stay normal
/// commands). The host's launcher then calls `start_wizard(id, host)`.
pub fn onboarding_entry_wizard_id(input: &str) -> Option<&'static str> {
    let lowered: Vec<String> = input
        .split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    let tokens: Vec<&str> = lowered.iter().map(String::as_str).collect();
    match tokens.as_slice() {
        ["start", "setup"] => Some("setup"),
        ["setup", "vault"] => Some("setup-vault"),
        ["setup", "llm"] => Some("setup-llm"),
        ["setup", "model"] | ["model"] => Some("setup-model"),
        _ => None,
    }
}

/// Register all four onboarding wizards into a host's registry. Called by the
/// desktop (`H = AppState`) and the core/CLI host (`H = CoreOnboardingCtx`).
pub fn register_onboarding_wizards<H: OnboardingHost>(registry: &mut WizardRegistry<H>) {
    registry.register(Arc::new(SetupFullWizard::<H>::new()));
    registry.register(Arc::new(SetupVaultWizard::<H>::new()));
    registry.register(Arc::new(SetupLlmWizard::<H>::new()));
    registry.register(Arc::new(SetupModelWizard::<H>::new()));
}

// ---------------------------------------------------------------------------
// Core/CLI host
// ---------------------------------------------------------------------------

/// The core/CLI onboarding host: owns the live wizard session (moved in and out
/// of the long-lived `CommandService` around each dispatch) and the registry.
/// `perform_native` uses the default (`Cancelled`), so the folder picker degrades
/// gracefully on the CLI.
pub struct CoreOnboardingCtx {
    session: tokio::sync::Mutex<WizardSession>,
    registry: WizardRegistry<CoreOnboardingCtx>,
}

impl CoreOnboardingCtx {
    /// Build a host taking ownership of `session` (restore it with `into_session`).
    pub fn new(session: WizardSession) -> Self {
        let mut registry = WizardRegistry::new();
        register_onboarding_wizards(&mut registry);
        Self {
            session: tokio::sync::Mutex::new(session),
            registry,
        }
    }

    /// Reclaim the session after a dispatch (to store back on the caller).
    pub fn into_session(self) -> WizardSession {
        self.session.into_inner()
    }
}

impl WizardHost for CoreOnboardingCtx {
    fn wizard_registry(&self) -> &WizardRegistry<Self> {
        &self.registry
    }
    fn wizard_session(&self) -> &tokio::sync::Mutex<WizardSession> {
        &self.session
    }
    // perform_native defaults to Cancelled — the CLI degradation path.
}

impl OnboardingHost for CoreOnboardingCtx {}

#[cfg(test)]
mod tests {
    //! Pure step-logic tests: transitions, choice lists, and model selection. The
    //! I/O paths (probe, config save) are global-config/network coupled and are
    //! covered by the CLI integration + manual verification, not here.
    use super::*;

    fn host() -> CoreOnboardingCtx {
        CoreOnboardingCtx::new(WizardSession::default())
    }
    fn wd(d: OnboardingData) -> WizardData {
        WizardData::new(d)
    }

    #[test]
    fn vault_menu_shows_continue_only_when_a_vault_is_set() {
        assert_eq!(vault_menu_choices(&OnboardingData::default()).len(), 2);
        let set = OnboardingData {
            vault_path: "/v".into(),
            ..Default::default()
        };
        assert_eq!(vault_menu_choices(&set).len(), 3);
    }

    #[test]
    fn vault_done_completes_for_vault_flow_else_advances_to_ollama() {
        assert!(matches!(
            vault_done(OnboardingFlow::Vault),
            WizardTransition::Complete
        ));
        assert!(matches!(
            vault_done(OnboardingFlow::Full),
            WizardTransition::Goto("ollama_menu")
        ));
    }

    #[tokio::test]
    async fn vault_menu_picker_choice_requests_the_native_folder_picker() {
        let mut d = wd(OnboardingData::default());
        let t = VaultMenuStep.accept("1", &mut d, &host()).await.unwrap();
        assert!(matches!(
            t,
            WizardTransition::Native(NativeAction::PickFolder { resubmit_to }) if resubmit_to == "vault_path"
        ));
    }

    #[tokio::test]
    async fn vault_menu_invalid_choice_sets_notice_and_stays() {
        let mut d = wd(OnboardingData::default());
        let t = VaultMenuStep.accept("9", &mut d, &host()).await.unwrap();
        assert!(matches!(t, WizardTransition::Stay));
        assert!(
            data(&d)
                .notice
                .as_deref()
                .unwrap()
                .contains("invalid choice")
        );
    }

    #[tokio::test]
    async fn vault_path_accepts_an_existing_dir_and_advances() {
        let dir = std::env::temp_dir();
        let mut d = wd(OnboardingData {
            flow: OnboardingFlow::Full,
            ..Default::default()
        });
        let t = VaultPathStep
            .accept(dir.to_str().unwrap(), &mut d, &host())
            .await
            .unwrap();
        assert!(matches!(t, WizardTransition::Goto("ollama_menu")));
        assert!(!data(&d).vault_path.is_empty());
    }

    #[tokio::test]
    async fn model_step_selects_by_index_and_advances() {
        let mut d = wd(OnboardingData {
            ollama_models: vec!["a".into(), "b".into()],
            ..Default::default()
        });
        let t = ModelStep.accept("2", &mut d, &host()).await.unwrap();
        assert!(matches!(t, WizardTransition::Next));
        assert_eq!(data(&d).selected_model, "b");
    }

    #[tokio::test]
    async fn model_step_index_out_of_range_errors() {
        let mut d = wd(OnboardingData {
            ollama_models: vec!["a".into()],
            ..Default::default()
        });
        let err = ModelStep.accept("5", &mut d, &host()).await.unwrap_err();
        assert!(err.contains("out of range"));
    }

    #[tokio::test]
    async fn model_step_free_text_name_advances() {
        let mut d = wd(OnboardingData::default());
        let t = ModelStep.accept("llama3", &mut d, &host()).await.unwrap();
        assert!(matches!(t, WizardTransition::Next));
        assert_eq!(data(&d).selected_model, "llama3");
    }

    #[tokio::test]
    async fn model_step_continue_requires_a_selected_model() {
        let mut empty = wd(OnboardingData {
            flow: OnboardingFlow::Llm,
            ..Default::default()
        });
        assert!(
            ModelStep
                .accept("continue", &mut empty, &host())
                .await
                .is_err()
        );
        let mut set = wd(OnboardingData {
            flow: OnboardingFlow::Llm,
            selected_model: "x".into(),
            ..Default::default()
        });
        assert!(matches!(
            ModelStep
                .accept("continue", &mut set, &host())
                .await
                .unwrap(),
            WizardTransition::Next
        ));
    }

    #[test]
    fn model_choices_offer_continue_only_with_a_current_model_in_llm_or_model_flows() {
        let full = OnboardingData {
            flow: OnboardingFlow::Full,
            selected_model: "x".into(),
            ollama_models: vec!["x".into()],
            ..Default::default()
        };
        assert!(!model_choices(&full).iter().any(|c| c.token == "continue"));
        let llm = OnboardingData {
            flow: OnboardingFlow::Llm,
            selected_model: "x".into(),
            ollama_models: vec!["x".into()],
            ..Default::default()
        };
        assert!(model_choices(&llm).iter().any(|c| c.token == "continue"));
    }
}
