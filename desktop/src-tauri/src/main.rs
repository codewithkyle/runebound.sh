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
    age: String,
    height: String,
    weight_lbs: String,
    background: String,
    want_need: String,
    secret_obstacle: String,
    carrying: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcRerollContext {
    name: String,
    race: String,
    sex: String,
    age: String,
    height: String,
    weight_lbs: String,
    background: String,
    want_need: String,
    secret_obstacle: String,
    carrying: Vec<String>,
    location: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RerollNpcFieldInput {
    field: String,
    prompt: Option<String>,
    npc: NpcRerollContext,
}

#[derive(Debug, Clone, Serialize)]
struct RerollNpcFieldResult {
    field: String,
    value: Option<String>,
    carrying: Option<Vec<String>>,
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
    age: String,
    height: String,
    weight_lbs: String,
    background: String,
    want_need: String,
    secret_obstacle: String,
    carrying: Vec<String>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum EntityType {
    Npc,
    Location,
}

#[derive(Debug, Clone, Serialize)]
struct EntitySuggestion {
    entity_type: EntityType,
    name: String,
    slug: String,
}

#[derive(Debug, Clone, Serialize)]
struct EntityDetails {
    id: String,
    entity_type: EntityType,
    name: String,
    slug: String,
    race: Option<String>,
    sex: Option<String>,
    age: Option<String>,
    height: Option<String>,
    weight_lbs: Option<String>,
    background: Option<String>,
    want_need: Option<String>,
    secret_obstacle: Option<String>,
    carrying: Option<Vec<String>>,
    location: Option<String>,
    vault_path: String,
    created_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SaveLocationDraftInput {
    id: String,
    name: String,
    slug: String,
    vault_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct SaveLocationDraftResult {
    id: String,
    slug: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
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

fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();

    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

fn parse_carrying_csv(value: &str) -> Vec<String> {
    let items: Vec<String> = value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    normalize_unknown_list(items)
}

fn carrying_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

fn carrying_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_unknown_list(items),
        Err(_) => parse_carrying_csv(value),
    }
}

fn canonical_npc_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "race" => "race",
        "sex" => "sex",
        "age" => "age",
        "height" => "height",
        "weight" | "weight_lbs" => "weight_lbs",
        "background" => "background",
        "want" | "need" | "want_need" => "want_need",
        "secret" | "obstacle" | "secret_obstacle" => "secret_obstacle",
        "carrying" => "carrying",
        "location" => {
            return Err("npc reroll location is not supported; use npc travel to <location>".to_string())
        }
        _ => {
            return Err(format!(
                "unknown npc reroll field: {}. valid fields: name, race, sex, age, height, weight, background, want, secret, carrying",
                raw
            ))
        }
    };

    Ok(field)
}

fn npc_context_summary(context: &NpcRerollContext) -> String {
    format!(
        "name={}, race={}, sex={}, age={}, height={}, weight_lbs={}, background={}, want_need={}, secret_obstacle={}, carrying={}, location={}",
        context.name,
        context.race,
        context.sex,
        context.age,
        context.height,
        context.weight_lbs,
        context.background,
        context.want_need,
        context.secret_obstacle,
        context.carrying.join(", "),
        context.location
    )
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
        "required": ["name", "race", "sex", "age", "height", "weight_lbs", "background", "want_need", "secret_obstacle", "carrying"],
        "properties": {
            "name": { "type": "string", "minLength": 1 },
            "race": { "type": "string", "minLength": 1 },
            "sex": { "type": "string", "enum": ["male", "female"] },
            "age": { "type": "string", "minLength": 1 },
            "height": { "type": "string", "minLength": 1 },
            "weight_lbs": { "type": "string", "minLength": 1 },
            "background": { "type": "string", "minLength": 1 },
            "want_need": { "type": "string", "minLength": 1 },
            "secret_obstacle": { "type": "string", "minLength": 1 },
            "carrying": {
                "type": "array",
                "minItems": 1,
                "items": { "type": "string", "minLength": 1 }
            }
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
                        "You generate concise D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying. Background must be 1-3 coherent sentences. carrying must be an array of item strings. Age should be years, height should be imperial like 5'11\", weight_lbs should be lbs as text like 180. Avoid these recent seeds: {}.{}",
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
        seed.age = normalize_unknown_text(&seed.age);
        seed.height = normalize_unknown_text(&seed.height);
        seed.weight_lbs = normalize_unknown_text(&seed.weight_lbs);
        seed.background = normalize_unknown_text(&seed.background);
        seed.want_need = normalize_unknown_text(&seed.want_need);
        seed.secret_obstacle = normalize_unknown_text(&seed.secret_obstacle);
        seed.carrying = normalize_unknown_list(seed.carrying);

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
async fn reroll_npc_field(
    input: RerollNpcFieldInput,
    state: tauri::State<'_, AppState>,
) -> Result<RerollNpcFieldResult, String> {
    let field = canonical_npc_reroll_field(&input.field)?;
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let config = loaded.effective;
    let model = config
        .ollama
        .model
        .clone()
        .ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

    let extra_prompt = input
        .prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("");

    let context_summary = npc_context_summary(&input.npc);
    let field_instructions = match field {
        "name" => "Generate a single fitting fantasy NPC name.",
        "race" => "Generate a fitting fantasy race for this NPC.",
        "sex" => "Generate sex as exactly male or female.",
        "age" => "Generate a concise age value (typically in years).",
        "height" => "Generate a height in imperial format like 5'11\".",
        "weight_lbs" => "Generate a weight in lbs as text, for example 185.",
        "background" => "Generate a coherent background in 1-3 sentences.",
        "want_need" => "Generate one concise Want.",
        "secret_obstacle" => "Generate one concise Secret.",
        "carrying" => "Generate a carrying list as practical comma-like item strings.",
        _ => "Generate a concise field value.",
    };

    let schema = if field == "carrying" {
        serde_json::json!({
            "type": "object",
            "required": ["carrying"],
            "properties": {
                "carrying": {
                    "type": "array",
                    "minItems": 1,
                    "items": { "type": "string", "minLength": 1 }
                }
            },
            "additionalProperties": false
        })
    } else if field == "sex" {
        serde_json::json!({
            "type": "object",
            "required": ["value"],
            "properties": {
                "value": { "type": "string", "enum": ["male", "female"] }
            },
            "additionalProperties": false
        })
    } else {
        serde_json::json!({
            "type": "object",
            "required": ["value"],
            "properties": {
                "value": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        })
    };

    let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.ollama.timeout_seconds))
        .build()
        .map_err(|err| err.to_string())?;

    for attempt in 0..4 {
        let base_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as i64)
            .unwrap_or(0);
        let run_seed = (base_seed + i64::from(attempt)) as i32;

        let payload = serde_json::json!({
            "model": model,
            "stream": false,
            "format": schema,
            "options": {
                "temperature": 1.05,
                "top_p": 0.92,
                "repeat_penalty": 1.12,
                "seed": run_seed
            },
            "messages": [
                {
                    "role": "system",
                    "content": "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context."
                },
                {
                    "role": "user",
                    "content": format!(
                        "NPC context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
                        context_summary,
                        field,
                        field_instructions,
                        if extra_prompt.is_empty() { "(none)" } else { extra_prompt }
                    )
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

        let parsed: serde_json::Value = match serde_json::from_str(content) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if field == "carrying" {
            let Some(items) = parsed.get("carrying").and_then(|item| item.as_array()) else {
                continue;
            };
            let next = normalize_unknown_list(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|value| value.to_string()))
                    .collect(),
            );
            if attempt < 3 && next == normalize_unknown_list(input.npc.carrying.clone()) {
                continue;
            }
            return Ok(RerollNpcFieldResult {
                field: field.to_string(),
                value: None,
                carrying: Some(next),
            });
        }

        let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
            continue;
        };
        let normalized = if field == "sex" {
            normalize_sex(raw_value)?
        } else {
            normalize_unknown_text(raw_value)
        };

        let current = match field {
            "name" => input.npc.name.clone(),
            "race" => input.npc.race.clone(),
            "sex" => input.npc.sex.clone(),
            "age" => input.npc.age.clone(),
            "height" => input.npc.height.clone(),
            "weight_lbs" => input.npc.weight_lbs.clone(),
            "background" => input.npc.background.clone(),
            "want_need" => input.npc.want_need.clone(),
            "secret_obstacle" => input.npc.secret_obstacle.clone(),
            _ => String::new(),
        };

        if attempt < 3 && normalized.eq_ignore_ascii_case(current.trim()) {
            continue;
        }

        return Ok(RerollNpcFieldResult {
            field: field.to_string(),
            value: Some(normalized),
            carrying: None,
        });
    }

    Err(format!("failed to reroll npc field: {}", field))
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
    let age = normalize_unknown_text(&input.age);
    let height = normalize_unknown_text(&input.height);
    let weight_lbs = normalize_unknown_text(&input.weight_lbs);
    let background = normalize_unknown_text(&input.background);
    let want_need = normalize_unknown_text(&input.want_need);
    let secret_obstacle = normalize_unknown_text(&input.secret_obstacle);
    let carrying = normalize_unknown_list(input.carrying);
    let carrying_db = carrying_to_db_text(&carrying)?;
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

    let (slug, relative_path, created_at, previous_path) = if let Some(current) = existing {
        let desired_base_slug = slugify(name);
        if desired_base_slug == current.slug {
            (
                current.slug,
                current.vault_path,
                current.created_at,
                None,
            )
        } else {
            let next_slug = unique_slug_for_dir(vault.root(), "npcs", &desired_base_slug);
            let next_path = PathBuf::from("npcs")
                .join(format!("{next_slug}.md"))
                .to_string_lossy()
                .to_string();
            (
                next_slug,
                next_path,
                current.created_at,
                Some(current.vault_path),
            )
        }
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
            None,
        )
    };

    let markdown = render_npc_markdown(&NpcFrontmatter {
        doc_type: "npc".to_string(),
        id: input.id.trim().to_string(),
        slug: slug.clone(),
        name: name.to_string(),
        race: race.to_string(),
        sex: sex.clone(),
        age: age.clone(),
        height: height.clone(),
        weight_lbs: weight_lbs.clone(),
        background: background.clone(),
        want_need: want_need.clone(),
        secret_obstacle: secret_obstacle.clone(),
        carrying: carrying.clone(),
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
        age,
        height,
        weight_lbs,
        background,
        want_need,
        secret_obstacle,
        carrying: carrying_db,
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

    if let Some(old_path) = previous_path {
        if old_path != npc_row.vault_path {
            db::delete_document_by_vault_path(&database.pool, &old_path)
                .await
                .map_err(|err| err.to_string())?;

            if let Ok(old_full_path) = vault.resolve_relative(&PathBuf::from(&old_path)) {
                if old_full_path.exists() {
                    std::fs::remove_file(&old_full_path).map_err(|err| {
                        format!(
                            "failed to remove old npc file {}: {}",
                            old_full_path.display(),
                            err
                        )
                    })?;
                }
            }
        }
    }

    Ok(SaveNpcDraftResult {
        id: npc_row.id,
        slug: npc_row.slug,
        vault_path: npc_row.vault_path,
        created_at: npc_row.created_at,
        updated_at: npc_row.updated_at,
    })
}

#[tauri::command]
async fn save_location_draft(
    input: SaveLocationDraftInput,
    state: tauri::State<'_, AppState>,
) -> Result<SaveLocationDraftResult, String> {
    if input.id.trim().is_empty() {
        return Err("location id cannot be empty".to_string());
    }

    let name = input.name.trim();
    if name.is_empty() {
        return Err("location name cannot be empty".to_string());
    }

    let slug = input.slug.trim();
    if slug.is_empty() {
        return Err("location slug cannot be empty".to_string());
    }

    let vault_path_relative = input.vault_path.trim();
    if vault_path_relative.is_empty() {
        return Err("location vault path cannot be empty".to_string());
    }

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
    let existing = db::find_location_by_id(&database.pool, input.id.trim())
        .await
        .map_err(|err| err.to_string())?;
    let created_at = existing
        .map(|location| location.created_at)
        .unwrap_or_else(|| now.clone());

    let markdown = render_location_markdown(&LocationFrontmatter {
        doc_type: "location".to_string(),
        id: input.id.trim().to_string(),
        slug: slug.to_string(),
        name: name.to_string(),
        created_at: created_at.clone(),
        updated_at: now.clone(),
    })
    .map_err(|err| err.to_string())?;

    let relative_path = PathBuf::from(vault_path_relative);
    vault
        .write_relative(&relative_path, &markdown)
        .map_err(|err| err.to_string())?;

    let location_row = db::LocationRow {
        id: input.id.trim().to_string(),
        slug: slug.to_string(),
        name: name.to_string(),
        vault_path: vault_path_relative.to_string(),
        created_at: created_at.clone(),
        updated_at: now.clone(),
    };

    db::upsert_location(&database.pool, &location_row)
        .await
        .map_err(|err| err.to_string())?;
    db::upsert_document_index(
        &database.pool,
        "location",
        &location_row.slug,
        Some(&location_row.name),
        &location_row.vault_path,
        &location_row.created_at,
        &location_row.updated_at,
    )
    .await
    .map_err(|err| err.to_string())?;

    Ok(SaveLocationDraftResult {
        id: location_row.id,
        slug: location_row.slug,
        vault_path: location_row.vault_path,
        created_at: location_row.created_at,
        updated_at: location_row.updated_at,
    })
}

#[tauri::command]
async fn search_entities(query: String, limit: Option<u32>) -> Result<Vec<EntitySuggestion>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let limit = i64::from(limit.unwrap_or(8)).clamp(1, 20);
    let database = db::init_database().await.map_err(|err| err.to_string())?;

    let npcs = db::search_npcs_by_name(&database.pool, trimmed, limit)
        .await
        .map_err(|err| err.to_string())?;
    let locations = db::search_locations_by_name(&database.pool, trimmed, limit)
        .await
        .map_err(|err| err.to_string())?;

    let mut items: Vec<EntitySuggestion> = npcs
        .into_iter()
        .map(|npc| EntitySuggestion {
            entity_type: EntityType::Npc,
            name: npc.name,
            slug: npc.slug,
        })
        .chain(locations.into_iter().map(|location| EntitySuggestion {
            entity_type: EntityType::Location,
            name: location.name,
            slug: location.slug,
        }))
        .collect();

    items.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    items.truncate(limit as usize);
    Ok(items)
}

#[tauri::command]
async fn resolve_entity(input: String) -> Result<Option<EntityDetails>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let database = db::init_database().await.map_err(|err| err.to_string())?;
    if let Some(npc) = db::find_npc_by_name_or_slug(&database.pool, trimmed)
        .await
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(EntityDetails {
            id: npc.id,
            entity_type: EntityType::Npc,
            name: npc.name,
            slug: npc.slug,
            race: Some(npc.race),
            sex: Some(npc.sex),
            age: Some(npc.age),
            height: Some(npc.height),
            weight_lbs: Some(npc.weight_lbs),
            background: Some(npc.background),
            want_need: Some(npc.want_need),
            secret_obstacle: Some(npc.secret_obstacle),
            carrying: Some(carrying_from_db_text(&npc.carrying)),
            location: Some(npc.location),
            vault_path: npc.vault_path,
            created_at: Some(npc.created_at),
        }));
    }

    if let Some(location) = db::find_location_by_name_or_slug(&database.pool, trimmed)
        .await
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(EntityDetails {
            id: location.id,
            entity_type: EntityType::Location,
            name: location.name,
            slug: location.slug,
            race: None,
            sex: None,
            age: None,
            height: None,
            weight_lbs: None,
            background: None,
            want_need: None,
            secret_obstacle: None,
            carrying: None,
            location: None,
            vault_path: location.vault_path,
            created_at: Some(location.created_at),
        }));
    }

    Ok(None)
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
            reroll_npc_field,
            ensure_location_exists,
            save_npc_draft,
            save_location_draft,
            search_entities,
            resolve_entity,
            get_command_manifest,
            parse_command_input,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
