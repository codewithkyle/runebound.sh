use serde::{Deserialize, Serialize};
use tauri::State;
use crate::app_state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcSeed {
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    pub carrying: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcRerollContext {
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    pub carrying: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerollNpcFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub npc: NpcRerollContext,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerollNpcFieldResult {
    pub field: String,
    pub value: Option<String>,
    pub carrying: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateNpcSeedInput {
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveNpcDraftInput {
    pub id: String,
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    pub carrying: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveNpcDraftResult {
    pub id: String,
    pub slug: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveLocationDraftInput {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    pub exports: Vec<String>,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveLocationDraftResult {
    pub id: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveFactionDraftInput {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    pub allies: Vec<String>,
    pub rivals_enemies: Vec<String>,
    pub reputation: String,
    pub current_tension: String,
    pub goals_short_term: Vec<String>,
    pub goals_long_term: Vec<String>,
    pub symbol_description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveFactionDraftResult {
    pub id: String,
    pub vault_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SoftDeleteEntityInput {
    pub entity_type: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoftDeleteEntityResult {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoSoftDeleteResult {
    pub success: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnsureLocationInput {
    pub location_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnsureLocationResult {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationRerollContext {
    pub name: String,
    pub kind_type: String,
    pub visual_description: String,
    pub history_background: String,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerollLocationFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub location: LocationRerollContext,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerollLocationFieldResult {
    pub field: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionRerollContext {
    pub name: String,
    pub kind_type: String,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub reputation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerollFactionFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub faction: FactionRerollContext,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerollFactionFieldResult {
    pub field: String,
    pub value: Option<String>,
}

pub fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" {
        Ok(normalized)
    } else {
        Err("sex must be one of: male, female".to_string())
    }
}

pub fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
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

pub fn parse_carrying_csv(value: &str) -> Vec<String> {
    let items: Vec<String> = value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    normalize_unknown_list(items)
}

pub fn normalize_optional_prompt(prompt: Option<String>) -> Option<String> {
    prompt.map(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed.to_string()
        }
    })
}

pub fn canonical_npc_reroll_field(field: &str) -> String {
    match field.to_ascii_lowercase().as_str() {
        "name" => "name".to_string(),
        "race" => "race".to_string(),
        "occupation" | "job" => "occupation".to_string(),
        "sex" | "gender" => "sex".to_string(),
        "age" => "age".to_string(),
        "height" => "height".to_string(),
        "weight" | "weight_lbs" => "weight_lbs".to_string(),
        "background" | "backstory" => "background".to_string(),
        "want" | "wantneed" | "want_need" | "wants" | "needs" => "want_need".to_string(),
        "secret" | "obstacle" | "secret_obstacle" => "secret_obstacle".to_string(),
        "carrying" | "items" | "equipment" => "carrying".to_string(),
        "location" => "location".to_string(),
        _ => field.to_string(),
    }
}

pub fn canonical_location_reroll_field(field: &str) -> String {
    match field.to_ascii_lowercase().as_str() {
        "name" => "name".to_string(),
        "kind" | "kind_type" => "kind_type".to_string(),
        "visual" | "visual_description" => "visual_description".to_string(),
        "history" | "history_background" => "history_background".to_string(),
        "tone" => "tone".to_string(),
        "authority" => "authority".to_string(),
        "danger" | "danger_level" => "danger_level".to_string(),
        "exports" => "exports".to_string(),
        "tension" | "current_tension" => "current_tension".to_string(),
        _ => field.to_string(),
    }
}

pub fn canonical_faction_reroll_field(field: &str) -> String {
    match field.to_ascii_lowercase().as_str() {
        "name" => "name".to_string(),
        "kind" | "kind_type" => "kind_type".to_string(),
        "description" | "public_description" => "public_description".to_string(),
        "agenda" | "true_agenda" => "true_agenda".to_string(),
        "methods" => "methods".to_string(),
        "leadership" => "leadership".to_string(),
        "headquarters" => "headquarters".to_string(),
        "sphere" | "sphere_of_influence" => "sphere_of_influence".to_string(),
        "resources" | "resources_assets" => "resources_assets".to_string(),
        "reputation" => "reputation".to_string(),
        "tension" | "current_tension" => "current_tension".to_string(),
        _ => field.to_string(),
    }
}

pub fn npc_context_summary(npc: &NpcRerollContext) -> String {
    let carrying_str = if npc.carrying.is_empty() {
        "nothing".to_string()
    } else {
        npc.carrying.join(", ")
    };
    format!(
        "{} the {} {} {} from {} (carrying: {})",
        npc.name, npc.age, npc.race, npc.occupation, npc.location, carrying_str
    )
}

pub fn location_context_summary(location: &LocationRerollContext) -> String {
    format!(
        "{} ({}) - danger: {}, tone: {}",
        location.name, location.kind_type, location.danger_level, location.tone
    )
}

pub fn faction_context_summary(faction: &FactionRerollContext) -> String {
    format!(
        "{} ({}) - reputation: {}",
        faction.name, faction.kind_type, faction.reputation
    )
}

pub fn normalize_location_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    let valid_types = ["hamlet", "town", "city", "dungeon", "hideout", "ruin", "guildhall", "landmark", "wilderness", "other"];
    if valid_types.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!("kind_type must be one of: {}", valid_types.join(", ")))
    }
}

pub fn normalize_location_danger_level(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let normalized = if trimmed.eq_ignore_ascii_case("unknown") {
        "Unknown".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    };
    let valid_levels = ["Unknown", "safe", "guarded", "risky", "deadly"];
    if valid_levels.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!("danger_level must be one of: {}", valid_levels.join(", ")))
    }
}

pub fn normalize_faction_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    let valid_types = ["guild", "cult", "military_order", "noble_house", "criminal_syndicate", "mercantile_league", "religious_order", "arcane_circle", "revolutionary_cell", "other"];
    if valid_types.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!("kind_type must be one of: {}", valid_types.join(", ")))
    }
}

pub fn parse_list_csv(value: &str) -> Vec<String> {
    value.split(',').map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect()
}

pub fn normalize_exports(values: Vec<String>) -> Vec<String> {
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

pub fn normalize_location_seed(mut seed: crate::services::ai_generation::LocationSeed) -> Result<crate::services::ai_generation::LocationSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_location_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed.kind_custom.as_ref().is_none_or(|value| value.trim().is_empty()) {
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

pub fn validate_location_details(seed: &crate::services::ai_generation::LocationSeed) -> Result<(), String> {
    if seed.name.trim().is_empty() {
        return Err("location name cannot be empty".to_string());
    }
    let vds = &seed.visual_description;
    let hbg = &seed.history_background;
    let ten = &seed.current_tension;
    let sentence_count = |s: &str| s.split_terminator(['.', '!', '?']).filter(|part| !part.trim().is_empty()).count();
    if *vds != "Unknown" {
        let count = sentence_count(vds);
        if count < 1 || count > 3 {
            return Err(format!("visual_description must be 1-3 sentences; got {}", count));
        }
    }
    if *hbg != "Unknown" {
        let count = sentence_count(hbg);
        if count < 2 || count > 5 {
            return Err(format!("history_background must be 2-5 sentences; got {}", count));
        }
    }
    if *ten != "Unknown" {
        let count = sentence_count(ten);
        if count < 1 || count > 2 {
            return Err(format!("current_tension must be 1-2 sentences; got {}", count));
        }
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
    let tone_words = seed.tone.split_whitespace().count();
    if seed.tone != "Unknown" && !(2..=5).contains(&tone_words) {
        return Err(format!("tone must be 2-5 words; got {}", tone_words));
    }
    Ok(())
}

pub fn normalize_faction_seed(mut seed: crate::services::ai_generation::FactionSeed) -> Result<crate::services::ai_generation::FactionSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_faction_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed.kind_custom.as_ref().is_none_or(|value| value.trim().is_empty()) {
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

pub fn validate_faction_details(seed: &crate::services::ai_generation::FactionSeed) -> Result<(), String> {
    if seed.name.trim().is_empty() {
        return Err("faction name cannot be empty".to_string());
    }
    let sentence_count = |s: &str| s.split_terminator(['.', '!', '?']).filter(|part| !part.trim().is_empty()).count();
    if seed.public_description != "Unknown" {
        let count = sentence_count(&seed.public_description);
        if count < 1 || count > 3 {
            return Err(format!("public_description must be 1-3 sentences; got {}", count));
        }
    }
    if seed.true_agenda != "Unknown" {
        let count = sentence_count(&seed.true_agenda);
        if count < 1 || count > 3 {
            return Err(format!("true_agenda must be 1-3 sentences; got {}", count));
        }
    }
    if seed.current_tension != "Unknown" {
        let count = sentence_count(&seed.current_tension);
        if count < 1 || count > 2 {
            return Err(format!("current_tension must be 1-2 sentences; got {}", count));
        }
    }
    if seed.symbol_description != "Unknown" {
        let count = sentence_count(&seed.symbol_description);
        if count != 1 {
            return Err(format!("symbol_description must be exactly 1 sentence; got {}", count));
        }
    }
    Ok(())
}

pub async fn reroll_npc_field(input: RerollNpcFieldInput, state: tauri::State<'_, AppState>) -> Result<RerollNpcFieldResult, String> {
    Err("reroll_npc_field not yet implemented in utils".to_string())
}

pub async fn reroll_location_field(input: RerollLocationFieldInput, state: tauri::State<'_, AppState>) -> Result<RerollLocationFieldResult, String> {
    Err("reroll_location_field not yet implemented in utils".to_string())
}

pub async fn reroll_faction_field(input: RerollFactionFieldInput, state: tauri::State<'_, AppState>) -> Result<RerollFactionFieldResult, String> {
    Err("reroll_faction_field not yet implemented in utils".to_string())
}

pub async fn save_npc_draft_impl(input: SaveNpcDraftInput, state: tauri::State<'_, AppState>) -> Result<SaveNpcDraftResult, String> {
    Err("save_npc_draft_impl not yet implemented in utils".to_string())
}

pub async fn save_location_draft_impl(input: SaveLocationDraftInput, state: tauri::State<'_, AppState>) -> Result<SaveLocationDraftResult, String> {
    Err("save_location_draft_impl not yet implemented in utils".to_string())
}

pub async fn save_faction_draft_impl(input: SaveFactionDraftInput, state: tauri::State<'_, AppState>) -> Result<SaveFactionDraftResult, String> {
    Err("save_faction_draft_impl not yet implemented in utils".to_string())
}

pub async fn ensure_location_exists(input: EnsureLocationInput, state: tauri::State<'_, AppState>) -> Result<EnsureLocationResult, String> {
    Err("ensure_location_exists not yet implemented in utils".to_string())
}

pub async fn resolve_entity(name: String) -> Result<Option<dnd_core::command::CommandClientEvent>, String> {
    Err("resolve_entity not yet implemented in utils".to_string())
}

pub async fn soft_delete_entity(input: SoftDeleteEntityInput) -> Result<SoftDeleteEntityResult, String> {
    Err("soft_delete_entity not yet implemented in utils".to_string())
}

pub async fn undo_last_soft_delete(entity_type: String, relative_path: String) -> Result<UndoSoftDeleteResult, String> {
    Err("undo_last_soft_delete not yet implemented in utils".to_string())
}