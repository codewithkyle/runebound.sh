use crate::repositories::{Database, GenerationRepository};
use crate::services::ai_generation::{
    build_reference_context,
    describe_recent_npc_occupation_anchors,
    occupation_anchor,
    parse_recent_npc_seeds,
    recent_occupation_anchor_set,
};
use crate::services::ollama_chat::{
    attempt_seed, build_chat_client, load_generation_config, post_chat_for_content,
};
use crate::utils::{
    normalize_exports,
    normalize_faction_kind_type,
    normalize_item_category,
    normalize_item_rarity,
    normalize_location_danger_level,
    normalize_location_kind_type,
    normalize_sex,
    normalize_unknown_list,
    normalize_unknown_text,
};
use dnd_core::config::AppConfig;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Resolve any `@references` in a custom reroll prompt into an authoritative
/// setting-context block appended to the system message. Returns an empty string
/// when the prompt is blank or references nothing, so a plain reroll is unchanged.
fn resolve_reference_suffix(
    config: &AppConfig,
    extra_prompt: &str,
    workspace_root: &Path,
) -> String {
    if extra_prompt.is_empty() {
        return String::new();
    }
    let context = build_reference_context(config, extra_prompt, workspace_root);
    if context.system_context.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", context.system_context)
    }
}

pub struct EntityRerollService;

