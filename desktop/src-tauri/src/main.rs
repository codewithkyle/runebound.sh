#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod repositories;
mod router;
mod services;
mod utils;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dnd_core::command::{CommandClientEvent, CommandResponse};
use dnd_core::command_manifest::{CommandManifest, CommandSpec};
use dnd_core::command_parse::{ParseResult, ParseStage, normalize_command_input, parse_command_input};
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::db;
use dnd_core::npc::{
    FactionFrontmatter, LocationFrontmatter, NpcFrontmatter, UNKNOWN_LOCATION, make_entity_id,
    merge_runebound_block, normalize_markdown_file_stem, now_timestamp, render_faction_markdown,
    render_location_markdown, render_npc_markdown, slugify,
    unique_slug_for_dir,
};
use dnd_core::vault::Vault;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::app_state::{AppState, EditorSession};
use crate::repositories::{
    Database, DocumentRepository, FactionRepository, GenerationRepository, LocationRepository,
    NpcRepository, ProdDocumentRepository, ProdFactionRepository, ProdGenerationRepository,
    ProdLocationRepository, ProdNpcRepository, ProdSoftDeleteRepository, ProdVaultRepository,
    SoftDeleteRepository, VaultRepository,
};

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

const LOCATION_KIND_TYPES: [&str; 10] = [
    "hamlet",
    "town",
    "city",
    "dungeon",
    "hideout",
    "ruin",
    "guildhall",
    "landmark",
    "wilderness",
    "other",
];

const LOCATION_DANGER_LEVELS: [&str; 5] = ["Unknown", "safe", "guarded", "risky", "deadly"];

const FACTION_KIND_TYPES: [&str; 10] = [
    "guild",
    "cult",
    "military_order",
    "noble_house",
    "criminal_syndicate",
    "mercantile_league",
    "religious_order",
    "arcane_circle",
    "revolutionary_cell",
    "other",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocationSeed {
    name: String,
    kind_type: String,
    kind_custom: Option<String>,
    visual_description: String,
    history_background: String,
    exports: Vec<String>,
    tone: String,
    authority: String,
    danger_level: String,
    current_tension: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateLocationSeedInput {
    prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocationRerollContext {
    name: String,
    kind_type: String,
    kind_custom: Option<String>,
    visual_description: String,
    history_background: String,
    exports: Vec<String>,
    tone: String,
    authority: String,
    danger_level: String,
    current_tension: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RerollLocationFieldInput {
    field: String,
    prompt: Option<String>,
    location: LocationRerollContext,
}

#[derive(Debug, Clone, Serialize)]
struct RerollLocationFieldResult {
    field: String,
    value: Option<String>,
    exports: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FactionSeed {
    name: String,
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
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateFactionSeedInput {
    prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FactionRerollContext {
    name: String,
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
}

#[derive(Debug, Clone, Deserialize)]
struct RerollFactionFieldInput {
    field: String,
    prompt: Option<String>,
    faction: FactionRerollContext,
}

#[derive(Debug, Clone, Serialize)]
struct RerollFactionFieldResult {
    field: String,
    value: Option<String>,
    list_value: Option<Vec<String>>,
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
    Faction,
}

impl EntityType {
    fn as_str(&self) -> &'static str {
        match self {
            EntityType::Npc => "npc",
            EntityType::Location => "location",
            EntityType::Faction => "faction",
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
    Faction,
    Reference,
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
    kind_type: Option<String>,
    kind_custom: Option<String>,
    visual_description: Option<String>,
    history_background: Option<String>,
    exports: Option<Vec<String>>,
    tone: Option<String>,
    authority: Option<String>,
    danger_level: Option<String>,
    current_tension: Option<String>,
    public_description: Option<String>,
    true_agenda: Option<String>,
    methods: Option<String>,
    leadership: Option<String>,
    headquarters: Option<String>,
    sphere_of_influence: Option<String>,
    resources_assets: Option<String>,
    allies: Option<Vec<String>>,
    rivals_enemies: Option<Vec<String>>,
    reputation: Option<String>,
    goals_short_term: Option<Vec<String>>,
    goals_long_term: Option<Vec<String>>,
    symbol_description: Option<String>,
    created_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SaveLocationDraftInput {
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
struct SaveFactionDraftInput {
    id: String,
    slug: String,
    name: String,
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
}

#[derive(Debug, Clone, Serialize)]
struct SaveFactionDraftResult {
    id: String,
    slug: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct VaultReferenceEntry {
    key: String,
    key_lower: String,
    markdown_path: Option<String>,
    is_dir: bool,
}

#[derive(Debug, Clone)]
struct ActiveReferenceQuery {
    at_index: usize,
    query: String,
}

#[derive(Debug, Clone, Default)]
struct PromptReferenceContext {
    system_context: String,
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
    kind_type: String,
    kind_custom: Option<String>,
    visual_description: String,
    history_background: String,
    exports: String,
    tone: String,
    authority: String,
    danger_level: String,
    current_tension: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FactionDeletePayload {
    id: String,
    slug: String,
    name: String,
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
    allies: String,
    rivals_enemies: String,
    reputation: String,
    current_tension: String,
    goals_short_term: String,
    goals_long_term: String,
    symbol_description: String,
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

fn normalize_location_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if LOCATION_KIND_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "kind_type must be one of: {}",
            LOCATION_KIND_TYPES.join(", ")
        ))
    }
}

fn normalize_location_danger_level(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let normalized = if trimmed.eq_ignore_ascii_case("unknown") {
        "Unknown".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    };
    if LOCATION_DANGER_LEVELS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "danger_level must be one of: {}",
            LOCATION_DANGER_LEVELS.join(", ")
        ))
    }
}

fn parse_list_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn normalize_exports(values: Vec<String>) -> Vec<String> {
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

fn exports_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

fn exports_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_exports(items),
        Err(_) => normalize_exports(parse_list_csv(value)),
    }
}

fn sentence_count(value: &str) -> usize {
    value
        .split_terminator(['.', '!', '?'])
        .filter(|part| !part.trim().is_empty())
        .count()
}

fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

fn validate_sentence_range(value: &str, min: usize, max: usize, field: &str) -> Result<(), String> {
    let count = sentence_count(value);
    if count < min || count > max {
        return Err(format!(
            "{field} must be {min}-{max} sentences; got {count}"
        ));
    }
    Ok(())
}

fn normalize_location_seed(mut seed: LocationSeed) -> Result<LocationSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_location_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed
            .kind_custom
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err("kind_custom is required when kind_type is other".to_string());
        }
    } else {
        seed.kind_custom = None;
    }
    seed.visual_description = normalize_unknown_text(&seed.visual_description);
    seed.history_background = normalize_unknown_text(&seed.history_background);
    seed.exports = normalize_exports(seed.exports);
    seed.tone = normalize_unknown_text(&seed.tone);
    seed.authority = normalize_unknown_text(&seed.authority);
    seed.danger_level = normalize_location_danger_level(&seed.danger_level)?;
    seed.current_tension = normalize_unknown_text(&seed.current_tension);
    Ok(seed)
}

fn validate_location_details(seed: &LocationSeed) -> Result<(), String> {
    if seed.name.trim().is_empty() {
        return Err("location name cannot be empty".to_string());
    }
    if seed.visual_description != "Unknown" {
        validate_sentence_range(&seed.visual_description, 1, 3, "visual_description")?;
    }
    if seed.history_background != "Unknown" {
        validate_sentence_range(&seed.history_background, 2, 5, "history_background")?;
    }
    if seed.current_tension != "Unknown" {
        validate_sentence_range(&seed.current_tension, 1, 2, "current_tension")?;
    }
    if seed.exports.is_empty() || seed.exports.len() > 3 {
        return Err("exports must have 1-3 items".to_string());
    }
    if !(seed.exports.len() == 1 && seed.exports[0] == "Unknown") {
        let empty_item = seed.exports.iter().any(|item| item.trim().is_empty());
        if empty_item {
            return Err("exports cannot contain empty items".to_string());
        }
    }
    if seed.tone != "Unknown" {
        let tone_words = word_count(&seed.tone);
        if !(2..=5).contains(&tone_words) {
            return Err(format!("tone must be 2-5 words; got {tone_words}"));
        }
    }
    Ok(())
}

fn normalize_faction_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if FACTION_KIND_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "kind_type must be one of: {}",
            FACTION_KIND_TYPES.join(", ")
        ))
    }
}

fn normalize_faction_seed(mut seed: FactionSeed) -> Result<FactionSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_faction_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed
            .kind_custom
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err("kind_custom is required when kind_type is other".to_string());
        }
    } else {
        seed.kind_custom = None;
    }

    seed.public_description = normalize_unknown_text(&seed.public_description);
    seed.true_agenda = normalize_unknown_text(&seed.true_agenda);
    seed.methods = normalize_unknown_text(&seed.methods);
    seed.leadership = normalize_unknown_text(&seed.leadership);
    seed.headquarters = normalize_unknown_text(&seed.headquarters);
    seed.sphere_of_influence = normalize_unknown_text(&seed.sphere_of_influence);
    seed.resources_assets = normalize_unknown_text(&seed.resources_assets);
    seed.allies = normalize_unknown_list(seed.allies);
    seed.rivals_enemies = normalize_unknown_list(seed.rivals_enemies);
    seed.reputation = normalize_unknown_text(&seed.reputation);
    seed.current_tension = normalize_unknown_text(&seed.current_tension);
    seed.goals_short_term = normalize_unknown_list(seed.goals_short_term);
    seed.goals_long_term = normalize_unknown_list(seed.goals_long_term);
    seed.symbol_description = normalize_unknown_text(&seed.symbol_description);
    Ok(seed)
}

fn validate_faction_details(seed: &FactionSeed) -> Result<(), String> {
    if seed.name.trim().is_empty() {
        return Err("faction name cannot be empty".to_string());
    }
    if seed.public_description != "Unknown" {
        validate_sentence_range(&seed.public_description, 1, 3, "public_description")?;
    }
    if seed.true_agenda != "Unknown" {
        validate_sentence_range(&seed.true_agenda, 1, 3, "true_agenda")?;
    }
    if seed.current_tension != "Unknown" {
        validate_sentence_range(&seed.current_tension, 1, 2, "current_tension")?;
    }
    if seed.symbol_description != "Unknown" {
        validate_sentence_range(&seed.symbol_description, 1, 1, "symbol_description")?;
    }
    Ok(())
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

fn faction_list_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

fn faction_list_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_unknown_list(items),
        Err(_) => normalize_unknown_list(parse_list_csv(value)),
    }
}

fn read_vault_file_if_exists(vault: &Vault, relative_path: &str) -> Result<Option<String>, String> {
    let relative = PathBuf::from(normalize_relative_path_for_storage(relative_path));
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
    let source_relative = normalize_relative_path_for_storage(source_relative);
    let target_relative = normalize_relative_path_for_storage(target_relative);
    let source_full = vault
        .resolve_relative(&PathBuf::from(&source_relative))
        .map_err(|err| err.to_string())?;
    if !source_full.exists() {
        return Err(format!(
            "source file does not exist: {}",
            source_full.display()
        ));
    }

    let target_full = vault
        .resolve_relative(&PathBuf::from(&target_relative))
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
        let relative = normalize_relative_path_for_storage(&relative);

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

fn canonical_location_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "kind" | "kind_type" => "kind_type",
        "kind_custom" | "custom_kind" => "kind_custom",
        "visual" | "visual_description" | "description" => "visual_description",
        "history" | "history_background" | "background" => "history_background",
        "exports" => "exports",
        "tone" => "tone",
        "authority" => "authority",
        "danger" | "danger_level" => "danger_level",
        "tension" | "current_tension" => "current_tension",
        _ => {
            return Err(format!(
                "unknown location reroll field: {}. valid fields: name, kind, kind_custom, visual, history, exports, tone, authority, danger, tension",
                raw
            ))
        }
    };
    Ok(field)
}

