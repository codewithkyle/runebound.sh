#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod router;

use std::path::PathBuf;
use std::time::Duration;

use dnd_core::command::{CommandClientEvent, CommandResponse};
use dnd_core::command_manifest::{CommandManifest, CommandSpec};
use dnd_core::command_parse::{ParseResult, ParseStage, normalize_command_input, parse_command_input};
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::db;
use dnd_core::npc::{
    LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp,
    merge_runebound_block, normalize_markdown_file_stem, render_location_markdown,
    render_npc_markdown, slugify,
    unique_slug_for_dir,
};
use dnd_core::vault::Vault;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::app_state::{AppState, EditorSession};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcSeed {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcRerollContext {
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

impl EntityType {
    fn as_str(&self) -> &'static str {
        match self {
            EntityType::Npc => "npc",
            EntityType::Location => "location",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct EntitySuggestion {
    entity_type: EntityType,
    name: String,
    slug: String,
}

#[derive(Debug, Clone, Serialize)]
struct CommandSuggestion {
    label: String,
    completion: String,
    helper_text: Option<SuggestionHelperText>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum SuggestionHelperText {
    Command,
    Npc,
    Location,
}

#[derive(Debug, Clone, Serialize)]
struct EntityDetails {
    id: String,
    entity_type: EntityType,
    name: String,
    slug: String,
    race: Option<String>,
    occupation: Option<String>,
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

#[derive(Debug, Clone, Deserialize)]
struct SoftDeleteEntityInput {
    target: String,
}

#[derive(Debug, Clone, Serialize)]
struct SoftDeleteEntityResult {
    entity_type: EntityType,
    id: String,
    name: String,
    slug: String,
    trash_vault_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct UndoSoftDeleteResult {
    entity_type: EntityType,
    id: String,
    name: String,
    slug: String,
    vault_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcDeletePayload {
    id: String,
    slug: String,
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
    carrying: String,
    location: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocationDeletePayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
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

fn read_vault_file_if_exists(vault: &Vault, relative_path: &str) -> Result<Option<String>, String> {
    let relative = PathBuf::from(relative_path);
    let full = vault.resolve_relative(&relative).map_err(|err| err.to_string())?;
    if !full.exists() {
        return Ok(None);
    }

    std::fs::read_to_string(&full)
        .map(Some)
        .map_err(|err| format!("failed to read vault file {}: {}", full.display(), err))
}

fn unique_trash_path(vault: &Vault, entity_dir: &str, slug: &str, timestamp: &str) -> Result<String, String> {
    let base = format!("{}-{}", slug, timestamp.replace(':', "").replace('-', ""));
    let mut candidate = format!(".trash/{entity_dir}/{base}.md");
    let mut index = 2;

    loop {
        let full = vault
            .resolve_relative(&PathBuf::from(&candidate))
            .map_err(|err| err.to_string())?;
        if !full.exists() {
            return Ok(candidate);
        }
        candidate = format!(".trash/{entity_dir}/{base}-{index}.md");
        index += 1;
    }
}

fn move_vault_file(vault: &Vault, source_relative: &str, target_relative: &str) -> Result<(), String> {
    let source_full = vault
        .resolve_relative(&PathBuf::from(source_relative))
        .map_err(|err| err.to_string())?;
    if !source_full.exists() {
        return Err(format!(
            "source file does not exist: {}",
            source_full.display()
        ));
    }

    let target_full = vault
        .resolve_relative(&PathBuf::from(target_relative))
        .map_err(|err| err.to_string())?;
    if let Some(parent) = target_full.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create trash directory {}: {}", parent.display(), err))?;
    }

    std::fs::rename(&source_full, &target_full).map_err(|err| {
        format!(
            "failed to move file from {} to {}: {}",
            source_full.display(),
            target_full.display(),
            err
        )
    })
}

fn unique_markdown_path_for_name(
    vault: &Vault,
    relative_dir: &str,
    display_name: &str,
    keep_path: Option<&str>,
) -> Result<String, String> {
    let base = normalize_markdown_file_stem(display_name);
    let mut candidate = base.clone();
    let mut index = 2;

    loop {
        let relative = PathBuf::from(relative_dir)
            .join(format!("{candidate}.md"))
            .to_string_lossy()
            .to_string();

        if keep_path.is_some_and(|existing| existing == relative) {
            return Ok(relative);
        }

        let full = vault
            .resolve_relative(&PathBuf::from(&relative))
            .map_err(|err| err.to_string())?;
        if !full.exists() {
            return Ok(relative);
        }

        candidate = format!("{base} {index}");
        index += 1;
    }
}

fn canonical_npc_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "race" => "race",
        "occupation" => "occupation",
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
                "unknown npc reroll field: {}. valid fields: name, race, occupation, sex, age, height, weight, background, want, secret, carrying",
                raw
            ))
        }
    };

    Ok(field)
}

fn npc_context_summary(context: &NpcRerollContext) -> String {
    format!(
        "name={}, race={}, occupation={}, sex={}, age={}, height={}, weight_lbs={}, background={}, want_need={}, secret_obstacle={}, carrying={}, location={}",
        context.name,
        context.race,
        context.occupation,
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
async fn suggest_command_input(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CommandSuggestion>, String> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }

    let manifest = dnd_core::command_manifest::command_manifest();
    let parsed = dnd_core::command_parse::parse_command_input(&input);
    let mut suggestions = build_command_suggestions(&manifest, &parsed, &input);

    let mode = {
        let editor = state.editor_session.lock().await;
        editor.mode
    };

    suggestions.retain(|suggestion| {
        let completion = suggestion.completion.trim().to_ascii_lowercase();
        let label = suggestion.label.trim().to_ascii_lowercase();

        if mode != app_state::EditorMode::Npc {
            if completion == "npc"
                || completion.starts_with("npc ")
                || label == "npc"
                || label.starts_with("npc ")
            {
                return false;
            }
            if completion == "reroll" || label == "reroll" {
                return false;
            }
        }

        if mode != app_state::EditorMode::Location
            && (completion == "location"
                || completion.starts_with("location ")
                || label == "location"
                || label.starts_with("location "))
        {
            return false;
        }

        if mode == app_state::EditorMode::None && (completion == "cancel" || label == "cancel") {
            return false;
        }

        true
    });

    let trimmed = input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let is_load_context = lowered == "load" || lowered.starts_with("load ");
    let is_delete_context = lowered == "delete" || lowered.starts_with("delete ");
    let search_query = if is_load_context {
        trimmed[4..].trim()
    } else if is_delete_context {
        trimmed[6..].trim()
    } else {
        trimmed
    };

    if !search_query.is_empty()
        && (is_load_context
            || is_delete_context
            || !starts_with_known_command_root(trimmed, &manifest))
    {
        let entity_results = search_entities(search_query.to_string(), Some(6)).await?;
        let prefix = if is_load_context {
            Some("load")
        } else if is_delete_context {
            Some("delete")
        } else {
            None
        };

        for entity in entity_results {
            let completion = match prefix {
                Some(value) => format!("{value} {}", entity.name),
                None => entity.name.clone(),
            };
            suggestions.push(CommandSuggestion {
                label: entity.name,
                completion,
                helper_text: Some(match entity.entity_type {
                    EntityType::Npc => SuggestionHelperText::Npc,
                    EntityType::Location => SuggestionHelperText::Location,
                }),
            });
        }
    }

    Ok(suggestions)
}

fn build_command_suggestions(
    manifest: &CommandManifest,
    parsed: &ParseResult,
    input: &str,
) -> Vec<CommandSuggestion> {
    if matches!(parsed.completion.stage, ParseStage::Root) {
        return build_root_suggestions(manifest, &parsed.completion.current_token);
    }

    if matches!(parsed.completion.stage, ParseStage::Subcommand) {
        return build_subcommand_suggestions(
            manifest,
            parsed.completion.root.as_deref(),
            input,
            &parsed.completion.current_token,
        );
    }

    build_argument_suggestions(manifest, parsed, input)
}

fn build_root_suggestions(manifest: &CommandManifest, token: &str) -> Vec<CommandSuggestion> {
    let prefix = token.to_ascii_lowercase();
    manifest
        .commands
        .iter()
        .filter(|cmd| cmd.show_in_autocomplete)
        .filter(|cmd| cmd.name.starts_with(&prefix))
        .map(|cmd| CommandSuggestion {
            label: cmd.name.clone(),
            completion: format!("{}{}", cmd.name, completion_suffix(cmd)),
            helper_text: Some(SuggestionHelperText::Command),
        })
        .collect()
}

fn build_subcommand_suggestions(
    manifest: &CommandManifest,
    root: Option<&str>,
    input: &str,
    token: &str,
) -> Vec<CommandSuggestion> {
    let Some(root) = root else {
        return Vec::new();
    };
    let Some(command) = find_command(manifest, root) else {
        return Vec::new();
    };

    let prefix = token.to_ascii_lowercase();
    let base = replace_current_token(input, token);
    command
        .subcommands
        .iter()
        .filter(|subcommand| subcommand.name.starts_with(&prefix))
        .map(|subcommand| CommandSuggestion {
            label: format!("{} {}", command.name, subcommand.name),
            completion: format!("{base}{} ", subcommand.name),
            helper_text: Some(SuggestionHelperText::Command),
        })
        .collect()
}

fn build_argument_suggestions(
    manifest: &CommandManifest,
    parsed: &ParseResult,
    input: &str,
) -> Vec<CommandSuggestion> {
    let Some(root) = parsed.completion.root.as_deref() else {
        return Vec::new();
    };
    let Some(command) = find_command(manifest, root) else {
        return Vec::new();
    };

    let subcommand = parsed
        .completion
        .subcommand
        .as_ref()
        .and_then(|item| command.subcommands.iter().find(|sub| sub.name == *item));

    if command.name == "npc" && subcommand.is_some_and(|item| item.name == "travel") {
        let normalized: Vec<String> = parsed
            .normalized_tokens
            .iter()
            .map(|token| token.to_ascii_lowercase())
            .collect();
        let has_to = normalized.len() >= 3 && normalized[2] == "to";
        if !has_to {
            return vec![CommandSuggestion {
                label: "npc travel to".to_string(),
                completion: "npc travel to ".to_string(),
                helper_text: Some(SuggestionHelperText::Command),
            }];
        }
    }

    if command.name == "npc"
        && subcommand.is_some_and(|item| item.name == "set" || item.name == "reroll")
    {
        let field_names = [
            "name",
            "race",
            "occupation",
            "sex",
            "age",
            "height",
            "weight",
            "background",
            "want",
            "secret",
            "carrying",
        ];
        let args = &parsed.normalized_tokens[2..];
        let should_suggest_fields =
            args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);

        if should_suggest_fields {
            let prefix = parsed.completion.current_token.to_ascii_lowercase();
            let base = replace_current_token(input, &parsed.completion.current_token);
            let prefix_label = if subcommand.is_some_and(|item| item.name == "set") {
                "npc set"
            } else {
                "npc reroll"
            };

            return field_names
                .iter()
                .filter(|field| field.starts_with(&prefix))
                .map(|field| CommandSuggestion {
                    label: format!("{prefix_label} {field}"),
                    completion: format!("{base}{field} "),
                    helper_text: Some(SuggestionHelperText::Command),
                })
                .collect();
        }
    }

    let options = match subcommand {
        Some(item) => &item.options,
        None => &command.options,
    };
    if options.is_empty() {
        return Vec::new();
    }

    let current = parsed.completion.current_token.to_ascii_lowercase();
    let used: std::collections::HashSet<String> = parsed
        .normalized_tokens
        .iter()
        .filter(|token| token.starts_with('-'))
        .cloned()
        .collect();
    let base = replace_current_token(input, &parsed.completion.current_token);
    let should_filter_prefix = current.starts_with('-') || !current.is_empty();

    options
        .iter()
        .filter(|option| !used.contains(&option.name) || option.takes_value)
        .filter(|option| !should_filter_prefix || option.name.starts_with(&current))
        .map(|option| {
            let label = match subcommand {
                Some(item) => format!("{} {} {}", command.name, item.name, option.name),
                None => format!("{} {}", command.name, option.name),
            };
            let suffix = if option.takes_value { " " } else { "" };
            CommandSuggestion {
                label,
                completion: format!("{base}{}{suffix}", option.name),
                helper_text: Some(SuggestionHelperText::Command),
            }
        })
        .collect()
}

fn find_command<'a>(manifest: &'a CommandManifest, root: &str) -> Option<&'a CommandSpec> {
    let normalized = root.to_ascii_lowercase();
    manifest
        .commands
        .iter()
        .find(|command| command.name == normalized)
}

fn replace_current_token(input: &str, current_token: &str) -> String {
    if current_token.is_empty() {
        return input.to_string();
    }

    let suffix_len = current_token.len();
    if input.len() < suffix_len {
        return input.to_string();
    }

    input[..input.len() - suffix_len].to_string()
}

fn completion_suffix(command: &CommandSpec) -> &'static str {
    if !command.subcommands.is_empty() || !command.options.is_empty() || command.requires_subcommand
    {
        " "
    } else {
        ""
    }
}

