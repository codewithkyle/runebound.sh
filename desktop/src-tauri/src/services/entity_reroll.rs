use crate::entities::kind::EntityKind;
use crate::entities::schema::{FieldAccess, canonical_field_spec, format_valid_field_list};
use crate::repositories::{Database, GenerationRepository};
use crate::services::ai_generation::{
    anchor_mechanic, build_reference_context, describe_recent_npc_occupation_anchors,
    occupation_anchor, parse_recent_npc_seeds, recent_occupation_anchor_set,
};
use crate::services::ollama_chat::{
    attempt_seed, build_chat_client, detail_directive, load_generation_config,
    post_chat_for_content,
};
use crate::utils::{
    normalize_exports, normalize_faction_kind_type, normalize_god_alignment, normalize_god_rank,
    normalize_item_category, normalize_item_rarity, normalize_location_danger_level,
    normalize_location_kind_type, normalize_sex, normalize_unknown_list, normalize_unknown_text,
};
use dnd_core::config::AppConfig;
use runebound_models::DungeonBeat;
use runebound_models::utils::DUNGEON_FUNCTIONS;
use std::collections::HashSet;

/// Resolve any `@references` in a custom reroll prompt into an authoritative
/// setting-context block appended to the system message. Returns an empty string
/// when the prompt is blank or references nothing, so a plain reroll is unchanged.
fn resolve_reference_suffix(config: &AppConfig, extra_prompt: &str) -> String {
    if extra_prompt.is_empty() {
        return String::new();
    }
    let context = build_reference_context(config, extra_prompt);
    if context.system_context.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", context.system_context)
    }
}

/// LLM sampling knobs for a reroll request. Hoisted from the per-kind literals
/// that were repeated inline in every `reroll_*` payload (P2.5). The values differ
/// by kind on purpose — NPCs/dungeons run hotter than the more constrained
/// item/faction/god fields.
struct Sampling {
    temperature: f64,
    top_p: f64,
    repeat_penalty: f64,
}

const NPC_SAMPLING: Sampling = Sampling {
    temperature: 1.05,
    top_p: 0.92,
    repeat_penalty: 1.12,
};
const LOCATION_SAMPLING: Sampling = Sampling {
    temperature: 1.03,
    top_p: 0.92,
    repeat_penalty: 1.12,
};
const FACTION_SAMPLING: Sampling = Sampling {
    temperature: 1.03,
    top_p: 0.92,
    repeat_penalty: 1.1,
};
const GOD_SAMPLING: Sampling = Sampling {
    temperature: 1.03,
    top_p: 0.92,
    repeat_penalty: 1.1,
};
const ITEM_SAMPLING: Sampling = Sampling {
    temperature: 1.02,
    top_p: 0.92,
    repeat_penalty: 1.1,
};
const DUNGEON_BEAT_SAMPLING: Sampling = Sampling {
    temperature: 1.05,
    top_p: 0.92,
    repeat_penalty: 1.12,
};
const DUNGEON_FIELD_SAMPLING: Sampling = Sampling {
    temperature: 1.05,
    top_p: 0.92,
    repeat_penalty: 1.1,
};

/// An attempt's verdict from the parsed JSON reply.
enum RerollStep<T> {
    /// Good result — return it.
    Accept(T),
    /// This attempt missed (bad JSON shape, or a dedup collision) — try again.
    Retry,
    /// Unrecoverable (e.g. the model returned an enum the caller propagates
    /// rather than retries) — fail the whole reroll.
    Fail(String),
}