fn location_context_summary(context: &LocationRerollContext) -> String {
    format!(
        "name={}, kind_type={}, kind_custom={}, visual_description={}, history_background={}, exports={}, tone={}, authority={}, danger_level={}, current_tension={}",
        context.name,
        context.kind_type,
        context.kind_custom.clone().unwrap_or_else(|| "(none)".to_string()),
        context.visual_description,
        context.history_background,
        context.exports.join(", "),
        context.tone,
        context.authority,
        context.danger_level,
        context.current_tension
    )
}

fn canonical_faction_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "kind" | "kind_type" => "kind_type",
        "kind_custom" => "kind_custom",
        "public" | "public_description" => "public_description",
        "agenda" | "true_agenda" => "true_agenda",
        "methods" => "methods",
        "leadership" => "leadership",
        "hq" | "headquarters" => "headquarters",
        "influence" | "sphere_of_influence" => "sphere_of_influence",
        "resources" | "resources_assets" => "resources_assets",
        "allies" => "allies",
        "rivals" | "rivals_enemies" => "rivals_enemies",
        "reputation" => "reputation",
        "tension" | "current_tension" => "current_tension",
        "goals_short" | "goals_short_term" => "goals_short_term",
        "goals_long" | "goals_long_term" => "goals_long_term",
        "symbol" | "sigil" | "banner" | "symbol_description" => "symbol_description",
        _ => {
            return Err(format!(
                "unknown faction reroll field: {}. valid fields: name, kind, kind_custom, public, agenda, methods, leadership, headquarters, influence, resources, allies, rivals, reputation, tension, goals_short, goals_long, symbol",
                raw
            ))
        }
    };

    Ok(field)
}

fn faction_context_summary(context: &FactionRerollContext) -> String {
    format!(
        "name={}, kind_type={}, kind_custom={}, public_description={}, true_agenda={}, methods={}, leadership={}, headquarters={}, sphere_of_influence={}, resources_assets={}, allies={}, rivals_enemies={}, reputation={}, current_tension={}, goals_short_term={}, goals_long_term={}, symbol_description={}",
        context.name,
        context.kind_type,
        context.kind_custom.clone().unwrap_or_else(|| "(none)".to_string()),
        context.public_description,
        context.true_agenda,
        context.methods,
        context.leadership,
        context.headquarters,
        context.sphere_of_influence,
        context.resources_assets,
        context.allies.join(", "),
        context.rivals_enemies.join(", "),
        context.reputation,
        context.current_tension,
        context.goals_short_term.join(", "),
        context.goals_long_term.join(", "),
        context.symbol_description,
    )
}

fn is_reference_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"')
}

fn can_start_reference_at(input: &str, at_index: usize) -> bool {
    if at_index == 0 {
        return true;
    }

    let before = input[..at_index].chars().next_back();
    before.is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\''))
}

fn extract_active_reference_query(input: &str) -> Option<ActiveReferenceQuery> {
    for (idx, ch) in input.char_indices().rev() {
        if ch != '@' {
            continue;
        }
        if !can_start_reference_at(input, idx) {
            continue;
        }

        return Some(ActiveReferenceQuery {
            at_index: idx,
            query: input[idx + 1..].to_string(),
        });
    }

    None
}

fn should_ignore_reference_component(component: &str) -> bool {
    component
        .split('/')
        .any(|part| part.starts_with('.') || part.eq_ignore_ascii_case("target"))
}

fn markdown_reference_key(relative_path: &str) -> Option<String> {
    let normalized = normalize_relative_path_for_storage(relative_path);
    let path = Path::new(&normalized);
    let ext = path.extension().and_then(|value| value.to_str())?;
    if !ext.eq_ignore_ascii_case("md") {
        return None;
    }

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let parent = path.parent().and_then(|value| value.to_str()).unwrap_or("");
    if parent.is_empty() {
        Some(stem.to_string())
    } else {
        Some(format!("{parent}/{stem}"))
    }
}

fn is_top_level_reference_key(key: &str, is_dir: bool) -> bool {
    if is_dir {
        let trimmed = key.trim_end_matches('/');
        !trimmed.is_empty() && !trimmed.contains('/')
    } else {
        !key.contains('/')
    }
}

fn load_vault_reference_entries(vault: &Vault) -> Result<Vec<VaultReferenceEntry>, String> {
    vault.ensure_root_exists().map_err(|err| err.to_string())?;

    let mut entries: HashMap<String, VaultReferenceEntry> = HashMap::new();
    let mut stack = vec![PathBuf::new()];

    while let Some(relative_dir) = stack.pop() {
        let full_dir = vault
            .resolve_relative(&relative_dir)
            .map_err(|err| err.to_string())?;
        let dir_entries = fs::read_dir(&full_dir)
            .map_err(|err| format!("failed to read directory {}: {}", full_dir.display(), err))?;

        for dir_entry in dir_entries {
            let dir_entry = match dir_entry {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("reference index warning: failed to read directory entry: {err}");
                    continue;
                }
            };
            let entry_path = dir_entry.path();
            let relative = match entry_path.strip_prefix(vault.root()) {
                Ok(value) => normalize_relative_path_for_storage(&value.to_string_lossy()),
                Err(_) => continue,
            };
            if should_ignore_reference_component(&relative) {
                continue;
            }

            if entry_path.is_dir() {
                let mut key = relative.trim_matches('/').to_string();
                if key.is_empty() {
                    continue;
                }
                key.push('/');
                entries.entry(key.clone()).or_insert_with(|| VaultReferenceEntry {
                    key: key.clone(),
                    key_lower: key.to_lowercase(),
                    markdown_path: None,
                    is_dir: true,
                });
                stack.push(PathBuf::from(relative));
                continue;
            }

            let Some(key) = markdown_reference_key(&relative) else {
                continue;
            };
            entries.entry(key.clone()).or_insert_with(|| VaultReferenceEntry {
                key: key.clone(),
                key_lower: key.to_lowercase(),
                markdown_path: Some(relative),
                is_dir: false,
            });
        }
    }

    let mut out: Vec<VaultReferenceEntry> = entries.into_values().collect();
    out.sort_by(|left, right| left.key_lower.cmp(&right.key_lower));
    Ok(out)
}

fn build_reference_suggestions_from_entries(
    input: &str,
    active: &ActiveReferenceQuery,
    entries: &[VaultReferenceEntry],
) -> Vec<CommandSuggestion> {
    let query_lower = normalize_relative_path_for_storage(&active.query).to_lowercase();
    let mut ranked: Vec<&VaultReferenceEntry> = entries
        .iter()
        .filter(|entry| {
            if query_lower.is_empty() {
                return is_top_level_reference_key(&entry.key, entry.is_dir);
            }
            entry.key_lower.starts_with(&query_lower)
        })
        .collect();

    ranked.sort_by(|left, right| left.key_lower.cmp(&right.key_lower));
    ranked
        .into_iter()
        .take(12)
        .map(|entry| {
            let completion_suffix = if entry.is_dir { "" } else { " " };
            CommandSuggestion {
                label: format!("@{}", entry.key),
                completion: format!(
                    "{}@{}{}",
                    &input[..active.at_index],
                    entry.key,
                    completion_suffix
                ),
                helper_text: Some(SuggestionHelperText::Reference),
            }
        })
        .collect()
}

fn extract_prompt_reference_keys(prompt: &str, entries: &[VaultReferenceEntry]) -> Vec<String> {
    let mut candidates: Vec<&VaultReferenceEntry> = entries
        .iter()
        .filter(|entry| !entry.is_dir && entry.markdown_path.is_some())
        .collect();
    candidates.sort_by(|left, right| right.key_lower.len().cmp(&left.key_lower.len()));

    let prompt_lower = prompt.to_lowercase();
    let mut cursor = 0;
    let mut matched = Vec::new();

    while cursor < prompt.len() {
        let next_at = match prompt[cursor..].find('@') {
            Some(offset) => cursor + offset,
            None => break,
        };
        if !can_start_reference_at(prompt, next_at) {
            cursor = next_at + 1;
            continue;
        }

        let tail_start = next_at + 1;
        let tail = &prompt_lower[tail_start..];
        let mut best: Option<&VaultReferenceEntry> = None;

        for candidate in &candidates {
            if !tail.starts_with(&candidate.key_lower) {
                continue;
            }
            let boundary_index = tail_start + candidate.key.len();
            let boundary_ok = prompt[boundary_index..]
                .chars()
                .next()
                .is_none_or(is_reference_boundary_char);
            if !boundary_ok {
                continue;
            }
            best = Some(*candidate);
            break;
        }

        if let Some(candidate) = best {
            matched.push(candidate.key.clone());
            cursor = tail_start + candidate.key.len();
            continue;
        }

        cursor = next_at + 1;
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for key in matched {
        let lowered = key.to_lowercase();
        if seen.insert(lowered) {
            unique.push(key);
        }
    }
    unique
}

fn build_prompt_reference_context(
    prompt: &str,
    entries: &[VaultReferenceEntry],
    vault: &Vault,
) -> PromptReferenceContext {
    const MAX_REFERENCE_DOCS: usize = 5;
    const MAX_METADATA_CHARS_PER_DOC: usize = 1800;

    let keys = extract_prompt_reference_keys(prompt, entries);
    if keys.is_empty() {
        return PromptReferenceContext::default();
    }

    let path_by_key: HashMap<String, String> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .markdown_path
                .as_ref()
                .map(|path| (entry.key.to_lowercase(), path.clone()))
        })
        .collect();

    let mut blocks = Vec::new();
    for key in keys.into_iter().take(MAX_REFERENCE_DOCS) {
        let Some(path) = path_by_key.get(&key.to_lowercase()) else {
            continue;
        };
        let contents = match vault.read_relative(Path::new(path)) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("reference context warning: failed reading {}: {}", path, err);
                continue;
            }
        };
        let Some(runebound) = extract_runebound_toml(&contents) else {
            continue;
        };
        let metadata = if runebound.len() > MAX_METADATA_CHARS_PER_DOC {
            format!("{}...", &runebound[..MAX_METADATA_CHARS_PER_DOC])
        } else {
            runebound
        };
        blocks.push(format!("@{key}\npath: {path}\n```toml\n{metadata}\n```"));
    }

    if blocks.is_empty() {
        return PromptReferenceContext::default();
    }

    PromptReferenceContext {
        system_context: format!(
            "Referenced vault metadata (treat as authoritative setting context):\n\n{}",
            blocks.join("\n\n")
        ),
    }
}

fn parse_recent_location_seeds(payloads: Vec<String>) -> Vec<LocationSeed> {
    payloads
        .into_iter()
        .filter_map(|payload| serde_json::from_str::<LocationSeed>(&payload).ok())
        .collect()
}

fn parse_recent_faction_seeds(payloads: Vec<String>) -> Vec<FactionSeed> {
    payloads
        .into_iter()
        .filter_map(|payload| serde_json::from_str::<FactionSeed>(&payload).ok())
        .collect()
}