fn starts_with_known_command_root(input: &str, manifest: &CommandManifest) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    let Some(first) = trimmed.split_whitespace().next() else {
        return false;
    };
    let lowered = first.to_ascii_lowercase();
    manifest.commands.iter().any(|command| command.name == lowered)
}

#[tauri::command]
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommandResponse, String> {
    let normalized_input = normalize_input_for_dispatch(&input);

    if let Some(response) =
        router::run_desktop_routed_command(&normalized_input, state.clone()).await?
    {
        let skip_history_push = matches!(
            response.client_event,
            Some(CommandClientEvent::ClearTerminal {
                clear_history: true
            })
        );
        if !skip_history_push {
            let trimmed = normalized_input.trim();
            if !trimmed.is_empty() {
                let mut service = state.command_service.lock().await;
                service.session_mut().push_history(trimmed, 50);
            }
        }
        return Ok(response);
    }

    let mut service = state.command_service.lock().await;
    Ok(service.execute_line(&normalized_input).await)
}

fn normalize_input_for_dispatch(input: &str) -> String {
    let normalized = normalize_command_input(input);
    let parsed = parse_command_input(&normalized);
    if parsed.canonical_input.is_empty() {
        normalized
    } else {
        parsed.canonical_input
    }
}

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
        "required": ["name", "race", "occupation", "sex", "age", "height", "weight_lbs", "background", "want_need", "secret_obstacle", "carrying"],
        "properties": {
            "name": { "type": "string", "minLength": 1 },
            "race": { "type": "string", "minLength": 1 },
            "occupation": { "type": "string", "minLength": 1 },
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
                        "You generate concise D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying. Background must be 1-3 coherent sentences. carrying must be an array of item strings. Age should be years, height should be imperial like 5'11\", weight_lbs should be lbs as text like 180. Avoid these recent seeds: {}.{}",
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
        seed.occupation = normalize_unknown_text(&seed.occupation);
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
        "occupation" => "Generate one concise occupation for this NPC.",
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
            "occupation" => input.npc.occupation.clone(),
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

    let database = db::init_database().await.map_err(|err| err.to_string())?;
    let slug = slugify(raw_name);
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

    let relative_path = if let Some(row) = existing.as_ref() {
        row.vault_path.clone()
    } else {
        unique_markdown_path_for_name(&vault, "locations", &canonical_name, None)?
    };
    let file_exists = vault
        .resolve_relative(&PathBuf::from(&relative_path))
        .map_err(|err| err.to_string())?
        .exists();

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
            .write_relative(&PathBuf::from(&relative_path), &content)
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
        vault_path: relative_path,
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
    let occupation = normalize_unknown_text(&input.occupation);
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
        let desired_path = unique_markdown_path_for_name(
            &vault,
            "npcs",
            name,
            Some(current.vault_path.as_str()),
        )?;

        if desired_base_slug == current.slug {
            (
                current.slug,
                desired_path.clone(),
                current.created_at,
                if desired_path == current.vault_path {
                    None
                } else {
                    Some(current.vault_path)
                },
            )
        } else {
            let next_slug = unique_slug_for_dir(vault.root(), "npcs", &desired_base_slug);
            (
                next_slug,
                desired_path,
                current.created_at,
                Some(current.vault_path),
            )
        }
    } else {
        let base_slug = slugify(name);
        let slug = unique_slug_for_dir(vault.root(), "npcs", &base_slug);
        (
            slug.clone(),
            unique_markdown_path_for_name(&vault, "npcs", name, None)?,
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
        occupation: occupation.clone(),
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

    let existing_markdown = if let Some(ref old_path) = previous_path {
        if old_path != &relative_path {
            match read_vault_file_if_exists(&vault, old_path) {
                Ok(Some(contents)) => Some(contents),
                Ok(None) => read_vault_file_if_exists(&vault, &relative_path)?,
                Err(err) => return Err(err),
            }
        } else {
            read_vault_file_if_exists(&vault, &relative_path)?
        }
    } else {
        read_vault_file_if_exists(&vault, &relative_path)?
    };
    let merged_markdown = match existing_markdown {
        Some(existing) => merge_runebound_block(&existing, &markdown),
        None => markdown,
    };

    vault
        .write_relative(&PathBuf::from(&relative_path), &merged_markdown)
        .map_err(|err| err.to_string())?;

    let npc_row = db::NpcRow {
        id: input.id.trim().to_string(),
        slug: slug.clone(),
        name: name.to_string(),
        race: race.to_string(),
        occupation,
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

    let _legacy_slug_input = input.slug.trim();
    let previous_vault_path_input = input.vault_path.trim();

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
    let (slug, relative_path, created_at, previous_path) = if let Some(current) = existing {
        let desired_base_slug = slugify(name);
        let desired_path = unique_markdown_path_for_name(
            &vault,
            "locations",
            name,
            Some(current.vault_path.as_str()),
        )?;

        if desired_base_slug == current.slug {
            (
                current.slug,
                desired_path.clone(),
                current.created_at,
                if desired_path == current.vault_path {
                    None
                } else {
                    Some(current.vault_path)
                },
            )
        } else {
            (
                unique_slug_for_dir(vault.root(), "locations", &desired_base_slug),
                desired_path,
                current.created_at,
                Some(current.vault_path),
            )
        }
    } else {
        let base_slug = slugify(name);
        (
            unique_slug_for_dir(vault.root(), "locations", &base_slug),
            unique_markdown_path_for_name(&vault, "locations", name, None)?,
            now.clone(),
            if previous_vault_path_input.is_empty() {
                None
            } else {
                Some(previous_vault_path_input.to_string())
            },
        )
    };

    let markdown = render_location_markdown(&LocationFrontmatter {
        doc_type: "location".to_string(),
        id: input.id.trim().to_string(),
        slug: slug.clone(),
        name: name.to_string(),
        created_at: created_at.clone(),
        updated_at: now.clone(),
    })
    .map_err(|err| err.to_string())?;

    let existing_markdown = if let Some(ref old_path) = previous_path {
        if old_path != &relative_path {
            match read_vault_file_if_exists(&vault, old_path) {
                Ok(Some(contents)) => Some(contents),
                Ok(None) => read_vault_file_if_exists(&vault, &relative_path)?,
                Err(err) => return Err(err),
            }
        } else {
            read_vault_file_if_exists(&vault, &relative_path)?
        }
    } else {
        read_vault_file_if_exists(&vault, &relative_path)?
    };
    let merged_markdown = match existing_markdown {
        Some(existing) => merge_runebound_block(&existing, &markdown),
        None => markdown,
    };

    vault
        .write_relative(&PathBuf::from(&relative_path), &merged_markdown)
        .map_err(|err| err.to_string())?;

    let location_row = db::LocationRow {
        id: input.id.trim().to_string(),
        slug,
        name: name.to_string(),
        vault_path: relative_path.clone(),
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

    if let Some(old_path) = previous_path {
        if old_path != location_row.vault_path {
            db::delete_document_by_vault_path(&database.pool, &old_path)
                .await
                .map_err(|err| err.to_string())?;

            if let Ok(old_full_path) = vault.resolve_relative(&PathBuf::from(&old_path)) {
                if old_full_path.exists() {
                    std::fs::remove_file(&old_full_path).map_err(|err| {
                        format!(
                            "failed to remove old location file {}: {}",
                            old_full_path.display(),
                            err
                        )
                    })?;
                }
            }
        }
    }

    Ok(SaveLocationDraftResult {
        id: location_row.id,
        slug: location_row.slug,
        vault_path: location_row.vault_path,
        created_at: location_row.created_at,
        updated_at: location_row.updated_at,
    })
}

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
            occupation: Some(npc.occupation),
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
            occupation: None,
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

async fn soft_delete_entity(
    input: SoftDeleteEntityInput,
    state: tauri::State<'_, AppState>,
) -> Result<SoftDeleteEntityResult, String> {
    let target = input.target.trim();
    if target.is_empty() {
        return Err("usage: delete <npc-or-location-name>".to_string());
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

    if let Some(npc) = db::find_npc_by_name_or_slug(&database.pool, target)
        .await
        .map_err(|err| err.to_string())?
    {
        let trash_path = unique_trash_path(&vault, "npcs", &npc.slug, &now)?;
        move_vault_file(&vault, &npc.vault_path, &trash_path)?;

        db::delete_npc_by_id(&database.pool, &npc.id)
            .await
            .map_err(|err| err.to_string())?;
        db::delete_document_by_vault_path(&database.pool, &npc.vault_path)
            .await
            .map_err(|err| err.to_string())?;

        let payload = NpcDeletePayload {
            id: npc.id.clone(),
            slug: npc.slug.clone(),
            name: npc.name.clone(),
            race: npc.race,
            occupation: npc.occupation,
            sex: npc.sex,
            age: npc.age,
            height: npc.height,
            weight_lbs: npc.weight_lbs,
            background: npc.background,
            want_need: npc.want_need,
            secret_obstacle: npc.secret_obstacle,
            carrying: npc.carrying,
            location: npc.location,
            vault_path: npc.vault_path.clone(),
            created_at: npc.created_at,
            updated_at: npc.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "npc".to_string(),
            entity_id: npc.id.clone(),
            name: npc.name.clone(),
            slug: npc.slug.clone(),
            original_vault_path: npc.vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        db::insert_soft_delete(&database.pool, &soft_delete_row)
            .await
            .map_err(|err| err.to_string())?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Npc,
            id: npc.id,
            name: npc.name,
            slug: npc.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(location) = db::find_location_by_name_or_slug(&database.pool, target)
        .await
        .map_err(|err| err.to_string())?
    {
        let trash_path = unique_trash_path(&vault, "locations", &location.slug, &now)?;
        move_vault_file(&vault, &location.vault_path, &trash_path)?;

        db::delete_location_by_id(&database.pool, &location.id)
            .await
            .map_err(|err| err.to_string())?;
        db::delete_document_by_vault_path(&database.pool, &location.vault_path)
            .await
            .map_err(|err| err.to_string())?;

        let payload = LocationDeletePayload {
            id: location.id.clone(),
            slug: location.slug.clone(),
            name: location.name.clone(),
            vault_path: location.vault_path.clone(),
            created_at: location.created_at,
            updated_at: location.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "location".to_string(),
            entity_id: location.id.clone(),
            name: location.name.clone(),
            slug: location.slug.clone(),
            original_vault_path: location.vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        db::insert_soft_delete(&database.pool, &soft_delete_row)
            .await
            .map_err(|err| err.to_string())?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Location,
            id: location.id,
            name: location.name,
            slug: location.slug,
            trash_vault_path: trash_path,
        });
    }

    Err(format!("no npc or location found for: {target}"))
}

async fn undo_last_soft_delete(state: tauri::State<'_, AppState>) -> Result<UndoSoftDeleteResult, String> {
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
    let Some(soft_delete) = db::latest_pending_soft_delete(&database.pool)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Err("nothing to undo".to_string());
    };

    let now = now_timestamp();

    if soft_delete.entity_type == "npc" {
        let payload: NpcDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = payload.vault_path;
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "npcs", &restored_slug);
            restored_vault_path = unique_markdown_path_for_name(&vault, "npcs", &payload.name, None)?;
        }

        move_vault_file(&vault, &soft_delete.trash_vault_path, &restored_vault_path)?;

        let npc_row = db::NpcRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            race: payload.race,
            occupation: payload.occupation,
            sex: payload.sex,
            age: payload.age,
            height: payload.height,
            weight_lbs: payload.weight_lbs,
            background: payload.background,
            want_need: payload.want_need,
            secret_obstacle: payload.secret_obstacle,
            carrying: payload.carrying,
            location: payload.location,
            vault_path: restored_vault_path.clone(),
            created_at: payload.created_at,
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

        db::mark_soft_delete_undone(&database.pool, soft_delete.id, &now)
            .await
            .map_err(|err| err.to_string())?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Npc,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    if soft_delete.entity_type == "location" {
        let payload: LocationDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = payload.vault_path;
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "locations", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "locations", &payload.name, None)?;
        }

        move_vault_file(&vault, &soft_delete.trash_vault_path, &restored_vault_path)?;

        let location_row = db::LocationRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            created_at: payload.created_at,
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

        db::mark_soft_delete_undone(&database.pool, soft_delete.id, &now)
            .await
            .map_err(|err| err.to_string())?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Location,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    Err(format!(
        "unsupported soft delete entity type: {}",
        soft_delete.entity_type
    ))
}

#[tauri::command]
fn get_command_manifest() -> CommandManifest {
    dnd_core::command_manifest::command_manifest()
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let command_service = dnd_core::service::CommandService::new(workspace_root.clone());

    tauri::Builder::default()
        .manage(AppState {
            workspace_root,
            command_service: Mutex::new(command_service),
            editor_session: Mutex::new(EditorSession::default()),
        })
        .invoke_handler(tauri::generate_handler![
            run_command,
            suggest_command_input,
            get_command_manifest,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