/// The shared 0..4 reroll attempt loop. Builds the chat payload from `model`, the
/// named `sampling` profile, an optional `num_ctx`, and the prebuilt
/// `system`/`user` messages with `schema` as the JSON `format`; POSTs it; parses
/// the reply; and hands the parsed value to `accept`, which decides per
/// [`RerollStep`]. Returns `not_produced()` after four exhausted attempts. This is
/// the loop every `reroll_*` method used to inline verbatim.
#[allow(clippy::too_many_arguments)]
async fn run_reroll_attempts<T>(
    client: &reqwest::Client,
    url: &str,
    model: &str,
    sampling: &Sampling,
    num_ctx: Option<u32>,
    system: &str,
    user: &str,
    schema: &serde_json::Value,
    not_produced: impl Fn() -> String,
    mut accept: impl FnMut(&serde_json::Value, i32) -> RerollStep<T>,
) -> Result<T, String> {
    for attempt in 0..4 {
        let run_seed = attempt_seed(attempt);
        let mut options = serde_json::json!({
            "temperature": sampling.temperature,
            "top_p": sampling.top_p,
            "repeat_penalty": sampling.repeat_penalty,
            "seed": run_seed,
        });
        if let Some(num_ctx) = num_ctx {
            options["num_ctx"] = serde_json::json!(num_ctx);
        }
        let payload = serde_json::json!({
            "model": model,
            "stream": false,
            "format": schema,
            "options": options,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        });

        let Some(content) = post_chat_for_content(client, url, &payload).await? else {
            continue;
        };
        let parsed: serde_json::Value = match serde_json::from_str(&content) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        match accept(&parsed, attempt) {
            RerollStep::Accept(value) => return Ok(value),
            RerollStep::Fail(err) => return Err(err),
            RerollStep::Retry => continue,
        }
    }
    Err(not_produced())
}

/// The "unknown reroll field" error, sharing the schema's `Reroll` field list so
/// the message matches `<entity> reroll help`.
fn reroll_unknown_field_error(kind: EntityKind, raw: &str) -> String {
    format!(
        "unknown {} reroll field: {}. valid fields: {}",
        kind.command_root(),
        raw,
        format_valid_field_list(kind, FieldAccess::Reroll)
    )
}

pub struct EntityRerollService;

