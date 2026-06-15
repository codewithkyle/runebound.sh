use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use command_handler::{
    CommandHandler, HandlerBridge, HandlerEntry, HandlerMetadata, HandlerRegistry,
};
use reqwest::StatusCode;

use crate::command_manifest::{CommandManifest, CommandSpec, command_manifest};
use crate::command_parse::{normalize_alias_tokens, normalize_command_input};
use crate::config::{AppConfig, load_effective, required_issues, save_config, validate_for_runtime};
use crate::db;
use crate::health::{self, CheckReport, OllamaHealth};
use crate::output::{
    command_ref, doc, heading, list, paragraph_text, paragraph_with_inlines, status, text_node,
};
use crate::session::{
    OllamaStepState, OnboardingFlow, OnboardingSession, SessionState, VaultStepState,
};
use crate::vault::{Vault, is_path_writable};
use command_specs::handler_metadata_for;

pub use runebound_models::events::{
    CommandClientEvent, CommandResponse, OutputSegment, OutputSegmentKind,
};
pub use runebound_models::output::{InlineNode, OutputBlock, OutputDoc, StatusTone};

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub output: String,
    pub output_doc: Option<OutputDoc>,
}

impl CommandOutput {
    fn text(output: String) -> Self {
        Self {
            output,
            output_doc: None,
        }
    }

    fn with_doc(output: String, output_doc: OutputDoc) -> Self {
        Self {
            output,
            output_doc: Some(output_doc),
        }
    }
}

type CoreHandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<CommandOutput>> + Send + 'a>>;

struct CoreHandler {
    inner: Arc<dyn for<'a> Fn(CoreHandlerInvocation<'a>) -> CoreHandlerFuture<'a> + Send + Sync>,
}

impl CoreHandler {
    fn new<F>(handler: F) -> Self
    where
        F: for<'a> Fn(CoreHandlerInvocation<'a>) -> CoreHandlerFuture<'a> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(handler),
        }
    }
}

impl HandlerBridge for CoreHandler {
    type Output = Result<CommandOutput>;
    type Invocation<'a> = CoreHandlerInvocation<'a>;

    fn invoke<'a>(
        &'a self,
        invocation: Self::Invocation<'a>,
    ) -> command_handler::HandlerFuture<'a, Self::Output> {
        (self.inner)(invocation)
    }
}

struct CoreHandlerInvocation<'a> {
    workspace_root: &'a Path,
    _tokens: &'a [String],
    lowered: &'a [String],
    manifest: &'a CommandManifest,
    session: &'a mut SessionState,
    raw_input: &'a str,
}

fn core_handler_registry() -> &'static HandlerRegistry<CoreHandler> {
    static REGISTRY: OnceLock<HandlerRegistry<CoreHandler>> = OnceLock::new();
    REGISTRY.get_or_init(build_core_handler_registry)
}

fn build_core_handler_registry() -> HandlerRegistry<CoreHandler> {
    let mut registry = HandlerRegistry::new();
    registry.register(status_handler_entry());
    registry.register(config_handler_entry());
    registry.register(help_handler_entry());
    registry.register(exit_handler_entry());
    registry.register(setup_handler_entry());
    registry
}

fn metadata_for(name: &str) -> HandlerMetadata {
    handler_metadata_for(name)
        .unwrap_or_else(|| panic!("missing handler metadata for {name}"))
        .into()
}

fn status_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "status",
        metadata_for("status"),
        CoreHandler::new(|invocation| {
            Box::pin(async move {
                match invocation.lowered.len() {
                    0 | 1 => execute_status(invocation.workspace_root).await,
                    2 if invocation.lowered[1] == "help" => Ok(CommandOutput::text(
                        render_command_help(invocation.manifest, "status"),
                    )),
                    _ => bail!("unknown status command. use `status help`"),
                }
            })
        }),
    )
}

fn config_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "config",
        metadata_for("config"),
        CoreHandler::new(|invocation| {
            Box::pin(async move { execute_config_command(invocation).await })
        }),
    )
}

fn help_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "help",
        metadata_for("help"),
        CoreHandler::new(|invocation| {
            Box::pin(async move { Ok(CommandOutput::text(render_root_help(invocation.manifest))) })
        }),
    )
}

fn exit_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "exit",
        metadata_for("exit"),
        CoreHandler::new(|invocation| {
            Box::pin(async move {
                match invocation.lowered.len() {
                    0 | 1 => Ok(CommandOutput::text("exiting".to_string())),
                    2 if invocation.lowered[1] == "help" => Ok(CommandOutput::text(
                        render_command_help(invocation.manifest, "exit"),
                    )),
                    _ => bail!("unknown exit command. use `exit help`"),
                }
            })
        }),
    )
}

fn setup_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "setup",
        metadata_for("setup"),
        CoreHandler::new(|invocation| {
            Box::pin(async move {
                match try_execute_onboarding(
                    invocation.workspace_root,
                    invocation.raw_input,
                    invocation.session,
                )
                .await?
                {
                    Some(output) => Ok(output),
                    None => bail!("unknown setup command"),
                }
            })
        }),
    )
}

pub async fn execute_line(workspace_root: &Path, input: &str) -> CommandResponse {
    let mut session = SessionState::default();
    execute_line_with_session(workspace_root, input, &mut session).await
}

