#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

use dnd_core::command::CommandResponse;
use dnd_core::command_manifest::{CommandManifest, command_manifest};
use dnd_core::command_parse::{ParseResult, parse_command_input as parse_shared_command_input};
use dnd_core::config::{load_effective, required_issues, save_config, validate_for_runtime};
use dnd_core::db;
use dnd_core::health;
use dnd_core::npc::{
    LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp,
    render_location_markdown, render_npc_markdown, slugify, unique_slug_for_dir,
};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcSeed {
    name: String,
    race: String,
    sex: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateNpcSeedInput {
    prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SaveNpcDraftInput {
    id: String,
    name: String,
    race: String,
    sex: String,
    location: String,
}

#[derive(Debug, Clone, Serialize)]
struct SaveNpcDraftResult {
    id: String,
    slug: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct EnsureLocationInput {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
struct EnsureLocationResult {
    name: String,
    slug: String,
    vault_path: String,
    created_file: bool,
    created_record: bool,
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

fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" {
        Ok(normalized)
    } else {
        Err("sex must be one of: male, female".to_string())
    }
}

fn recent_name_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn parse_recent_npc_seeds(payloads: Vec<String>) -> Vec<NpcSeed> {
    payloads
        .into_iter()
        .filter_map(|payload| serde_json::from_str::<NpcSeed>(&payload).ok())
        .collect()
}

fn describe_recent_npc_seeds(seeds: &[NpcSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }

    let items: Vec<String> = seeds
        .iter()
        .take(10)
        .map(|seed| format!("{} | {} | {}", seed.name, seed.race, seed.sex))
        .collect();
    items.join("; ")
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
async fn generate_npc_seed(
    input: GenerateNpcSeedInput,
    state: tauri::State<'_, AppState>,
) -> Result<NpcSeed, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let config = loaded.effective;
    let model = config
        .ollama
        .model
        .clone()
        .ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

    let user_prompt = input
        .prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("Generate one D&D NPC for a fantasy campaign.");

    let database = db::init_database().await.map_err(|err| err.to_string())?;
    let recent_payloads = db::recent_generation_prompts(&database.pool, "npc_seed", 20)
        .await
        .map_err(|err| err.to_string())?;
    let recent_seeds = parse_recent_npc_seeds(recent_payloads);
    let recent_names = recent_name_set(&recent_seeds);
    let recent_context = describe_recent_npc_seeds(&recent_seeds);

    let schema = serde_json::json!({
        "type": "object",
        "required": ["name", "race", "sex"],
        "properties": {
            "name": { "type": "string", "minLength": 1 },
            "race": { "type": "string", "minLength": 1 },
            "sex": { "type": "string", "enum": ["male", "female"] }
        },
        "additionalProperties": false
    });

    let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.ollama.timeout_seconds))
        .build()
        .map_err(|err| err.to_string())?;

    let mut seen_attempt_names = std::collections::HashSet::new();

    for attempt in 0..5 {
        let base_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as i64)
            .unwrap_or(0);
        let run_seed = (base_seed + i64::from(attempt)) as i32;
        let repair_note = if attempt == 0 {
            ""
        } else {
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names."
        };

        let payload = serde_json::json!({
            "model": model,
            "stream": false,
            "format": schema,
            "options": {
                "temperature": 1.1,
                "top_p": 0.92,
                "repeat_penalty": 1.15,
                "seed": run_seed
            },
            "messages": [
                {
                    "role": "system",
                    "content": format!(
                        "You generate concise D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, sex. Avoid these recent seeds: {}.{}",
                        recent_context,
                        repair_note,
                    )
                },
                {
                    "role": "user",
                    "content": user_prompt
                }
            ]
        });

        let response = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|err| err.to_string())?;

        if !response.status().is_success() {
            return Err(format!("ollama chat failed with status {}", response.status()));
        }

        let value: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
        let Some(content) = value
            .get("message")
            .and_then(|msg| msg.get("content"))
            .and_then(|content| content.as_str())
        else {
            continue;
        };

        let parsed: Result<NpcSeed, _> = serde_json::from_str(content);
        let Ok(mut seed) = parsed else {
            continue;
        };

        seed.name = seed.name.trim().to_string();
        seed.race = seed.race.trim().to_string();
        seed.sex = normalize_sex(&seed.sex)?;

        if seed.name.is_empty() || seed.race.is_empty() {
            continue;
        }

        let normalized_name = seed.name.to_ascii_lowercase();
        if recent_names.contains(&normalized_name) || seen_attempt_names.contains(&normalized_name) {
            continue;
        }
        seen_attempt_names.insert(normalized_name);

        let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
        db::insert_generation(&database.pool, "npc_seed", None, &serialized_seed)
            .await
            .map_err(|err| err.to_string())?;

        return Ok(seed);
    }

    Err("failed to generate valid structured NPC output from ollama".to_string())
}

