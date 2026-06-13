#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

use dnd_core::command::CommandResponse;
use dnd_core::command_manifest::{CommandManifest, command_manifest};
use dnd_core::command_parse::{ParseResult, parse_command_input as parse_shared_command_input};
use dnd_core::config::{load_effective, required_issues, save_config};
use dnd_core::db;
use dnd_core::health;
use dnd_core::vault::{Vault, is_path_writable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
struct SetupState {
    needs_setup: bool,
    issues: Vec<String>,
    global_config_path: String,
    default_ollama_base_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaProbeResult {
    ok: bool,
    detail: String,
    models: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SaveOnboardingInput {
    vault_path: String,
    ollama_base_url: String,
    model: String,
}

#[derive(Debug, Clone, Serialize)]
struct SaveOnboardingResult {
    config_path: String,
    vault_path: String,
    db_path: String,
    warnings: Vec<String>,
}

struct AppState {
    workspace_root: PathBuf,
}

fn normalize_ollama_base_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

#[tauri::command]
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommandResponse, String> {
    Ok(dnd_core::command::execute_line(&state.workspace_root, &input).await)
}

#[tauri::command]
fn get_setup_state(state: tauri::State<'_, AppState>) -> Result<SetupState, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    let issues = required_issues(&loaded.effective);

    Ok(SetupState {
        needs_setup: !issues.is_empty(),
        issues,
        global_config_path: loaded.paths.global.display().to_string(),
        default_ollama_base_url: loaded.effective.ollama.base_url,
    })
}

#[tauri::command]
fn validate_vault_path(path: String) -> Result<(), String> {
    let normalized = path.trim();
    if normalized.is_empty() {
        return Err("vault path cannot be empty".to_string());
    }

    let resolved = shellexpand::tilde(normalized).to_string();
    let path = PathBuf::from(resolved);

    if !path.exists() {
        return Err(format!("vault path does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("vault path is not a directory: {}", path.display()));
    }
    is_path_writable(&path).map_err(|err| err.to_string())
}

#[tauri::command]
async fn probe_ollama(base_url: String, timeout_seconds: u64) -> Result<OllamaProbeResult, String> {
    let normalized_base_url = normalize_ollama_base_url(&base_url);
    health::validate_ollama_url(&normalized_base_url).map_err(|err| err.to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|err| err.to_string())?;

    let url = format!("{}/api/tags", normalized_base_url.trim_end_matches('/'));
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    if !response.status().is_success() {
        return Ok(OllamaProbeResult {
            ok: false,
            detail: format!("ollama responded with {}", response.status()),
            models: Vec::new(),
        });
    }

    let value: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
    let mut names = Vec::new();
    if let Some(models) = value.get("models").and_then(|m| m.as_array()) {
        for model in models {
            if let Some(name) = model.get("name").and_then(|n| n.as_str()) {
                names.push(name.to_string());
            }
        }
    }

    Ok(OllamaProbeResult {
        ok: true,
        detail: if names.is_empty() {
            "connected (no models returned)".to_string()
        } else {
            format!("connected ({} model(s) found)", names.len())
        },
        models: names,
    })
}

#[tauri::command]
async fn save_onboarding_config(
    input: SaveOnboardingInput,
    state: tauri::State<'_, AppState>,
) -> Result<SaveOnboardingResult, String> {
    validate_vault_path(input.vault_path.clone())?;
    let normalized_base_url = normalize_ollama_base_url(&input.ollama_base_url);
    health::validate_ollama_url(&normalized_base_url).map_err(|err| err.to_string())?;

    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    let mut config = loaded.effective;
    config.vault.path = Some(PathBuf::from(
        shellexpand::tilde(input.vault_path.trim()).to_string(),
    ));
    config.ollama.base_url = normalized_base_url;
    config.ollama.model = Some(input.model.trim().to_string());

    let issues = required_issues(&config);
    if !issues.is_empty() {
        return Err(format!(
            "missing required config:\n- {}",
            issues.join("\n- ")
        ));
    }

    let config_path = save_config(&state.workspace_root, &config).map_err(|err| err.to_string())?;
    let vault_path = config
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure().map_err(|err| err.to_string())?;
    let db = db::init_database().await.map_err(|err| err.to_string())?;

    let report = health::run_quick_checks(&config).await;
    let warnings = report
        .items
        .into_iter()
        .filter(|item| !item.ok)
        .map(|item| format!("{}: {}", item.name, item.detail))
        .collect();

    Ok(SaveOnboardingResult {
        config_path: config_path.display().to_string(),
        vault_path: vault.root().display().to_string(),
        db_path: db.path.display().to_string(),
        warnings,
    })
}

#[tauri::command]
fn get_command_manifest() -> CommandManifest {
    command_manifest()
}

#[tauri::command]
fn parse_command_input(input: String) -> ParseResult {
    parse_shared_command_input(&input)
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    tauri::Builder::default()
        .manage(AppState { workspace_root })
        .invoke_handler(tauri::generate_handler![
            run_command,
            get_setup_state,
            validate_vault_path,
            probe_ollama,
            save_onboarding_config,
            get_command_manifest,
            parse_command_input,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
