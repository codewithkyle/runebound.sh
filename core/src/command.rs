use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use anyhow::{Result, anyhow, bail};
use command_handler::{
    CommandHandler, HandlerBridge, HandlerEntry, HandlerMetadata, HandlerRegistry,
};

use crate::command_manifest::{
    CommandManifest, CommandSpec, InputContext, command_availability, command_manifest,
};
use crate::command_parse::{normalize_alias_tokens, normalize_command_input};
use crate::config::{
    AppConfig, Verbosity, load_effective, required_issues, save_config, validate_for_runtime,
};
use crate::db;
use crate::health::{self, CheckReport, OllamaHealth};
use crate::output::{
    code, command_ref, doc, heading, list, paragraph_text, paragraph_with_inlines, status,
    text_node,
};
use crate::session::SessionState;
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

    /// Build from a structured doc, deriving the plain-text `output` from it so the
    /// two can't drift (P7.2). Use this whenever the text is just a flattening of
    /// the doc — `with_doc` stays for the few cases where the text is built
    /// independently (e.g. the doctor report).
    fn from_doc(output_doc: OutputDoc) -> Self {
        Self {
            output: output_doc.to_plain_text(),
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
    _tokens: &'a [String],
    lowered: &'a [String],
    manifest: &'a CommandManifest,
    /// Session state for handlers that mutate it; currently unread (the onboarding
    /// handler that used it moved to the wizard engine).
    _session: &'a mut SessionState,
    /// Kept on the invocation for handlers that need the verbatim line; currently
    /// unread (the onboarding handler that used it moved to the wizard engine).
    _raw_input: &'a str,
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
    registry.register(ping_handler_entry());
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
                    0 | 1 => execute_status().await,
                    2 if invocation.lowered[1] == "help" => Ok(CommandOutput::from_doc(
                        command_help_doc(invocation.manifest, "status", None),
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
            Box::pin(async move {
                // Core renders the default surface; the desktop overrides this
                // handler to resolve the entity-editor and wizard contexts.
                Ok(help_overview(invocation.manifest, &InputContext::Default))
            })
        }),
    )
}

/// Render the root help index for `context`, listing every command runnable
/// there. Shared by the core handler and the desktop's context-aware override.
pub fn render_help_overview(context: &InputContext) -> CommandOutput {
    help_overview(&command_manifest(), context)
}

fn help_overview(manifest: &CommandManifest, context: &InputContext) -> CommandOutput {
    CommandOutput::from_doc(root_help_doc(manifest, context))
}

/// Whether a command should appear in `context`'s help index. The default
/// surface's commands are always listed (they remain runnable everywhere); an
/// editor context additionally lists its own context-specific commands.
fn help_lists_command(name: &str, context: &InputContext) -> bool {
    let availability = command_availability(name);
    availability.is_visible_in(context) || availability.is_visible_in(&InputContext::Default)
}

fn exit_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "exit",
        metadata_for("exit"),
        CoreHandler::new(|invocation| {
            Box::pin(async move {
                match invocation.lowered.len() {
                    0 | 1 => Ok(CommandOutput::text("exiting".to_string())),
                    2 if invocation.lowered[1] == "help" => Ok(CommandOutput::from_doc(
                        command_help_doc(invocation.manifest, "exit", None),
                    )),
                    _ => bail!("unknown exit command. use `exit help`"),
                }
            })
        }),
    )
}

