use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Parser;
use dialoguer::{Confirm, Input, Select};
use dnd_core::command::{Cli, Command, ConfigCommand, InitArgs, execute_parsed};
use dnd_core::config::{
    AppConfig, ConfigScope, determine_default_write_scope, load_effective, required_issues,
    save_config,
};
use dnd_core::db;
use dnd_core::health::{self, CheckReport};
use dnd_core::vault::{Vault, is_path_writable};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let workspace_root = std::env::current_dir()?;

    if cli.command.is_none() {
        run_startup(&workspace_root).await?;
        return Ok(());
    }

    if let Some(Command::Config {
        command: ConfigCommand::Init(args),
    }) = &cli.command
    {
        if is_interactive_init(args) {
            let _ = run_setup_wizard(&workspace_root, args.clone()).await?;
            return Ok(());
        }
    }

    let output = execute_parsed(&workspace_root, cli).await?;
    println!("{output}");
    Ok(())
}

async fn run_startup(workspace_root: &Path) -> Result<()> {
    let loaded = load_effective(workspace_root)?;
    if !required_issues(&loaded.effective).is_empty() {
        println!("first-time setup is required before continuing.");
        let _ = run_setup_wizard(workspace_root, InitArgs::default()).await?;
    }

    let output = execute_parsed(
        workspace_root,
        Cli {
            command: Some(Command::Status),
        },
    )
    .await?;
    println!("{output}");

    Ok(())
}

async fn run_setup_wizard(workspace_root: &Path, args: InitArgs) -> Result<AppConfig> {
    if args.global && args.workspace {
        bail!("choose only one scope: --global or --workspace");
    }

    let loaded = load_effective(workspace_root)?;
    let mut config = loaded.effective;

    println!("dnd-assistant setup");
    println!("this will save your vault and Ollama settings.");

    let vault_default = config
        .vault
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let vault_path = prompt_vault_path(vault_default)?;
    config.vault.path = Some(vault_path.clone());

    let ollama_base = Input::<String>::new()
        .with_prompt("Ollama base URL")
        .default(config.ollama.base_url.clone())
        .validate_with(|input: &String| -> std::result::Result<(), &str> {
            if input.trim().is_empty() {
                return Err("URL cannot be empty");
            }
            Ok(())
        })
        .interact_text()?;
    config.ollama.base_url = ollama_base;

    let discovered_models =
        fetch_ollama_models(&config.ollama.base_url, config.ollama.timeout_seconds)
            .await
            .unwrap_or_default();
    config.ollama.model = Some(prompt_model(config.ollama.model.clone(), &discovered_models)?);

    if !args.skip_test {
        let report = health::run_quick_checks(&config).await;
        print_report("setup validation", &report);
        if !report.is_ok() {
            let proceed = Confirm::new()
                .with_prompt("Some checks failed. Save config anyway?")
                .default(true)
                .interact()?;
            if !proceed {
                bail!("setup cancelled");
            }
        }
    }

    let scope = if args.global {
        ConfigScope::Global
    } else if args.workspace {
        ConfigScope::Workspace
    } else {
        determine_default_write_scope(&load_effective(workspace_root)?)
    };

    let path = save_config(workspace_root, scope, &config)?;

    let vault = Vault::new(vault_path);
    vault.ensure_structure()?;
    let _ = db::init_database().await?;

    println!("config saved to {}", path.display());
    println!("vault initialized at {}", vault.root().display());

    Ok(config)
}

fn prompt_vault_path(default_value: String) -> Result<PathBuf> {
    let input = Input::<String>::new()
        .with_prompt("Obsidian vault path")
        .default(default_value)
        .validate_with(|value: &String| -> std::result::Result<(), &str> {
            if value.trim().is_empty() {
                return Err("path cannot be empty");
            }
            Ok(())
        })
        .interact_text()?;

    let expanded = shellexpand::tilde(input.trim()).to_string();
    let path = PathBuf::from(expanded);

    if !path.exists() {
        bail!("vault path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("vault path is not a directory: {}", path.display());
    }
    is_path_writable(&path)?;

    Ok(path)
}

fn prompt_model(current: Option<String>, discovered: &[String]) -> Result<String> {
    if discovered.is_empty() {
        let default_model = current.unwrap_or_else(|| "llama3.1:8b".to_string());
        let typed = Input::<String>::new()
            .with_prompt("Default Ollama model")
            .default(default_model)
            .interact_text()?;
        return Ok(typed);
    }

    let mut options = discovered.to_vec();
    options.push("Custom...".to_string());
    let default_index = current
        .as_ref()
        .and_then(|needle| discovered.iter().position(|m| m == needle))
        .unwrap_or(0);
    let selection = Select::new()
        .with_prompt("Choose default Ollama model")
        .items(&options)
        .default(default_index)
        .interact()?;

    if selection < discovered.len() {
        Ok(discovered[selection].clone())
    } else {
        let typed = Input::<String>::new()
            .with_prompt("Default Ollama model")
            .with_initial_text(current.unwrap_or_else(|| "llama3.1:8b".to_string()))
            .interact_text()?;
        Ok(typed)
    }
}

async fn fetch_ollama_models(base_url: &str, timeout_seconds: u64) -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()?;

    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        bail!("ollama responded with {}", response.status());
    }

    let value: serde_json::Value = response.json().await?;
    let mut names = Vec::new();
    if let Some(models) = value.get("models").and_then(|m| m.as_array()) {
        for model in models {
            if let Some(name) = model.get("name").and_then(|n| n.as_str()) {
                names.push(name.to_string());
            }
        }
    }

    Ok(names)
}

fn print_report(title: &str, report: &CheckReport) {
    println!("{title}:");
    for item in &report.items {
        let status = if item.ok { "OK" } else { "FAIL" };
        println!("- [{status}] {}: {}", item.name, item.detail);
    }
}

fn is_interactive_init(args: &InitArgs) -> bool {
    !args.global
        && !args.workspace
        && !args.skip_test
        && args.vault_path.is_none()
        && args.ollama_base_url.is_none()
        && args.model.is_none()
}
