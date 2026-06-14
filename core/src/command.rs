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
use serde::Serialize;

use crate::command_manifest::{CommandManifest, CommandSpec, command_manifest};
use crate::command_parse::{normalize_alias_tokens, normalize_command_input};
use crate::config::{load_effective, required_issues, save_config, validate_for_runtime};
use crate::db;
use crate::health::{self, CheckReport};
use crate::output::{
    InlineNode, OutputBlock, OutputDoc, StatusTone, command_ref, doc, heading, list,
    paragraph_text, paragraph_with_inlines, status, text_node,
};
use crate::session::SessionState;
use crate::vault::{Vault, is_path_writable};
use command_specs::handler_metadata_for;

#[derive(Debug, Clone, Serialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
    pub segments: Vec<OutputSegment>,
    pub output_doc: Option<OutputDoc>,
    pub client_event: Option<CommandClientEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandClientEvent {
    LoadNpcDraft {
        id: String,
        name: String,
        race: String,
        occupation: String,
        sex: String,
        age: String,
        height: String,
        weight_lbs: String,
        background: String,
        want_need: String,
        secret_obstacle: String,
        carrying: Vec<String>,
        location: String,
    },
    LoadLocationDraft {
        id: String,
        name: String,
        slug: String,
        vault_path: String,
        kind_type: String,
        kind_custom: Option<String>,
        visual_description: String,
        history_background: String,
        exports: Vec<String>,
        tone: String,
        authority: String,
        danger_level: String,
        current_tension: String,
    },
    LoadFactionDraft {
        id: String,
        name: String,
        slug: String,
        vault_path: String,
        kind_type: String,
        kind_custom: Option<String>,
        public_description: String,
        true_agenda: String,
        methods: String,
        leadership: String,
        headquarters: String,
        sphere_of_influence: String,
        resources_assets: String,
        allies: Vec<String>,
        rivals_enemies: Vec<String>,
        reputation: String,
        current_tension: String,
        goals_short_term: Vec<String>,
        goals_long_term: Vec<String>,
        symbol_description: String,
    },
    ClearDrafts,
    ClearTerminal {
        clear_history: bool,
    },
    ExitRequested,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputSegment {
    pub kind: OutputSegmentKind,
    pub text: String,
    pub command_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSegmentKind {
    Text,
    Error,
}

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
        session.onboarding.active = true;
        if session.onboarding.ollama_base_url.trim().is_empty() {
            session.onboarding.ollama_base_url = loaded.effective.ollama.base_url;
        }

        if session.onboarding.step == 0 {
            session.onboarding.step = 1;
            return Ok(Some(CommandOutput::text(
                [
                    "## Step 1: Vault Path",
                    "runebound.sh needs your Obsidian vault directory so it can read and write your campaign content.",
                    "Enter your vault directory path and press Enter.",
                    "Example: /path/to/your/Obsidian/Vault",
                ]
                .join("\n"),
            )));
        }

        return Ok(Some(CommandOutput::text(
            "setup already started. use show setup or continue with next step.".to_string(),
        )));
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
                "set vault <path>",
                "set ollama <url>",
                "test ollama",
                "set model <name>",
                "use model <index>",
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
        session.onboarding.active = false;
        session.onboarding.step = 0;
        session.onboarding.ollama_models.clear();
        return Ok(Some(CommandOutput::text(
            "setup cancelled. run start setup anytime to continue.".to_string(),
        )));
    }

    if let Some(path) = extract_trailing_argument(trimmed, "set vault") {
        let expanded = expand_tilde_path(&path);
        validate_vault_path_for_onboarding(&expanded)?;

        session.onboarding.vault_path = expanded.display().to_string();
        if session.onboarding.step < 2 {
            session.onboarding.step = 2;
        }

        return Ok(Some(CommandOutput::text(
            [
                "## Step 2: Ollama server",
                &format!("vault set to: {}", session.onboarding.vault_path),
                "Enter your Ollama URL and press Enter.",
                "Example: http://127.0.0.1:11434",
            ]
            .join("\n"),
        )));
    }

    if let Some(url) = extract_trailing_argument(trimmed, "set ollama") {
        let normalized = normalize_ollama_input(&url);
        if normalized.is_empty() {
            bail!("ollama URL cannot be empty");
        }
        health::validate_ollama_url(&normalized)?;
        session.onboarding.ollama_base_url = normalized.clone();
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
        session.onboarding.step = 3;

        let mut lines = vec![
            "## Step 3: Model".to_string(),
            detail,
            "Enter a model name and press Enter.".to_string(),
            "Or enter a model number from the list below.".to_string(),
        ];
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
        return Ok(Some(CommandOutput::text(lines.join("\n"))));
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
        if session.onboarding.step < 4 {
            session.onboarding.step = 4;
        }
        return Ok(Some(CommandOutput::text(
            [
                &format!("model selected: {selected}"),
                "## Step 4: Save config",
                "Type save to finish.",
            ]
            .join("\n"),
        )));
    }

    if let Some(model_name) = extract_trailing_argument(trimmed, "set model") {
        if model_name.trim().is_empty() {
            bail!("model name cannot be empty");
        }
        session.onboarding.selected_model = model_name.clone();
        if session.onboarding.step < 4 {
            session.onboarding.step = 4;
        }
        return Ok(Some(CommandOutput::text(
            [
                &format!("model set to: {model_name}"),
                "## Step 4: Save config",
                "Type save to finish.",
            ]
            .join("\n"),
        )));
    }

    if session.onboarding.step == 1 && !trimmed.is_empty() {
        let expanded = expand_tilde_path(trimmed);
        validate_vault_path_for_onboarding(&expanded)?;
        session.onboarding.vault_path = expanded.display().to_string();
        session.onboarding.step = 2;
        return Ok(Some(CommandOutput::text(
            [
                "## Step 2: Ollama server",
                &format!("vault set to: {}", session.onboarding.vault_path),
                "Enter your Ollama URL and press Enter.",
                "Example: http://127.0.0.1:11434",
            ]
            .join("\n"),
        )));
    }

    if session.onboarding.step == 2 && !trimmed.is_empty() {
        let normalized = normalize_ollama_input(trimmed);
        health::validate_ollama_url(&normalized)?;
        let (detail, models) = probe_ollama_models(&normalized, 15).await?;

        session.onboarding.ollama_base_url = normalized;
        session.onboarding.ollama_models = models.clone();
        if session.onboarding.selected_model.trim().is_empty() && !models.is_empty() {
            session.onboarding.selected_model = models[0].clone();
        }
        session.onboarding.step = 3;

        let mut lines = vec![
            "## Step 3: Model".to_string(),
            detail,
            "Enter a model name and press Enter.".to_string(),
            "Or enter a model number from the list below.".to_string(),
        ];
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
        return Ok(Some(CommandOutput::text(lines.join("\n"))));
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
        session.onboarding.step = 4;
        return Ok(Some(CommandOutput::text(
            [
                &format!("model set to: {}", session.onboarding.selected_model),
                "## Step 4: Save config",
                "Type save to finish.",
            ]
            .join("\n"),
        )));
    }

    if lowered == ["save"] || lowered == ["save", "setup"] {
        if session.onboarding.vault_path.trim().is_empty() {
            bail!("vault path is missing. run set vault <path>.");
        }
        if session.onboarding.ollama_base_url.trim().is_empty() {
            bail!("ollama URL is missing. run set ollama <url>.");
        }
        if session.onboarding.selected_model.trim().is_empty() {
            bail!("model is missing. run set model <name> or use model <index>.");
        }

        let loaded = load_effective(workspace_root)?;
        let mut config = loaded.effective;
        config.vault.path = Some(PathBuf::from(&session.onboarding.vault_path));
        config.ollama.base_url = session.onboarding.ollama_base_url.clone();
        config.ollama.model = Some(session.onboarding.selected_model.clone());

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

        session.onboarding.active = false;
        session.onboarding.step = 0;
        session.onboarding.ollama_models.clear();

        let mut lines = vec![
            "## Onboarding complete".to_string(),
            format!("config saved: {}", config_path.display()),
            format!("vault ready: {}", vault.root().display()),
            format!("database ready: {}", db.path.display()),
        ];
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

    let model = config
        .ollama
        .model
        .clone()
        .unwrap_or_else(|| "(not set)".to_string());

    let text = format!(
        "## System Status\nrunebound.sh is connected and ready to work.\n\nvault: {}\nollama endpoint: {}\nollama model: {}\ndatabase: {}",
        vault.root().display(),
        config.ollama.base_url,
        model,
        db.path.display()
    );
    let output_doc = doc()
        .with_block(heading(2, "System Status"))
        .with_block(status(
            StatusTone::Success,
            "runebound.sh is connected and ready to work.",
        ))
        .with_block(list(vec![
            vec![text_node(format!("vault: {}", vault.root().display()))],
            vec![text_node(format!(
                "ollama endpoint: {}",
                config.ollama.base_url
            ))],
            vec![text_node(format!("ollama model: {model}"))],
            vec![text_node(format!("database: {}", db.path.display()))],
        ]));

    Ok(CommandOutput::with_doc(text, output_doc))
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

    use super::execute_line_result;

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
}