pub async fn execute_line_with_session(
    workspace_root: &Path,
    input: &str,
    session: &mut SessionState,
) -> CommandResponse {
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        session.push_history(trimmed, 50);
    }

    let normalized_input = normalize_command_input(input);
    match try_execute_onboarding(workspace_root, &normalized_input, session).await {
        Ok(Some(output)) => {
            let output_text = output.output.clone();
            return CommandResponse {
                ok: true,
                output: output.output,
                error: None,
                exit_code: 0,
                segments: vec![OutputSegment {
                    kind: OutputSegmentKind::Text,
                    text: output_text,
                    command_ref: None,
                }],
                output_doc: output.output_doc,
                client_event: None,
            };
        }
        Ok(None) => {}
        Err(err) => {
            let error_text = err.to_string();
            return CommandResponse {
                ok: false,
                output: String::new(),
                error: Some(error_text.clone()),
                exit_code: 1,
                segments: vec![OutputSegment {
                    kind: OutputSegmentKind::Error,
                    text: error_text,
                    command_ref: None,
                }],
                output_doc: Some(output_doc_from_error_text(err.to_string())),
                client_event: None,
            };
        }
    }

    match execute_line_internal(workspace_root, input, session).await {
        Ok(output) => {
            let output_text = output.output.clone();
            CommandResponse {
                ok: true,
                output: output.output,
                error: None,
                exit_code: 0,
                segments: vec![OutputSegment {
                    kind: OutputSegmentKind::Text,
                    text: output_text,
                    command_ref: None,
                }],
                output_doc: output.output_doc,
                client_event: None,
            }
        }
        Err(err) => {
            let error_text = err.to_string();
            CommandResponse {
                ok: false,
                output: String::new(),
                error: Some(error_text.clone()),
                exit_code: 1,
                segments: vec![OutputSegment {
                    kind: OutputSegmentKind::Error,
                    text: error_text,
                    command_ref: None,
                }],
                output_doc: Some(output_doc_from_error_text(err.to_string())),
                client_event: None,
            }
        }
    }
}

