#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

use dnd_core::command::CommandResponse;
use dnd_core::command::{CommandClientEvent, OutputSegment, OutputSegmentKind};
use dnd_core::command_manifest::CommandManifest;
use dnd_core::command_parse::ParseResult;
use dnd_core::config::{load_effective, required_issues, save_config, validate_for_runtime};
use dnd_core::db;
use dnd_core::health;
use dnd_core::npc::{
    LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp,
    merge_runebound_block, normalize_markdown_file_stem, render_location_markdown,
    render_npc_markdown, slugify,
    unique_slug_for_dir,
};
use dnd_core::vault::{Vault, is_path_writable};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

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

struct AppState {
    workspace_root: PathBuf,
    command_service: Mutex<dnd_core::service::CommandService>,
    editor_session: Mutex<EditorSession>,
}

#[derive(Debug, Clone)]
struct NpcDraftSession {
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

#[derive(Debug, Clone)]
struct LocationDraftSession {
    id: String,
    name: String,
    slug: String,
    vault_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorMode {
    None,
    Npc,
    Location,
}

#[derive(Debug, Default)]
struct EditorSession {
    mode: EditorMode,
    npc_draft: Option<NpcDraftSession>,
    location_draft: Option<LocationDraftSession>,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::None
    }
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
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommandResponse, String> {
    if let Some(response) = run_desktop_routed_command(&input, state.clone()).await? {
        return Ok(response);
    }

    let mut service = state.command_service.lock().await;
    Ok(service.execute_line(&input).await)
}

async fn run_desktop_routed_command(
    input: &str,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommandResponse>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "create help" || lowered == "create --help" {
        return Ok(Some(ok_response(
            ["## Create commands", "create npc", "create npc <prompt text>"].join("\n"),
            None,
        )));
    }

    if lowered == "create npc" || lowered.starts_with("create npc ") {
        let prompt = if trimmed.len() > 10 {
            let value = trimmed[10..].trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        };

        let seed = generate_npc_seed(GenerateNpcSeedInput { prompt }, state.clone()).await?;
        let draft = NpcDraftSession {
            id: make_entity_id("npc"),
            name: seed.name.trim().to_string(),
            race: seed.race.trim().to_string(),
            occupation: normalize_unknown_text(&seed.occupation),
            sex: normalize_sex(&seed.sex)?,
            age: normalize_unknown_text(&seed.age),
            height: normalize_unknown_text(&seed.height),
            weight_lbs: normalize_unknown_text(&seed.weight_lbs),
            background: normalize_unknown_text(&seed.background),
            want_need: normalize_unknown_text(&seed.want_need),
            secret_obstacle: normalize_unknown_text(&seed.secret_obstacle),
            carrying: normalize_unknown_list(seed.carrying),
            location: UNKNOWN_LOCATION.to_string(),
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.location_draft = None;
            editor.npc_draft = Some(draft.clone());
        }

        let output = format!(
            "## NPC Draft\nname: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}",
            draft.name,
            draft.race,
            draft.occupation,
            draft.sex,
            draft.age,
            draft.height,
            draft.weight_lbs,
            draft.background,
            draft.want_need,
            draft.secret_obstacle,
            draft.carrying.join(", "),
            draft.location,
        );

        return Ok(Some(ok_response(output, Some(npc_event_from_draft(&draft)))));
    }

    if lowered == "npc help" || lowered == "npc --help" {
        let has_draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.is_some()
        };
        if !has_draft {
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            [
                "## NPC editor commands",
                "npc show",
                "npc rename <name>",
                "npc set <field> <value>",
                "npc travel to <location>",
                "npc reroll <field> [prompt]",
                "reroll",
                "npc save",
                "npc cancel",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "location help" || lowered == "location --help" {
        let has_draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.is_some()
        };
        if !has_draft {
            return Ok(Some(ok_response(
                "no active location draft. run load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            [
                "## Location editor commands",
                "location show",
                "location rename <name>",
                "location save",
                "location cancel",
            ]
            .join("\n"),
            None,
        )));
    }

    if lowered == "npc show" {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        };
        let Some(draft) = draft else {
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        };
        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered == "location show" {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.clone()
        };
        let Some(draft) = draft else {
            return Ok(Some(ok_response(
                "no active location draft. run load <name>.".to_string(),
                None,
            )));
        };
        return Ok(Some(ok_response(
            location_summary_text(&draft),
            Some(location_event_from_draft(&draft)),
        )));
    }

    if lowered == "npc cancel" {
        let had_draft = {
            let mut editor = state.editor_session.lock().await;
            let had = editor.npc_draft.is_some();
            if had {
                editor.npc_draft = None;
                editor.mode = if editor.location_draft.is_some() {
                    EditorMode::Location
                } else {
                    EditorMode::None
                };
            }
            had
        };
        if !had_draft {
            return Ok(Some(ok_response(
                "no active npc draft. run create npc or load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            "npc draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    if lowered == "location cancel" {
        let had_draft = {
            let mut editor = state.editor_session.lock().await;
            let had = editor.location_draft.is_some();
            if had {
                editor.location_draft = None;
                editor.mode = if editor.npc_draft.is_some() {
                    EditorMode::Npc
                } else {
                    EditorMode::None
                };
            }
            had
        };
        if !had_draft {
            return Ok(Some(ok_response(
                "no active location draft. run load <name>.".to_string(),
                None,
            )));
        }
        return Ok(Some(ok_response(
            "location draft discarded.".to_string(),
            Some(CommandClientEvent::ClearDrafts),
        )));
    }

    if lowered == "cancel" {
        let mode = {
            let editor = state.editor_session.lock().await;
            editor.mode
        };
        if mode == EditorMode::Npc {
            let mut editor = state.editor_session.lock().await;
            editor.npc_draft = None;
            editor.mode = if editor.location_draft.is_some() {
                EditorMode::Location
            } else {
                EditorMode::None
            };
            return Ok(Some(ok_response(
                "npc draft discarded.".to_string(),
                Some(CommandClientEvent::ClearDrafts),
            )));
        }
        if mode == EditorMode::Location {
            let mut editor = state.editor_session.lock().await;
            editor.location_draft = None;
            editor.mode = if editor.npc_draft.is_some() {
                EditorMode::Npc
            } else {
                EditorMode::None
            };
            return Ok(Some(ok_response(
                "location draft discarded.".to_string(),
                Some(CommandClientEvent::ClearDrafts),
            )));
        }
    }

    if lowered == "reroll" || lowered == "npc reroll" {
        let draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        };
        let Some(mut draft) = draft else {
            return Ok(None);
        };

        let seed = generate_npc_seed(GenerateNpcSeedInput { prompt: None }, state.clone()).await?;
        draft.name = seed.name.trim().to_string();
        draft.race = seed.race.trim().to_string();
        draft.occupation = normalize_unknown_text(&seed.occupation);
        draft.sex = normalize_sex(&seed.sex)?;
        draft.age = normalize_unknown_text(&seed.age);
        draft.height = normalize_unknown_text(&seed.height);
        draft.weight_lbs = normalize_unknown_text(&seed.weight_lbs);
        draft.background = normalize_unknown_text(&seed.background);
        draft.want_need = normalize_unknown_text(&seed.want_need);
        draft.secret_obstacle = normalize_unknown_text(&seed.secret_obstacle);
        draft.carrying = normalize_unknown_list(seed.carrying);

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.location_draft = None;
            editor.npc_draft = Some(draft.clone());
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc rename ") {
        let name = trimmed[10..].trim();
        if name.is_empty() {
            return Ok(Some(ok_response("npc name cannot be empty.".to_string(), None)));
        }
        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;
        draft.name = name.to_string();
        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }
        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc set ") {
        let mut parts = trimmed.splitn(4, char::is_whitespace);
        let _ = parts.next();
        let _ = parts.next();
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default().trim();
        if value.is_empty() {
            return Ok(Some(ok_response(
                "npc set value cannot be empty.".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let Some(canonical) = canonical_npc_set_field(field) else {
            return Ok(Some(ok_response(
                format!(
                    "unknown npc field: {}. valid fields: name, race, occupation, sex, age, height, weight, background, want, secret, carrying",
                    field
                ),
                None,
            )));
        };

        match canonical {
            "name" => draft.name = value.to_string(),
            "race" => draft.race = value.to_string(),
            "occupation" => draft.occupation = value.to_string(),
            "sex" => draft.sex = normalize_sex(value)?,
            "age" => draft.age = value.to_string(),
            "height" => draft.height = value.to_string(),
            "weight_lbs" => draft.weight_lbs = value.to_string(),
            "background" => draft.background = value.to_string(),
            "want_need" => draft.want_need = value.to_string(),
            "secret_obstacle" => draft.secret_obstacle = value.to_string(),
            "carrying" => draft.carrying = parse_carrying_csv(value),
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc travel ") {
        if !lowered.starts_with("npc travel to ") {
            return Ok(Some(ok_response(
                "usage: npc travel to <location>".to_string(),
                None,
            )));
        }
        let location_name = trimmed[14..].trim();
        if location_name.is_empty() {
            return Ok(Some(ok_response("location cannot be empty.".to_string(), None)));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let result = ensure_location_exists(
            EnsureLocationInput {
                name: location_name.to_string(),
            },
            state.clone(),
        )
        .await?;
        draft.location = if result.name.trim().is_empty() {
            location_name.to_string()
        } else {
            result.name
        };

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered == "npc save" || lowered == "save" {
        let mode = {
            let editor = state.editor_session.lock().await;
            editor.mode
        };

        if mode == EditorMode::Npc {
            let draft = {
                let editor = state.editor_session.lock().await;
                editor.npc_draft.clone()
            }
            .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

            let result = save_npc_draft(
                SaveNpcDraftInput {
                    id: draft.id.clone(),
                    name: draft.name.clone(),
                    race: draft.race.clone(),
                    occupation: draft.occupation.clone(),
                    sex: draft.sex.clone(),
                    age: draft.age.clone(),
                    height: draft.height.clone(),
                    weight_lbs: draft.weight_lbs.clone(),
                    background: draft.background.clone(),
                    want_need: draft.want_need.clone(),
                    secret_obstacle: draft.secret_obstacle.clone(),
                    carrying: draft.carrying.clone(),
                    location: draft.location.clone(),
                },
                state.clone(),
            )
            .await?;

            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::None;
                editor.npc_draft = None;
                editor.location_draft = None;
            }

            let output = [
                "## NPC saved".to_string(),
                format!("id: {}", result.id),
                format!("slug: {}", result.slug),
                format!("vault: {}", result.vault_path),
                format!("updated: {}", result.updated_at),
            ]
            .join("\n");

            return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
        }

        if mode == EditorMode::Location {
            let draft = {
                let editor = state.editor_session.lock().await;
                editor.location_draft.clone()
            }
            .ok_or_else(|| "no active location draft. run load <name>.".to_string())?;

            let result = save_location_draft(
                SaveLocationDraftInput {
                    id: draft.id.clone(),
                    name: draft.name.clone(),
                    slug: draft.slug.clone(),
                    vault_path: draft.vault_path.clone(),
                },
                state.clone(),
            )
            .await?;

            {
                let mut editor = state.editor_session.lock().await;
                editor.mode = EditorMode::None;
                editor.npc_draft = None;
                editor.location_draft = None;
            }

            let output = [
                "## Location saved".to_string(),
                format!("id: {}", result.id),
                format!("slug: {}", result.slug),
                format!("vault: {}", result.vault_path),
                format!("updated: {}", result.updated_at),
            ]
            .join("\n");

            return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
        }
    }

    if lowered.starts_with("npc reroll ") {
        let args = trimmed[11..].trim();
        if args.is_empty() {
            return Ok(Some(ok_response(
                "usage: npc reroll <field> [prompt]".to_string(),
                None,
            )));
        }
        let mut split = args.splitn(2, char::is_whitespace);
        let field = split.next().unwrap_or_default().trim().to_string();
        let prompt = split.next().map(|value| value.trim().to_string());

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.npc_draft.clone()
        }
        .ok_or_else(|| "no active npc draft. run create npc or load <name>.".to_string())?;

        let rerolled = reroll_npc_field(
            RerollNpcFieldInput {
                field,
                prompt,
                npc: NpcRerollContext {
                    name: draft.name.clone(),
                    race: draft.race.clone(),
                    occupation: draft.occupation.clone(),
                    sex: draft.sex.clone(),
                    age: draft.age.clone(),
                    height: draft.height.clone(),
                    weight_lbs: draft.weight_lbs.clone(),
                    background: draft.background.clone(),
                    want_need: draft.want_need.clone(),
                    secret_obstacle: draft.secret_obstacle.clone(),
                    carrying: draft.carrying.clone(),
                    location: draft.location.clone(),
                },
            },
            state.clone(),
        )
        .await?;

        match rerolled.field.as_str() {
            "name" => {
                if let Some(value) = rerolled.value {
                    draft.name = value;
                }
            }
            "race" => {
                if let Some(value) = rerolled.value {
                    draft.race = value;
                }
            }
            "occupation" => {
                if let Some(value) = rerolled.value {
                    draft.occupation = value;
                }
            }
            "sex" => {
                if let Some(value) = rerolled.value {
                    draft.sex = normalize_sex(&value)?;
                }
            }
            "age" => {
                if let Some(value) = rerolled.value {
                    draft.age = value;
                }
            }
            "height" => {
                if let Some(value) = rerolled.value {
                    draft.height = value;
                }
            }
            "weight_lbs" => {
                if let Some(value) = rerolled.value {
                    draft.weight_lbs = value;
                }
            }
            "background" => {
                if let Some(value) = rerolled.value {
                    draft.background = value;
                }
            }
            "want_need" => {
                if let Some(value) = rerolled.value {
                    draft.want_need = value;
                }
            }
            "secret_obstacle" => {
                if let Some(value) = rerolled.value {
                    draft.secret_obstacle = value;
                }
            }
            "carrying" => {
                if let Some(carrying) = rerolled.carrying {
                    draft.carrying = carrying;
                }
            }
            _ => {}
        }

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Npc;
            editor.npc_draft = Some(draft.clone());
            editor.location_draft = None;
        }

        return Ok(Some(ok_response(
            npc_summary_text(&draft),
            Some(npc_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("location rename ") {
        let name = trimmed[16..].trim();
        if name.is_empty() {
            return Ok(Some(ok_response(
                "location name cannot be empty.".to_string(),
                None,
            )));
        }

        let mut draft = {
            let editor = state.editor_session.lock().await;
            editor.location_draft.clone()
        }
        .ok_or_else(|| "no active location draft. run load <name>.".to_string())?;
        draft.name = name.to_string();

        {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::Location;
            editor.location_draft = Some(draft.clone());
            editor.npc_draft = None;
        }

        return Ok(Some(ok_response(
            location_summary_text(&draft),
            Some(location_event_from_draft(&draft)),
        )));
    }

    if lowered.starts_with("npc ") {
        return Ok(Some(ok_response("unknown npc command.".to_string(), None)));
    }

    if lowered.starts_with("location ") {
        return Ok(Some(ok_response(
            "unknown location command.".to_string(),
            None,
        )));
    }

    if lowered == "load" {
        return Ok(Some(ok_response(
            "usage: load <npc-or-location-name>".to_string(),
            None,
        )));
    }

    if lowered.starts_with("load ") {
        let target = trimmed[4..].trim();
        if target.is_empty() {
            return Ok(Some(ok_response(
                "usage: load <npc-or-location-name>".to_string(),
                None,
            )));
        }

        let entity = resolve_entity(target.to_string()).await?;
        let Some(entity) = entity else {
            return Ok(Some(ok_response(
                format!("no npc or location found for: {target}"),
                None,
            )));
        };

        let (output, event) = match entity.entity_type {
            EntityType::Npc => {
                let draft = NpcDraftSession {
                    id: entity.id.clone(),
                    name: entity.name.clone(),
                    race: entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
                    occupation: entity
                        .occupation
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    sex: normalize_sex(
                        &entity
                            .sex
                            .clone()
                            .unwrap_or_else(|| "male".to_string()),
                    )
                    .unwrap_or_else(|_| "male".to_string()),
                    age: entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
                    height: entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
                    weight_lbs: entity
                        .weight_lbs
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    background: entity
                        .background
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    want_need: entity
                        .want_need
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    secret_obstacle: entity
                        .secret_obstacle
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    carrying: entity
                        .carrying
                        .clone()
                        .unwrap_or_else(|| vec!["Unknown".to_string()]),
                    location: entity
                        .location
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                };
                {
                    let mut editor = state.editor_session.lock().await;
                    editor.mode = EditorMode::Npc;
                    editor.location_draft = None;
                    editor.npc_draft = Some(draft.clone());
                }

                let carrying = entity
                    .carrying
                    .as_ref()
                    .map(|items| items.join(", "))
                    .unwrap_or_else(|| "Unknown".to_string());
                (
                    format!(
                        "## NPC\nname: {}\nslug: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}\npath: {}",
                        entity.name,
                        entity.slug,
                        entity.race.clone().unwrap_or_else(|| "Unknown".to_string()),
                        entity
                            .occupation
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        entity.sex.clone().unwrap_or_else(|| "Unknown".to_string()),
                        entity.age.clone().unwrap_or_else(|| "Unknown".to_string()),
                        entity.height.clone().unwrap_or_else(|| "Unknown".to_string()),
                        entity
                            .weight_lbs
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        entity
                            .background
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        entity
                            .want_need
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        entity
                            .secret_obstacle
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        carrying,
                        entity
                            .location
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        entity.vault_path
                    ),
                    Some(npc_event_from_draft(&draft)),
                )
            }
            EntityType::Location => {
                let draft = LocationDraftSession {
                    id: entity.id.clone(),
                    name: entity.name.clone(),
                    slug: entity.slug.clone(),
                    vault_path: entity.vault_path.clone(),
                };
                {
                    let mut editor = state.editor_session.lock().await;
                    editor.mode = EditorMode::Location;
                    editor.npc_draft = None;
                    editor.location_draft = Some(draft.clone());
                }

                (
                    format!(
                        "## Location\nname: {}\nslug: {}\npath: {}",
                        entity.name, entity.slug, entity.vault_path
                    ),
                    Some(location_event_from_draft(&draft)),
                )
            }
        };

        return Ok(Some(ok_response(output, event)));
    }

    if lowered == "delete" {
        return Ok(Some(ok_response(
            "usage: delete <npc-or-location-name>".to_string(),
            None,
        )));
    }

    if lowered.starts_with("delete ") {
        let target = trimmed[6..].trim();
        if target.is_empty() {
            return Ok(Some(ok_response(
                "usage: delete <npc-or-location-name>".to_string(),
                None,
            )));
        }

        let result = soft_delete_entity(
            SoftDeleteEntityInput {
                target: target.to_string(),
            },
            state.clone(),
        )
        .await?;

        let output = [
            "## Deleted".to_string(),
            format!("type: {}", result.entity_type.as_str()),
            format!("name: {}", result.name),
            format!("slug: {}", result.slug),
            format!("trash: {}", result.trash_vault_path),
            "tip: run undo to restore it.".to_string(),
        ]
        .join("\n");

        let should_clear = {
            let editor = state.editor_session.lock().await;
            editor
                .npc_draft
                .as_ref()
                .is_some_and(|draft| draft.id == result.id)
                || editor
                    .location_draft
                    .as_ref()
                    .is_some_and(|draft| draft.id == result.id)
        };

        if should_clear {
            let mut editor = state.editor_session.lock().await;
            editor.mode = EditorMode::None;
            editor.npc_draft = None;
            editor.location_draft = None;
            return Ok(Some(ok_response(output, Some(CommandClientEvent::ClearDrafts))));
        }

        return Ok(Some(ok_response(output, None)));
    }

    if lowered == "undo" {
        let result = undo_last_soft_delete(state).await?;
        let output = [
            "## Undo complete".to_string(),
            format!("type: {}", result.entity_type.as_str()),
            format!("name: {}", result.name),
            format!("slug: {}", result.slug),
            format!("vault: {}", result.vault_path),
        ]
        .join("\n");
        return Ok(Some(ok_response(output, None)));
    }

    Ok(None)
}

fn npc_event_from_draft(draft: &NpcDraftSession) -> CommandClientEvent {
    CommandClientEvent::LoadNpcDraft {
        id: draft.id.clone(),
        name: draft.name.clone(),
        race: draft.race.clone(),
        occupation: draft.occupation.clone(),
        sex: draft.sex.clone(),
        age: draft.age.clone(),
        height: draft.height.clone(),
        weight_lbs: draft.weight_lbs.clone(),
        background: draft.background.clone(),
        want_need: draft.want_need.clone(),
        secret_obstacle: draft.secret_obstacle.clone(),
        carrying: draft.carrying.clone(),
        location: draft.location.clone(),
    }
}

fn location_event_from_draft(draft: &LocationDraftSession) -> CommandClientEvent {
    CommandClientEvent::LoadLocationDraft {
        id: draft.id.clone(),
        name: draft.name.clone(),
        slug: draft.slug.clone(),
        vault_path: draft.vault_path.clone(),
    }
}

fn npc_summary_text(draft: &NpcDraftSession) -> String {
    format!(
        "## NPC Draft\nname: {}\nrace: {}\noccupation: {}\nsex: {}\nage: {}\nheight: {}\nweight: {}\nbackground: {}\nwant: {}\nsecret: {}\ncarrying: {}\nlocation: {}",
        draft.name,
        draft.race,
        draft.occupation,
        draft.sex,
        draft.age,
        draft.height,
        draft.weight_lbs,
        draft.background,
        draft.want_need,
        draft.secret_obstacle,
        draft.carrying.join(", "),
        draft.location,
    )
}

fn location_summary_text(draft: &LocationDraftSession) -> String {
    format!(
        "## Location Draft\nname: {}\nslug: {}\npath: {}",
        draft.name, draft.slug, draft.vault_path
    )
}

fn canonical_npc_set_field(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "name" => Some("name"),
        "race" => Some("race"),
        "occupation" => Some("occupation"),
        "sex" => Some("sex"),
        "age" => Some("age"),
        "height" => Some("height"),
        "weight" | "weight_lbs" => Some("weight_lbs"),
        "background" => Some("background"),
        "want" | "need" | "want_need" => Some("want_need"),
        "secret" | "obstacle" | "secret_obstacle" => Some("secret_obstacle"),
        "carrying" => Some("carrying"),
        _ => None,
    }
}

fn ok_response(output: String, client_event: Option<CommandClientEvent>) -> CommandResponse {
    CommandResponse {
        ok: true,
        output: output.clone(),
        error: None,
        exit_code: 0,
        segments: vec![OutputSegment {
            kind: OutputSegmentKind::Text,
            text: output,
            command_ref: None,
        }],
        output_doc: None,
        client_event,
    }
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

#[tauri::command]
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

#[tauri::command]
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
fn parse_command_input(input: String) -> ParseResult {
    dnd_core::command_parse::parse_command_input(&input)
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
            soft_delete_entity,
            undo_last_soft_delete,
            get_command_manifest,
            parse_command_input,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
