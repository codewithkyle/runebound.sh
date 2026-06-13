use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::{
    ConfigScope, determine_default_write_scope, load_effective, required_issues, save_config,
    validate_for_runtime,
};
use crate::db;
use crate::health::{self, CheckReport};
use crate::vault::{Vault, is_path_writable};

#[derive(Debug, Parser)]
#[command(name = "dnd-assistant")]
#[command(about = "D&D assistant command runner", long_about = None)]
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
}

pub async fn execute_line(workspace_root: &Path, input: &str) -> CommandResponse {
    match execute_line_result(workspace_root, input).await {
        Ok(output) => CommandResponse {
            ok: true,
            output,
            error: None,
            exit_code: 0,
        },
        Err(err) => CommandResponse {
            ok: false,
            output: String::new(),
            error: Some(err.to_string()),
            exit_code: 1,
        },
    }
}

pub async fn execute_line_result(workspace_root: &Path, input: &str) -> Result<String> {
    let mut argv = vec!["dnd-assistant".to_string()];
    let parsed_words = shell_words::split(input).map_err(|e| anyhow!("invalid command input: {e}"))?;
    argv.extend(parsed_words);

    let cli = Cli::try_parse_from(argv).map_err(|e| anyhow!(e.to_string()))?;
    execute_parsed(workspace_root, cli).await
}

pub async fn execute_parsed(workspace_root: &Path, cli: Cli) -> Result<String> {
    match cli.command {
        Some(Command::Config { command }) => execute_config_command(workspace_root, command).await,
        Some(Command::Status) | None => execute_status(workspace_root).await,
    }
}

async fn execute_config_command(workspace_root: &Path, command: ConfigCommand) -> Result<String> {
    match command {
        ConfigCommand::Init(args) => execute_noninteractive_init(workspace_root, args).await,
        ConfigCommand::Show => {
            let loaded = load_effective(workspace_root)?;
            let mut out = String::new();
            out.push_str(&format!("global config: {}\n", loaded.paths.global.display()));
            out.push_str(&format!("workspace config: {}\n", loaded.paths.workspace.display()));
            out.push('\n');
            out.push_str(&toml::to_string_pretty(&loaded.effective)?);

            let issues = required_issues(&loaded.effective);
            if !issues.is_empty() {
                out.push_str("\nincomplete config:\n");
                for issue in issues {
                    out.push_str(&format!("- {issue}\n"));
                }
            }

            Ok(out.trim_end().to_string())
        }
        ConfigCommand::Test => {
            let loaded = load_effective(workspace_root)?;
            let report = health::run_quick_checks(&loaded.effective).await;
            let out = format_report("config test", &report);
            if !report.is_ok() {
                bail!("{out}\none or more checks failed");
            }
            Ok(out)
        }
        ConfigCommand::Doctor => {
            let loaded = load_effective(workspace_root)?;
            let report = health::run_doctor_checks(&loaded.effective, workspace_root).await;
            let out = format_report("config doctor", &report);
            if !report.is_ok() {
                bail!("{out}\none or more checks failed");
            }
            Ok(out)
        }
    }
}

async fn execute_status(workspace_root: &Path) -> Result<String> {
    let loaded = load_effective(workspace_root)?;
    let config = loaded.effective;

    let issues = required_issues(&config);
    if !issues.is_empty() {
        bail!(
            "first-time setup required. run `config init --vault-path <path> --ollama-base-url <url> --model <name>`\n- {}",
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

    Ok(format!(
        "ready\nvault: {}\ndatabase: {}",
        vault.root().display(),
        db.path.display()
    ))
}

async fn execute_noninteractive_init(workspace_root: &Path, args: InitArgs) -> Result<String> {
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
            "missing required config. provide flags like `--vault-path`, `--ollama-base-url`, `--model`\n- {}",
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
    }

    Ok(out)
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