async fn try_execute_onboarding(
    workspace_root: &Path,
    input: &str,
    session: &mut SessionState,
) -> Result<Option<CommandOutput>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let lowered: Vec<String> = trimmed
        .split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered.is_empty() {
        return Ok(None);
    }

    if lowered == ["start", "setup"] {
        let loaded = load_effective(workspace_root)?;
        let current_vault = config_vault_path_string(&loaded.effective);
        session.onboarding.active = true;
        session.onboarding.flow = OnboardingFlow::Full;
        if session.onboarding.ollama_base_url.trim().is_empty() {
            session.onboarding.ollama_base_url = loaded.effective.ollama.base_url.clone();
        }

        // Show the vault menu on first entry, or when re-entered while still at
        // the vault step (e.g. after the native picker was cancelled).
        if session.onboarding.step <= 1 {
            session.onboarding.step = 1;
            session.onboarding.vault_substate = VaultStepState::MenuShown;
            if session.onboarding.vault_path.trim().is_empty()
                && let Some(path) = &current_vault
            {
                session.onboarding.vault_path = path.clone();
            }
            let current = if session.onboarding.vault_path.trim().is_empty() {
                None
            } else {
                Some(session.onboarding.vault_path.clone())
            };
            return Ok(Some(CommandOutput::text(vault_menu_text(
                current.as_deref(),
                OnboardingFlow::Full,
            ))));
        }

        return Ok(Some(CommandOutput::text(
            "setup already started. use show setup or continue with next step.".to_string(),
        )));
    }

    if lowered == ["setup", "vault"] {
        let loaded = load_effective(workspace_root)?;
        let current_vault = config_vault_path_string(&loaded.effective);
        session.onboarding.active = true;
        session.onboarding.flow = OnboardingFlow::Vault;
        session.onboarding.step = 1;
        session.onboarding.vault_substate = VaultStepState::MenuShown;
        if let Some(path) = &current_vault {
            session.onboarding.vault_path = path.clone();
        }
        return Ok(Some(CommandOutput::text(vault_menu_text(
            current_vault.as_deref(),
            OnboardingFlow::Vault,
        ))));
    }

    if lowered == ["setup", "llm"] {
        let loaded = load_effective(workspace_root)?;
        session.onboarding.active = true;
        session.onboarding.flow = OnboardingFlow::Llm;
        session.onboarding.ollama_base_url = loaded.effective.ollama.base_url.clone();
        if let Some(m) = &loaded.effective.ollama.model {
            session.onboarding.selected_model = m.clone();
        }
        return Ok(Some(enter_ollama_menu(&mut session.onboarding, None)));
    }

    if lowered == ["model"] || lowered == ["setup", "model"] {
        let loaded = load_effective(workspace_root)?;
        let base_url = loaded.effective.ollama.base_url.clone();
        if base_url.trim().is_empty() {
            bail!("no Ollama server is configured. run setup llm first.");
        }
        let normalized = normalize_ollama_input(&base_url);
        health::validate_ollama_url(&normalized)?;
        // Probe the configured server for its model list (shows the spinner).
        let (detail, models) = probe_ollama_models(&normalized, 15).await?;

        session.onboarding.active = true;
        session.onboarding.flow = OnboardingFlow::Model;
        session.onboarding.vault_substate = VaultStepState::Inactive;
        session.onboarding.ollama_substate = OllamaStepState::Inactive;
        session.onboarding.ollama_base_url = normalized;
        session.onboarding.ollama_models = models.clone();
        if let Some(model) = &loaded.effective.ollama.model {
            session.onboarding.selected_model = model.clone();
        } else if session.onboarding.selected_model.trim().is_empty() && !models.is_empty() {
            session.onboarding.selected_model = models[0].clone();
        }
        session.onboarding.step = 3;

        return Ok(Some(CommandOutput::text(model_step_text(
            &detail,
            &models,
            OnboardingFlow::Model,
            &session.onboarding.selected_model,
        ))));
    }

    if lowered == ["setup", "help"] {
        if !session.onboarding.active {
            let loaded = load_effective(workspace_root)?;
            session.onboarding.active = true;
            session.onboarding.step = 0;
            if session.onboarding.ollama_base_url.trim().is_empty() {
                session.onboarding.ollama_base_url = loaded.effective.ollama.base_url;
            }
        }
        return Ok(Some(CommandOutput::text(
            [
                "## Setup commands",
                "start setup",
                "setup vault",
                "setup llm",
                "setup model",
                "set vault <path>",
                "set ollama <url>",
                "test ollama",
                "set model <name>",
                "use model <index>",
                "continue",
                "show setup",
                "save",
                "cancel setup",
            ]
            .join("\n"),
        )));
    }

    if !session.onboarding.active {
        return Ok(None);
    }

    if lowered == ["show", "setup"] {
        return Ok(Some(CommandOutput::text(
            [
                "## Current setup",
                &format!(
                    "vault: {}",
                    if session.onboarding.vault_path.trim().is_empty() {
                        "(not set)"
                    } else {
                        session.onboarding.vault_path.as_str()
                    }
                ),
                &format!(
                    "ollama: {}",
                    if session.onboarding.ollama_base_url.trim().is_empty() {
                        "(not set)"
                    } else {
                        session.onboarding.ollama_base_url.as_str()
                    }
                ),
                &format!(
                    "model: {}",
                    if session.onboarding.selected_model.trim().is_empty() {
                        "(not set)"
                    } else {
                        session.onboarding.selected_model.as_str()
                    }
                ),
            ]
            .join("\n"),
        )));
    }

    if lowered == ["cancel", "setup"] {
        reset_onboarding(&mut session.onboarding);
        return Ok(Some(CommandOutput::text(
            "setup cancelled. run start setup anytime to continue.".to_string(),
        )));
    }

    // `continue` at the model step (LLM/model flows) keeps the current model and
    // saves. The Ollama-server step's `continue` is handled by the menu block below.
    if lowered == ["continue"]
        && session.onboarding.step == 3
        && matches!(
            session.onboarding.flow,
            OnboardingFlow::Llm | OnboardingFlow::Model
        )
    {
        if session.onboarding.selected_model.trim().is_empty() {
            bail!("no model is selected. choose a model first.");
        }
        return Ok(Some(save_model_step(workspace_root, session).await?));
    }

    if let Some(path) = extract_trailing_argument(trimmed, "set vault") {
        let expanded = expand_tilde_path(&path);
        validate_vault_path_for_onboarding(&expanded)?;

        session.onboarding.vault_path = expanded.display().to_string();
        session.onboarding.vault_substate = VaultStepState::Inactive;

        if session.onboarding.flow == OnboardingFlow::Vault {
            return Ok(Some(save_vault_section(workspace_root, session).await?));
        }

        let vault = session.onboarding.vault_path.clone();
        return Ok(Some(enter_ollama_menu(&mut session.onboarding, Some(&vault))));
    }

    if let Some(url) = extract_trailing_argument(trimmed, "set ollama") {
        let normalized = normalize_ollama_input(&url);
        if normalized.is_empty() {
            bail!("ollama URL cannot be empty");
        }
        health::validate_ollama_url(&normalized)?;
        session.onboarding.ollama_base_url = normalized.clone();
        session.onboarding.ollama_substate = OllamaStepState::Inactive;
        if session.onboarding.step < 2 {
            session.onboarding.step = 2;
        }
        return Ok(Some(CommandOutput::text(format!(
            "ollama URL set to: {normalized}\nrun test ollama to verify connection."
        ))));
    }

    if lowered == ["test", "ollama"] {
        let normalized = normalize_ollama_input(&session.onboarding.ollama_base_url);
        health::validate_ollama_url(&normalized)?;
        let (detail, models) = probe_ollama_models(&normalized, 15).await?;
        session.onboarding.ollama_base_url = normalized;
        session.onboarding.ollama_models = models.clone();
        if session.onboarding.selected_model.trim().is_empty() && !models.is_empty() {
            session.onboarding.selected_model = models[0].clone();
        }
        session.onboarding.ollama_substate = OllamaStepState::Inactive;
        session.onboarding.step = 3;

        return Ok(Some(CommandOutput::text(model_step_text(
            &detail,
            &models,
            session.onboarding.flow,
            &session.onboarding.selected_model,
        ))));
    }

    if let Some(index_input) = extract_trailing_argument(trimmed, "use model") {
        let index = index_input
            .parse::<usize>()
            .map_err(|_| anyhow!("model index out of range: {index_input}"))?;
        if index == 0 || index > session.onboarding.ollama_models.len() {
            bail!("model index out of range: {index_input}");
        }
        let selected = session.onboarding.ollama_models[index - 1].clone();
        session.onboarding.selected_model = selected.clone();
        if session.onboarding.flow != OnboardingFlow::Full {
            return Ok(Some(save_model_step(workspace_root, session).await?));
        }
        if session.onboarding.step < 4 {
            session.onboarding.step = 4;
        }
        return Ok(Some(CommandOutput::text(save_prompt_text(&selected))));
    }

    if let Some(model_name) = extract_trailing_argument(trimmed, "set model") {
        if model_name.trim().is_empty() {
            bail!("model name cannot be empty");
        }
        session.onboarding.selected_model = model_name.clone();
        if session.onboarding.flow != OnboardingFlow::Full {
            return Ok(Some(save_model_step(workspace_root, session).await?));
        }
        if session.onboarding.step < 4 {
            session.onboarding.step = 4;
        }
        return Ok(Some(CommandOutput::text(save_prompt_text(&model_name))));
    }

    if session.onboarding.vault_substate == VaultStepState::MenuShown {
        match trimmed {
            "1" => bail!(
                "the dialog picker is only available in the desktop app; choose 2 to type a path"
            ),
            "2" => {
                session.onboarding.vault_substate = VaultStepState::AwaitingPath;
                return Ok(Some(CommandOutput::text(
                    [
                        "Enter the path to your vault and press Enter.",
                        "Example: /path/to/your/Obsidian/Vault",
                    ]
                    .join("\n"),
                )));
            }
            "3" | "continue" if !session.onboarding.vault_path.trim().is_empty() => {
                if session.onboarding.flow == OnboardingFlow::Vault {
                    return Ok(Some(save_vault_section(workspace_root, session).await?));
                }
                let vault = session.onboarding.vault_path.clone();
                return Ok(Some(enter_ollama_menu(&mut session.onboarding, Some(&vault))));
            }
            other => {
                let current = if session.onboarding.vault_path.trim().is_empty() {
                    None
                } else {
                    Some(session.onboarding.vault_path.clone())
                };
                return Ok(Some(CommandOutput::text(format!(
                    "invalid choice: {other}\n\n{}",
                    vault_menu_text(current.as_deref(), session.onboarding.flow)
                ))));
            }
        }
    }

    if session.onboarding.vault_substate == VaultStepState::AwaitingPath && !trimmed.is_empty() {
        let expanded = expand_tilde_path(trimmed);
        validate_vault_path_for_onboarding(&expanded)?;
        session.onboarding.vault_path = expanded.display().to_string();
        session.onboarding.vault_substate = VaultStepState::Inactive;
        if session.onboarding.flow == OnboardingFlow::Vault {
            return Ok(Some(save_vault_section(workspace_root, session).await?));
        }
        let vault = session.onboarding.vault_path.clone();
        return Ok(Some(enter_ollama_menu(&mut session.onboarding, Some(&vault))));
    }

    // Ollama server menu: choose between configuring a new server or continuing
    // with the current one.
    if session.onboarding.ollama_substate == OllamaStepState::MenuShown {
        match trimmed {
            "1" => {
                session.onboarding.ollama_substate = OllamaStepState::AwaitingUrl;
                return Ok(Some(CommandOutput::text(ollama_url_prompt_text())));
            }
            "2" | "continue" => {
                let normalized = normalize_ollama_input(&session.onboarding.ollama_base_url);
                health::validate_ollama_url(&normalized)?;
                let (detail, models) = probe_ollama_models(&normalized, 15).await?;
                session.onboarding.ollama_base_url = normalized;
                session.onboarding.ollama_models = models.clone();
                if session.onboarding.selected_model.trim().is_empty() && !models.is_empty() {
                    session.onboarding.selected_model = models[0].clone();
                }
                session.onboarding.ollama_substate = OllamaStepState::Inactive;
                session.onboarding.step = 3;
                return Ok(Some(CommandOutput::text(model_step_text(
                    &detail,
                    &models,
                    session.onboarding.flow,
                    &session.onboarding.selected_model,
                ))));
            }
            other => {
                return Ok(Some(CommandOutput::text(format!(
                    "invalid choice: {other}\n\n{}",
                    ollama_menu_text(&session.onboarding.ollama_base_url, None)
                ))));
            }
        }
    }

    if session.onboarding.ollama_substate == OllamaStepState::AwaitingUrl && !trimmed.is_empty() {
        let normalized = normalize_ollama_input(trimmed);
        health::validate_ollama_url(&normalized)?;
        let (detail, models) = probe_ollama_models(&normalized, 15).await?;

        session.onboarding.ollama_base_url = normalized;
        session.onboarding.ollama_models = models.clone();
        if session.onboarding.selected_model.trim().is_empty() && !models.is_empty() {
            session.onboarding.selected_model = models[0].clone();
        }
        session.onboarding.ollama_substate = OllamaStepState::Inactive;
        session.onboarding.step = 3;

        return Ok(Some(CommandOutput::text(model_step_text(
            &detail,
            &models,
            session.onboarding.flow,
            &session.onboarding.selected_model,
        ))));
    }

    if session.onboarding.step == 3 && !trimmed.is_empty() {
        if let Ok(index) = trimmed.parse::<usize>() {
            if index >= 1 && index <= session.onboarding.ollama_models.len() {
                session.onboarding.selected_model =
                    session.onboarding.ollama_models[index - 1].clone();
            } else {
                bail!("model index out of range: {trimmed}");
            }
        } else {
            session.onboarding.selected_model = trimmed.to_string();
        }
        if session.onboarding.flow != OnboardingFlow::Full {
            return Ok(Some(save_model_step(workspace_root, session).await?));
        }
        session.onboarding.step = 4;
        return Ok(Some(CommandOutput::text(save_prompt_text(
            &session.onboarding.selected_model,
        ))));
    }

    if lowered == ["save"] || lowered == ["save", "setup"] {
        if session.onboarding.vault_path.trim().is_empty() {
            bail!("vault path is missing. run set vault <path>.");
        }
        if session.onboarding.ollama_base_url.trim().is_empty() {
            bail!("ollama URL is missing. run set ollama <url>.");
        }

        let missing_model = session.onboarding.selected_model.trim().is_empty();

        let loaded = load_effective(workspace_root)?;
        let mut config = loaded.effective;
        config.vault.path = Some(PathBuf::from(&session.onboarding.vault_path));
        config.ollama.base_url = session.onboarding.ollama_base_url.clone();
        if !missing_model {
            config.ollama.model = Some(session.onboarding.selected_model.clone());
        }

        let issues = required_issues(&config);
        if !issues.is_empty() {
            bail!("missing required config:\n- {}", issues.join("\n- "));
        }

        let config_path = save_config(workspace_root, &config)?;
        let vault_path = config
            .vault
            .path
            .clone()
            .ok_or_else(|| anyhow!("vault.path is not configured"))?;
        let vault = Vault::new(vault_path);
        vault.ensure_structure()?;
        let db = db::init_database().await?;

        let report = health::run_quick_checks(&config).await;
        let warnings: Vec<String> = report
            .items
            .into_iter()
            .filter(|item| !item.ok)
            .map(|item| format!("{}: {}", item.name, item.detail))
            .collect();

        reset_onboarding(&mut session.onboarding);

        let mut lines = vec![
            "## Onboarding complete".to_string(),
            format!("config saved: {}", config_path.display()),
            format!("vault ready: {}", vault.root().display()),
            format!("database ready: {}", db.path.display()),
        ];
        if missing_model {
            lines.push("ollama model not set; run `start setup` later to choose a model if you plan to use AI generation.".to_string());
        }
        if !warnings.is_empty() {
            lines.push("setup warnings:".to_string());
            lines.extend(warnings.iter().map(|warning| format!("- {warning}")));
        }

        return Ok(Some(CommandOutput::text(lines.join("\n"))));
    }

    Ok(Some(CommandOutput::text(
        "setup mode is active. use setup help to continue guided onboarding.".to_string(),
    )))
}