#[tauri::command]
async fn ensure_location_exists(
    input: EnsureLocationInput,
    state: tauri::State<'_, AppState>,
) -> Result<EnsureLocationResult, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure().map_err(|err| err.to_string())?;

    let raw_name = input.name.trim();
    if raw_name.is_empty() {
        return Err("location name cannot be empty".to_string());
    }
    if raw_name.eq_ignore_ascii_case(UNKNOWN_LOCATION) {
        return Ok(EnsureLocationResult {
            name: UNKNOWN_LOCATION.to_string(),
            slug: slugify(UNKNOWN_LOCATION),
            vault_path: String::new(),
            created_file: false,
            created_record: false,
        });
    }

    let slug = slugify(raw_name);
    let relative_path = PathBuf::from("locations").join(format!("{slug}.md"));
    let full_path = vault
        .resolve_relative(&relative_path)
        .map_err(|err| err.to_string())?;
    let file_exists = full_path.exists();

    let database = db::init_database().await.map_err(|err| err.to_string())?;
    let existing = db::find_location_by_slug(&database.pool, &slug)
        .await
        .map_err(|err| err.to_string())?;

    let mut created_file = false;
    let mut created_record = false;
    let now = now_timestamp();
    let id = existing
        .as_ref()
        .map(|row| row.id.clone())
        .unwrap_or_else(|| make_entity_id("loc"));
    let canonical_name = existing
        .as_ref()
        .map(|row| row.name.clone())
        .unwrap_or_else(|| raw_name.to_string());
    let created_at = existing
        .as_ref()
        .map(|row| row.created_at.clone())
        .unwrap_or_else(|| now.clone());

    if !file_exists {
        let content = render_location_markdown(&LocationFrontmatter {
            doc_type: "location".to_string(),
            id: id.clone(),
            slug: slug.clone(),
            name: canonical_name.clone(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
        })
        .map_err(|err| err.to_string())?;
        vault
            .write_relative(&relative_path, &content)
            .map_err(|err| err.to_string())?;
        created_file = true;
    }

    if existing.is_none() {
        created_record = true;
    }

    let row = db::LocationRow {
        id,
        slug: slug.clone(),
        name: canonical_name.clone(),
        vault_path: relative_path.to_string_lossy().to_string(),
        created_at,
        updated_at: now.clone(),
    };

    db::upsert_location(&database.pool, &row)
        .await
        .map_err(|err| err.to_string())?;
    db::upsert_document_index(
        &database.pool,
        "location",
        &row.slug,
        Some(&row.name),
        &row.vault_path,
        &row.created_at,
        &row.updated_at,
    )
    .await
    .map_err(|err| err.to_string())?;

    Ok(EnsureLocationResult {
        name: canonical_name,
        slug,
        vault_path: row.vault_path,
        created_file,
        created_record,
    })
}

#[tauri::command]
async fn save_npc_draft(
    input: SaveNpcDraftInput,
    state: tauri::State<'_, AppState>,
) -> Result<SaveNpcDraftResult, String> {
    if input.id.trim().is_empty() {
        return Err("npc id cannot be empty".to_string());
    }

    let name = input.name.trim();
    if name.is_empty() {
        return Err("npc name cannot be empty".to_string());
    }
    let race = input.race.trim();
    if race.is_empty() {
        return Err("npc race cannot be empty".to_string());
    }
    let sex = normalize_sex(&input.sex)?;
    let location = if input.location.trim().is_empty() {
        UNKNOWN_LOCATION.to_string()
    } else {
        input.location.trim().to_string()
    };

    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    vault.ensure_structure().map_err(|err| err.to_string())?;

    let database = db::init_database().await.map_err(|err| err.to_string())?;
    let now = now_timestamp();

    let existing = db::find_npc_by_id(&database.pool, input.id.trim())
        .await
        .map_err(|err| err.to_string())?;

    let (slug, relative_path, created_at) = if let Some(current) = existing {
        (current.slug, current.vault_path, current.created_at)
    } else {
        let base_slug = slugify(name);
        let slug = unique_slug_for_dir(vault.root(), "npcs", &base_slug);
        (
            slug.clone(),
            PathBuf::from("npcs")
                .join(format!("{slug}.md"))
                .to_string_lossy()
                .to_string(),
            now.clone(),
        )
    };

    let markdown = render_npc_markdown(&NpcFrontmatter {
        doc_type: "npc".to_string(),
        id: input.id.trim().to_string(),
        slug: slug.clone(),
        name: name.to_string(),
        race: race.to_string(),
        sex: sex.clone(),
        location: location.clone(),
        created_at: created_at.clone(),
        updated_at: now.clone(),
    })
    .map_err(|err| err.to_string())?;

    vault
        .write_relative(&PathBuf::from(&relative_path), &markdown)
        .map_err(|err| err.to_string())?;

    let npc_row = db::NpcRow {
        id: input.id.trim().to_string(),
        slug: slug.clone(),
        name: name.to_string(),
        race: race.to_string(),
        sex,
        location,
        vault_path: relative_path.clone(),
        created_at: created_at.clone(),
        updated_at: now.clone(),
    };

    db::upsert_npc(&database.pool, &npc_row)
        .await
        .map_err(|err| err.to_string())?;
    db::upsert_document_index(
        &database.pool,
        "npc",
        &npc_row.slug,
        Some(&npc_row.name),
        &npc_row.vault_path,
        &npc_row.created_at,
        &npc_row.updated_at,
    )
    .await
    .map_err(|err| err.to_string())?;

    Ok(SaveNpcDraftResult {
        id: npc_row.id,
        slug: npc_row.slug,
        vault_path: npc_row.vault_path,
        created_at: npc_row.created_at,
        updated_at: npc_row.updated_at,
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
            generate_npc_seed,
            ensure_location_exists,
            save_npc_draft,
            get_command_manifest,
            parse_command_input,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
