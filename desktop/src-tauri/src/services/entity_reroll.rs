use crate::repositories::{Database, GenerationRepository};
use crate::services::ai_generation::{
    anchor_mechanic,
    build_reference_context,
    describe_recent_npc_occupation_anchors,
    occupation_anchor,
    parse_recent_npc_seeds,
    recent_occupation_anchor_set,
};
use crate::services::ollama_chat::{
    attempt_seed, build_chat_client, detail_directive, load_generation_config, post_chat_for_content,
};
use crate::utils::{
    normalize_exports,
    normalize_faction_kind_type,
    normalize_god_alignment,
    normalize_god_rank,
    normalize_item_category,
    normalize_item_rarity,
    normalize_location_danger_level,
    normalize_location_kind_type,
    normalize_sex,
    normalize_unknown_list,
    normalize_unknown_text,
};
use runebound_models::DungeonBeat;
use runebound_models::utils::{DUNGEON_FUNCTIONS, GOD_ALIGNMENTS, GOD_RANKS};
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
                            "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}{}",
                            if field == "occupation" {
                                " For occupation rerolls, avoid repeating occupation roots seen in recent NPC generations unless the user explicitly asks for one."
                            } else {
                                ""
                            },
                            reference_suffix,
                            detail_directive(config.generation.verbosity)
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
                            "You update one location field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
                            reference_suffix,
                            detail_directive(config.generation.verbosity)
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
                            "You update one faction field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
                            reference_suffix,
                            detail_directive(config.generation.verbosity)
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

    pub async fn reroll_god_field(
        &self,
        input: RerollGodFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollGodFieldResult, String> {
        let field = canonical_god_reroll_field(&input.field)?;
        let (config, model) = load_generation_config(workspace_root)?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = god_context_summary(&input.god);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);
        let rank_instruction = format!("Generate one rank enum value from: {}.", GOD_RANKS.join(", "));
        let alignment_instruction = format!("Generate one alignment enum value from: {}.", GOD_ALIGNMENTS.join(", "));
        let field_instructions: &str = match field {
            "name" => "Generate a concise fantasy deity name.",
            "epithet" => "Generate a short by-name or honorific (e.g. The Stormcaller).",
            "rank" => &rank_instruction,
            "rank_custom" => "Generate a concise custom divine rank label.",
            "alignment" => &alignment_instruction,
            "domains" => "Generate 1-5 divine domain strings (e.g. war, death, harvest).",
            "symbol" => "Generate exactly 1 sentence describing the holy symbol/sigil/iconography.",
            "appearance" => "Generate 1-3 sentences describing how the deity manifests.",
            "dogma" => "Generate core teachings/commandments in 1-3 sentences.",
            "realm" => "Generate a concise home plane or divine realm.",
            "worshippers" => "Generate a concise description of who venerates the deity.",
            "clergy" => "Generate a concise description of how the priesthood is organized.",
            "allies" => "Generate 1-5 allied deity or power strings.",
            "rivals" => "Generate 1-5 rival or enemy strings.",
            _ => "Generate a concise field value.",
        };

        let schema = if ["domains", "allies", "rivals"].contains(&field) {
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
                            "You update one deity field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
                            reference_suffix,
                            detail_directive(config.generation.verbosity)
                        )
                    },
                    {
                        "role": "user",
                        "content": format!(
                            "God context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
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

            if ["domains", "allies", "rivals"].contains(&field) {
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
                    "domains" => input.god.domains.clone(),
                    "allies" => input.god.allies.clone(),
                    "rivals" => input.god.rivals.clone(),
                    _ => Vec::new(),
                };
                if attempt < 3 && next == normalize_unknown_list(current) {
                    continue;
                }
                return Ok(RerollGodFieldResult {
                    field: field.to_string(),
                    value: None,
                    list_value: Some(next),
                });
            }

            let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                continue;
            };
            let normalized = match field {
                "rank" => match normalize_god_rank(raw_value) {
                    Ok(value) => value,
                    Err(_) => continue,
                },
                "alignment" => match normalize_god_alignment(raw_value) {
                    Ok(value) => value,
                    Err(_) => continue,
                },
                _ => normalize_unknown_text(raw_value),
            };

            let current = match field {
                "name" => input.god.name.clone(),
                "epithet" => input.god.epithet.clone(),
                "rank" => input.god.rank.clone(),
                "rank_custom" => input.god.rank_custom.clone().unwrap_or_default(),
                "alignment" => input.god.alignment.clone(),
                "symbol" => input.god.symbol.clone(),
                "appearance" => input.god.appearance.clone(),
                "dogma" => input.god.dogma.clone(),
                "realm" => input.god.realm.clone(),
                "worshippers" => input.god.worshippers.clone(),
                "clergy" => input.god.clergy.clone(),
                _ => String::new(),
            };

            if attempt < 3 && normalized.eq_ignore_ascii_case(current.trim()) {
                continue;
            }

            return Ok(RerollGodFieldResult {
                field: field.to_string(),
                value: Some(normalized),
                list_value: None,
            });
        }

        Err(format!("failed to reroll god field: {}", field))
    }

    /// Regenerate a single beat against the frozen rest of the dungeon. The other
    /// four beats are sent verbatim as context; only `beats[beat_index]` is
    /// rerolled. Both its `function` AND its rolled `content_type` stay fixed — the
    /// content type was deterministically assigned (see `dungeon_plan`) and the
    /// model is no good at picking it, so the reroll only rewrites the prose
    /// (idea, player_goals, lever, loot, design_note) for that same room type.
    pub async fn reroll_dungeon_beat(
        &self,
        input: RerollDungeonBeatInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollDungeonBeatResult, String> {
        let beat_index = input.beat_index;
        if beat_index >= DUNGEON_FUNCTIONS.len() {
            return Err("beat index out of range".to_string());
        }
        let current = input
            .dungeon
            .beats
            .get(beat_index)
            .cloned()
            .ok_or_else(|| "dungeon is missing the beat to reroll".to_string())?;
        let function = DUNGEON_FUNCTIONS[beat_index];

        let (config, model) = load_generation_config(workspace_root)?;
        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);

        let prev = if beat_index > 0 {
            DUNGEON_FUNCTIONS[beat_index - 1]
        } else {
            "the dungeon opening"
        };
        let next = if beat_index + 1 < DUNGEON_FUNCTIONS.len() {
            DUNGEON_FUNCTIONS[beat_index + 1]
        } else {
            "the payoff"
        };

        let frozen = dungeon_context_summary(&input.dungeon, Some(beat_index));

        // The rolled content type is authoritative — it is NOT regenerated. The
        // model only rewrites the prose for that same room type.
        let content_type = current.content_type.trim().to_string();
        let mechanic = anchor_mechanic(&content_type);

        let schema = serde_json::json!({
            "type": "object",
            "required": ["idea", "player_goals", "lever", "design_note"],
            "additionalProperties": false,
            "properties": {
                "idea": { "type": "string", "minLength": 1 },
                "player_goals": { "type": "string", "minLength": 1 },
                "lever": { "type": "string", "minLength": 1 },
                "loot": { "type": ["string", "null"] },
                "design_note": { "type": "string", "minLength": 1 }
            }
        });

        // A cache room always pays out, regardless of where it sits; otherwise the
        // function decides whether loot belongs here.
        let loot_rule = if content_type.eq_ignore_ascii_case("cache") {
            "Loot REQUIRED — name a concrete reward the party claims here."
        } else if function == "Resolution" || function == "Climax" {
            "This beat may carry loot (the payoff/boss hoard)."
        } else if function == "Setback" {
            "Set loot to null — the Setback is where players pay, not collect."
        } else {
            "Set loot to null."
        };

        let (client, url) = build_chat_client(&config)?;

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
                    "seed": run_seed,
                    "num_ctx": config.ollama.num_ctx
                },
                "messages": [
                    {
                        "role": "system",
                        "content": format!(
                            "You are a 5-room-dungeon oracle regenerating ONE beat for a game master. Return only JSON matching the schema. Keep each field tight and SPECIFIC BUT UNRESOLVED (a concrete spark, never the answer). idea is 1-2 sentences (for combat: tactics/behavior, never creature names). player_goals is one sentence — the clear, concrete goal for the players here (what they must learn, do, reach, or overcome). lever is one hook/question in 1-2 sentences. design_note is one sentence to the GM, out of fiction, on how this beat fits the overall dungeon and story. This beat is a room or area INSIDE the dungeon's single location (shown below) — keep it there; do not move the party to a new region, town, or building. {loot_rule} This beat's room type is FIXED as `{content_type}` ({mechanic}). Do NOT change the type — keep the idea squarely a {content_type} room; only the wording changes.{reference_suffix}",
                            loot_rule = loot_rule,
                            content_type = content_type,
                            mechanic = mechanic,
                            reference_suffix = reference_suffix
                        )
                    },
                    {
                        "role": "user",
                        "content": format!(
                            "Dungeon so far (the other four beats are frozen — stay coherent with them):\n{frozen}\n\nRegenerate ONLY beat {n} (function = {function}, type = {content_type} — keep this type). It must follow the {prev} beat and feed the {next} beat. Optional shaping prompt: {shape}",
                            frozen = frozen,
                            n = beat_index + 1,
                            function = function,
                            content_type = content_type,
                            prev = prev,
                            next = next,
                            shape = if extra_prompt.is_empty() { "(none)" } else { extra_prompt }
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

            let Some(idea) = parsed.get("idea").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(player_goals) = parsed.get("player_goals").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(lever) = parsed.get("lever").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(design_note) = parsed.get("design_note").and_then(|v| v.as_str()) else {
                continue;
            };
            let loot = parsed
                .get("loot")
                .and_then(|v| v.as_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"));

            return Ok(RerollDungeonBeatResult {
                beat: DungeonBeat {
                    function: function.to_string(),
                    content_type: content_type.clone(),
                    idea: normalize_unknown_text(idea),
                    player_goals: normalize_unknown_text(player_goals),
                    lever: normalize_unknown_text(lever),
                    loot,
                    design_note: normalize_unknown_text(design_note),
                    // Preserve the rolled overlay/faction tint across a single-beat
                    // reroll — only the prose changes, not the plan metadata.
                    overlay: current.overlay.clone(),
                    factions: current.factions,
                },
            });
        }

        Err(format!("failed to reroll dungeon beat {}", beat_index + 1))
    }

    /// Scalar reroll for the dungeon-level `premise` or `name` line, against the
    /// frozen rest of the dungeon (mirrors the item/god scalar reroll).
    pub async fn reroll_dungeon_field(
        &self,
        input: RerollDungeonFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollDungeonFieldResult, String> {
        let field = match input.field.trim().to_ascii_lowercase().as_str() {
            "premise" | "spine" => "premise",
            "location" | "place" => "location",
            "name" => "name",
            other => {
                return Err(format!(
                    "unknown dungeon reroll field: {}. rerollable fields: name, location, premise (or a beat name)",
                    other
                ))
            }
        };

        let (config, model) = load_generation_config(workspace_root)?;
        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt, workspace_root);
        let frozen = dungeon_context_summary(&input.dungeon, None);

        let instruction = match field {
            "premise" => "Generate a single-line spine summarizing the whole dungeon (one sentence; specific but unresolved).",
            "location" => "Generate the single bounded place all five beats sit inside — one short phrase naming one explorable location the party moves deeper into (e.g. 'a drowned bell-foundry'), never a region or a journey.",
            _ => "Generate a concise, evocative name for the dungeon.",
        };
        let current = match field {
            "premise" => input.dungeon.premise.clone(),
            "location" => input.dungeon.location.clone(),
            _ => input.dungeon.name.clone(),
        };

        let schema = serde_json::json!({
            "type": "object",
            "required": ["value"],
            "additionalProperties": false,
            "properties": { "value": { "type": "string", "minLength": 1 } }
        });

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..4 {
            let run_seed = attempt_seed(attempt);
            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.05, "top_p": 0.92, "repeat_penalty": 1.1, "seed": run_seed },
                "messages": [
                    {
                        "role": "system",
                        "content": format!(
                            "You update one dungeon field for a game master. Return only JSON matching schema. Keep it coherent with the dungeon below.{reference_suffix}",
                            reference_suffix = reference_suffix
                        )
                    },
                    {
                        "role": "user",
                        "content": format!(
                            "Dungeon: {frozen}\nField to reroll: {field}\nInstruction: {instruction}\nOptional shaping prompt: {shape}",
                            frozen = frozen,
                            field = field,
                            instruction = instruction,
                            shape = if extra_prompt.is_empty() { "(none)" } else { extra_prompt }
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
            let Some(raw_value) = parsed.get("value").and_then(|v| v.as_str()) else {
                continue;
            };
            let normalized = normalize_unknown_text(raw_value);
            if attempt < 3 && normalized.eq_ignore_ascii_case(current.trim()) {
                continue;
            }
            return Ok(RerollDungeonFieldResult {
                field: field.to_string(),
                value: Some(normalized),
            });
        }

        Err(format!("failed to reroll dungeon field: {}", field))
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
                            "You update one RPG item field. Return only valid JSON matching the schema.{}{}",
                            reference_suffix,
                            detail_directive(config.generation.verbosity)
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GodRerollContext {
    pub name: String,
    pub epithet: String,
    pub rank: String,
    pub rank_custom: Option<String>,
    pub alignment: String,
    pub domains: Vec<String>,
    pub symbol: String,
    pub appearance: String,
    pub dogma: String,
    pub realm: String,
    pub worshippers: String,
    pub clergy: String,
    pub allies: Vec<String>,
    pub rivals: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollGodFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub god: GodRerollContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollGodFieldResult {
    pub field: String,
    pub value: Option<String>,
    pub list_value: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonRerollContext {
    pub name: String,
    pub location: String,
    pub premise: String,
    pub topology: String,
    pub tone: String,
    pub twist: String,
    pub beats: Vec<DungeonBeat>,
}

impl DungeonRerollContext {
    pub fn from_draft(draft: &runebound_models::DungeonDraft) -> Self {
        Self {
            name: draft.name.clone(),
            location: draft.location.clone(),
            premise: draft.premise.clone(),
            topology: draft.topology.clone(),
            tone: draft.tone.clone(),
            twist: draft.twist.clone(),
            beats: draft.beats.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollDungeonBeatInput {
    pub beat_index: usize,
    pub prompt: Option<String>,
    pub dungeon: DungeonRerollContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollDungeonBeatResult {
    pub beat: DungeonBeat,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RerollDungeonFieldInput {
    pub field: String,
    pub prompt: Option<String>,
    pub dungeon: DungeonRerollContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RerollDungeonFieldResult {
    pub field: String,
    pub value: Option<String>,
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

fn canonical_god_reroll_field(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    let field = match normalized.as_str() {
        "name" => "name",
        "epithet" | "title" => "epithet",
        "rank" | "status" => "rank",
        "rank_custom" => "rank_custom",
        "alignment" | "align" => "alignment",
        "domains" | "portfolio" => "domains",
        "symbol" | "sigil" | "holy_symbol" => "symbol",
        "appearance" | "avatar" => "appearance",
        "dogma" | "tenets" | "creed" => "dogma",
        "realm" | "plane" => "realm",
        "worshippers" | "followers" => "worshippers",
        "clergy" | "priesthood" | "church" => "clergy",
        "allies" => "allies",
        "rivals" | "enemies" => "rivals",
        _ => {
            return Err(format!(
                "unknown god reroll field: {}. valid fields: name, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals",
                raw
            ))
        }
    };

    Ok(field)
}

fn god_context_summary(context: &GodRerollContext) -> String {
    format!(
        "name={}, epithet={}, rank={}, rank_custom={}, alignment={}, domains={}, symbol={}, appearance={}, dogma={}, realm={}, worshippers={}, clergy={}, allies={}, rivals={}",
        context.name,
        context.epithet,
        context.rank,
        context.rank_custom.clone().unwrap_or_else(|| "(none)".to_string()),
        context.alignment,
        context.domains.join(", "),
        context.symbol,
        context.appearance,
        context.dogma,
        context.realm,
        context.worshippers,
        context.clergy,
        context.allies.join(", "),
        context.rivals.join(", "),
    )
}

/// Serialize the spine, dials, topology, and the dungeon's beats for frozen
/// reroll context. When `skip_index` is `Some(i)`, beat `i` is marked as the one
/// being regenerated (its body is omitted) so the model rewrites only that beat
/// while staying coherent with the others.
fn dungeon_context_summary(
    context: &DungeonRerollContext,
    skip_index: Option<usize>,
) -> String {
    let mut lines = vec![
        format!("location (all beats are rooms/areas inside this one place): {}", context.location),
        format!("premise (spine): {}", context.premise),
        format!("tone: {}", context.tone),
        format!("twist: {}", context.twist),
        format!("topology: {}", context.topology),
    ];
    for (i, beat) in context.beats.iter().enumerate() {
        let function = DUNGEON_FUNCTIONS.get(i).copied().unwrap_or("Beat");
        if skip_index == Some(i) {
            lines.push(format!(
                "beat {} [{}] (THIS IS THE BEAT TO REGENERATE)",
                i + 1,
                function
            ));
        } else {
            lines.push(format!(
                "beat {} [{}] type={} | idea={} | player_goals={} | lever={} | loot={}",
                i + 1,
                function,
                beat.content_type,
                beat.idea,
                beat.player_goals,
                beat.lever,
                beat.loot.as_deref().unwrap_or("none"),
            ));
        }
    }
    lines.join("\n")
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