fn reset_onboarding(onboarding: &mut OnboardingSession) {
    onboarding.active = false;
    onboarding.step = 0;
    onboarding.flow = OnboardingFlow::Full;
    onboarding.vault_substate = VaultStepState::Inactive;
    onboarding.ollama_substate = OllamaStepState::Inactive;
    onboarding.ollama_models.clear();
}

fn config_vault_path_string(config: &AppConfig) -> Option<String> {
    config
        .vault
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .filter(|value| !value.trim().is_empty())
}

fn vault_menu_text(current: Option<&str>, _flow: OnboardingFlow) -> String {
    let mut lines = vec!["## Vault setup".to_string()];
    let current = current.filter(|value| !value.trim().is_empty());
    if let Some(path) = current {
        lines.push(format!("current vault: {path}"));
    }
    lines.push("1: Select a vault with the dialog picker".to_string());
    lines.push("2: Type the path to the vault".to_string());
    if current.is_some() {
        lines.push("3: Continue (keep current vault)".to_string());
    }
    lines.join("\n")
}

fn ollama_menu_text(url: &str, vault_set: Option<&str>) -> String {
    let mut lines = Vec::new();
    if let Some(vault) = vault_set.filter(|value| !value.trim().is_empty()) {
        lines.push(format!("vault set to: {vault}"));
    }
    lines.push("## Step 2: Ollama server".to_string());
    lines.push(format!("current server: {url}"));
    lines.push("1: Configure a new server".to_string());
    lines.push(format!("2: Continue with {url}"));
    lines.join("\n")
}