fn ping_handler_entry() -> HandlerEntry<CoreHandler> {
    HandlerEntry::new(
        "ping",
        metadata_for("ping"),
        CoreHandler::new(|invocation| {
            Box::pin(async move {
                match invocation.lowered.len() {
                    0 | 1 => execute_ping().await,
                    2 if invocation.lowered[1] == "help" => Ok(CommandOutput::from_doc(
                        command_help_doc(invocation.manifest, "ping", None),
                    )),
                    _ => bail!("unknown ping command. use `ping help`"),
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
                // The interactive launchers (`setup vault|llm|model`, `start setup`,
                // `model`) are intercepted ahead of dispatch by the wizard route in
                // `CommandService::execute_line` (and the desktop `run_command`), so
                // they never reach this handler. What remains is the static help and
                // the direct `setup verbosity` config write.
                match invocation.lowered.get(1).map(String::as_str) {
                    None | Some("help") => Ok(CommandOutput::from_doc(command_help_doc(
                        invocation.manifest,
                        "setup",
                        None,
                    ))),
                    Some("verbosity") => {
                        let loaded = load_effective()?;
                        match invocation.lowered.get(2) {
                            None => Ok(CommandOutput::text(format!(
                                "generation verbosity is '{}'.\nUsage: setup verbosity <brief|medium|verbose>",
                                loaded.effective.generation.verbosity.as_str(),
                            ))),
                            Some(raw) => {
                                let Some(level) = Verbosity::parse(raw) else {
                                    bail!(
                                        "unknown verbosity '{raw}'. choose one of: brief, medium, verbose"
                                    );
                                };
                                let mut config = loaded.effective;
                                config.generation.verbosity = level;
                                let path = save_config(&config)?;
                                Ok(CommandOutput::text(format!(
                                    "generation verbosity set to '{}' ({}).",
                                    level.as_str(),
                                    path.display()
                                )))
                            }
                        }
                    }
                    Some("vault") | Some("llm") | Some("model") => bail!(
                        "`setup {}` starts an interactive wizard; run it from the console.",
                        invocation.lowered[1]
                    ),
                    Some(other) => bail!("unknown setup command: {other}. use `setup help`"),
                }
            })
        }),
    )
}

pub async fn execute_line(input: &str) -> CommandResponse {
    let mut session = SessionState::default();
    execute_line_with_session(input, &mut session).await
}

pub async fn execute_line_with_session(input: &str, session: &mut SessionState) -> CommandResponse {
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        session.push_history(trimmed, 50);
    }

    match execute_line_internal(input, session).await {
        Ok(output) => {
            let output_text = output.output.clone();
            // Every successful response carries a structured doc — the command's
            // own if it built one, else a single paragraph of its text — so the
            // frontend renders backend nodes and never has to parse prose.
            let document = output
                .output_doc
                .unwrap_or_else(|| doc().with_block(paragraph_text(output_text.clone())));
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
                output_doc: Some(document),
                client_event: None,
                wizard: None,
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
                output_doc: Some(output_doc_from_error(&err)),
                client_event: None,
                wizard: None,
            }
        }
    }
}

pub(crate) fn config_vault_path_string(config: &AppConfig) -> Option<String> {
    config
        .vault
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn expand_tilde_path(input: &str) -> PathBuf {
    if input == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }

    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }

    PathBuf::from(input)
}

pub(crate) fn validate_vault_path_for_onboarding(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("vault path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("vault path is not a directory: {}", path.display());
    }
    is_path_writable(path)?;
    Ok(())
}

pub(crate) fn normalize_ollama_input(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("://") {
        return trimmed.to_string();
    }
    format!("http://{trimmed}")
}

/// Probe an Ollama server during setup and summarize the model list for display.
///
/// Delegates the `/api/tags` request to the shared [`health::probe_ollama`] and
/// surfaces failures as errors so the setup spinner flips to an error state.
pub(crate) async fn probe_ollama_models(
    base_url: &str,
    timeout_seconds: u64,
) -> Result<(String, Vec<String>)> {
    let probe = health::probe_ollama(base_url, timeout_seconds).await;
    if let Some(error) = probe.error {
        bail!("{error}");
    }

    let detail = if probe.models.is_empty() {
        "connected (no models returned)".to_string()
    } else {
        format!("connected ({} model(s) found)", probe.models.len())
    };

    Ok((detail, probe.models))
}

pub async fn execute_line_result(input: &str) -> Result<CommandOutput> {
    let mut session = SessionState::default();
    execute_line_internal(input, &mut session).await
}

async fn execute_line_internal(input: &str, session: &mut SessionState) -> Result<CommandOutput> {
    let normalized_input = normalize_command_input(input);
    let parsed_words =
        shell_words::split(&normalized_input).map_err(|e| anyhow!("invalid command input: {e}"))?;
    let manifest = command_manifest();
    let normalized_words = normalize_alias_tokens(&parsed_words, &manifest);
    execute_dispatched(&normalized_words, &manifest, session, &normalized_input).await
}

/// Reject `-h`/`--help` anywhere in a command, returning the phrase-help hint to
/// surface instead. Shared by the core dispatch path and the desktop dispatch seam
/// (`main.rs`) so the rejection is uniform across Core and Desktop commands.
pub fn reject_help_flags(tokens: &[String]) -> Option<String> {
    let has_help_flag = tokens
        .iter()
        .any(|token| token.eq_ignore_ascii_case("-h") || token.eq_ignore_ascii_case("--help"));
    if !has_help_flag {
        return None;
    }

    let hint = tokens
        .first()
        .map(|root| {
            let root = root.to_ascii_lowercase();
            format!("use `{root} help` or `help {root}`")
        })
        .unwrap_or_else(|| "use `help`".to_string());
    Some(format!("`-h`/`--help` is not supported; {hint}"))
}