impl EntityRerollService {
    pub async fn reroll_npc_field(
        &self,
        input: RerollNpcFieldInput,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollNpcFieldResult, String> {
        // `location` is set via `npc travel`, not rerolled — keep the specific hint
        // rather than the generic "unknown field" the schema lookup would give.
        if input.field.trim().eq_ignore_ascii_case("location") {
            return Err(
                "npc reroll location is not supported; use npc travel to <location>".to_string(),
            );
        }
        let spec = canonical_field_spec(EntityKind::Npc, &input.field, FieldAccess::Reroll)
            .ok_or_else(|| reroll_unknown_field_error(EntityKind::Npc, &input.field))?;
        let field = spec.canonical;
        let (config, model) = load_generation_config()?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = npc_context_summary(&input.npc);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);
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

        let system = format!(
            "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}{}",
            if field == "occupation" {
                " For occupation rerolls, avoid repeating occupation roots seen in recent NPC generations unless the user explicitly asks for one."
            } else {
                ""
            },
            reference_suffix,
            detail_directive(config.generation.verbosity)
        );
        let user = format!(
            "NPC context: {}\nField to reroll: {}\nInstruction: {}\nRecent occupation roots to avoid: {}\nOptional shaping prompt: {}",
            context_summary,
            field,
            spec.reroll_instruction,
            if field == "occupation" {
                recent_occupation_context.as_str()
            } else {
                "(n/a)"
            },
            if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;
        let mut seen_attempt_occupation_anchors = HashSet::new();

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &NPC_SAMPLING,
            None,
            &system,
            &user,
            &schema,
            || format!("failed to reroll npc field: {}", field),
            |parsed, attempt| {
                if field == "carrying" {
                    let Some(items) = parsed.get("carrying").and_then(|item| item.as_array())
                    else {
                        return RerollStep::Retry;
                    };
                    let next = normalize_unknown_list(
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(|value| value.to_string()))
                            .collect(),
                    );
                    if attempt < 3 && next == normalize_unknown_list(input.npc.carrying.clone()) {
                        return RerollStep::Retry;
                    }
                    return RerollStep::Accept(RerollNpcFieldResult {
                        field: field.to_string(),
                        value: None,
                        carrying: Some(next),
                    });
                }

                let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                    return RerollStep::Retry;
                };
                let normalized = if field == "sex" {
                    match normalize_sex(raw_value) {
                        Ok(value) => value,
                        Err(err) => return RerollStep::Fail(err),
                    }
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
                    return RerollStep::Retry;
                }

                if field == "occupation" {
                    let anchor = occupation_anchor(&normalized);
                    if anchor != "unknown"
                        && (anchor == current_occupation_anchor
                            || recent_occupation_anchors.contains(&anchor)
                            || seen_attempt_occupation_anchors.contains(&anchor))
                    {
                        return RerollStep::Retry;
                    }
                    if anchor != "unknown" {
                        seen_attempt_occupation_anchors.insert(anchor);
                    }
                }

                RerollStep::Accept(RerollNpcFieldResult {
                    field: field.to_string(),
                    value: Some(normalized),
                    carrying: None,
                })
            },
        )
        .await
    }

    pub async fn reroll_location_field(
        &self,
        input: RerollLocationFieldInput,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollLocationFieldResult, String> {
        let spec = canonical_field_spec(EntityKind::Location, &input.field, FieldAccess::Reroll)
            .ok_or_else(|| reroll_unknown_field_error(EntityKind::Location, &input.field))?;
        let field = spec.canonical;
        let (config, model) = load_generation_config()?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = location_context_summary(&input.location);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);

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

        let system = format!(
            "You update one location field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
            reference_suffix,
            detail_directive(config.generation.verbosity)
        );
        let user = format!(
            "Location context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
            context_summary,
            field,
            spec.reroll_instruction,
            if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &LOCATION_SAMPLING,
            None,
            &system,
            &user,
            &schema,
            || format!("failed to reroll location field: {}", field),
            |parsed, attempt| {
                if field == "exports" {
                    let Some(items) = parsed.get("exports").and_then(|item| item.as_array()) else {
                        return RerollStep::Retry;
                    };
                    let next = normalize_exports(
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(|value| value.to_string()))
                            .collect(),
                    );
                    if next.is_empty() || next.len() > 3 {
                        return RerollStep::Retry;
                    }
                    if attempt < 3 && next == normalize_exports(input.location.exports.clone()) {
                        return RerollStep::Retry;
                    }
                    return RerollStep::Accept(RerollLocationFieldResult {
                        field: field.to_string(),
                        value: None,
                        exports: Some(next),
                    });
                }

                let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                    return RerollStep::Retry;
                };

                let normalized = match field {
                    "kind_type" => match normalize_location_kind_type(raw_value) {
                        Ok(value) => value,
                        Err(_) => return RerollStep::Retry,
                    },
                    "danger_level" => match normalize_location_danger_level(raw_value) {
                        Ok(value) => value,
                        Err(_) => return RerollStep::Retry,
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
                    return RerollStep::Retry;
                }

                RerollStep::Accept(RerollLocationFieldResult {
                    field: field.to_string(),
                    value: Some(normalized),
                    exports: None,
                })
            },
        )
        .await
    }

    pub async fn reroll_faction_field(
        &self,
        input: RerollFactionFieldInput,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollFactionFieldResult, String> {
        let spec = canonical_field_spec(EntityKind::Faction, &input.field, FieldAccess::Reroll)
            .ok_or_else(|| reroll_unknown_field_error(EntityKind::Faction, &input.field))?;
        let field = spec.canonical;
        let (config, model) = load_generation_config()?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = faction_context_summary(&input.faction);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);

        let is_list = [
            "allies",
            "rivals_enemies",
            "goals_short_term",
            "goals_long_term",
        ]
        .contains(&field);
        let schema = if is_list {
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

        let system = format!(
            "You update one faction field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
            reference_suffix,
            detail_directive(config.generation.verbosity)
        );
        let user = format!(
            "Faction context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
            context_summary,
            field,
            spec.reroll_instruction,
            if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &FACTION_SAMPLING,
            None,
            &system,
            &user,
            &schema,
            || format!("failed to reroll faction field: {}", field),
            |parsed, attempt| {
                if is_list {
                    let Some(items) = parsed.get("list").and_then(|item| item.as_array()) else {
                        return RerollStep::Retry;
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
                        return RerollStep::Retry;
                    }
                    return RerollStep::Accept(RerollFactionFieldResult {
                        field: field.to_string(),
                        value: None,
                        list_value: Some(next),
                    });
                }

                let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                    return RerollStep::Retry;
                };
                let normalized = if field == "kind_type" {
                    match normalize_faction_kind_type(raw_value) {
                        Ok(value) => value,
                        Err(_) => return RerollStep::Retry,
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
                    return RerollStep::Retry;
                }

                RerollStep::Accept(RerollFactionFieldResult {
                    field: field.to_string(),
                    value: Some(normalized),
                    list_value: None,
                })
            },
        )
        .await
    }

    pub async fn reroll_god_field(
        &self,
        input: RerollGodFieldInput,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollGodFieldResult, String> {
        let spec = canonical_field_spec(EntityKind::God, &input.field, FieldAccess::Reroll)
            .ok_or_else(|| reroll_unknown_field_error(EntityKind::God, &input.field))?;
        let field = spec.canonical;
        let (config, model) = load_generation_config()?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = god_context_summary(&input.god);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);

        let is_list = ["domains", "allies", "rivals"].contains(&field);
        let schema = if is_list {
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

        let system = format!(
            "You update one deity field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}{}",
            reference_suffix,
            detail_directive(config.generation.verbosity)
        );
        let user = format!(
            "God context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
            context_summary,
            field,
            spec.reroll_instruction,
            if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &GOD_SAMPLING,
            None,
            &system,
            &user,
            &schema,
            || format!("failed to reroll god field: {}", field),
            |parsed, attempt| {
                if is_list {
                    let Some(items) = parsed.get("list").and_then(|item| item.as_array()) else {
                        return RerollStep::Retry;
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
                        return RerollStep::Retry;
                    }
                    return RerollStep::Accept(RerollGodFieldResult {
                        field: field.to_string(),
                        value: None,
                        list_value: Some(next),
                    });
                }

                let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                    return RerollStep::Retry;
                };
                let normalized = match field {
                    "rank" => match normalize_god_rank(raw_value) {
                        Ok(value) => value,
                        Err(_) => return RerollStep::Retry,
                    },
                    "alignment" => match normalize_god_alignment(raw_value) {
                        Ok(value) => value,
                        Err(_) => return RerollStep::Retry,
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
                    return RerollStep::Retry;
                }

                RerollStep::Accept(RerollGodFieldResult {
                    field: field.to_string(),
                    value: Some(normalized),
                    list_value: None,
                })
            },
        )
        .await
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

        let (config, model) = load_generation_config()?;
        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);

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

        let system = format!(
            "You are a 5-room-dungeon oracle regenerating ONE beat for a game master. Return only JSON matching the schema. Keep each field tight and SPECIFIC BUT UNRESOLVED (a concrete spark, never the answer). idea is 1-2 sentences (for combat: tactics/behavior, never creature names). player_goals is one sentence — the clear, concrete goal for the players here (what they must learn, do, reach, or overcome). lever is one hook/question in 1-2 sentences. design_note is one sentence to the GM, out of fiction, on how this beat fits the overall dungeon and story. This beat is a room or area INSIDE the dungeon's single location (shown below) — keep it there; do not move the party to a new region, town, or building. {loot_rule} This beat's room type is FIXED as `{content_type}` ({mechanic}). Do NOT change the type — keep the idea squarely a {content_type} room; only the wording changes.{reference_suffix}",
            loot_rule = loot_rule,
            content_type = content_type,
            mechanic = mechanic,
            reference_suffix = reference_suffix
        );
        let user = format!(
            "Dungeon so far (the other four beats are frozen — stay coherent with them):\n{frozen}\n\nRegenerate ONLY beat {n} (function = {function}, type = {content_type} — keep this type). It must follow the {prev} beat and feed the {next} beat. Optional shaping prompt: {shape}",
            frozen = frozen,
            n = beat_index + 1,
            function = function,
            content_type = content_type,
            prev = prev,
            next = next,
            shape = if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &DUNGEON_BEAT_SAMPLING,
            Some(config.ollama.num_ctx),
            &system,
            &user,
            &schema,
            || format!("failed to reroll dungeon beat {}", beat_index + 1),
            |parsed, _attempt| {
                let Some(idea) = parsed.get("idea").and_then(|v| v.as_str()) else {
                    return RerollStep::Retry;
                };
                let Some(player_goals) = parsed.get("player_goals").and_then(|v| v.as_str()) else {
                    return RerollStep::Retry;
                };
                let Some(lever) = parsed.get("lever").and_then(|v| v.as_str()) else {
                    return RerollStep::Retry;
                };
                let Some(design_note) = parsed.get("design_note").and_then(|v| v.as_str()) else {
                    return RerollStep::Retry;
                };
                let loot = parsed
                    .get("loot")
                    .and_then(|v| v.as_str())
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"));

                RerollStep::Accept(RerollDungeonBeatResult {
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
                })
            },
        )
        .await
    }

    /// Scalar reroll for the dungeon-level `premise` or `name` line, against the
    /// frozen rest of the dungeon (mirrors the item/god scalar reroll).
    pub async fn reroll_dungeon_field(
        &self,
        input: RerollDungeonFieldInput,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollDungeonFieldResult, String> {
        // Resolved inline (not via `reroll_unknown_field_error`) to keep the
        // "(or a beat name)" hint — beats are rerolled through `reroll_dungeon_beat`.
        let field = match input.field.trim().to_ascii_lowercase().as_str() {
            "premise" | "spine" => "premise",
            "location" | "place" => "location",
            "name" => "name",
            other => {
                return Err(format!(
                    "unknown dungeon reroll field: {}. rerollable fields: name, location, premise (or a beat name)",
                    other
                ));
            }
        };
        let instruction = canonical_field_spec(EntityKind::Dungeon, field, FieldAccess::Reroll)
            .map(|spec| spec.reroll_instruction)
            .unwrap_or("Generate a concise field value.");

        let (config, model) = load_generation_config()?;
        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);
        let frozen = dungeon_context_summary(&input.dungeon, None);

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

        let system = format!(
            "You update one dungeon field for a game master. Return only JSON matching schema. Keep it coherent with the dungeon below.{reference_suffix}",
            reference_suffix = reference_suffix
        );
        let user = format!(
            "Dungeon: {frozen}\nField to reroll: {field}\nInstruction: {instruction}\nOptional shaping prompt: {shape}",
            frozen = frozen,
            field = field,
            instruction = instruction,
            shape = if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &DUNGEON_FIELD_SAMPLING,
            None,
            &system,
            &user,
            &schema,
            || format!("failed to reroll dungeon field: {}", field),
            |parsed, attempt| {
                let Some(raw_value) = parsed.get("value").and_then(|v| v.as_str()) else {
                    return RerollStep::Retry;
                };
                let normalized = normalize_unknown_text(raw_value);
                if attempt < 3 && normalized.eq_ignore_ascii_case(current.trim()) {
                    return RerollStep::Retry;
                }
                RerollStep::Accept(RerollDungeonFieldResult {
                    field: field.to_string(),
                    value: Some(normalized),
                })
            },
        )
        .await
    }

    pub async fn reroll_item_field(
        &self,
        input: RerollItemFieldInput,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollItemFieldResult, String> {
        let spec = canonical_field_spec(EntityKind::Item, &input.field, FieldAccess::Reroll)
            .ok_or_else(|| reroll_unknown_field_error(EntityKind::Item, &input.field))?;
        let field = spec.canonical;
        let (config, model) = load_generation_config()?;

        let extra_prompt = input
            .prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("");

        let context_summary = item_context_summary(&input.item);
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt);

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

        let system = format!(
            "You update one RPG item field. Return only valid JSON matching the schema.{}{}",
            reference_suffix,
            detail_directive(config.generation.verbosity)
        );
        let user = format!(
            "Item context: {}\nField to reroll: {}\nInstruction: {}\nOptional shaping prompt: {}",
            context_summary,
            field,
            spec.reroll_instruction,
            if extra_prompt.is_empty() {
                "(none)"
            } else {
                extra_prompt
            }
        );

        let (client, url) = build_chat_client(&config)?;

        run_reroll_attempts(
            &client,
            &url,
            &model,
            &ITEM_SAMPLING,
            None,
            &system,
            &user,
            &schema,
            || format!("failed to reroll item field: {}", field),
            |parsed, attempt| {
                if field == "materials" {
                    let Some(items) = parsed.get("materials").and_then(|item| item.as_array())
                    else {
                        return RerollStep::Retry;
                    };
                    let next = normalize_unknown_list(
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(|value| value.to_string()))
                            .collect(),
                    );
                    if attempt < 3 && next == input.item.materials {
                        return RerollStep::Retry;
                    }
                    return RerollStep::Accept(RerollItemFieldResult {
                        field: field.to_string(),
                        value: None,
                        materials: Some(next),
                    });
                }

                let Some(raw_value) = parsed.get("value").and_then(|item| item.as_str()) else {
                    return RerollStep::Retry;
                };
                let normalized = match field {
                    "category" => match normalize_item_category(raw_value) {
                        Ok(value) => value,
                        Err(err) => return RerollStep::Fail(err),
                    },
                    "rarity" => match normalize_item_rarity(raw_value) {
                        Ok(value) => value,
                        Err(err) => return RerollStep::Fail(err),
                    },
                    _ => normalize_unknown_text(raw_value),
                };

                if attempt < 3 {
                    let matches_current = match field {
                        "name" => normalized.eq_ignore_ascii_case(input.item.name.trim()),
                        "category" => normalized == input.item.category,
                        "rarity" => normalized == input.item.rarity,
                        "attunement" => {
                            normalized.eq_ignore_ascii_case(input.item.attunement.trim())
                        }
                        "appearance" => {
                            normalized.eq_ignore_ascii_case(input.item.appearance.trim())
                        }
                        "abilities" => normalized.eq_ignore_ascii_case(input.item.abilities.trim()),
                        "drawbacks" => normalized.eq_ignore_ascii_case(input.item.drawbacks.trim()),
                        "history" => normalized.eq_ignore_ascii_case(input.item.history.trim()),
                        "value" => normalized.eq_ignore_ascii_case(input.item.value.trim()),
                        "location" => normalized.eq_ignore_ascii_case(input.item.location.trim()),
                        _ => false,
                    };
                    if matches_current {
                        return RerollStep::Retry;
                    }
                }

                RerollStep::Accept(RerollItemFieldResult {
                    field: field.to_string(),
                    value: Some(normalized),
                    materials: None,
                })
            },
        )
        .await
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

fn location_context_summary(context: &LocationRerollContext) -> String {
    format!(
        "name={}, kind_type={}, kind_custom={}, visual_description={}, history_background={}, exports={}, tone={}, authority={}, danger_level={}, current_tension={}",
        context.name,
        context.kind_type,
        context
            .kind_custom
            .clone()
            .unwrap_or_else(|| "(none)".to_string()),
        context.visual_description,
        context.history_background,
        context.exports.join(", "),
        context.tone,
        context.authority,
        context.danger_level,
        context.current_tension
    )
}

fn faction_context_summary(context: &FactionRerollContext) -> String {
    format!(
        "name={}, kind_type={}, kind_custom={}, public_description={}, true_agenda={}, methods={}, leadership={}, headquarters={}, sphere_of_influence={}, resources_assets={}, allies={}, rivals_enemies={}, reputation={}, current_tension={}, goals_short_term={}, goals_long_term={}, symbol_description={}",
        context.name,
        context.kind_type,
        context
            .kind_custom
            .clone()
            .unwrap_or_else(|| "(none)".to_string()),
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

fn god_context_summary(context: &GodRerollContext) -> String {
    format!(
        "name={}, epithet={}, rank={}, rank_custom={}, alignment={}, domains={}, symbol={}, appearance={}, dogma={}, realm={}, worshippers={}, clergy={}, allies={}, rivals={}",
        context.name,
        context.epithet,
        context.rank,
        context
            .rank_custom
            .clone()
            .unwrap_or_else(|| "(none)".to_string()),
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
fn dungeon_context_summary(context: &DungeonRerollContext, skip_index: Option<usize>) -> String {
    let mut lines = vec![
        format!(
            "location (all beats are rooms/areas inside this one place): {}",
            context.location
        ),
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

fn item_context_summary(context: &ItemRerollContext) -> String {
    format!(
        "name={}, category={}, rarity={}, attunement={}, materials={}, appearance={}, abilities={}, drawbacks={}, history={}, value={}, location={}",
        context.name,
        context.category,
        context.rarity,
        context.attunement,
        context.materials.join(", "),
        context.appearance,
        context.abilities,
        context.drawbacks,
        context.history,
        context.value,
        context.location
    )
}