fn ollama_url_prompt_text() -> String {
    [
        "Enter your Ollama URL and press Enter.",
        "Example: http://127.0.0.1:11434",
    ]
    .join("\n")
}

/// Move the session into the Ollama step's menu sub-state and render its prompt.
fn enter_ollama_menu(onboarding: &mut OnboardingSession, vault_set: Option<&str>) -> CommandOutput {
    onboarding.step = 2;
    onboarding.vault_substate = VaultStepState::Inactive;
    onboarding.ollama_substate = OllamaStepState::MenuShown;
    CommandOutput::text(ollama_menu_text(&onboarding.ollama_base_url, vault_set))
}

fn model_step_text(detail: &str, models: &[String], flow: OnboardingFlow, current_model: &str) -> String {
    let mut lines = vec![
        "## Step 3: Model".to_string(),
        detail.to_string(),
        "Enter a model name and press Enter.".to_string(),
        "Or enter a model number from the list below.".to_string(),
    ];
    if matches!(flow, OnboardingFlow::Llm | OnboardingFlow::Model)
        && !current_model.trim().is_empty()
    {
        lines.push(format!("Or type continue to keep {current_model}."));
    }
    if models.is_empty() {
        lines.push("(no models returned)".to_string());
    } else {
        lines.extend(
            models
                .iter()
                .enumerate()
                .map(|(index, model)| format!("{}: {}", index + 1, model)),
        );
    }
    lines.join("\n")
}

fn save_prompt_text(model: &str) -> String {
    [
        &format!("model set to: {model}"),
        "## Step 4: Save config",
        "Type save to finish.",
    ]
    .join("\n")
}

async fn save_vault_section(
    workspace_root: &Path,
    session: &mut SessionState,
) -> Result<CommandOutput> {
    if session.onboarding.vault_path.trim().is_empty() {
        bail!("vault path is missing.");
    }

    let loaded = load_effective(workspace_root)?;
    let mut config = loaded.effective;
    config.vault.path = Some(PathBuf::from(&session.onboarding.vault_path));

    let config_path = save_config(workspace_root, &config)?;
    let vault_path = config
        .vault
        .path
        .clone()
        .ok_or_else(|| anyhow!("vault.path is not configured"))?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure()?;

    reset_onboarding(&mut session.onboarding);

    Ok(CommandOutput::text(
        [
            "## Vault updated".to_string(),
            format!("config saved: {}", config_path.display()),
            format!("vault ready: {}", vault.root().display()),
        ]
        .join("\n"),
    ))
}

async fn save_llm_section(
    workspace_root: &Path,
    session: &mut SessionState,
) -> Result<CommandOutput> {
    if session.onboarding.ollama_base_url.trim().is_empty() {
        bail!("ollama URL is missing. run set ollama <url>.");
    }

    let loaded = load_effective(workspace_root)?;
    let mut config = loaded.effective;
    config.ollama.base_url = session.onboarding.ollama_base_url.clone();
    let missing_model = session.onboarding.selected_model.trim().is_empty();
    if !missing_model {
        config.ollama.model = Some(session.onboarding.selected_model.clone());
    }

    let config_path = save_config(workspace_root, &config)?;

    let mut lines = vec![
        "## LLM updated".to_string(),
        format!("config saved: {}", config_path.display()),
        format!("ollama: {}", config.ollama.base_url),
    ];
    if missing_model {
        lines.push("model not set; run `setup llm` later to choose one.".to_string());
    } else {
        lines.push(format!("model: {}", session.onboarding.selected_model));
    }

    reset_onboarding(&mut session.onboarding);

    Ok(CommandOutput::text(lines.join("\n")))
}

/// Persist the chosen model at the end of the model step, picking the right
/// scope for the active flow.
async fn save_model_step(
    workspace_root: &Path,
    session: &mut SessionState,
) -> Result<CommandOutput> {
    match session.onboarding.flow {
        OnboardingFlow::Model => save_model_section(workspace_root, session).await,
        _ => save_llm_section(workspace_root, session).await,
    }
}

/// Save only the selected Ollama model, leaving the server URL untouched.
async fn save_model_section(
    workspace_root: &Path,
    session: &mut SessionState,
) -> Result<CommandOutput> {
    if session.onboarding.selected_model.trim().is_empty() {
        bail!("no model is selected. choose a model first.");
    }

    let loaded = load_effective(workspace_root)?;
    let mut config = loaded.effective;
    config.ollama.model = Some(session.onboarding.selected_model.clone());

    let config_path = save_config(workspace_root, &config)?;
    let model = session.onboarding.selected_model.clone();

    reset_onboarding(&mut session.onboarding);

    Ok(CommandOutput::text(
        [
            "## Model updated".to_string(),
            format!("config saved: {}", config_path.display()),
            format!("model: {model}"),
        ]
        .join("\n"),
    ))
}

fn extract_trailing_argument(input: &str, prefix: &str) -> Option<String> {
    let lowered = input.to_ascii_lowercase();
    let prefix_lower = prefix.to_ascii_lowercase();
    if !lowered.starts_with(&prefix_lower) {
        return None;
    }

    let rest = input[prefix.len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn expand_tilde_path(input: &str) -> PathBuf {
    if input == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }

    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    PathBuf::from(input)
}

fn validate_vault_path_for_onboarding(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("vault path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("vault path is not a directory: {}", path.display());
    }
    is_path_writable(path)?;
    Ok(())
}

fn normalize_ollama_input(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("://") {
        return trimmed.to_string();
    }
    format!("http://{trimmed}")
}

async fn probe_ollama_models(
    base_url: &str,
    timeout_seconds: u64,
) -> Result<(String, Vec<String>)> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()?;
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response = client.get(url).send().await?;

    if response.status() != StatusCode::OK {
        bail!("ollama responded with {}", response.status());
    }

    let value: serde_json::Value = response.json().await?;
    let mut models = Vec::new();
    if let Some(items) = value.get("models").and_then(|item| item.as_array()) {
        for item in items {
            if let Some(name) = item.get("name").and_then(|name| name.as_str()) {
                models.push(name.to_string());
            }
        }
    }

    let detail = if models.is_empty() {
        "connected (no models returned)".to_string()
    } else {
        format!("connected ({} model(s) found)", models.len())
    };

    Ok((detail, models))
}