async fn execute_dispatched(
    tokens: &[String],
    manifest: &CommandManifest,
    session: &mut SessionState,
    raw_input: &str,
) -> Result<CommandOutput> {
    if let Some(message) = reject_help_flags(tokens) {
        bail!("{message}");
    }

    let lowered: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();

    let registry = core_handler_registry();
    let handler_name = if lowered.is_empty() {
        "status"
    } else {
        lowered[0].as_str()
    };

    if let Some(entry) = registry.get(handler_name) {
        let invocation = CoreHandlerInvocation {
            _tokens: tokens,
            lowered: &lowered,
            manifest,
            _session: session,
            _raw_input: raw_input,
        };
        return entry.execute(invocation).await;
    }

    if lowered.is_empty() {
        bail!("unknown command. use `help`");
    }

    bail!("unknown command: {}. use `help`", lowered[0]);
}

async fn execute_config_command(invocation: CoreHandlerInvocation<'_>) -> Result<CommandOutput> {
    let lowered = invocation.lowered;
    let manifest = invocation.manifest;
    if lowered.len() == 1 || (lowered.len() == 2 && lowered[1] == "help") {
        return Ok(CommandOutput::from_doc(command_help_doc(
            manifest, "config", None,
        )));
    }

    if lowered.len() == 3 && lowered[2] == "help" {
        return Ok(CommandOutput::from_doc(command_help_doc(
            manifest,
            "config",
            Some(&lowered[1]),
        )));
    }

    match lowered[1].as_str() {
        "show" if lowered.len() == 2 => {
            let loaded = load_effective()?;
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
            let loaded = load_effective()?;
            let report = health::run_doctor_checks(&loaded.effective).await;
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

fn find_manifest_command<'a>(manifest: &'a CommandManifest, root: &str) -> Option<&'a CommandSpec> {
    manifest
        .commands
        .iter()
        .find(|command| command.name.eq_ignore_ascii_case(root))
}

fn examples_block(examples: &[String]) -> OutputBlock {
    let items: Vec<Vec<InlineNode>> = examples
        .iter()
        .map(|example| vec![code(example.clone())])
        .collect();
    list(items)
}

/// The clickable root help index. The plain-text `output` is derived from this doc
/// via [`OutputDoc::to_plain_text`] (P7.2), so there is one source per help surface.
fn root_help_doc(manifest: &CommandManifest, context: &InputContext) -> OutputDoc {
    let items: Vec<Vec<InlineNode>> = manifest
        .commands
        .iter()
        .filter(|command| command.show_in_autocomplete)
        .filter(|command| help_lists_command(&command.name, context))
        .map(|command| {
            vec![
                command_ref(command.name.clone(), format!("{} help", command.name)),
                text_node(format!(" — {}", command.summary)),
            ]
        })
        .collect();
    doc()
        .with_block(heading(2, "Commands"))
        .with_block(list(items))
        .with_block(paragraph_text(
            "Use `<command> help` or `help <command>` for details.",
        ))
}

/// Structured, clickable help for a single command (`subcommand = None`) or one
/// of its subcommands. Subcommands render as `command_ref`s and examples as code;
/// the plain-text fallback is derived from this doc via [`OutputDoc::to_plain_text`].
fn command_help_doc(manifest: &CommandManifest, root: &str, subcommand: Option<&str>) -> OutputDoc {
    let Some(command) = find_manifest_command(manifest, root) else {
        return doc().with_block(paragraph_with_inlines(vec![
            text_node(format!("unknown command: {root}. use ")),
            command_ref("help", "help"),
        ]));
    };

    if let Some(sub_name) = subcommand {
        let Some(sub) = command
            .subcommands
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(sub_name))
        else {
            return doc().with_block(paragraph_with_inlines(vec![
                text_node(format!("unknown {root} command. use ")),
                command_ref(format!("{root} help"), format!("{root} help")),
            ]));
        };

        let mut document = doc()
            .with_block(heading(2, format!("{} {}", command.name, sub.name)))
            .with_block(paragraph_text(sub.summary.clone()));
        if !sub.examples.is_empty() {
            document = document
                .with_block(heading(3, "Examples"))
                .with_block(examples_block(&sub.examples));
        }
        return document;
    }

    let mut document = doc()
        .with_block(heading(2, command.name.clone()))
        .with_block(paragraph_text(command.summary.clone()));
    if !command.subcommands.is_empty() {
        let items: Vec<Vec<InlineNode>> = command
            .subcommands
            .iter()
            .map(|sub| {
                vec![
                    command_ref(
                        format!("{} {}", command.name, sub.name),
                        format!("{} {} help", command.name, sub.name),
                    ),
                    text_node(format!(" — {}", sub.summary)),
                ]
            })
            .collect();
        document = document
            .with_block(heading(3, "Subcommands"))
            .with_block(list(items));
    }
    if !command.examples.is_empty() {
        document = document
            .with_block(heading(3, "Examples"))
            .with_block(examples_block(&command.examples));
    }
    document
}