impl EntityRerollService {
    pub async fn reroll_npc_field(
        &self,
        input: RerollNpcFieldInput,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollNpcFieldResult, String> {
        let field = canonical_npc_reroll_field(&input.field)?;
        let (config, model) = load_generation_config(workspace_root)?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = npc_context_summary(&input.npc);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);
        let (recent_occupation_anchors, recent_occupation_context) = if field == "occupation" {
            let recent_payloads = generation_repo
                .recent_prompts(database, "npc_seed", 20)
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

        let (client, url) = build_chat_client(&config)?;

        let mut seen_attempt_occupation_anchors = HashSet::new();

        for attempt in 0..4 {
            let run_seed = attempt_seed(attempt);

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
                            "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
                            if field == "occupation" {
                                " For occupation rerolls, avoid repeating occupation roots seen in recent NPC generations unless the user explicitly asks for one."
                            } else {
                                ""
                            },
                            reference_suffix
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

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: serde_json::Value = match serde_json::from_str(&content) {
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

    pub async fn reroll_location_field(
        &self,
        input: RerollLocationFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollLocationFieldResult, String> {
        let field = canonical_location_reroll_field(&input.field)?;
        let (config, model) = load_generation_config(workspace_root)?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = location_context_summary(&input.location);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);
        let field_instructions = match field {
            "name" => "Generate a concise, fitting fantasy location name.",
            "kind_type" => "Generate one kind_type enum value from: hamlet, town, city, dungeon, hideout, ruin, guildhall, landmark, wilderness, other.",
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

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..4 {
            let run_seed = attempt_seed(attempt);

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
                        "content": format!(
                            "You update one location field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}",
                            reference_suffix
                        )
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

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: serde_json::Value = match serde_json::from_str(&content) {
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

    pub async fn reroll_faction_field(
        &self,
        input: RerollFactionFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollFactionFieldResult, String> {
        let field = canonical_faction_reroll_field(&input.field)?;
        let (config, model) = load_generation_config(workspace_root)?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = faction_context_summary(&input.faction);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);
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

        let schema = if [
            "allies",
            "rivals_enemies",
            "goals_short_term",
            "goals_long_term",
        ]
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

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..4 {
            let run_seed = attempt_seed(attempt);

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
                        "content": format!(
                            "You update one faction field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}",
                            reference_suffix
                        )
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

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: serde_json::Value = match serde_json::from_str(&content) {
                Ok(parsed) => parsed,
                Err(_) => continue,
            };

            if [
                "allies",
                "rivals_enemies",
                "goals_short_term",
                "goals_long_term",
            ]
            .contains(&field)
            {
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

    pub async fn reroll_item_field(
        &self,
        input: RerollItemFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollItemFieldResult, String> {
        let field = canonical_item_reroll_field(&input.field)?;
        let (config, model) = load_generation_config(workspace_root)?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = item_context_summary(&input.item);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);
        let field_instructions = match field {
            "name" => "Generate a concise, evocative item name.",
            "category" => "Generate one category from: weapon, armor, consumable, wondrous, arcane_focus, tool, trinket, other.",
            "rarity" => "Generate rarity as one of: unknown, common, uncommon, rare, very_rare, legendary, artifact.",
            "attunement" => "Describe attunement requirements in a short phrase (or 'None').",
            "materials" => "List 1-4 notable materials as concise strings.",
            "appearance" => "Describe appearance in 1-2 sentences.",
            "abilities" => "Describe abilities/powers in 1-3 sentences.",
            "drawbacks" => "Describe drawbacks/costs in up to 2 sentences (or 'None').",
            "history" => "Describe history/origin in 1-3 sentences.",
            "value" => "Provide estimated value in format like '1000gp' or '250sp' or '50cp' (amount + currency suffix).",
            "location" => "Provide current location or hiding place.",
            _ => "Generate a concise field value.",
        };

        let schema = if field == "materials" {
            serde_json::json!({
                "type": "object",
                "required": ["materials"],
                "properties": {
                    "materials": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 4,
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

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..4 {
            let run_seed = attempt_seed(attempt);

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": {
                    "temperature": 1.02,
                    "top_p": 0.92,
                    "repeat_penalty": 1.1,
                    "seed": run_seed
                },
                "messages": [
                    {
                        "role": "system",
                        "content": format!(
                            "You update one RPG item field. Return only valid JSON matching the schema.{}",
                            reference_suffix
                        )
                    },
                    {
                        "role": "user",
                        "content": format!(
                            "Item context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
                            context_summary,
                            field,
                            field_instructions,
                            if extra_prompt.is_empty() { "(none)" } else { extra_prompt }
                        )
                    }
                ]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: serde_json::Value = match serde_json::from_str(&content) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if field == "materials" {
                let Some(items) = parsed.get("materials").and_then(|item| item.as_array()) else {
                    continue;
                };
                let next = normalize_unknown_list(
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(|value| value.to_string()))
                        .collect(),
                );
                if attempt < 3 && next == input.item.materials {
                    continue;
                }
                return Ok(RerollItemFieldResult {
                    field: field.to_string(),
                    value: None,
                    materials: Some(next),
                });
            }

            let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                continue;
            };
            let normalized = match field {
                "category" => normalize_item_category(raw_value)?,
                "rarity" => normalize_item_rarity(raw_value)?,
                _ => normalize_unknown_text(raw_value),
            };

            if attempt < 3 {
                let matches_current = match field {
                    "name" => normalized.eq_ignore_ascii_case(input.item.name.trim()),
                    "category" => normalized == input.item.category,
                    "rarity" => normalized == input.item.rarity,
                    "attunement" => normalized.eq_ignore_ascii_case(input.item.attunement.trim()),
                    "appearance" => normalized.eq_ignore_ascii_case(input.item.appearance.trim()),
                    "abilities" => normalized.eq_ignore_ascii_case(input.item.abilities.trim()),
                    "drawbacks" => normalized.eq_ignore_ascii_case(input.item.drawbacks.trim()),
                    "history" => normalized.eq_ignore_ascii_case(input.item.history.trim()),
                    "value" => normalized.eq_ignore_ascii_case(input.item.value.trim()),
                    "location" => normalized.eq_ignore_ascii_case(input.item.location.trim()),
                    _ => false,
                };
                if matches_current {
                    continue;
                }
            }

            return Ok(RerollItemFieldResult {
                field: field.to_string(),
                value: Some(normalized),
                materials: None,
            });
        }

        Err(format!("failed to reroll item field: {}", field))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollNpcFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub npc: NpcRerollContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollNpcFieldResult {
    pub field: String,
    pub value: Option<String>,
    pub carrying: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocationRerollContext {
    pub name: String,
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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollLocationFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub location: LocationRerollContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollLocationFieldResult {
    pub field: String,
    pub value: Option<String>,
    pub exports: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FactionRerollContext {
    pub name: String,
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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollFactionFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub faction: FactionRerollContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollFactionFieldResult {
    pub field: String,
    pub value: Option<String>,
    pub list_value: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollItemFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub item: ItemRerollContext,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ItemRerollContext {
    pub name: String,
    pub category: String,
    pub rarity: String,
    pub attunement: String,
    pub materials: Vec<String>,
    pub appearance: String,
    pub abilities: String,
    pub drawbacks: String,
    pub history: String,
    pub value: String,
    pub location: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollItemFieldResult {
    pub field: String,
    pub value: Option<String>,
    pub materials: Option<Vec<String>>,
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

fn canonical_item_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "category" => "category",
        "rarity" => "rarity",
        "attunement" | "attune" => "attunement",
        "materials" => "materials",
        "appearance" => "appearance",
        "abilities" | "ability" => "abilities",
        "drawbacks" | "drawback" => "drawbacks",
        "history" => "history",
        "value" => "value",
        "location" => "location",
        _ => {
            return Err(format!(
                "unknown item reroll field: {}. valid fields: name, category, rarity, attunement, materials, appearance, abilities, drawbacks, history, value, location",
                raw
            ))
        }
    };
    Ok(field)
}

fn item_context_summary(context: &ItemRerollContext) -> String {
    format!(
        "name={}, category={}, rarity={}, attunement={}, materials={}, appearance={}, abilities={}, drawbacks={}, history={}, value={}, location={}",
        context.name, context.category, context.rarity, context.attunement, context.materials.join(", "), context.appearance, context.abilities, context.drawbacks, context.history, context.value, context.location
    )
}