pub async fn execute_line_result(workspace_root: &Path, input: &str) -> Result<CommandOutput> {
    let mut session = SessionState::default();
    execute_line_internal(workspace_root, input, &mut session).await
}

async fn execute_line_internal(
    workspace_root: &Path,
    input: &str,
    session: &mut SessionState,
) -> Result<CommandOutput> {
    let normalized_input = normalize_command_input(input);
    let parsed_words =
        shell_words::split(&normalized_input).map_err(|e| anyhow!("invalid command input: {e}"))?;
    let manifest = command_manifest();
    let normalized_words = normalize_alias_tokens(&parsed_words, &manifest);
    let rewritten_tokens = rewrite_onboarding_tokens(&normalized_words, &normalized_input, session);
    let tokens_ref = if let Some(ref rewritten) = rewritten_tokens {
        rewritten.as_slice()
    } else {
        &normalized_words
    };
    execute_dispatched(
        workspace_root,
        tokens_ref,
        &manifest,
        session,
        &normalized_input,
    )
    .await
}

fn rewrite_onboarding_tokens(
    tokens: &[String],
    raw_input: &str,
    session: &SessionState,
) -> Option<Vec<String>> {
    if !session.onboarding.active {
        return None;
    }

    if tokens.is_empty() {
        let trimmed = raw_input.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(vec![
            "setup".to_string(),
            "input".to_string(),
            trimmed.to_string(),
        ]);
    }

    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("save") {
        return Some(vec!["setup".to_string(), "save".to_string()]);
    }

    None
}

async fn execute_dispatched(
    workspace_root: &Path,
    tokens: &[String],
    manifest: &CommandManifest,
    session: &mut SessionState,
    raw_input: &str,
) -> Result<CommandOutput> {
    let lowered: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();

    if lowered
        .iter()
        .any(|token| token == "-h" || token == "--help")
    {
        let hint = lowered
            .first()
            .map(|root| format!("use `{root} help` or `help {root}`"))
            .unwrap_or_else(|| "use `help`".to_string());
        bail!("`-h`/`--help` is not supported; {hint}");
    }

    let registry = core_handler_registry();
    let handler_name = if lowered.is_empty() {
        "status"
    } else {
        lowered[0].as_str()
    };

    if let Some(entry) = registry.get(handler_name) {
        let invocation = CoreHandlerInvocation {
            workspace_root,
            _tokens: tokens,
            lowered: &lowered,
            manifest,
            session,
            raw_input,
        };
        return entry.execute(invocation).await;
    }

    if lowered.is_empty() {
        bail!("unknown command. use `help`");
    }

    bail!("unknown command: {}. use `help`", lowered[0]);
}

async fn execute_config_command(invocation: CoreHandlerInvocation<'_>) -> Result<CommandOutput> {
    let workspace_root = invocation.workspace_root;
    let lowered = invocation.lowered;
    let manifest = invocation.manifest;
    if lowered.len() == 1 || (lowered.len() == 2 && lowered[1] == "help") {
        return Ok(CommandOutput::text(render_command_help(manifest, "config")));
    }

    if lowered.len() == 3 && lowered[2] == "help" {
        return Ok(CommandOutput::text(render_subcommand_help(
            manifest,
            "config",
            &lowered[1],
        )));
    }

    match lowered[1].as_str() {
        "show" if lowered.len() == 2 => {
            let loaded = load_effective(workspace_root)?;
            let mut out = String::new();
            out.push_str(&format!(
                "global config: {}\n",
                loaded.paths.global.display()
            ));
            out.push('\n');
            out.push_str(&toml::to_string_pretty(&loaded.effective)?);

            let issues = required_issues(&loaded.effective);
            if !issues.is_empty() {
                out.push_str("\nincomplete config:\n");
                for issue in issues {
                    out.push_str(&format!("- {issue}\n"));
                }
            }

            Ok(CommandOutput::text(out.trim_end().to_string()))
        }
        "test" if lowered.len() == 2 => {
            let loaded = load_effective(workspace_root)?;
            let report = health::run_doctor_checks(&loaded.effective, workspace_root).await;
            let out = format_report("config test", &report);
            let output_doc = report_output_doc("Config Test", &report);
            if !report.is_ok() {
                bail!("{out}\none or more checks failed");
            }
            Ok(CommandOutput::with_doc(out, output_doc))
        }
        _ => bail!("unknown config command. use `config help`"),
    }
}

fn render_root_help(manifest: &CommandManifest) -> String {
    let mut lines = vec!["## Commands".to_string()];
    for command in manifest
        .commands
        .iter()
        .filter(|command| command.execution == crate::command_manifest::CommandExecution::Core)
    {
        lines.push(format!("{} - {}", command.name, command.summary));
    }
    lines.push(String::new());
    lines.push("Use `<command> help` or `help <command>` for details.".to_string());
    lines.join("\n")
}

fn render_command_help(manifest: &CommandManifest, root: &str) -> String {
    let Some(command) = find_manifest_command(manifest, root) else {
        return format!("unknown command: {root}. use `help`");
    };

    let mut lines = vec![format!("## {}", command.name), command.summary.clone()];
    if !command.subcommands.is_empty() {
        lines.push(String::new());
        lines.push("Subcommands:".to_string());
        for subcommand in &command.subcommands {
            lines.push(format!(
                "- {} {} - {}",
                command.name, subcommand.name, subcommand.summary
            ));
        }
    }

    if !command.examples.is_empty() {
        lines.push(String::new());
        lines.push("Examples:".to_string());
        for example in &command.examples {
            lines.push(format!("- {example}"));
        }
    }

    lines.join("\n")
}