async fn execute_status() -> Result<CommandOutput> {
    let loaded = load_effective()?;
    let global_config_path = loaded.paths.global.display().to_string();
    let config = loaded.effective;

    let issues = required_issues(&config);
    if !issues.is_empty() {
        // Surface the bootstrap gate as typed data, not a prose string: the
        // structured doc is built directly from `issues` (see `setup_required_doc`)
        // instead of being re-parsed out of a rendered message.
        return Err(SetupRequired {
            issues,
            global_config_path,
        }
        .into());
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

/// Timeout for interactive setup probes. Longer than the boot budget (the user is
/// actively waiting and may point at a remote server) but still bounded so a dead
/// server can't hang setup. Distinct from `ollama.timeout_seconds`, which is the
/// LLM generation budget and far too long for a connectivity check.
pub(crate) const OLLAMA_SETUP_TIMEOUT_SECONDS: u64 = 15;

/// Probe the configured Ollama server to confirm the LLM is running.
///
/// Backs the `ping` command (and its `reconnect` alias). Fails (so the spinner
/// flips to error) when the server is unreachable; reports a warning when the
/// server answers but the configured model is missing.
async fn execute_ping() -> Result<CommandOutput> {
    let loaded = load_effective()?;
    let config = loaded.effective;

    if config.ollama.base_url.trim().is_empty() {
        bail!("no Ollama server is configured. run `setup llm` to configure one.");
    }

    let health = health::check_ollama_health(&config, OLLAMA_BOOT_TIMEOUT_SECONDS).await;
    let endpoint = config.ollama.base_url.clone();
    let model = config
        .ollama
        .model
        .clone()
        .unwrap_or_else(|| "(not set)".to_string());

    if !health.reachable {
        bail!("Ollama is offline: {}.", health.detail);
    }

    let (tone, line) = if health.model_available {
        (
            StatusTone::Success,
            format!("Ollama is online; model {model} is available."),
        )
    } else {
        (
            StatusTone::Warning,
            format!("Ollama is online, but {}.", health.detail),
        )
    };

    let text = format!("## Ollama ping\n{line}\n\nendpoint: {endpoint}\nmodel: {model}");
    let output_doc = doc()
        .with_block(heading(2, "Ollama ping"))
        .with_block(status(tone, line))
        .with_block(list(vec![
            vec![text_node(format!("endpoint: {endpoint}"))],
            vec![text_node(format!("model: {model}"))],
        ]));

    Ok(CommandOutput::with_doc(text, output_doc))
}

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

/// The bootstrap gate: required config is missing, so the app can't run yet.
///
/// Modeled as a typed error (not a prose string) so the structured `OutputDoc`
/// is built directly from `issues` rather than re-parsed from a rendered message.
/// `Display` keeps a readable message for the plain-text `error` field, logs, and
/// the CLI. Kept `ok:false` (a non-zero exit) by design — this is a gate, but
/// `status` should still fail for scripting.
#[derive(Debug)]
pub(crate) struct SetupRequired {
    pub issues: Vec<String>,
    pub global_config_path: String,
}

impl std::fmt::Display for SetupRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "First-time setup required\n\n\
             runebound.sh needs two connections before it can run: an Ollama LLM endpoint and an Obsidian vault path.\n\n\
             Run `start setup` to begin guided setup.\n\n\
             Config paths\n- global: {}\n\n\
             Missing required values\n- {}",
            self.global_config_path,
            self.issues.join("\n- "),
        )
    }
}

impl std::error::Error for SetupRequired {}

/// Build the structured error doc for a failed command, keying off the typed
/// error rather than sniffing its rendered text. The bootstrap gate gets a
/// `Warning`-toned doc with a clickable `start setup`; everything else is a plain
/// `Error`-toned status block.
pub(crate) fn output_doc_from_error(err: &anyhow::Error) -> OutputDoc {
    if let Some(setup) = err.downcast_ref::<SetupRequired>() {
        return setup_required_doc(setup);
    }
    error_status_doc(err.to_string())
}