fn recent_faction_name_set(seeds: &[FactionSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn describe_recent_faction_seeds(seeds: &[FactionSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.reputation))
        .collect::<Vec<_>>()
        .join("; ")
}

fn describe_recent_location_seeds(seeds: &[LocationSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| {
            format!(
                "{} | {} | {}",
                seed.name,
                seed.kind_type,
                seed.danger_level
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn recent_name_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn occupation_tokens(value: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "a",
        "an",
        "and",
        "as",
        "at",
        "by",
        "deceased",
        "ex",
        "for",
        "former",
        "from",
        "in",
        "of",
        "on",
        "retired",
        "the",
        "to",
        "under",
        "with",
    ];

    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .filter(|token| !STOP_WORDS.contains(&token.as_str()))
        .collect()
}

fn occupation_anchor(value: &str) -> String {
    occupation_tokens(value)
        .into_iter()
        .next()
        .unwrap_or_else(|| "unknown".to_string())
}

fn recent_occupation_anchor_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| occupation_anchor(&seed.occupation))
        .filter(|anchor| !anchor.is_empty() && anchor != "unknown")
        .collect()
}

fn recent_location_name_set(seeds: &[LocationSeed]) -> std::collections::HashSet<String> {
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
        .map(|seed| {
            format!(
                "{} | {} | {} | {}",
                seed.name, seed.race, seed.sex, seed.occupation
            )
        })
        .collect();
    items.join("; ")
}

fn describe_recent_npc_occupation_anchors(seeds: &[NpcSeed]) -> String {
    let mut anchors: Vec<String> = recent_occupation_anchor_set(seeds).into_iter().collect();
    if anchors.is_empty() {
        return "none".to_string();
    }
    anchors.sort();
    anchors.truncate(12);
    anchors.join(", ")
}

fn npc_travel_location_query(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "npc travel to" {
        return Some(String::new());
    }
    if lowered.starts_with("npc travel to ") {
        return Some(trimmed[14..].trim().to_string());
    }

    None
}

#[tauri::command]
async fn suggest_command_input(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CommandSuggestion>, String> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Some(active_ref) = extract_active_reference_query(&input) {
        if active_ref
            .query
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
        {
            return Ok(Vec::new());
        }

        if !active_ref.query.trim().starts_with('-') {
            let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
            if let Some(vault_path) = loaded.effective.vault.path {
                let vault = Vault::new(vault_path);
                if vault.ensure_root_exists().is_ok() {
                    let entries = load_vault_reference_entries(&vault)?;
                    let suggestions =
                        build_reference_suggestions_from_entries(&input, &active_ref, &entries);
                    return Ok(suggestions);
                }
            }

            return Ok(Vec::new());
        }
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
            if mode != app_state::EditorMode::Location
                && mode != app_state::EditorMode::Faction
                && (completion == "reroll" || label == "reroll")
            {
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

        if mode != app_state::EditorMode::Faction
            && (completion == "faction"
                || completion.starts_with("faction ")
                || label == "faction"
                || label.starts_with("faction "))
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
    let is_show_context = lowered == "show" || lowered.starts_with("show ");
    let is_preview_context = lowered == "preview" || lowered.starts_with("preview ");
    let search_query = if is_load_context {
        trimmed[4..].trim()
    } else if is_delete_context {
        trimmed[6..].trim()
    } else if is_show_context {
        trimmed[4..].trim()
    } else if is_preview_context {
        trimmed[7..].trim()
    } else {
        trimmed
    };

    if !search_query.is_empty()
        && (is_load_context
            || is_delete_context
            || is_show_context
            || is_preview_context
            || !starts_with_known_command_root(trimmed, &manifest))
    {
        let entity_results = search_entities(state.inner(), search_query.to_string(), Some(6)).await?;
        let prefix = if is_load_context {
            Some("load")
        } else if is_delete_context {
            Some("delete")
        } else if is_show_context {
            Some("show")
        } else if is_preview_context {
            Some("preview")
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
                    EntityType::Faction => SuggestionHelperText::Faction,
                }),
            });
        }
    }

    if mode == app_state::EditorMode::Npc {
        if let Some(location_query) = npc_travel_location_query(trimmed) {
            let location_names =
                search_location_names(state.inner(), location_query, Some(8)).await?;
            for location_name in location_names {
                suggestions.push(CommandSuggestion {
                    label: location_name.clone(),
                    completion: format!("npc travel to {} ", location_name),
                    helper_text: Some(SuggestionHelperText::Location),
                });
            }
        }
    }

    let mut seen = HashSet::new();
    suggestions.retain(|suggestion| {
        let key = suggestion.completion.trim().to_ascii_lowercase();
        seen.insert(key)
    });

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

    if command.name == "location"
        && subcommand.is_some_and(|item| item.name == "set" || item.name == "reroll")
    {
        let field_names = [
            "name",
            "kind",
            "kind_custom",
            "visual",
            "history",
            "exports",
            "tone",
            "authority",
            "danger",
            "tension",
        ];
        let args = &parsed.normalized_tokens[2..];
        let should_suggest_fields =
            args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);

        if should_suggest_fields {
            let prefix = parsed.completion.current_token.to_ascii_lowercase();
            let base = replace_current_token(input, &parsed.completion.current_token);
            let prefix_label = if subcommand.is_some_and(|item| item.name == "set") {
                "location set"
            } else {
                "location reroll"
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

    if command.name == "faction"
        && subcommand.is_some_and(|item| item.name == "set" || item.name == "reroll")
    {
        let field_names = [
            "name",
            "kind",
            "kind_custom",
            "public",
            "agenda",
            "methods",
            "leadership",
            "headquarters",
            "influence",
            "resources",
            "allies",
            "rivals",
            "reputation",
            "tension",
            "goals_short",
            "goals_long",
            "symbol",
        ];
        let args = &parsed.normalized_tokens[2..];
        let should_suggest_fields =
            args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);

        if should_suggest_fields {
            let prefix = parsed.completion.current_token.to_ascii_lowercase();
            let base = replace_current_token(input, &parsed.completion.current_token);
            let prefix_label = if subcommand.is_some_and(|item| item.name == "set") {
                "faction set"
            } else {
                "faction reroll"
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
    let parsed = parse_command_input(&normalized_input);
    if !parsed.valid {
        let has_unknown_command = parsed
            .diagnostics
            .iter()
            .any(|diag| diag.code == "unknown_command");

        if !has_unknown_command {
            if let Some(diag) = parsed.diagnostics.first() {
                return Err(diag.message.clone());
            }
            return Err("invalid command".to_string());
        }
    }

    if let Some(response) =
        router::dispatch_desktop_command(&normalized_input, &parsed.normalized_tokens, state.clone())
            .await?
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
    normalize_command_input(input)
}

pub(crate) fn normalize_relative_path_for_storage(path: &str) -> String {
    path.replace('\\', "/")
}

pub(crate) fn path_for_display(path: &str) -> String {
    if MAIN_SEPARATOR == '\\' {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

fn stable_id_from_relative(prefix: &str, relative_path: &str) -> String {
    let suffix: String = relative_path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}_{suffix}")
}

fn file_stem_name(relative_path: &str) -> String {
    Path::new(relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

fn extract_runebound_toml(contents: &str) -> Option<String> {
    let start = contents.find("```runebound")?;
    let mut body = &contents[start + "```runebound".len()..];
    if let Some(rest) = body.strip_prefix("\r\n") {
        body = rest;
    } else if let Some(rest) = body.strip_prefix('\n') {
        body = rest;
    }

    let end = body.find("\n```").or_else(|| body.find("```"))?;
    let block = body[..end].trim();
    if block.is_empty() {
        None
    } else {
        Some(block.to_string())
    }
}

fn collect_markdown_files_under(
    root: &Path,
    relative_dir: &str,
) -> Result<Vec<(String, String)>, String> {
    let base_dir = root.join(relative_dir);
    if !base_dir.exists() {
        return Ok(Vec::new());
    }

    let mut stack = vec![base_dir];
    let mut files = Vec::new();

    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .map_err(|err| format!("failed to read directory {}: {}", current.display(), err))?;

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("startup sync warning: failed to read directory entry: {err}");
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let is_md = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
            if !is_md {
                continue;
            }

            let relative = match path.strip_prefix(root) {
                Ok(relative) => normalize_relative_path_for_storage(&relative.to_string_lossy()),
                Err(_) => continue,
            };
            match fs::read_to_string(&path) {
                Ok(contents) => files.push((relative, contents)),
                Err(err) => {
                    eprintln!(
                        "startup sync warning: failed to read markdown file {}: {}",
                        path.display(),
                        err
                    );
                }
            }
        }
    }

    Ok(files)
}

fn scan_npc_row_from_markdown(relative_path: &str, contents: &str) -> db::NpcRow {
    let parsed = extract_runebound_toml(contents).and_then(|toml_text| toml::from_str::<toml::Value>(&toml_text).ok());
    let now = now_timestamp();
    let fallback_name = file_stem_name(relative_path);

    let name = parsed
        .as_ref()
        .and_then(|value| value.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name)
        .to_string();
    let slug = parsed
        .as_ref()
        .and_then(|value| value.get("slug").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify(&name));
    let id = parsed
        .as_ref()
        .and_then(|value| value.get("id").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| stable_id_from_relative("npc", relative_path));

    let sex = parsed
        .as_ref()
        .and_then(|value| value.get("sex").and_then(toml::Value::as_str))
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| value == "male" || value == "female")
        .unwrap_or_else(|| "male".to_string());

    let carrying = parsed
        .as_ref()
        .and_then(|value| value.get("carrying").and_then(toml::Value::as_array))
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| vec!["Unknown".to_string()]);

    db::NpcRow {
        id,
        slug,
        name,
        race: parsed
            .as_ref()
            .and_then(|value| value.get("race").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        occupation: parsed
            .as_ref()
            .and_then(|value| value.get("occupation").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        sex,
        age: parsed
            .as_ref()
            .and_then(|value| value.get("age").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        height: parsed
            .as_ref()
            .and_then(|value| value.get("height").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        weight_lbs: parsed
            .as_ref()
            .and_then(|value| value.get("weight_lbs").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        background: parsed
            .as_ref()
            .and_then(|value| value.get("background").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        want_need: parsed
            .as_ref()
            .and_then(|value| value.get("want_need").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        secret_obstacle: parsed
            .as_ref()
            .and_then(|value| value.get("secret_obstacle").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        carrying: serde_json::to_string(&carrying).unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        location: parsed
            .as_ref()
            .and_then(|value| value.get("location").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| UNKNOWN_LOCATION.to_string()),
        vault_path: normalize_relative_path_for_storage(relative_path),
        created_at: parsed
            .as_ref()
            .and_then(|value| value.get("created_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| now.clone()),
        updated_at: parsed
            .as_ref()
            .and_then(|value| value.get("updated_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or(now),
    }
}

fn scan_location_row_from_markdown(relative_path: &str, contents: &str) -> db::LocationRow {
    let parsed = extract_runebound_toml(contents).and_then(|toml_text| toml::from_str::<toml::Value>(&toml_text).ok());
    let now = now_timestamp();
    let fallback_name = file_stem_name(relative_path);

    let name = parsed
        .as_ref()
        .and_then(|value| value.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name)
        .to_string();
    let slug = parsed
        .as_ref()
        .and_then(|value| value.get("slug").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify(&name));
    let id = parsed
        .as_ref()
        .and_then(|value| value.get("id").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| stable_id_from_relative("loc", relative_path));

    let kind_raw = parsed
        .as_ref()
        .and_then(|value| {
            value
                .get("kind_type")
                .or_else(|| value.get("kind"))
                .and_then(toml::Value::as_str)
        })
        .unwrap_or("other");
    let kind_type = normalize_location_kind_type(kind_raw).unwrap_or_else(|_| "other".to_string());

    let mut kind_custom = parsed
        .as_ref()
        .and_then(|value| value.get("kind_custom").and_then(toml::Value::as_str))
        .map(normalize_unknown_text);
    if kind_type == "other" && kind_custom.is_none() {
        kind_custom = Some("Unknown".to_string());
    }
    if kind_type != "other" {
        kind_custom = None;
    }

    let exports = parsed
        .as_ref()
        .and_then(|value| value.get("exports"))
        .and_then(|raw| {
            if let Some(items) = raw.as_array() {
                let out: Vec<String> = items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToString::to_string)
                    .collect();
                return Some(out);
            }
            raw.as_str().map(parse_list_csv)
        })
        .unwrap_or_else(|| vec!["Unknown".to_string()]);
    let exports = normalize_exports(exports);

    db::LocationRow {
        id,
        slug,
        name,
        vault_path: normalize_relative_path_for_storage(relative_path),
        kind_type,
        kind_custom,
        visual_description: parsed
            .as_ref()
            .and_then(|value| value.get("visual_description").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        history_background: parsed
            .as_ref()
            .and_then(|value| value.get("history_background").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        exports: serde_json::to_string(&exports).unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        tone: parsed
            .as_ref()
            .and_then(|value| value.get("tone").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        authority: parsed
            .as_ref()
            .and_then(|value| value.get("authority").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        danger_level: parsed
            .as_ref()
            .and_then(|value| value.get("danger_level").and_then(toml::Value::as_str))
            .map(|value| normalize_location_danger_level(value).unwrap_or_else(|_| "Unknown".to_string()))
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: parsed
            .as_ref()
            .and_then(|value| value.get("current_tension").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at: parsed
            .as_ref()
            .and_then(|value| value.get("created_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| now.clone()),
        updated_at: parsed
            .as_ref()
            .and_then(|value| value.get("updated_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or(now),
    }
}

fn scan_faction_row_from_markdown(relative_path: &str, contents: &str) -> db::FactionRow {
    let parsed = extract_runebound_toml(contents)
        .and_then(|toml_text| toml::from_str::<toml::Value>(&toml_text).ok());
    let now = now_timestamp();
    let fallback_name = file_stem_name(relative_path);

    let name = parsed
        .as_ref()
        .and_then(|value| value.get("name").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&fallback_name)
        .to_string();
    let slug = parsed
        .as_ref()
        .and_then(|value| value.get("slug").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| slugify(&name));
    let id = parsed
        .as_ref()
        .and_then(|value| value.get("id").and_then(toml::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| stable_id_from_relative("fac", relative_path));

    let kind_raw = parsed
        .as_ref()
        .and_then(|value| {
            value
                .get("kind_type")
                .or_else(|| value.get("kind"))
                .and_then(toml::Value::as_str)
        })
        .unwrap_or("other");
    let kind_type = normalize_faction_kind_type(kind_raw).unwrap_or_else(|_| "other".to_string());

    let mut kind_custom = parsed
        .as_ref()
        .and_then(|value| value.get("kind_custom").and_then(toml::Value::as_str))
        .map(normalize_unknown_text);
    if kind_type == "other" && kind_custom.is_none() {
        kind_custom = Some("Unknown".to_string());
    }
    if kind_type != "other" {
        kind_custom = None;
    }

    let parse_list_field = |field: &str| {
        parsed
            .as_ref()
            .and_then(|value| value.get(field))
            .and_then(|raw| {
                if let Some(items) = raw.as_array() {
                    let out: Vec<String> = items
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(ToString::to_string)
                        .collect();
                    return Some(out);
                }
                raw.as_str().map(parse_list_csv)
            })
            .unwrap_or_else(|| vec!["Unknown".to_string()])
    };

    db::FactionRow {
        id,
        slug,
        name,
        vault_path: normalize_relative_path_for_storage(relative_path),
        kind_type,
        kind_custom,
        public_description: parsed
            .as_ref()
            .and_then(|value| value.get("public_description").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        true_agenda: parsed
            .as_ref()
            .and_then(|value| value.get("true_agenda").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        methods: parsed
            .as_ref()
            .and_then(|value| value.get("methods").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        leadership: parsed
            .as_ref()
            .and_then(|value| value.get("leadership").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        headquarters: parsed
            .as_ref()
            .and_then(|value| value.get("headquarters").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        sphere_of_influence: parsed
            .as_ref()
            .and_then(|value| value.get("sphere_of_influence").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        resources_assets: parsed
            .as_ref()
            .and_then(|value| value.get("resources_assets").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        allies: serde_json::to_string(&normalize_unknown_list(parse_list_field("allies")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        rivals_enemies: serde_json::to_string(&normalize_unknown_list(parse_list_field("rivals_enemies")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        reputation: parsed
            .as_ref()
            .and_then(|value| value.get("reputation").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: parsed
            .as_ref()
            .and_then(|value| value.get("current_tension").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        goals_short_term: serde_json::to_string(&normalize_unknown_list(parse_list_field("goals_short_term")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        goals_long_term: serde_json::to_string(&normalize_unknown_list(parse_list_field("goals_long_term")))
            .unwrap_or_else(|_| "[\"Unknown\"]".to_string()),
        symbol_description: parsed
            .as_ref()
            .and_then(|value| value.get("symbol_description").and_then(toml::Value::as_str))
            .map(normalize_unknown_text)
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at: parsed
            .as_ref()
            .and_then(|value| value.get("created_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| now.clone()),
        updated_at: parsed
            .as_ref()
            .and_then(|value| value.get("updated_at").and_then(toml::Value::as_str))
            .map(str::to_string)
            .unwrap_or(now),
    }
}

async fn sync_database_from_vault(
    workspace_root: &Path,
    database: Arc<Database>,
    npc_repo: Arc<dyn NpcRepository>,
    location_repo: Arc<dyn LocationRepository>,
    faction_repo: Arc<dyn FactionRepository>,
    document_repo: Arc<dyn DocumentRepository>,
) -> Result<(), String> {
    let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
    if !loaded.effective.vault.autoscan_on_start {
        return Ok(());
    }

    let Some(vault_path) = loaded.effective.vault.path.clone() else {
        return Ok(());
    };

    let vault = Vault::new(vault_path);
    vault.ensure_structure().map_err(|err| err.to_string())?;

    let npc_files = collect_markdown_files_under(vault.root(), "npcs")?;
    let mut scanned_npc_paths = HashSet::new();
    for (relative_path, contents) in npc_files {
        let row = scan_npc_row_from_markdown(&relative_path, &contents);
        scanned_npc_paths.insert(row.vault_path.clone());
        npc_repo.upsert(database.as_ref(), &row).await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "npc",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing_npcs = npc_repo.list_all(database.as_ref()).await?;
    for npc in existing_npcs {
        if npc.vault_path.starts_with("npcs/") && !scanned_npc_paths.contains(&npc.vault_path) {
            npc_repo.delete_by_id(database.as_ref(), &npc.id).await?;
            document_repo
                .delete_by_vault_path(database.as_ref(), &npc.vault_path)
                .await?;
        }
    }

    let location_files = collect_markdown_files_under(vault.root(), "locations")?;
    let mut scanned_location_paths = HashSet::new();
    for (relative_path, contents) in location_files {
        let row = scan_location_row_from_markdown(&relative_path, &contents);
        scanned_location_paths.insert(row.vault_path.clone());
        location_repo.upsert(database.as_ref(), &row).await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "location",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing_locations = location_repo.list_all(database.as_ref()).await?;
    for location in existing_locations {
        if location.vault_path.starts_with("locations/")
            && !scanned_location_paths.contains(&location.vault_path)
        {
            location_repo
                .delete_by_id(database.as_ref(), &location.id)
                .await?;
            document_repo
                .delete_by_vault_path(database.as_ref(), &location.vault_path)
                .await?;
        }
    }

    let faction_files = collect_markdown_files_under(vault.root(), "factions")?;
    let mut scanned_faction_paths = HashSet::new();
    for (relative_path, contents) in faction_files {
        let row = scan_faction_row_from_markdown(&relative_path, &contents);
        scanned_faction_paths.insert(row.vault_path.clone());
        faction_repo.upsert(database.as_ref(), &row).await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "faction",
                &row.slug,
                Some(&row.name),
                &row.vault_path,
                &row.created_at,
                &row.updated_at,
            )
            .await?;
    }

    let existing_factions = faction_repo.list_all(database.as_ref()).await?;
    for faction in existing_factions {
        if faction.vault_path.starts_with("factions/")
            && !scanned_faction_paths.contains(&faction.vault_path)
        {
            faction_repo
                .delete_by_id(database.as_ref(), &faction.id)
                .await?;
            document_repo
                .delete_by_vault_path(database.as_ref(), &faction.vault_path)
                .await?;
        }
    }

    Ok(())
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

    let database = state.database();
    let generation_repo = state.generation_repo();

    let user_prompt = input
        .prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("Generate one D&D NPC for a fantasy campaign.");

    let reference_context = if let Some(vault_path) = config.vault.path.clone() {
        let vault = Vault::new(vault_path);
        if vault.ensure_root_exists().is_ok() {
            match load_vault_reference_entries(&vault) {
                Ok(entries) => build_prompt_reference_context(user_prompt, &entries, &vault),
                Err(err) => {
                    eprintln!("reference context warning: {err}");
                    PromptReferenceContext::default()
                }
            }
        } else {
            PromptReferenceContext::default()
        }
    } else {
        PromptReferenceContext::default()
    };

    let recent_payloads = generation_repo
        .recent_prompts(database.as_ref(), "npc_seed", 20)
        .await?;
    let recent_seeds = parse_recent_npc_seeds(recent_payloads);
    let recent_names = recent_name_set(&recent_seeds);
    let recent_context = describe_recent_npc_seeds(&recent_seeds);
    let recent_occupation_anchors = recent_occupation_anchor_set(&recent_seeds);
    let recent_occupation_context = describe_recent_npc_occupation_anchors(&recent_seeds);

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
    let mut seen_attempt_occupation_anchors = std::collections::HashSet::new();

    for attempt in 0..5 {
        let base_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as i64)
            .unwrap_or(0);
        let run_seed = (base_seed + i64::from(attempt)) as i32;
        let repair_note = if attempt == 0 {
            ""
        } else {
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names and occupations."
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
                        "You generate concise D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying. Background must be 1-3 coherent sentences. carrying must be an array of item strings. Age should be years, height should be imperial like 5'11\", weight_lbs should be lbs as text like 180. Prefer occupations different from recent occupations and avoid occupation roots in this list unless explicitly requested: {}. Avoid these recent seeds: {}.{}{}",
                        recent_occupation_context,
                        recent_context,
                        repair_note,
                        if reference_context.system_context.is_empty() {
                            String::new()
                        } else {
                            format!("\n\n{}", reference_context.system_context)
                        },
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
        let occupation_anchor = occupation_anchor(&seed.occupation);
        if occupation_anchor != "unknown"
            && (recent_occupation_anchors.contains(&occupation_anchor)
                || seen_attempt_occupation_anchors.contains(&occupation_anchor))
        {
            continue;
        }
        seen_attempt_names.insert(normalized_name);
        seen_attempt_occupation_anchors.insert(occupation_anchor);

        let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
        generation_repo
            .insert(database.as_ref(), "npc_seed", None, &serialized_seed)
            .await?;

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
    let database = state.database();
    let generation_repo = state.generation_repo();
    let (recent_occupation_anchors, recent_occupation_context) = if field == "occupation" {
        let recent_payloads = generation_repo
            .recent_prompts(database.as_ref(), "npc_seed", 20)
            .await?;
        let recent_seeds = parse_recent_npc_seeds(recent_payloads);
        (
            recent_occupation_anchor_set(&recent_seeds),
            describe_recent_npc_occupation_anchors(&recent_seeds),
        )
    } else {
        (HashSet::new(), "none".to_string())
    };
    let current_occupation_anchor = occupation_anchor(&input.npc.occupation);
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

    let mut seen_attempt_occupation_anchors = HashSet::new();

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
                    "content": format!(
                        "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}",
                        if field == "occupation" {
                            " For occupation rerolls, avoid repeating occupation roots seen in recent NPC generations unless the user explicitly asks for one."
                        } else {
                            ""
                        }
                    )
                },
                {
                    "role": "user",
                    "content": format!(
                        "NPC context: {}\nField to reroll: {}\nInstruction: {}\nRecent occupation roots to avoid: {}\nOptional shaping prompt: {}",
                        context_summary,
                        field,
                        field_instructions,
                        if field == "occupation" { &recent_occupation_context } else { "(n/a)" },
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

        if field == "occupation" {
            let anchor = occupation_anchor(&normalized);
            if anchor != "unknown"
                && (anchor == current_occupation_anchor
                    || recent_occupation_anchors.contains(&anchor)
                    || seen_attempt_occupation_anchors.contains(&anchor))
            {
                continue;
            }
            if anchor != "unknown" {
                seen_attempt_occupation_anchors.insert(anchor);
            }
        }

        return Ok(RerollNpcFieldResult {
            field: field.to_string(),
            value: Some(normalized),
            carrying: None,
        });
    }

    Err(format!("failed to reroll npc field: {}", field))
}

async fn generate_location_seed(
    input: GenerateLocationSeedInput,
    state: tauri::State<'_, AppState>,
) -> Result<LocationSeed, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let config = loaded.effective;
    let model = config
        .ollama
        .model
        .clone()
        .ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

    let database = state.database();
    let generation_repo = state.generation_repo();

    let user_prompt = input
        .prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("Generate one distinct fantasy location for a D&D campaign.");

    let recent_payloads = generation_repo
        .recent_prompts(database.as_ref(), "location_seed", 20)
        .await?;
    let recent_seeds = parse_recent_location_seeds(recent_payloads);
    let recent_names = recent_location_name_set(&recent_seeds);
    let recent_context = describe_recent_location_seeds(&recent_seeds);

    let schema = serde_json::json!({
        "type": "object",
        "required": [
            "name",
            "kind_type",
            "visual_description",
            "history_background",
            "exports",
            "tone",
            "authority",
            "danger_level",
            "current_tension"
        ],
        "properties": {
            "name": { "type": "string", "minLength": 1 },
            "kind_type": { "type": "string", "enum": LOCATION_KIND_TYPES },
            "kind_custom": { "type": ["string", "null"] },
            "visual_description": { "type": "string", "minLength": 1 },
            "history_background": { "type": "string", "minLength": 1 },
            "exports": {
                "type": "array",
                "minItems": 1,
                "maxItems": 3,
                "items": { "type": "string", "minLength": 1 }
            },
            "tone": { "type": "string", "minLength": 1 },
            "authority": { "type": "string", "minLength": 1 },
            "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
            "current_tension": { "type": "string", "minLength": 1 }
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
                "temperature": 1.08,
                "top_p": 0.93,
                "repeat_penalty": 1.14,
                "seed": run_seed
            },
            "messages": [
                {
                    "role": "system",
                    "content": format!(
                        "You generate concise, usable D&D location seeds. Return only JSON with fields name, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension. visual_description must be 1-3 sentences. history_background must be 2-5 sentences. exports must have 1-3 short items. tone must be 2-5 words. current_tension must be 1-2 sentences. If kind_type is not other, kind_custom must be null. Avoid these recent seeds: {}.{}",
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

        let parsed: Result<LocationSeed, _> = serde_json::from_str(content);
        let Ok(seed) = parsed else {
            continue;
        };

        let seed = match normalize_location_seed(seed) {
            Ok(seed) => seed,
            Err(_) => continue,
        };
        if validate_location_details(&seed).is_err() {
            continue;
        }

        let normalized_name = seed.name.to_ascii_lowercase();
        if recent_names.contains(&normalized_name) || seen_attempt_names.contains(&normalized_name) {
            continue;
        }
        seen_attempt_names.insert(normalized_name);

        let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
        generation_repo
            .insert(database.as_ref(), "location_seed", None, &serialized_seed)
            .await?;

        return Ok(seed);
    }

    Err("failed to generate valid structured location output from ollama".to_string())
}

async fn reroll_location_field(
    input: RerollLocationFieldInput,
    state: tauri::State<'_, AppState>,
) -> Result<RerollLocationFieldResult, String> {
    let field = canonical_location_reroll_field(&input.field)?;
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

    let context_summary = location_context_summary(&input.location);
    let field_instructions = match field {
        "name" => "Generate a concise, fitting fantasy location name.",
        "kind_type" => {
            "Generate one kind_type enum value from: hamlet, town, city, dungeon, hideout, ruin, guildhall, landmark, wilderness, other."
        }
        "kind_custom" => "Generate a concise custom kind label for this location.",
        "visual_description" => "Generate a visual description in 1-3 sentences.",
        "history_background" => "Generate a history/background in 2-5 sentences.",
        "exports" => "Generate 1-3 exports as concise industry or specialty item strings.",
        "tone" => "Generate a mood tone in 2-5 words.",
        "authority" => "Generate who controls or governs this location.",
        "danger_level" => "Generate danger_level as one of: Unknown, safe, guarded, risky, deadly.",
        "current_tension" => "Generate current_tension in 1-2 sentences.",
        _ => "Generate a concise field value.",
    };

    let schema = if field == "exports" {
        serde_json::json!({
            "type": "object",
            "required": ["exports"],
            "properties": {
                "exports": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 3,
                    "items": { "type": "string", "minLength": 1 }
                }
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
                "temperature": 1.03,
                "top_p": 0.92,
                "repeat_penalty": 1.12,
                "seed": run_seed
            },
            "messages": [
                {
                    "role": "system",
                    "content": "You update one location field for a game master. Return only valid JSON matching schema. Keep it coherent with context."
                },
                {
                    "role": "user",
                    "content": format!(
                        "Location context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
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

        if field == "exports" {
            let Some(items) = parsed.get("exports").and_then(|item| item.as_array()) else {
                continue;
            };
            let next = normalize_exports(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|value| value.to_string()))
                    .collect(),
            );
            if next.is_empty() || next.len() > 3 {
                continue;
            }
            if attempt < 3 && next == normalize_exports(input.location.exports.clone()) {
                continue;
            }
            return Ok(RerollLocationFieldResult {
                field: field.to_string(),
                value: None,
                exports: Some(next),
            });
        }

        let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
            continue;
        };

        let normalized = match field {
            "kind_type" => match normalize_location_kind_type(raw_value) {
                Ok(value) => value,
                Err(_) => continue,
            },
            "danger_level" => match normalize_location_danger_level(raw_value) {
                Ok(value) => value,
                Err(_) => continue,
            },
            _ => normalize_unknown_text(raw_value),
        };

        let current = match field {
            "name" => input.location.name.clone(),
            "kind_type" => input.location.kind_type.clone(),
            "kind_custom" => input.location.kind_custom.clone().unwrap_or_default(),
            "visual_description" => input.location.visual_description.clone(),
            "history_background" => input.location.history_background.clone(),
            "tone" => input.location.tone.clone(),
            "authority" => input.location.authority.clone(),
            "danger_level" => input.location.danger_level.clone(),
            "current_tension" => input.location.current_tension.clone(),
            _ => String::new(),
        };

        if attempt < 3 && normalized.eq_ignore_ascii_case(current.trim()) {
            continue;
        }

        return Ok(RerollLocationFieldResult {
            field: field.to_string(),
            value: Some(normalized),
            exports: None,
        });
    }

    Err(format!("failed to reroll location field: {}", field))
}

async fn generate_faction_seed(
    input: GenerateFactionSeedInput,
    state: tauri::State<'_, AppState>,
) -> Result<FactionSeed, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let config = loaded.effective;
    let model = config
        .ollama
        .model
        .clone()
        .ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

    let database = state.database();
    let generation_repo = state.generation_repo();

    let user_prompt = input
        .prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("Generate one distinct fantasy faction for a D&D campaign.");

    let reference_context = if let Some(vault_path) = config.vault.path.clone() {
        let vault = Vault::new(vault_path);
        if vault.ensure_root_exists().is_ok() {
            match load_vault_reference_entries(&vault) {
                Ok(entries) => build_prompt_reference_context(user_prompt, &entries, &vault),
                Err(err) => {
                    eprintln!("reference context warning: {err}");
                    PromptReferenceContext::default()
                }
            }
        } else {
            PromptReferenceContext::default()
        }
    } else {
        PromptReferenceContext::default()
    };

    let recent_payloads = generation_repo
        .recent_prompts(database.as_ref(), "faction_seed", 20)
        .await?;
    let recent_seeds = parse_recent_faction_seeds(recent_payloads);
    let recent_names = recent_faction_name_set(&recent_seeds);
    let recent_context = describe_recent_faction_seeds(&recent_seeds);
    let enforce_unique_name = reference_context.system_context.is_empty();

    let schema = serde_json::json!({
        "type": "object",
        "required": [
            "name", "kind_type", "public_description", "true_agenda", "methods", "leadership", "headquarters", "sphere_of_influence", "resources_assets", "allies", "rivals_enemies", "reputation", "current_tension", "goals_short_term", "goals_long_term", "symbol_description"
        ],
        "properties": {
            "name": { "type": "string", "minLength": 1 },
            "kind_type": { "type": "string", "enum": FACTION_KIND_TYPES },
            "kind_custom": { "type": ["string", "null"] },
            "public_description": { "type": "string", "minLength": 1 },
            "true_agenda": { "type": "string", "minLength": 1 },
            "methods": { "type": "string", "minLength": 1 },
            "leadership": { "type": "string", "minLength": 1 },
            "headquarters": { "type": "string", "minLength": 1 },
            "sphere_of_influence": { "type": "string", "minLength": 1 },
            "resources_assets": { "type": "string", "minLength": 1 },
            "allies": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
            "rivals_enemies": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
            "reputation": { "type": "string", "minLength": 1 },
            "current_tension": { "type": "string", "minLength": 1 },
            "goals_short_term": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
            "goals_long_term": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
            "symbol_description": { "type": "string", "minLength": 1 }
        },
        "additionalProperties": false
    });

    let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.ollama.timeout_seconds))
        .build()
        .map_err(|err| err.to_string())?;

    let mut seen_attempt_names = HashSet::new();

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
                "temperature": 1.08,
                "top_p": 0.93,
                "repeat_penalty": 1.12,
                "seed": run_seed
            },
            "messages": [
                {
                    "role": "system",
                    "content": format!(
                        "You generate concise, usable D&D faction seeds. Return only JSON with fields name, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description. public_description, true_agenda, and methods should be 1-3 sentences. current_tension should be 1-2 sentences. symbol_description should be exactly 1 sentence describing symbol/sigil/colors/banner/iconography. If kind_type is not other, kind_custom must be null. If referenced vault metadata includes an established name for an organization, group, guild, or house, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {}.{}{}",
                        recent_context,
                        repair_note,
                        if reference_context.system_context.is_empty() {
                            String::new()
                        } else {
                            format!("\n\n{}", reference_context.system_context)
                        },
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

        let parsed: Result<FactionSeed, _> = serde_json::from_str(content);
        let Ok(seed) = parsed else {
            continue;
        };

        let seed = match normalize_faction_seed(seed) {
            Ok(seed) => seed,
            Err(_) => continue,
        };
        if validate_faction_details(&seed).is_err() {
            continue;
        }

        let normalized_name = seed.name.to_ascii_lowercase();
        if enforce_unique_name
            && (recent_names.contains(&normalized_name)
                || seen_attempt_names.contains(&normalized_name))
        {
            continue;
        }
        if enforce_unique_name {
            seen_attempt_names.insert(normalized_name);
        }

        let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
        generation_repo
            .insert(database.as_ref(), "faction_seed", None, &serialized_seed)
            .await?;

        return Ok(seed);
    }

    Err("failed to generate valid structured faction output from ollama".to_string())
}

async fn reroll_faction_field(
    input: RerollFactionFieldInput,
    state: tauri::State<'_, AppState>,
) -> Result<RerollFactionFieldResult, String> {
    let field = canonical_faction_reroll_field(&input.field)?;
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

    let context_summary = faction_context_summary(&input.faction);
    let field_instructions = match field {
        "name" => "Generate a concise fantasy faction name.",
        "kind_type" => "Generate one kind_type enum value from: guild, cult, military_order, noble_house, criminal_syndicate, mercantile_league, religious_order, arcane_circle, revolutionary_cell, other.",
        "kind_custom" => "Generate a concise custom faction kind label.",
        "public_description" => "Generate a public-facing description in 1-3 sentences.",
        "true_agenda" => "Generate the hidden agenda in 1-3 sentences.",
        "methods" => "Generate methods in 1-3 concise sentences.",
        "leadership" => "Generate concise leadership details.",
        "headquarters" => "Generate concise headquarters details.",
        "sphere_of_influence" => "Generate concise sphere of influence details.",
        "resources_assets" => "Generate concise resources/assets details.",
        "allies" => "Generate 1-5 ally strings.",
        "rivals_enemies" => "Generate 1-5 rival or enemy strings.",
        "reputation" => "Generate concise public reputation.",
        "current_tension" => "Generate current tension in 1-2 sentences.",
        "goals_short_term" => "Generate 1-5 short-term goals.",
        "goals_long_term" => "Generate 1-5 long-term goals.",
        "symbol_description" => "Generate exactly 1 sentence describing symbol/sigil/colors/banner/iconography.",
        _ => "Generate a concise field value.",
    };

    let schema = if ["allies", "rivals_enemies", "goals_short_term", "goals_long_term"]
        .contains(&field)
    {
        serde_json::json!({
            "type": "object",
            "required": ["list"],
            "properties": {
                "list": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 5,
                    "items": { "type": "string", "minLength": 1 }
                }
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
                "temperature": 1.03,
                "top_p": 0.92,
                "repeat_penalty": 1.1,
                "seed": run_seed
            },
            "messages": [
                {
                    "role": "system",
                    "content": "You update one faction field for a game master. Return only valid JSON matching schema. Keep it coherent with context."
                },
                {
                    "role": "user",
                    "content": format!(
                        "Faction context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
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

        if ["allies", "rivals_enemies", "goals_short_term", "goals_long_term"].contains(&field) {
            let Some(items) = parsed.get("list").and_then(|item| item.as_array()) else {
                continue;
            };
            let next = normalize_unknown_list(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|value| value.to_string()))
                    .collect(),
            );
            let current = match field {
                "allies" => input.faction.allies.clone(),
                "rivals_enemies" => input.faction.rivals_enemies.clone(),
                "goals_short_term" => input.faction.goals_short_term.clone(),
                "goals_long_term" => input.faction.goals_long_term.clone(),
                _ => Vec::new(),
            };
            if attempt < 3 && next == normalize_unknown_list(current) {
                continue;
            }
            return Ok(RerollFactionFieldResult {
                field: field.to_string(),
                value: None,
                list_value: Some(next),
            });
        }

        let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
            continue;
        };
        let normalized = if field == "kind_type" {
            match normalize_faction_kind_type(raw_value) {
                Ok(value) => value,
                Err(_) => continue,
            }
        } else {
            normalize_unknown_text(raw_value)
        };

        let current = match field {
            "name" => input.faction.name.clone(),
            "kind_type" => input.faction.kind_type.clone(),
            "kind_custom" => input.faction.kind_custom.clone().unwrap_or_default(),
            "public_description" => input.faction.public_description.clone(),
            "true_agenda" => input.faction.true_agenda.clone(),
            "methods" => input.faction.methods.clone(),
            "leadership" => input.faction.leadership.clone(),
            "headquarters" => input.faction.headquarters.clone(),
            "sphere_of_influence" => input.faction.sphere_of_influence.clone(),
            "resources_assets" => input.faction.resources_assets.clone(),
            "reputation" => input.faction.reputation.clone(),
            "current_tension" => input.faction.current_tension.clone(),
            "symbol_description" => input.faction.symbol_description.clone(),
            _ => String::new(),
        };

        if attempt < 3 && normalized.eq_ignore_ascii_case(current.trim()) {
            continue;
        }

        return Ok(RerollFactionFieldResult {
            field: field.to_string(),
            value: Some(normalized),
            list_value: None,
        });
    }

    Err(format!("failed to reroll faction field: {}", field))
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
    state.vault_repo().ensure_structure(&vault)?;

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

    let database = state.database();
    let location_repo = state.location_repo();
    let document_repo = state.document_repo();
    let slug = slugify(raw_name);
    let existing = location_repo
        .find_by_slug(database.as_ref(), &slug)
        .await?;

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
        normalize_relative_path_for_storage(&row.vault_path)
    } else {
        unique_markdown_path_for_name(&vault, "locations", &canonical_name, None)?
    };
    let file_exists = vault
        .resolve_relative(&PathBuf::from(&relative_path))
        .map_err(|err| err.to_string())?
        .exists();

    if !file_exists {
        let default_exports = vec!["Unknown".to_string()];
        let content = render_location_markdown(&LocationFrontmatter {
            doc_type: "location".to_string(),
            id: id.clone(),
            slug: slug.clone(),
            name: canonical_name.clone(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: default_exports.clone(),
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
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
        kind_type: existing
            .as_ref()
            .map(|row| row.kind_type.clone())
            .unwrap_or_else(|| "other".to_string()),
        kind_custom: existing
            .as_ref()
            .and_then(|row| row.kind_custom.clone())
            .or_else(|| Some("Unknown".to_string())),
        visual_description: existing
            .as_ref()
            .map(|row| row.visual_description.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        history_background: existing
            .as_ref()
            .map(|row| row.history_background.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        exports: existing
            .as_ref()
            .map(|row| row.exports.clone())
            .unwrap_or_else(|| "[\"Unknown\"]".to_string()),
        tone: existing
            .as_ref()
            .map(|row| row.tone.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        authority: existing
            .as_ref()
            .map(|row| row.authority.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        danger_level: existing
            .as_ref()
            .map(|row| row.danger_level.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: existing
            .as_ref()
            .map(|row| row.current_tension.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at,
        updated_at: now.clone(),
    };

    location_repo
        .upsert(database.as_ref(), &row)
        .await?;
    document_repo
        .upsert_index(
            database.as_ref(),
            "location",
            &row.slug,
            Some(&row.name),
            &row.vault_path,
            &row.created_at,
            &row.updated_at,
        )
        .await?;

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

    let database = state.database();
    let npc_repo = state.npc_repo();
    let document_repo = state.document_repo();
    let now = now_timestamp();

    let existing = npc_repo
        .find_by_id(database.as_ref(), input.id.trim())
        .await?;

    let (slug, relative_path, created_at, previous_path) = if let Some(current) = existing {
        let current_vault_path = normalize_relative_path_for_storage(&current.vault_path);
        let desired_base_slug = slugify(name);
        let desired_path = unique_markdown_path_for_name(
            &vault,
            "npcs",
            name,
            Some(current_vault_path.as_str()),
        )?;

        if desired_base_slug == current.slug {
            (
                current.slug,
                desired_path.clone(),
                current.created_at,
                if desired_path == current_vault_path {
                    None
                } else {
                    Some(current_vault_path)
                },
            )
        } else {
            let next_slug = unique_slug_for_dir(vault.root(), "npcs", &desired_base_slug);
            (
                next_slug,
                desired_path,
                current.created_at,
                Some(current_vault_path),
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

    npc_repo
        .upsert(database.as_ref(), &npc_row)
        .await?;
    document_repo
        .upsert_index(
            database.as_ref(),
            "npc",
            &npc_row.slug,
            Some(&npc_row.name),
            &npc_row.vault_path,
            &npc_row.created_at,
            &npc_row.updated_at,
        )
        .await?;

    if let Some(old_path) = previous_path {
        if old_path != npc_row.vault_path {
            document_repo
                .delete_by_vault_path(database.as_ref(), &old_path)
                .await?;

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
    let previous_vault_path_input = normalize_relative_path_for_storage(input.vault_path.trim());

    let kind_type = normalize_location_kind_type(&input.kind_type)?;
    let mut kind_custom = input.kind_custom.map(|value| normalize_unknown_text(&value));
    if kind_type == "other" {
        if kind_custom
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err("kind_custom is required when kind_type is other".to_string());
        }
    } else {
        kind_custom = None;
    }
    let visual_description = normalize_unknown_text(&input.visual_description);
    let history_background = normalize_unknown_text(&input.history_background);
    let exports = normalize_exports(input.exports);
    let tone = normalize_unknown_text(&input.tone);
    let authority = normalize_unknown_text(&input.authority);
    let danger_level = normalize_location_danger_level(&input.danger_level)?;
    let current_tension = normalize_unknown_text(&input.current_tension);

    validate_location_details(&LocationSeed {
        name: name.to_string(),
        kind_type: kind_type.clone(),
        kind_custom: kind_custom.clone(),
        visual_description: visual_description.clone(),
        history_background: history_background.clone(),
        exports: exports.clone(),
        tone: tone.clone(),
        authority: authority.clone(),
        danger_level: danger_level.clone(),
        current_tension: current_tension.clone(),
    })?;
    let exports_db = exports_to_db_text(&exports)?;

    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let location_repo = state.location_repo();
    let document_repo = state.document_repo();
    let now = now_timestamp();
    let existing = location_repo
        .find_by_id(database.as_ref(), input.id.trim())
        .await?;
    let (slug, relative_path, created_at, previous_path) = if let Some(current) = existing {
        let current_vault_path = normalize_relative_path_for_storage(&current.vault_path);
        let desired_base_slug = slugify(name);
        let desired_path = unique_markdown_path_for_name(
            &vault,
            "locations",
            name,
            Some(current_vault_path.as_str()),
        )?;

        if desired_base_slug == current.slug {
            (
                current.slug,
                desired_path.clone(),
                current.created_at,
                if desired_path == current_vault_path {
                    None
                } else {
                    Some(current_vault_path)
                },
            )
        } else {
            (
                unique_slug_for_dir(vault.root(), "locations", &desired_base_slug),
                desired_path,
                current.created_at,
                Some(current_vault_path),
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
        kind_type: kind_type.clone(),
        kind_custom: kind_custom.clone(),
        visual_description: visual_description.clone(),
        history_background: history_background.clone(),
        exports: exports.clone(),
        tone: tone.clone(),
        authority: authority.clone(),
        danger_level: danger_level.clone(),
        current_tension: current_tension.clone(),
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
        kind_type,
        kind_custom,
        visual_description,
        history_background,
        exports: exports_db,
        tone,
        authority,
        danger_level,
        current_tension,
        created_at: created_at.clone(),
        updated_at: now.clone(),
    };

    location_repo
        .upsert(database.as_ref(), &location_row)
        .await?;
    document_repo
        .upsert_index(
            database.as_ref(),
            "location",
            &location_row.slug,
            Some(&location_row.name),
            &location_row.vault_path,
            &location_row.created_at,
            &location_row.updated_at,
        )
        .await?;

    if let Some(old_path) = previous_path {
        if old_path != location_row.vault_path {
            document_repo
                .delete_by_vault_path(database.as_ref(), &old_path)
                .await?;

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

async fn save_faction_draft(
    input: SaveFactionDraftInput,
    state: tauri::State<'_, AppState>,
) -> Result<SaveFactionDraftResult, String> {
    if input.id.trim().is_empty() {
        return Err("faction id cannot be empty".to_string());
    }
    if input.name.trim().is_empty() {
        return Err("faction name cannot be empty".to_string());
    }

    let kind_type = normalize_faction_kind_type(&input.kind_type)?;
    let kind_custom = if kind_type == "other" {
        let value = input
            .kind_custom
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "kind_custom is required when kind_type is other".to_string())?;
        Some(value.to_string())
    } else {
        None
    };

    let public_description = normalize_unknown_text(&input.public_description);
    let true_agenda = normalize_unknown_text(&input.true_agenda);
    let methods = normalize_unknown_text(&input.methods);
    let leadership = normalize_unknown_text(&input.leadership);
    let headquarters = normalize_unknown_text(&input.headquarters);
    let sphere_of_influence = normalize_unknown_text(&input.sphere_of_influence);
    let resources_assets = normalize_unknown_text(&input.resources_assets);
    let allies = normalize_unknown_list(input.allies);
    let rivals_enemies = normalize_unknown_list(input.rivals_enemies);
    let reputation = normalize_unknown_text(&input.reputation);
    let current_tension = normalize_unknown_text(&input.current_tension);
    let goals_short_term = normalize_unknown_list(input.goals_short_term);
    let goals_long_term = normalize_unknown_list(input.goals_long_term);
    let symbol_description = normalize_unknown_text(&input.symbol_description);

    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let now = now_timestamp();
    let existing = faction_repo
        .find_by_id(database.as_ref(), input.id.trim())
        .await?;

    let provided_relative_path = normalize_relative_path_for_storage(input.vault_path.trim());
    let previous_path = if let Some(row) = existing.as_ref() {
        Some(normalize_relative_path_for_storage(&row.vault_path))
    } else if !provided_relative_path.is_empty() {
        Some(provided_relative_path)
    } else {
        None
    };
    let created_at = existing
        .as_ref()
        .map(|row| row.created_at.clone())
        .unwrap_or_else(|| now.clone());

    let provided_slug = input.slug.trim();
    let base_slug = if provided_slug.is_empty() {
        slugify(&input.name)
    } else {
        slugify(provided_slug)
    };
    let slug;
    if let Some(row) = existing.as_ref() {
        if row.slug != base_slug {
            slug = base_slug;
        } else {
            slug = row.slug.clone();
        }
    } else {
        slug = unique_slug_for_dir(vault.root(), "factions", &base_slug);
    }

    let desired_path = unique_markdown_path_for_name(
        &vault,
        "factions",
        &input.name,
        previous_path.as_deref(),
    )?;

    let frontmatter = FactionFrontmatter {
        doc_type: "faction".to_string(),
        id: input.id.trim().to_string(),
        slug: slug.clone(),
        name: input.name.trim().to_string(),
        kind_type: kind_type.clone(),
        kind_custom: kind_custom.clone(),
        public_description: public_description.clone(),
        true_agenda: true_agenda.clone(),
        methods: methods.clone(),
        leadership: leadership.clone(),
        headquarters: headquarters.clone(),
        sphere_of_influence: sphere_of_influence.clone(),
        resources_assets: resources_assets.clone(),
        allies: allies.clone(),
        rivals_enemies: rivals_enemies.clone(),
        reputation: reputation.clone(),
        current_tension: current_tension.clone(),
        goals_short_term: goals_short_term.clone(),
        goals_long_term: goals_long_term.clone(),
        symbol_description: symbol_description.clone(),
        created_at: created_at.clone(),
        updated_at: now.clone(),
    };

    let runebound_block = render_faction_markdown(&frontmatter).map_err(|err| err.to_string())?;
    let existing_file = read_vault_file_if_exists(&vault, &desired_path)?;
    let content = if let Some(current) = existing_file {
        merge_runebound_block(&current, &runebound_block)
    } else {
        runebound_block
    };
    vault
        .write_relative(&PathBuf::from(&desired_path), &content)
        .map_err(|err| err.to_string())?;

    let allies_db = faction_list_to_db_text(&allies)?;
    let rivals_db = faction_list_to_db_text(&rivals_enemies)?;
    let goals_short_db = faction_list_to_db_text(&goals_short_term)?;
    let goals_long_db = faction_list_to_db_text(&goals_long_term)?;

    let faction_row = db::FactionRow {
        id: input.id.trim().to_string(),
        slug,
        name: input.name.trim().to_string(),
        vault_path: desired_path,
        kind_type,
        kind_custom,
        public_description,
        true_agenda,
        methods,
        leadership,
        headquarters,
        sphere_of_influence,
        resources_assets,
        allies: allies_db,
        rivals_enemies: rivals_db,
        reputation,
        current_tension,
        goals_short_term: goals_short_db,
        goals_long_term: goals_long_db,
        symbol_description,
        created_at: created_at.clone(),
        updated_at: now.clone(),
    };

    faction_repo
        .upsert(database.as_ref(), &faction_row)
        .await?;
    document_repo
        .upsert_index(
            database.as_ref(),
            "faction",
            &faction_row.slug,
            Some(&faction_row.name),
            &faction_row.vault_path,
            &faction_row.created_at,
            &faction_row.updated_at,
        )
        .await?;

    if let Some(old_path) = previous_path {
        if old_path != faction_row.vault_path {
            document_repo
                .delete_by_vault_path(database.as_ref(), &old_path)
                .await?;
            if let Ok(old_full_path) = vault.resolve_relative(&PathBuf::from(&old_path)) {
                if old_full_path.exists() {
                    std::fs::remove_file(&old_full_path).map_err(|err| {
                        format!(
                            "failed to remove old faction file {}: {}",
                            old_full_path.display(),
                            err
                        )
                    })?;
                }
            }
        }
    }

    Ok(SaveFactionDraftResult {
        id: faction_row.id,
        slug: faction_row.slug,
        vault_path: faction_row.vault_path,
        created_at: faction_row.created_at,
        updated_at: faction_row.updated_at,
    })
}

async fn search_entities(
    state: &AppState,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<EntitySuggestion>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let limit = i64::from(limit.unwrap_or(8)).clamp(1, 20);
    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();

    let npcs = npc_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let locations = location_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let factions = faction_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;

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
        .chain(factions.into_iter().map(|faction| EntitySuggestion {
            entity_type: EntityType::Faction,
            name: faction.name,
            slug: faction.slug,
        }))
        .collect();

    items.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    items.truncate(limit as usize);
    Ok(items)
}

async fn search_location_names(
    state: &AppState,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<String>, String> {
    let limit = i64::from(limit.unwrap_or(8)).clamp(1, 20);
    let database = state.database();
    let location_repo = state.location_repo();
    let rows = location_repo
        .search_by_name(database.as_ref(), query.trim(), limit)
        .await?;

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let name = row.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let key = name.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(name);
        }
    }

    Ok(out)
}

async fn resolve_entity(input: String, state: &AppState) -> Result<Option<EntityDetails>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();

    if let Some(npc) = npc_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
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
            vault_path: normalize_relative_path_for_storage(&npc.vault_path),
            kind_type: None,
            kind_custom: None,
            visual_description: None,
            history_background: None,
            exports: None,
            tone: None,
            authority: None,
            danger_level: None,
            current_tension: None,
            public_description: None,
            true_agenda: None,
            methods: None,
            leadership: None,
            headquarters: None,
            sphere_of_influence: None,
            resources_assets: None,
            allies: None,
            rivals_enemies: None,
            reputation: None,
            goals_short_term: None,
            goals_long_term: None,
            symbol_description: None,
            created_at: Some(npc.created_at),
        }));
    }

    if let Some(location) = location_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
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
            vault_path: normalize_relative_path_for_storage(&location.vault_path),
            kind_type: Some(location.kind_type),
            kind_custom: location.kind_custom,
            visual_description: Some(location.visual_description),
            history_background: Some(location.history_background),
            exports: Some(exports_from_db_text(&location.exports)),
            tone: Some(location.tone),
            authority: Some(location.authority),
            danger_level: Some(location.danger_level),
            current_tension: Some(location.current_tension),
            public_description: None,
            true_agenda: None,
            methods: None,
            leadership: None,
            headquarters: None,
            sphere_of_influence: None,
            resources_assets: None,
            allies: None,
            rivals_enemies: None,
            reputation: None,
            goals_short_term: None,
            goals_long_term: None,
            symbol_description: None,
            created_at: Some(location.created_at),
        }));
    }

    if let Some(faction) = faction_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: faction.id,
            entity_type: EntityType::Faction,
            name: faction.name,
            slug: faction.slug,
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
            vault_path: normalize_relative_path_for_storage(&faction.vault_path),
            kind_type: Some(faction.kind_type),
            kind_custom: faction.kind_custom,
            visual_description: None,
            history_background: None,
            exports: None,
            tone: None,
            authority: None,
            danger_level: None,
            current_tension: Some(faction.current_tension),
            public_description: Some(faction.public_description),
            true_agenda: Some(faction.true_agenda),
            methods: Some(faction.methods),
            leadership: Some(faction.leadership),
            headquarters: Some(faction.headquarters),
            sphere_of_influence: Some(faction.sphere_of_influence),
            resources_assets: Some(faction.resources_assets),
            allies: Some(faction_list_from_db_text(&faction.allies)),
            rivals_enemies: Some(faction_list_from_db_text(&faction.rivals_enemies)),
            reputation: Some(faction.reputation),
            goals_short_term: Some(faction_list_from_db_text(&faction.goals_short_term)),
            goals_long_term: Some(faction_list_from_db_text(&faction.goals_long_term)),
            symbol_description: Some(faction.symbol_description),
            created_at: Some(faction.created_at),
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
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let soft_delete_repo = state.soft_delete_repo();
    let now = now_timestamp();

    if let Some(npc) = npc_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&npc.vault_path);
        let trash_path = unique_trash_path(&vault, "npcs", &npc.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        npc_repo
            .delete_by_id(database.as_ref(), &npc.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &npc.vault_path)
            .await?;

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
            vault_path: normalized_vault_path.clone(),
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
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Npc,
            id: npc.id,
            name: npc.name,
            slug: npc.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(location) = location_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&location.vault_path);
        let trash_path = unique_trash_path(&vault, "locations", &location.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        location_repo
            .delete_by_id(database.as_ref(), &location.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &location.vault_path)
            .await?;

        let payload = LocationDeletePayload {
            id: location.id.clone(),
            slug: location.slug.clone(),
            name: location.name.clone(),
            vault_path: normalized_vault_path.clone(),
            kind_type: location.kind_type,
            kind_custom: location.kind_custom,
            visual_description: location.visual_description,
            history_background: location.history_background,
            exports: location.exports,
            tone: location.tone,
            authority: location.authority,
            danger_level: location.danger_level,
            current_tension: location.current_tension,
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
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Location,
            id: location.id,
            name: location.name,
            slug: location.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(faction) = faction_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&faction.vault_path);
        let trash_path = unique_trash_path(&vault, "factions", &faction.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        faction_repo
            .delete_by_id(database.as_ref(), &faction.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &faction.vault_path)
            .await?;

        let payload = FactionDeletePayload {
            id: faction.id.clone(),
            slug: faction.slug.clone(),
            name: faction.name.clone(),
            vault_path: normalized_vault_path.clone(),
            kind_type: faction.kind_type,
            kind_custom: faction.kind_custom,
            public_description: faction.public_description,
            true_agenda: faction.true_agenda,
            methods: faction.methods,
            leadership: faction.leadership,
            headquarters: faction.headquarters,
            sphere_of_influence: faction.sphere_of_influence,
            resources_assets: faction.resources_assets,
            allies: faction.allies,
            rivals_enemies: faction.rivals_enemies,
            reputation: faction.reputation,
            current_tension: faction.current_tension,
            goals_short_term: faction.goals_short_term,
            goals_long_term: faction.goals_long_term,
            symbol_description: faction.symbol_description,
            created_at: faction.created_at,
            updated_at: faction.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "faction".to_string(),
            entity_id: faction.id.clone(),
            name: faction.name.clone(),
            slug: faction.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Faction,
            id: faction.id,
            name: faction.name,
            slug: faction.slug,
            trash_vault_path: trash_path,
        });
    }

    Err(format!("no npc, location, or faction found for: {target}"))
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
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let soft_delete_repo = state.soft_delete_repo();

    let Some(soft_delete) = soft_delete_repo
        .latest_pending(database.as_ref())
        .await?
    else {
        return Err("nothing to undo".to_string());
    };

    let now = now_timestamp();

    if soft_delete.entity_type == "npc" {
        let payload: NpcDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "npcs", &restored_slug);
            restored_vault_path = unique_markdown_path_for_name(&vault, "npcs", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

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

        npc_repo
            .upsert(database.as_ref(), &npc_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "npc",
                &npc_row.slug,
                Some(&npc_row.name),
                &npc_row.vault_path,
                &npc_row.created_at,
                &npc_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

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
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "locations", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "locations", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let location_row = db::LocationRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            kind_type: payload.kind_type,
            kind_custom: payload.kind_custom,
            visual_description: payload.visual_description,
            history_background: payload.history_background,
            exports: payload.exports,
            tone: payload.tone,
            authority: payload.authority,
            danger_level: payload.danger_level,
            current_tension: payload.current_tension,
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        location_repo
            .upsert(database.as_ref(), &location_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "location",
                &location_row.slug,
                Some(&location_row.name),
                &location_row.vault_path,
                &location_row.created_at,
                &location_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Location,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    if soft_delete.entity_type == "faction" {
        let payload: FactionDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "factions", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "factions", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let faction_row = db::FactionRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            kind_type: payload.kind_type,
            kind_custom: payload.kind_custom,
            public_description: payload.public_description,
            true_agenda: payload.true_agenda,
            methods: payload.methods,
            leadership: payload.leadership,
            headquarters: payload.headquarters,
            sphere_of_influence: payload.sphere_of_influence,
            resources_assets: payload.resources_assets,
            allies: payload.allies,
            rivals_enemies: payload.rivals_enemies,
            reputation: payload.reputation,
            current_tension: payload.current_tension,
            goals_short_term: payload.goals_short_term,
            goals_long_term: payload.goals_long_term,
            symbol_description: payload.symbol_description,
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        faction_repo
            .upsert(database.as_ref(), &faction_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "faction",
                &faction_row.slug,
                Some(&faction_row.name),
                &faction_row.vault_path,
                &faction_row.created_at,
                &faction_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Faction,
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

#[cfg(test)]
mod tests {
    use super::{
        ActiveReferenceQuery, LocationSeed, VaultReferenceEntry,
        build_reference_suggestions_from_entries, extract_active_reference_query,
        extract_prompt_reference_keys, extract_runebound_toml, normalize_input_for_dispatch,
        normalize_location_seed, normalize_relative_path_for_storage, npc_travel_location_query,
        occupation_anchor, path_for_display, recent_occupation_anchor_set,
        scan_npc_row_from_markdown, validate_location_details,
        describe_recent_npc_occupation_anchors,
    };

    #[test]
    fn dispatch_preserves_windows_backslashes() {
        let input = r"set vault C:\Users\andrewk9\Documents\DND";
        assert_eq!(normalize_input_for_dispatch(input), input);
    }

    #[test]
    fn dispatch_only_unwraps_markdown_backticks() {
        let input = "  `set vault C:\\Users\\andrewk9\\Documents\\DND`  ";
        assert_eq!(
            normalize_input_for_dispatch(input),
            r"set vault C:\Users\andrewk9\Documents\DND"
        );
    }

    #[test]
    fn normalizes_storage_paths_to_forward_slashes() {
        assert_eq!(
            normalize_relative_path_for_storage(r"npcs\grave cleric.md"),
            "npcs/grave cleric.md"
        );
    }

    #[test]
    fn displays_paths_with_host_separator() {
        let displayed = path_for_display("locations/frostholm.md");
        if std::path::MAIN_SEPARATOR == '\\' {
            assert_eq!(displayed, r"locations\frostholm.md");
        } else {
            assert_eq!(displayed, "locations/frostholm.md");
        }
    }

    #[test]
    fn location_seed_requires_custom_kind_for_other() {
        let seed = LocationSeed {
            name: "Gloomreach".to_string(),
            kind_type: "other".to_string(),
            kind_custom: None,
            visual_description: "Moss-slick walls drip in torchlight.".to_string(),
            history_background: "Built by exiles. Later seized by smugglers.".to_string(),
            exports: vec!["amber resin".to_string()],
            tone: "wet tense".to_string(),
            authority: "Smuggler council".to_string(),
            danger_level: "risky".to_string(),
            current_tension: "A rival gang stalks the tunnels.".to_string(),
        };

        let err = normalize_location_seed(seed).expect_err("expected missing kind_custom error");
        assert!(err.contains("kind_custom"));
    }

    #[test]
    fn location_seed_validation_accepts_unknown_backcompat_values() {
        let seed = LocationSeed {
            name: "Unknown Hold".to_string(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: vec!["Unknown".to_string()],
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
        };

        validate_location_details(&seed).expect("expected Unknown defaults to pass validation");
    }

    #[test]
    fn extracts_runebound_toml_block() {
        let markdown = "# Note\n\n```runebound\ntype = \"npc\"\nname = \"Aelar\"\n```\n";
        let block = extract_runebound_toml(markdown).expect("expected runebound block");
        assert!(block.contains("type = \"npc\""));
        assert!(block.contains("name = \"Aelar\""));
    }

    #[test]
    fn indexes_npc_from_filename_without_runebound_block() {
        let row = scan_npc_row_from_markdown("npcs/Father Elen.md", "# Existing notes");
        assert_eq!(row.name, "Father Elen");
        assert_eq!(row.slug, "father-elen");
        assert_eq!(row.vault_path, "npcs/Father Elen.md");
    }

    #[test]
    fn extracts_active_reference_query_from_tail() {
        let input = "create npc a duke for @locations/Aegis";
        let active = extract_active_reference_query(input).expect("expected active reference");
        assert_eq!(active.at_index, 22);
        assert_eq!(active.query, "locations/Aegis");
    }

    #[test]
    fn does_not_treat_email_as_reference_query() {
        let input = "create npc envoy named a@b";
        let active = extract_active_reference_query(input);
        assert!(active.is_none());
    }

    #[test]
    fn prompt_reference_matching_prefers_longest_entry() {
        let entries = vec![
            VaultReferenceEntry {
                key: "locations/Aegis".to_string(),
                key_lower: "locations/aegis".to_string(),
                markdown_path: Some("locations/Aegis.md".to_string()),
                is_dir: false,
            },
            VaultReferenceEntry {
                key: "locations/Aegis Isle".to_string(),
                key_lower: "locations/aegis isle".to_string(),
                markdown_path: Some("locations/Aegis Isle.md".to_string()),
                is_dir: false,
            },
        ];

        let found = extract_prompt_reference_keys(
            "create npc a duke for @locations/Aegis Isle during winter",
            &entries,
        );
        assert_eq!(found, vec!["locations/Aegis Isle"]);
    }

    #[test]
    fn prompt_reference_matching_supports_multiple_mentions() {
        let entries = vec![
            VaultReferenceEntry {
                key: "locations/Aegis Isle".to_string(),
                key_lower: "locations/aegis isle".to_string(),
                markdown_path: Some("locations/Aegis Isle.md".to_string()),
                is_dir: false,
            },
            VaultReferenceEntry {
                key: "npcs/Lady Aisling Everlynn".to_string(),
                key_lower: "npcs/lady aisling everlynn".to_string(),
                markdown_path: Some("npcs/Lady Aisling Everlynn.md".to_string()),
                is_dir: false,
            },
        ];

        let found = extract_prompt_reference_keys(
            "create npc sibling of @npcs/Lady Aisling Everlynn from @locations/Aegis Isle",
            &entries,
        );
        assert_eq!(
            found,
            vec![
                "npcs/Lady Aisling Everlynn".to_string(),
                "locations/Aegis Isle".to_string(),
            ]
        );
    }

    #[test]
    fn empty_reference_query_suggests_top_level_directories() {
        let entries = vec![
            VaultReferenceEntry {
                key: "locations/".to_string(),
                key_lower: "locations/".to_string(),
                markdown_path: None,
                is_dir: true,
            },
            VaultReferenceEntry {
                key: "npcs/".to_string(),
                key_lower: "npcs/".to_string(),
                markdown_path: None,
                is_dir: true,
            },
            VaultReferenceEntry {
                key: "locations/Aegis Isle".to_string(),
                key_lower: "locations/aegis isle".to_string(),
                markdown_path: Some("locations/Aegis Isle.md".to_string()),
                is_dir: false,
            },
        ];

        let active = ActiveReferenceQuery {
            at_index: 11,
            query: String::new(),
        };
        let suggestions = build_reference_suggestions_from_entries("create npc @", &active, &entries);
        let labels: Vec<String> = suggestions.into_iter().map(|item| item.label).collect();

        assert_eq!(labels, vec!["@locations/".to_string(), "@npcs/".to_string()]);
    }

    #[test]
    fn occupation_anchor_ignores_descriptive_fillers() {
        assert_eq!(
            occupation_anchor("former cartographer, current wanderer"),
            "cartographer"
        );
        assert_eq!(occupation_anchor("Cartographer & explorer (deceased)"), "cartographer");
    }

    #[test]
    fn recent_occupation_anchor_set_collects_unique_roots() {
        let seeds = vec![
            super::NpcSeed {
                name: "A".to_string(),
                race: "Human".to_string(),
                occupation: "former cartographer, current wanderer".to_string(),
                sex: "male".to_string(),
                age: "30".to_string(),
                height: "5'10\"".to_string(),
                weight_lbs: "170".to_string(),
                background: "Unknown".to_string(),
                want_need: "Unknown".to_string(),
                secret_obstacle: "Unknown".to_string(),
                carrying: vec!["Unknown".to_string()],
            },
            super::NpcSeed {
                name: "B".to_string(),
                race: "Elf".to_string(),
                occupation: "cartographer & explorer (deceased)".to_string(),
                sex: "female".to_string(),
                age: "29".to_string(),
                height: "5'8\"".to_string(),
                weight_lbs: "130".to_string(),
                background: "Unknown".to_string(),
                want_need: "Unknown".to_string(),
                secret_obstacle: "Unknown".to_string(),
                carrying: vec!["Unknown".to_string()],
            },
        ];

        let anchors = recent_occupation_anchor_set(&seeds);
        assert_eq!(anchors.len(), 1);
        assert!(anchors.contains("cartographer"));
    }

    #[test]
    fn describe_recent_occupation_anchors_is_compact_and_unique() {
        let seeds = vec![
            super::NpcSeed {
                name: "A".to_string(),
                race: "Human".to_string(),
                occupation: "former cartographer".to_string(),
                sex: "male".to_string(),
                age: "30".to_string(),
                height: "5'10\"".to_string(),
                weight_lbs: "170".to_string(),
                background: "Unknown".to_string(),
                want_need: "Unknown".to_string(),
                secret_obstacle: "Unknown".to_string(),
                carrying: vec!["Unknown".to_string()],
            },
            super::NpcSeed {
                name: "B".to_string(),
                race: "Elf".to_string(),
                occupation: "cartographer and explorer".to_string(),
                sex: "female".to_string(),
                age: "29".to_string(),
                height: "5'8\"".to_string(),
                weight_lbs: "130".to_string(),
                background: "Unknown".to_string(),
                want_need: "Unknown".to_string(),
                secret_obstacle: "Unknown".to_string(),
                carrying: vec!["Unknown".to_string()],
            },
        ];

        let described = describe_recent_npc_occupation_anchors(&seeds);
        assert_eq!(described, "cartographer");
    }

    #[test]
    fn parses_npc_travel_location_query_for_typeahead() {
        assert_eq!(
            npc_travel_location_query("npc travel to Aegis Isle"),
            Some("Aegis Isle".to_string())
        );
        assert_eq!(npc_travel_location_query("npc travel to"), Some(String::new()));
        assert_eq!(npc_travel_location_query("npc travel"), None);
    }
}

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let database = tauri::async_runtime::block_on(db::init_database())
        .expect("failed to initialize sqlite database");
    let database = Arc::new(database);

    let vault_repo: Arc<dyn VaultRepository> = Arc::new(ProdVaultRepository);
    let npc_repo: Arc<dyn NpcRepository> = Arc::new(ProdNpcRepository);
    let location_repo: Arc<dyn LocationRepository> = Arc::new(ProdLocationRepository);
    let faction_repo: Arc<dyn FactionRepository> = Arc::new(ProdFactionRepository);
    let document_repo: Arc<dyn DocumentRepository> = Arc::new(ProdDocumentRepository);
    let generation_repo: Arc<dyn GenerationRepository> = Arc::new(ProdGenerationRepository);
    let soft_delete_repo: Arc<dyn SoftDeleteRepository> = Arc::new(ProdSoftDeleteRepository);

    if let Err(err) = tauri::async_runtime::block_on(sync_database_from_vault(
        &workspace_root,
        database.clone(),
        npc_repo.clone(),
        location_repo.clone(),
        faction_repo.clone(),
        document_repo.clone(),
    )) {
        eprintln!("startup vault sync skipped: {err}");
    }

    let command_service = dnd_core::service::CommandService::new(workspace_root.clone());

    tauri::Builder::default()
        .manage(AppState {
            workspace_root,
            command_service: Mutex::new(command_service),
            editor_session: Mutex::new(EditorSession::default()),
            database: database.clone(),
            vault_repo: vault_repo.clone(),
            npc_repo: npc_repo.clone(),
            location_repo: location_repo.clone(),
            faction_repo: faction_repo.clone(),
            document_repo: document_repo.clone(),
            generation_repo: generation_repo.clone(),
            soft_delete_repo: soft_delete_repo.clone(),
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