fn render_subcommand_help(manifest: &CommandManifest, root: &str, subcommand: &str) -> String {
    let Some(command) = find_manifest_command(manifest, root) else {
        return format!("unknown command: {root}. use `help`");
    };
    let Some(sub) = command
        .subcommands
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(subcommand))
    else {
        return format!("unknown {root} command. use `{root} help`");
    };

    let mut lines = vec![
        format!("## {} {}", command.name, sub.name),
        sub.summary.clone(),
    ];
    if !sub.examples.is_empty() {
        lines.push(String::new());
        lines.push("Examples:".to_string());
        for example in &sub.examples {
            lines.push(format!("- {example}"));
        }
    }
    lines.join("\n")
}

fn find_manifest_command<'a>(manifest: &'a CommandManifest, root: &str) -> Option<&'a CommandSpec> {
    manifest
        .commands
        .iter()
        .find(|command| command.name.eq_ignore_ascii_case(root))
}

async fn execute_status(workspace_root: &Path) -> Result<CommandOutput> {
    let loaded = load_effective(workspace_root)?;
    let global_config_path = loaded.paths.global.display().to_string();
    let config = loaded.effective;

    let issues = required_issues(&config);
    if !issues.is_empty() {
        bail!(
            "## First-time setup required\n\
             \nrunebound.sh needs two connections before it can run: an Ollama LLM endpoint and an Obsidian vault path.\n\
             \nRun `start setup` to begin guided setup.\n\
             \n## Config paths\n\
             - global: {global_config_path}\n\
             \n## Missing required values\n- {}",
            issues.join("\n- ")
        );
    }

    validate_for_runtime(&config)?;

    let vault_path = config
        .vault
        .path
        .clone()
        .ok_or_else(|| anyhow!("vault.path is not configured"))?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure()?;

    let db = db::init_database().await?;
    db::health_check(&db.pool).await?;

    let health = health::check_ollama_health(&config, OLLAMA_BOOT_TIMEOUT_SECONDS).await;

    Ok(render_motd(
        &vault.root().display().to_string(),
        &config.ollama.base_url,
        config.ollama.model.as_deref(),
        &health,
    ))
}

/// Short timeout for boot/status probes so a dead server doesn't stall startup.
pub const OLLAMA_BOOT_TIMEOUT_SECONDS: u64 = 5;

/// Render the welcome/MOTD system-status output with an accurate connection line.
///
/// The status line is only the green "ready to work" message when the server is
/// reachable and the configured model is present; otherwise it is a warning that
/// names the reason (the app still opens for non-AI work).
pub fn render_motd(
    vault_root: &str,
    endpoint: &str,
    model: Option<&str>,
    health: &OllamaHealth,
) -> CommandOutput {
    let model_str = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(not set)");

    let (tone, status_line) = if health.reachable && health.model_available {
        (
            StatusTone::Success,
            "runebound.sh is connected and ready to work.".to_string(),
        )
    } else {
        (
            StatusTone::Warning,
            format!(
                "runebound.sh is running, but AI generation is unavailable: {}.",
                health.detail
            ),
        )
    };

    let text = format!(
        "## System Status\n{status_line}\n\nvault: {vault_root}\nollama endpoint: {endpoint}\nollama model: {model_str}"
    );
    let output_doc = doc()
        .with_block(heading(2, "System Status"))
        .with_block(status(tone, status_line))
        .with_block(list(vec![
            vec![text_node(format!("vault: {vault_root}"))],
            vec![text_node(format!("ollama endpoint: {endpoint}"))],
            vec![text_node(format!("ollama model: {model_str}"))],
        ]));

    CommandOutput::with_doc(text, output_doc)
}

fn format_report(title: &str, report: &CheckReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("{title}:\n"));
    for item in &report.items {
        let status = if item.ok { "OK" } else { "FAIL" };
        out.push_str(&format!("- [{status}] {}: {}\n", item.name, item.detail));
    }
    out.trim_end().to_string()
}

fn report_output_doc(title: &str, report: &CheckReport) -> OutputDoc {
    let mut output_doc = doc();
    output_doc.push(heading(2, title));
    output_doc.push(report_list_block(report));
    output_doc
}

fn report_list_block(report: &CheckReport) -> OutputBlock {
    let items: Vec<Vec<InlineNode>> = report
        .items
        .iter()
        .map(|item| {
            let state = if item.ok { "[OK]" } else { "[FAIL]" };
            vec![text_node(format!("{state} {}: {}", item.name, item.detail))]
        })
        .collect();

    list(items)
}

fn output_doc_from_error_text(message: String) -> OutputDoc {
    if message.to_lowercase().contains("first-time setup required") {
        let missing_values = extract_missing_values(&message);
        let mut output_doc = doc();
        output_doc.push(heading(2, "First-time setup required"));
        output_doc.push(paragraph_text(
            "runebound.sh needs two connections before it can run: an Ollama LLM endpoint and an Obsidian vault path."
                .to_string(),
        ));
        output_doc.push(paragraph_with_inlines(vec![command_ref(
            "start setup",
            "start setup",
        )]));
        if !missing_values.is_empty() {
            output_doc.push(heading(2, "Missing required values"));
            output_doc.push(list(
                missing_values
                    .into_iter()
                    .map(|value| vec![text_node(value)])
                    .collect(),
            ));
        }
        return output_doc;
    }

    doc().with_block(status(StatusTone::Error, message))
}