/// A plain error doc: a single `Error`-toned status block. Used for ordinary
/// failures and by the service layer's error responses.
pub(crate) fn error_status_doc(message: impl Into<String>) -> OutputDoc {
    doc().with_block(status(StatusTone::Error, message.into()))
}

fn setup_required_doc(setup: &SetupRequired) -> OutputDoc {
    let mut document = doc()
        .with_block(heading(2, "First-time setup required"))
        .with_block(paragraph_text(
            "runebound.sh needs two connections before it can run: an Ollama LLM endpoint and an Obsidian vault path.",
        ))
        .with_block(paragraph_with_inlines(vec![
            text_node("Run "),
            command_ref("start setup", "start setup"),
            text_node(" to begin guided setup."),
        ]));
    if !setup.issues.is_empty() {
        document = document
            .with_block(heading(3, "Missing required values"))
            .with_block(list(
                setup
                    .issues
                    .iter()
                    .map(|issue| vec![text_node(issue.clone())])
                    .collect(),
            ));
    }
    document
}

#[cfg(test)]
mod tests {
    use super::{
        SetupRequired, build_core_handler_registry, execute_line_result, output_doc_from_error,
    };
    use command_specs::{CommandExecution, command_manifest};

    /// `model` (and the `setup`/`start` launchers) are dispatched via the wizard
    /// route ahead of registry lookup, so `model` has no core registry entry.
    /// See docs/command-contexts.md §4.
    const ONBOARDING_INTERCEPTED_CORE: &[&str] = &["model"];

    #[test]
    fn every_core_command_has_a_registered_handler() {
        let registry = build_core_handler_registry();
        for command in command_manifest().commands {
            if !matches!(command.execution, CommandExecution::Core) {
                continue;
            }
            if ONBOARDING_INTERCEPTED_CORE.contains(&command.name.as_str()) {
                continue;
            }
            assert!(
                registry.get(&command.name).is_some(),
                "manifest declares core command `{}` but no handler is registered in \
                 build_core_handler_registry()",
                command.name,
            );
        }
    }

    #[tokio::test]
    async fn rejects_help_flags_in_favor_of_phrase_help() {
        let result = execute_line_result("config --help").await;
        let error = result.expect_err("expected --help to be rejected");
        assert!(error.to_string().contains("not supported"));
        assert!(error.to_string().contains("config help"));
    }

    #[tokio::test]
    async fn supports_help_prefix_normalization() {
        let result = execute_line_result("help config")
            .await
            .expect("expected help config to succeed");
        assert!(result.output.contains("## config"));
    }

    #[test]
    fn setup_required_error_builds_a_structured_doc_from_issues() {
        use runebound_models::output::{InlineNode, OutputBlock};

        let err: anyhow::Error = SetupRequired {
            issues: vec!["vault.path is not configured".to_string()],
            global_config_path: "/tmp/config.toml".to_string(),
        }
        .into();
        let document = output_doc_from_error(&err);

        // Leads with a heading (not an Error-toned status), so the frontend renders
        // it as a neutral gate rather than a hard error — keyed off the doc's
        // structure, not a string match.
        assert!(matches!(
            document.blocks.first(),
            Some(OutputBlock::Heading { level: 2, .. })
        ));
        // Offers a clickable `start setup` (a real command_ref, not prose-guessing).
        let has_start_setup = document.blocks.iter().any(|block| {
            match block {
            OutputBlock::Paragraph { inlines } => inlines.iter().any(|node| {
                matches!(node, InlineNode::CommandRef { command, .. } if command == "start setup")
            }),
            _ => false,
        }
        });
        assert!(
            has_start_setup,
            "setup doc must offer a clickable start setup"
        );
        // The typed issues render structurally as a list (no `- ` re-parse).
        let has_issue = document.blocks.iter().any(|block| match block {
            OutputBlock::List { items } => items.iter().any(|item| {
                item.iter().any(|node| {
                    matches!(node, InlineNode::Text { text } if text.contains("vault.path is not configured"))
                })
            }),
            _ => false,
        });
        assert!(
            has_issue,
            "missing-values list must include the typed issue"
        );
    }

    #[test]
    fn ordinary_errors_render_as_an_error_status() {
        use runebound_models::output::{OutputBlock, StatusTone};

        let document = output_doc_from_error(&anyhow::anyhow!("something broke"));
        assert!(matches!(
            document.blocks.as_slice(),
            [OutputBlock::Status {
                tone: StatusTone::Error,
                ..
            }]
        ));
    }
}
