use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::command_manifest::command_manifest;
use crate::command_parse::{normalize_alias_tokens, normalize_command_input};
use crate::config::{
    ConfigScope, determine_default_write_scope, load_effective, required_issues, save_config,
    validate_for_runtime,
};
use crate::db;
use crate::health::{self, CheckReport};
use crate::output::{
    InlineNode, OutputBlock, OutputDoc, StatusTone, command_ref, doc, heading, list,
    paragraph_text, paragraph_with_inlines, status, text_node,
};
use crate::vault::{Vault, is_path_writable};

#[derive(Debug, Parser)]
#[command(name = "dnd-assistant")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Status,
    Exit,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Init(InitArgs),
    Show,
    Test,
    Doctor,
}

#[derive(Debug, Clone, Args, Default)]
pub struct InitArgs {
    #[arg(long)]
    pub global: bool,
    #[arg(long)]
    pub workspace: bool,
    #[arg(long)]
    pub skip_test: bool,
    #[arg(long = "vault-path")]
    pub vault_path: Option<String>,
    #[arg(long = "ollama-base-url")]
    pub ollama_base_url: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
    pub segments: Vec<OutputSegment>,
    pub output_doc: Option<OutputDoc>,
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

pub async fn execute_line(workspace_root: &Path, input: &str) -> CommandResponse {
    match execute_line_result(workspace_root, input).await {
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
            }
        }
    }
}

pub async fn execute_line_result(workspace_root: &Path, input: &str) -> Result<CommandOutput> {
    let normalized_input = normalize_command_input(input);
    let mut argv = vec!["dnd-assistant".to_string()];
    let parsed_words =
        shell_words::split(&normalized_input).map_err(|e| anyhow!("invalid command input: {e}"))?;
    let normalized_words = normalize_alias_tokens(&parsed_words, &command_manifest());
    argv.extend(normalized_words);

    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                let text = strip_binary_name_from_help(&err.to_string());
                return Ok(CommandOutput::text(text));
            }
            return Err(anyhow!(strip_binary_name_from_help(&err.to_string())));
        }
    };
    execute_parsed(workspace_root, cli).await
}

pub async fn execute_parsed(workspace_root: &Path, cli: Cli) -> Result<CommandOutput> {
    match cli.command {
        Some(Command::Config { command }) => execute_config_command(workspace_root, command).await,
        Some(Command::Exit) => Ok(CommandOutput::text("exiting".to_string())),
        Some(Command::Status) | None => execute_status(workspace_root).await,
    }
}