fn extract_missing_values(message: &str) -> Vec<String> {
    let mut values = Vec::new();
    for line in message.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("- ") {
            if !value.is_empty() {
                values.push(value.to_string());
            }
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        execute_line_result, execute_line_with_session, ollama_menu_text, vault_menu_text,
    };
    use crate::session::{OllamaStepState, OnboardingFlow, SessionState, VaultStepState};

    // A unique, writable directory per test. The shared temp dir cannot be used
    // because `is_path_writable` writes a fixed-name probe file, which races
    // across parallel tests.
    fn unique_temp_vault(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("runebound-test-{name}"));
        std::fs::create_dir_all(&dir).expect("create temp vault dir");
        dir
    }

    #[tokio::test]
    async fn rejects_help_flags_in_favor_of_phrase_help() {
        let result = execute_line_result(Path::new("."), "config --help").await;
        let error = result.expect_err("expected --help to be rejected");
        assert!(error.to_string().contains("not supported"));
        assert!(error.to_string().contains("config help"));
    }

    #[tokio::test]
    async fn supports_help_prefix_normalization() {
        let result = execute_line_result(Path::new("."), "help config")
            .await
            .expect("expected help config to succeed");
        assert!(result.output.contains("## config"));
    }

    #[tokio::test]
    async fn start_setup_shows_vault_menu() {
        let mut session = SessionState::default();
        let resp = execute_line_with_session(Path::new("."), "start setup", &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("## Vault setup"));
        assert!(resp.output.contains("1: Select a vault with the dialog picker"));
        assert!(resp.output.contains("2: Type the path to the vault"));
        assert!(session.onboarding.active);
        assert_eq!(session.onboarding.flow, OnboardingFlow::Full);
        assert_eq!(session.onboarding.vault_substate, VaultStepState::MenuShown);
    }

    #[tokio::test]
    async fn vault_dialog_choice_errors_in_core() {
        let mut session = SessionState::default();
        execute_line_with_session(Path::new("."), "start setup", &mut session).await;
        let resp = execute_line_with_session(Path::new("."), "1", &mut session).await;
        assert!(!resp.ok);
        assert!(resp.error.unwrap_or_default().contains("desktop app"));
    }

    #[tokio::test]
    async fn vault_type_path_choice_awaits_path() {
        let mut session = SessionState::default();
        execute_line_with_session(Path::new("."), "start setup", &mut session).await;
        let resp = execute_line_with_session(Path::new("."), "2", &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("Enter the path"));
        assert_eq!(session.onboarding.vault_substate, VaultStepState::AwaitingPath);
    }

    #[tokio::test]
    async fn vault_invalid_choice_reprints_menu() {
        let mut session = SessionState::default();
        execute_line_with_session(Path::new("."), "start setup", &mut session).await;
        let resp = execute_line_with_session(Path::new("."), "banana", &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("invalid choice"));
        assert!(resp.output.contains("## Vault setup"));
        assert_eq!(session.onboarding.vault_substate, VaultStepState::MenuShown);
    }

    #[tokio::test]
    async fn full_flow_typed_path_advances_to_ollama() {
        // Uses a unique temp dir as a stand-in vault: it exists, is a directory,
        // and is writable. The Full flow does not save at the vault step, so no
        // config is written.
        let tmp = unique_temp_vault("full-flow-advances");
        let mut session = SessionState::default();
        execute_line_with_session(Path::new("."), "start setup", &mut session).await;
        execute_line_with_session(Path::new("."), "2", &mut session).await;
        let resp = execute_line_with_session(Path::new("."), &tmp.display().to_string(), &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("## Step 2: Ollama server"));
        // The "vault set to" confirmation precedes the step heading (tweak #2).
        let vault_idx = resp.output.find("vault set to").expect("vault confirmation");
        let heading_idx = resp.output.find("## Step 2").expect("step heading");
        assert!(vault_idx < heading_idx);
        assert_eq!(session.onboarding.step, 2);
        assert_eq!(session.onboarding.ollama_substate, OllamaStepState::MenuShown);
        assert_eq!(session.onboarding.vault_substate, VaultStepState::Inactive);
        assert_eq!(session.onboarding.flow, OnboardingFlow::Full);
    }

    #[tokio::test]
    async fn ollama_menu_configure_new_awaits_url() {
        let tmp = unique_temp_vault("ollama-menu-awaits-url");
        let mut session = SessionState::default();
        execute_line_with_session(Path::new("."), "start setup", &mut session).await;
        execute_line_with_session(Path::new("."), "2", &mut session).await;
        execute_line_with_session(Path::new("."), &tmp.display().to_string(), &mut session).await;
        let resp = execute_line_with_session(Path::new("."), "1", &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("Enter your Ollama URL"));
        assert_eq!(session.onboarding.ollama_substate, OllamaStepState::AwaitingUrl);
    }

    #[test]
    fn vault_menu_shows_current_before_options() {
        let text = vault_menu_text(Some("/tmp/vault"), OnboardingFlow::Full);
        let current_idx = text.find("current vault: /tmp/vault").expect("current vault");
        let option_idx = text.find("1: Select").expect("option 1");
        assert!(current_idx < option_idx);
        assert!(text.contains("3: Continue"));
    }

    #[test]
    fn ollama_menu_offers_continue_with_current_server() {
        let text = ollama_menu_text("http://host:1234", Some("/tmp/vault"));
        let vault_idx = text.find("vault set to: /tmp/vault").expect("vault line");
        let heading_idx = text.find("## Step 2").expect("heading");
        let server_idx = text.find("current server: http://host:1234").expect("server line");
        let option_idx = text.find("1: Configure").expect("option 1");
        assert!(vault_idx < heading_idx);
        assert!(server_idx < option_idx);
        assert!(text.contains("2: Continue with http://host:1234"));
    }

    #[tokio::test]
    async fn setup_vault_uses_vault_flow() {
        let mut session = SessionState::default();
        let resp = execute_line_with_session(Path::new("."), "setup vault", &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("## Vault setup"));
        assert!(session.onboarding.active);
        assert_eq!(session.onboarding.flow, OnboardingFlow::Vault);
        assert_eq!(session.onboarding.vault_substate, VaultStepState::MenuShown);
    }

    #[tokio::test]
    async fn setup_llm_uses_llm_flow() {
        let mut session = SessionState::default();
        let resp = execute_line_with_session(Path::new("."), "setup llm", &mut session).await;
        assert!(resp.ok, "error: {:?}", resp.error);
        assert!(resp.output.contains("## Step 2: Ollama server"));
        assert!(resp.output.contains("1: Configure a new server"));
        assert!(resp.output.contains("2: Continue with"));
        assert!(session.onboarding.active);
        assert_eq!(session.onboarding.flow, OnboardingFlow::Llm);
        assert_eq!(session.onboarding.step, 2);
        assert_eq!(session.onboarding.ollama_substate, OllamaStepState::MenuShown);
    }

    #[test]
    fn session_state_round_trips_with_new_fields() {
        let session = SessionState::default();
        let json = serde_json::to_string(&session).expect("serialize");
        let back: SessionState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.onboarding.flow, OnboardingFlow::Full);
        assert_eq!(back.onboarding.vault_substate, VaultStepState::Inactive);
    }
}