async fn execute_config_command(
    workspace_root: &Path,
    command: ConfigCommand,
) -> Result<CommandOutput> {
    match command {
        ConfigCommand::Init(args) => execute_noninteractive_init(workspace_root, args).await,
        ConfigCommand::Show => {
            let loaded = load_effective(workspace_root)?;
            let mut out = String::new();
            out.push_str(&format!(
                "global config: {}\n",
                loaded.paths.global.display()
            ));
            out.push_str(&format!(
                "workspace config: {}\n",
                loaded.paths.workspace.display()
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
        ConfigCommand::Test => {
            let loaded = load_effective(workspace_root)?;
            let report = health::run_quick_checks(&loaded.effective).await;
            let out = format_report("config test", &report);
            let output_doc = report_output_doc("Config Test", &report);
            if !report.is_ok() {
                bail!("{out}\none or more checks failed");
            }
            Ok(CommandOutput::with_doc(out, output_doc))
        }
        ConfigCommand::Doctor => {
            let loaded = load_effective(workspace_root)?;
            let report = health::run_doctor_checks(&loaded.effective, workspace_root).await;
            let out = format_report("config doctor", &report);
            let output_doc = report_output_doc("Config Doctor", &report);
            if !report.is_ok() {
                bail!("{out}\none or more checks failed");
            }
            Ok(CommandOutput::with_doc(out, output_doc))
        }
    }
}

async fn execute_status(workspace_root: &Path) -> Result<CommandOutput> {
    let loaded = load_effective(workspace_root)?;
    let global_config_path = loaded.paths.global.display().to_string();
    let workspace_config_path = loaded.paths.workspace.display().to_string();
    let config = loaded.effective;

    let issues = required_issues(&config);
    if !issues.is_empty() {
        bail!(
            "## First-time setup required\n\
             \n## Bootstrap config\n\
             config init --vault-path <path> --ollama-base-url <url> --model <name>\n\
             \n## Config paths\n\
             - global: {global_config_path}\n\
             - workspace: {workspace_config_path}\n\
             \n## Optional next steps\n\
             - use config init --workspace ... to keep settings local to this project\n\
             - run config show to verify saved values\n\
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

async fn execute_noninteractive_init(
    workspace_root: &Path,
    args: InitArgs,
) -> Result<CommandOutput> {
    if args.global && args.workspace {
        bail!("choose only one scope: --global or --workspace");
    }

    let loaded = load_effective(workspace_root)?;
    let mut config = loaded.effective;

    if let Some(vault_path) = args.vault_path {
        let path = PathBuf::from(vault_path);
        if !path.exists() {
            bail!("vault path does not exist: {}", path.display());
        }
        if !path.is_dir() {
            bail!("vault path is not a directory: {}", path.display());
        }
        is_path_writable(&path)?;
        config.vault.path = Some(path);
    }

    if let Some(ollama_base_url) = args.ollama_base_url {
        config.ollama.base_url = ollama_base_url;
    }

    if let Some(model) = args.model {
        config.ollama.model = Some(model);
    }

    let issues = required_issues(&config);
    if !issues.is_empty() {
        bail!(
            "missing required config. provide flags like --vault-path, --ollama-base-url, --model\n- {}",
            issues.join("\n- ")
        );
    }

    let scope = if args.global {
        ConfigScope::Global
    } else if args.workspace {
        ConfigScope::Workspace
    } else {
        determine_default_write_scope(&load_effective(workspace_root)?)
    };

    let path = save_config(workspace_root, scope, &config)?;

    let vault_root = config
        .vault
        .path
        .clone()
        .ok_or_else(|| anyhow!("vault.path is not configured"))?;
    let vault = Vault::new(vault_root);
    vault.ensure_structure()?;
    let db = db::init_database().await?;

    let mut out = format!(
        "config saved to {}\nvault initialized at {}\ndatabase ready at {}",
        path.display(),
        vault.root().display(),
        db.path.display()
    );

    if !args.skip_test {
        let report = health::run_quick_checks(&config).await;
        out.push_str("\n\n");
        out.push_str(&format_report("setup validation", &report));
        if !report.is_ok() {
            bail!("{out}\none or more checks failed");
        }

        let mut output_doc = doc();
        output_doc.push(heading(2, "Config Initialized"));
        output_doc.push(paragraph_text(format!(
            "config saved to {}",
            path.display()
        )));
        output_doc.push(paragraph_text(format!(
            "vault initialized at {}",
            vault.root().display()
        )));
        output_doc.push(paragraph_text(format!(
            "database ready at {}",
            db.path.display()
        )));
        output_doc.push(heading(2, "Setup Validation"));
        output_doc.push(report_list_block(&report));
        return Ok(CommandOutput::with_doc(out, output_doc));
    }

    Ok(CommandOutput::text(out))
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
        let mut output_doc = doc();
        output_doc.push(heading(2, "First-time setup required"));
        output_doc.push(heading(2, "Bootstrap config"));
        output_doc.push(paragraph_with_inlines(vec![command_ref(
            "config init --vault-path <path> --ollama-base-url <url> --model <name>",
            "config init --vault-path <path> --ollama-base-url <url> --model <name>",
        )]));
        output_doc.push(heading(2, "Optional next steps"));
        output_doc.push(list(vec![
            vec![
                text_node("use "),
                command_ref("config init --workspace", "config init --workspace"),
                text_node(" ... to keep settings local to this project"),
            ],
            vec![
                text_node("run "),
                command_ref("config show", "config show"),
                text_node(" to verify saved values"),
            ],
        ]));
        output_doc.push(paragraph_text(message));
        return output_doc;
    }

    doc().with_block(status(StatusTone::Error, message))
}

fn strip_binary_name_from_help(text: &str) -> String {
    let mut out = Vec::new();

    for line in text.lines() {
        if line.starts_with("Usage: dnd-assistant ") {
            out.push(line.replacen("Usage: dnd-assistant ", "Usage: ", 1));
        } else if line == "Usage: dnd-assistant" {
            out.push("Usage:".to_string());
        } else {
            out.push(line.to_string());
        }
    }

    out.join("\n")
}
