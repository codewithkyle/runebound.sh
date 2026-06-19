use crate::entities::kind::EntityKind;
use crate::entities::schema::{
    EntityFieldSpec, FieldAccess, ValueKind, canonical_field_spec, format_valid_field_list,
};
use crate::repositories::{Database, GenerationRepository};
use crate::services::ai_generation::{
    anchor_mechanic, build_reference_context, describe_recent_npc_occupation_anchors,
    occupation_anchor, parse_recent_npc_seeds, recent_occupation_anchor_set,
};
use crate::services::ollama_chat::{
    ChatClient, OllamaChatClient, attempt_seed, detail_directive, load_generation_config,
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
async fn resolve_reference_suffix(config: &AppConfig, extra_prompt: &str) -> String {
    if extra_prompt.is_empty() {
        return String::new();
    }
    let context = build_reference_context(config, extra_prompt).await;
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

/// Build the Ollama `/api/chat` request body for one reroll attempt. Pure (no I/O)
/// so characterization tests can assert the exact payload a given field produces;
/// the only per-attempt-varying input is `run_seed`.
fn build_reroll_payload(
    model: &str,
    sampling: &Sampling,
    num_ctx: Option<u32>,
    run_seed: i32,
    schema: &serde_json::Value,
    system: &str,
    user: &str,
) -> serde_json::Value {
    let mut options = serde_json::json!({
        "temperature": sampling.temperature,
        "top_p": sampling.top_p,
        "repeat_penalty": sampling.repeat_penalty,
        "seed": run_seed,
    });
    if let Some(num_ctx) = num_ctx {
        options["num_ctx"] = serde_json::json!(num_ctx);
    }
    serde_json::json!({
        "model": model,
        "stream": false,
        "format": schema,
        "options": options,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    })
}

/// The shared 0..4 reroll attempt loop. Builds the chat payload from `model`, the
/// named `sampling` profile, an optional `num_ctx`, and the prebuilt
/// `system`/`user` messages with `schema` as the JSON `format`; POSTs it through the
/// [`ChatClient`] seam; parses the reply; and hands the parsed value to `accept`,
/// which decides per [`RerollStep`]. Returns `not_produced()` after four exhausted
/// attempts. This is the loop every `reroll_*` method used to inline verbatim.
#[allow(clippy::too_many_arguments)]
async fn run_reroll_attempts<T>(
    client: &dyn ChatClient,
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
        let payload =
            build_reroll_payload(model, sampling, num_ctx, run_seed, schema, system, user);

        let Some(content) = client.post_chat(&payload).await? else {
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

/// The structured-output JSON `format` for a single rerolled field. List fields
/// emit an array (under `list_key`) with `minItems: 1` and an optional `maxItems`;
/// the one enum-in-schema field (`npc sex`) emits a string `enum`; every other
/// field is a non-empty string under `value`. Other enum-typed fields (`kind_type`,
/// `rank`, `category`, …) deliberately use the plain string schema and lean on the
/// normalizer — matching the prior per-kind code, which only spelled `sex` out.
fn reroll_field_schema(
    spec: &EntityFieldSpec,
    list_key: &str,
    list_max: Option<u32>,
) -> serde_json::Value {
    if spec.value_kind == ValueKind::List {
        let mut array = serde_json::json!({
            "type": "array",
            "minItems": 1,
            "items": { "type": "string", "minLength": 1 }
        });
        if let Some(max) = list_max {
            array["maxItems"] = serde_json::json!(max);
        }
        return serde_json::json!({
            "type": "object",
            "required": [list_key],
            "properties": { list_key: array },
            "additionalProperties": false
        });
    }
    if spec.canonical == "sex" {
        return serde_json::json!({
            "type": "object",
            "required": ["value"],
            "properties": { "value": { "type": "string", "enum": ["male", "female"] } },
            "additionalProperties": false
        });
    }
    serde_json::json!({
        "type": "object",
        "required": ["value"],
        "properties": { "value": { "type": "string", "minLength": 1 } },
        "additionalProperties": false
    })
}

/// The reroll system prompt for `kind`. Items carry their own (deliberately
/// different) framing; the other kinds share one template differing only by the
/// entity noun, plus the NPC-only occupation-avoidance clause. `reference_suffix`
/// and the verbosity directive append exactly as the prior inline prompts did.
fn reroll_system_prompt(
    kind: EntityKind,
    field: &str,
    reference_suffix: &str,
    verbosity: dnd_core::config::Verbosity,
) -> String {
    let detail = detail_directive(verbosity);
    if kind == EntityKind::Item {
        return format!(
            "You update one RPG item field. Return only valid JSON matching the schema.{reference_suffix}{detail}"
        );
    }
    let noun = match kind {
        EntityKind::Npc => "NPC",
        EntityKind::Location => "location",
        EntityKind::Faction => "faction",
        EntityKind::God => "deity",
        _ => "entity",
    };
    let occupation_clause = if kind == EntityKind::Npc && field == "occupation" {
        " For occupation rerolls, avoid repeating occupation roots seen in recent NPC generations unless the user explicitly asks for one."
    } else {
        ""
    };
    format!(
        "You update one {noun} field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{occupation_clause}{reference_suffix}{detail}"
    )
}

/// The reroll user message. Every kind shares the
/// `<Label> context / Field / Instruction / Optional shaping prompt` shape; NPC
/// additionally carries a `Recent occupation roots to avoid:` line, passed as
/// `occupation_line` (always `Some` for NPC — `"(n/a)"` off the occupation field —
/// and `None` for every other kind, which omits the line).
fn reroll_user_message(
    label: &str,
    context_summary: &str,
    field: &str,
    reroll_instruction: &str,
    extra_prompt: &str,
    occupation_line: Option<&str>,
) -> String {
    let shaping = if extra_prompt.is_empty() {
        "(none)"
    } else {
        extra_prompt
    };
    match occupation_line {
        Some(occupation) => format!(
            "{label} context: {context_summary}\nField to reroll: {field}\nInstruction: {reroll_instruction}\nRecent occupation roots to avoid: {occupation}\nOptional shaping prompt: {shaping}"
        ),
        None => format!(
            "{label} context: {context_summary}\nField to reroll: {field}\nInstruction: {reroll_instruction}\nOptional shaping prompt: {shaping}"
        ),
    }
}

/// The rerolled value for one field: either a scalar replacement or a whole list.
/// Replaces the seven near-identical `Reroll*FieldResult { value, <list> }` shapes
/// with one the domains match once.
#[derive(Debug, Clone)]
pub enum RerollValue {
    Scalar(String),
    List(Vec<String>),
}

/// NPC-occupation-only dedup context: the anchor of the current occupation plus the
/// recent occupation anchors to avoid (fetched from prior generations). `None` for
/// every other (kind, field), where the anchor machinery stays inert.
struct OccupationDedup {
    current_anchor: String,
    recent_anchors: HashSet<String>,
}

/// Outcome of normalizing one scalar reroll value. `Retry` (a coercible enum miss)
/// re-prompts; `Fail` (a closed-enum violation) aborts — preserving the prior
/// per-field split: `sex`/`category`/`rarity` fail, `kind_type`/`danger_level`/
/// `rank`/`alignment` retry, everything else is infallible free text.
enum ScalarNorm {
    Value(String),
    Retry,
    Fail(String),
}

/// Normalize a scalar reroll value for `(kind, field)`, centralizing the per-field
/// normalizer + fail-vs-retry choice the five methods used to inline.
fn normalize_reroll_scalar(kind: EntityKind, field: &str, raw: &str) -> ScalarNorm {
    let coerce_or_retry = |result: Result<String, String>| match result {
        Ok(value) => ScalarNorm::Value(value),
        Err(_) => ScalarNorm::Retry,
    };
    let coerce_or_fail = |result: Result<String, String>| match result {
        Ok(value) => ScalarNorm::Value(value),
        Err(err) => ScalarNorm::Fail(err),
    };
    match (kind, field) {
        (EntityKind::Npc, "sex") => coerce_or_fail(normalize_sex(raw)),
        (EntityKind::Location, "kind_type") => coerce_or_retry(normalize_location_kind_type(raw)),
        (EntityKind::Location, "danger_level") => {
            coerce_or_retry(normalize_location_danger_level(raw))
        }
        (EntityKind::Faction, "kind_type") => coerce_or_retry(normalize_faction_kind_type(raw)),
        (EntityKind::God, "rank") => coerce_or_retry(normalize_god_rank(raw)),
        (EntityKind::God, "alignment") => coerce_or_retry(normalize_god_alignment(raw)),
        (EntityKind::Item, "category") => coerce_or_fail(normalize_item_category(raw)),
        (EntityKind::Item, "rarity") => coerce_or_fail(normalize_item_rarity(raw)),
        _ => ScalarNorm::Value(normalize_unknown_text(raw)),
    }
}

/// Normalize a list reroll value. `None` signals a retry — only `location exports`
/// rejects (empty or > 3 after `normalize_exports`); every other list uses
/// `normalize_unknown_list` and always accepts (the schema enforces `minItems`).
fn normalize_reroll_list(kind: EntityKind, field: &str, raw: Vec<String>) -> Option<Vec<String>> {
    if kind == EntityKind::Location && field == "exports" {
        let next = normalize_exports(raw);
        if next.is_empty() || next.len() > 3 {
            return None;
        }
        return Some(next);
    }
    Some(normalize_unknown_list(raw))
}

/// Whether a freshly normalized scalar equals the current value (so the attempt is a
/// no-op and should retry). `item category`/`rarity` compare exactly (canonical enum
/// values); everything else is case-insensitive on the trimmed current value.
fn dedup_scalar_matches(kind: EntityKind, field: &str, normalized: &str, current: &str) -> bool {
    if kind == EntityKind::Item && (field == "category" || field == "rarity") {
        normalized == current
    } else {
        normalized.eq_ignore_ascii_case(current.trim())
    }
}

/// Whether a freshly normalized list equals the current list (retry if so). The
/// current list is re-normalized with the field's own normalizer before comparing —
/// except `item materials`, which compares against the stored list verbatim
/// (preserving the prior per-field behavior).
fn dedup_list_matches(kind: EntityKind, field: &str, next: &[String], current: &[String]) -> bool {
    let baseline = match (kind, field) {
        (EntityKind::Item, "materials") => current.to_vec(),
        (EntityKind::Location, "exports") => normalize_exports(current.to_vec()),
        _ => normalize_unknown_list(current.to_vec()),
    };
    next == baseline.as_slice()
}

/// Serialize a typed reroll context into a field map so the generic accept loop can
/// read any field's current value by name (for dedup) without a per-kind match.
fn context_snapshot<T: serde::Serialize>(
    context: &T,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    match serde_json::to_value(context).map_err(|err| err.to_string())? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err("reroll context did not serialize to an object".to_string()),
    }
}

/// The current scalar value of `field` from a context snapshot (absent / null → "").
fn snapshot_scalar(snapshot: &serde_json::Map<String, serde_json::Value>, field: &str) -> String {
    snapshot
        .get(field)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

/// The current list value of `field` from a context snapshot (absent → empty).
fn snapshot_list(
    snapshot: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    snapshot
        .get(field)
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str().map(|value| value.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// The generic reroll accept loop shared by every entity field. Replaces the five
/// near-identical per-kind closures: extracts the value (list under `list_key`, or
/// the scalar `value`), normalizes it via [`normalize_reroll_scalar`] /
/// [`normalize_reroll_list`], retries on a no-op (per [`dedup_scalar_matches`] /
/// [`dedup_list_matches`]), and applies the NPC occupation-anchor dedup when
/// `occupation` is set. The per-attempt "current" value is read from `snapshot`.
#[allow(clippy::too_many_arguments)]
async fn run_field_reroll(
    client: &dyn ChatClient,
    model: &str,
    sampling: &Sampling,
    kind: EntityKind,
    field: &str,
    is_list: bool,
    list_key: &str,
    snapshot: &serde_json::Map<String, serde_json::Value>,
    system: &str,
    user: &str,
    schema: &serde_json::Value,
    occupation: Option<OccupationDedup>,
) -> Result<RerollValue, String> {
    let mut seen_attempt_occupation_anchors: HashSet<String> = HashSet::new();
    run_reroll_attempts(
        client,
        model,
        sampling,
        None,
        system,
        user,
        schema,
        || format!("failed to reroll {} field: {}", kind.command_root(), field),
        |parsed, attempt| {
            if is_list {
                let Some(items) = parsed.get(list_key).and_then(|value| value.as_array()) else {
                    return RerollStep::Retry;
                };
                let raw: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.as_str().map(|value| value.to_string()))
                    .collect();
                let Some(next) = normalize_reroll_list(kind, field, raw) else {
                    return RerollStep::Retry;
                };
                if attempt < 3
                    && dedup_list_matches(kind, field, &next, &snapshot_list(snapshot, field))
                {
                    return RerollStep::Retry;
                }
                return RerollStep::Accept(RerollValue::List(next));
            }

            let Some(raw_value) = parsed.get("value").and_then(|value| value.as_str()) else {
                return RerollStep::Retry;
            };
            let normalized = match normalize_reroll_scalar(kind, field, raw_value) {
                ScalarNorm::Value(value) => value,
                ScalarNorm::Retry => return RerollStep::Retry,
                ScalarNorm::Fail(err) => return RerollStep::Fail(err),
            };
            if attempt < 3
                && dedup_scalar_matches(kind, field, &normalized, &snapshot_scalar(snapshot, field))
            {
                return RerollStep::Retry;
            }
            if let Some(occupation) = &occupation {
                let anchor = occupation_anchor(&normalized);
                if anchor != "unknown"
                    && (anchor == occupation.current_anchor
                        || occupation.recent_anchors.contains(&anchor)
                        || seen_attempt_occupation_anchors.contains(&anchor))
                {
                    return RerollStep::Retry;
                }
                if anchor != "unknown" {
                    seen_attempt_occupation_anchors.insert(anchor);
                }
            }
            RerollStep::Accept(RerollValue::Scalar(normalized))
        },
    )
    .await
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;
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

        let schema = reroll_field_schema(spec, "carrying", None);
        let system = reroll_system_prompt(
            EntityKind::Npc,
            field,
            &reference_suffix,
            config.generation.verbosity,
        );
        let occupation_line = if field == "occupation" {
            recent_occupation_context.as_str()
        } else {
            "(n/a)"
        };
        let user = reroll_user_message(
            "NPC",
            &context_summary,
            field,
            spec.reroll_instruction,
            extra_prompt,
            Some(occupation_line),
        );

        let client = OllamaChatClient::from_config(&config)?;
        let snapshot = context_snapshot(&input.npc)?;
        let occupation = if field == "occupation" {
            Some(OccupationDedup {
                current_anchor: current_occupation_anchor,
                recent_anchors: recent_occupation_anchors,
            })
        } else {
            None
        };

        let value = run_field_reroll(
            &client,
            &model,
            &NPC_SAMPLING,
            EntityKind::Npc,
            field,
            spec.value_kind == ValueKind::List,
            "carrying",
            &snapshot,
            &system,
            &user,
            &schema,
            occupation,
        )
        .await?;

        Ok(match value {
            RerollValue::List(carrying) => RerollNpcFieldResult {
                field: field.to_string(),
                value: None,
                carrying: Some(carrying),
            },
            RerollValue::Scalar(scalar) => RerollNpcFieldResult {
                field: field.to_string(),
                value: Some(scalar),
                carrying: None,
            },
        })
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;

        let schema = reroll_field_schema(spec, "exports", Some(3));
        let system = reroll_system_prompt(
            EntityKind::Location,
            field,
            &reference_suffix,
            config.generation.verbosity,
        );
        let user = reroll_user_message(
            "Location",
            &context_summary,
            field,
            spec.reroll_instruction,
            extra_prompt,
            None,
        );

        let client = OllamaChatClient::from_config(&config)?;
        let snapshot = context_snapshot(&input.location)?;

        let value = run_field_reroll(
            &client,
            &model,
            &LOCATION_SAMPLING,
            EntityKind::Location,
            field,
            spec.value_kind == ValueKind::List,
            "exports",
            &snapshot,
            &system,
            &user,
            &schema,
            None,
        )
        .await?;

        Ok(match value {
            RerollValue::List(exports) => RerollLocationFieldResult {
                field: field.to_string(),
                value: None,
                exports: Some(exports),
            },
            RerollValue::Scalar(scalar) => RerollLocationFieldResult {
                field: field.to_string(),
                value: Some(scalar),
                exports: None,
            },
        })
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;

        let schema = reroll_field_schema(spec, "list", Some(5));
        let system = reroll_system_prompt(
            EntityKind::Faction,
            field,
            &reference_suffix,
            config.generation.verbosity,
        );
        let user = reroll_user_message(
            "Faction",
            &context_summary,
            field,
            spec.reroll_instruction,
            extra_prompt,
            None,
        );

        let client = OllamaChatClient::from_config(&config)?;
        let snapshot = context_snapshot(&input.faction)?;

        let value = run_field_reroll(
            &client,
            &model,
            &FACTION_SAMPLING,
            EntityKind::Faction,
            field,
            spec.value_kind == ValueKind::List,
            "list",
            &snapshot,
            &system,
            &user,
            &schema,
            None,
        )
        .await?;

        Ok(match value {
            RerollValue::List(list_value) => RerollFactionFieldResult {
                field: field.to_string(),
                value: None,
                list_value: Some(list_value),
            },
            RerollValue::Scalar(scalar) => RerollFactionFieldResult {
                field: field.to_string(),
                value: Some(scalar),
                list_value: None,
            },
        })
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;

        let schema = reroll_field_schema(spec, "list", Some(5));
        let system = reroll_system_prompt(
            EntityKind::God,
            field,
            &reference_suffix,
            config.generation.verbosity,
        );
        let user = reroll_user_message(
            "God",
            &context_summary,
            field,
            spec.reroll_instruction,
            extra_prompt,
            None,
        );

        let client = OllamaChatClient::from_config(&config)?;
        let snapshot = context_snapshot(&input.god)?;

        let value = run_field_reroll(
            &client,
            &model,
            &GOD_SAMPLING,
            EntityKind::God,
            field,
            spec.value_kind == ValueKind::List,
            "list",
            &snapshot,
            &system,
            &user,
            &schema,
            None,
        )
        .await?;

        Ok(match value {
            RerollValue::List(list_value) => RerollGodFieldResult {
                field: field.to_string(),
                value: None,
                list_value: Some(list_value),
            },
            RerollValue::Scalar(scalar) => RerollGodFieldResult {
                field: field.to_string(),
                value: Some(scalar),
                list_value: None,
            },
        })
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;

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

        let client = OllamaChatClient::from_config(&config)?;

        run_reroll_attempts(
            &client,
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;
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

        let client = OllamaChatClient::from_config(&config)?;

        run_reroll_attempts(
            &client,
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
        let reference_suffix = resolve_reference_suffix(&config, extra_prompt).await;

        let schema = reroll_field_schema(spec, "materials", Some(4));
        let system = reroll_system_prompt(
            EntityKind::Item,
            field,
            &reference_suffix,
            config.generation.verbosity,
        );
        let user = reroll_user_message(
            "Item",
            &context_summary,
            field,
            spec.reroll_instruction,
            extra_prompt,
            None,
        );

        let client = OllamaChatClient::from_config(&config)?;
        let snapshot = context_snapshot(&input.item)?;

        let value = run_field_reroll(
            &client,
            &model,
            &ITEM_SAMPLING,
            EntityKind::Item,
            field,
            spec.value_kind == ValueKind::List,
            "materials",
            &snapshot,
            &system,
            &user,
            &schema,
            None,
        )
        .await?;

        Ok(match value {
            RerollValue::List(materials) => RerollItemFieldResult {
                field: field.to_string(),
                value: None,
                materials: Some(materials),
            },
            RerollValue::Scalar(scalar) => RerollItemFieldResult {
                field: field.to_string(),
                value: Some(scalar),
                materials: None,
            },
        })
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
    pub resources_assets: Vec<String>,
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
        context.resources_assets.join(", "),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ollama_chat::MockChatClient;
    use dnd_core::config::Verbosity;

    fn spec(kind: EntityKind, field: &str) -> &'static EntityFieldSpec {
        canonical_field_spec(kind, field, FieldAccess::Reroll).expect("rerollable field")
    }

    // --- request-side golden tests: lock the exact schema/system/user/payload the
    // generic helpers build, so the (later) accept-side collapse can't drift them.

    #[test]
    fn schema_bounded_list_field_emits_array_under_its_key() {
        let got = reroll_field_schema(spec(EntityKind::Location, "exports"), "exports", Some(3));
        assert_eq!(
            got,
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
        );
    }

    #[test]
    fn schema_unbounded_list_omits_max_items() {
        let got = reroll_field_schema(spec(EntityKind::Npc, "carrying"), "carrying", None);
        assert_eq!(
            got,
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
        );
    }

    #[test]
    fn schema_sex_is_the_only_enum_in_schema() {
        let got = reroll_field_schema(spec(EntityKind::Npc, "sex"), "n/a", None);
        assert_eq!(
            got,
            serde_json::json!({
                "type": "object",
                "required": ["value"],
                "properties": { "value": { "type": "string", "enum": ["male", "female"] } },
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn schema_other_enum_fields_use_a_plain_string() {
        // `god rank` is value_kind Enum, but the prior code emitted a plain string
        // schema (the normalizer coerces it) — only `sex` was spelled out.
        let got = reroll_field_schema(spec(EntityKind::God, "rank"), "n/a", None);
        assert_eq!(
            got,
            serde_json::json!({
                "type": "object",
                "required": ["value"],
                "properties": { "value": { "type": "string", "minLength": 1 } },
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn system_prompt_npc_non_occupation_matches_prior_template() {
        let suffix = "\n\nREF";
        let got = reroll_system_prompt(EntityKind::Npc, "name", suffix, Verbosity::Brief);
        assert_eq!(
            got,
            format!(
                "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{suffix}{}",
                detail_directive(Verbosity::Brief)
            )
        );
    }

    #[test]
    fn system_prompt_npc_occupation_adds_the_avoidance_clause() {
        let got = reroll_system_prompt(EntityKind::Npc, "occupation", "", Verbosity::Brief);
        assert_eq!(
            got,
            format!(
                "You update one NPC field for a game master. Return only valid JSON matching schema. Keep it coherent with context. For occupation rerolls, avoid repeating occupation roots seen in recent NPC generations unless the user explicitly asks for one.{}",
                detail_directive(Verbosity::Brief)
            )
        );
    }

    #[test]
    fn system_prompt_uses_the_right_noun_per_kind() {
        for (kind, noun) in [
            (EntityKind::Location, "location"),
            (EntityKind::Faction, "faction"),
            (EntityKind::God, "deity"),
        ] {
            let got = reroll_system_prompt(kind, "name", "", Verbosity::Brief);
            assert_eq!(
                got,
                format!(
                    "You update one {noun} field for a game master. Return only valid JSON matching schema. Keep it coherent with context.{}",
                    detail_directive(Verbosity::Brief)
                ),
                "kind {kind:?}"
            );
        }
    }

    #[test]
    fn system_prompt_item_uses_its_distinct_template() {
        let got = reroll_system_prompt(EntityKind::Item, "name", "", Verbosity::Brief);
        assert_eq!(
            got,
            format!(
                "You update one RPG item field. Return only valid JSON matching the schema.{}",
                detail_directive(Verbosity::Brief)
            )
        );
    }

    #[test]
    fn user_message_omits_occupation_line_when_none() {
        let got = reroll_user_message("Location", "name=Foo", "name", "Generate a name.", "", None);
        assert_eq!(
            got,
            "Location context: name=Foo\nField to reroll: name\nInstruction: Generate a name.\nOptional shaping prompt: (none)"
        );
    }

    #[test]
    fn user_message_includes_occupation_line_and_shaping_prompt() {
        let got = reroll_user_message(
            "NPC",
            "name=Foo",
            "occupation",
            "Generate one.",
            "gritty",
            Some("smith, guard"),
        );
        assert_eq!(
            got,
            "NPC context: name=Foo\nField to reroll: occupation\nInstruction: Generate one.\nRecent occupation roots to avoid: smith, guard\nOptional shaping prompt: gritty"
        );
    }

    #[test]
    fn payload_wraps_messages_schema_and_sampling() {
        let schema = reroll_field_schema(spec(EntityKind::Npc, "name"), "n/a", None);
        let payload =
            build_reroll_payload("test-model", &NPC_SAMPLING, None, 42, &schema, "SYS", "USR");
        assert_eq!(
            payload,
            serde_json::json!({
                "model": "test-model",
                "stream": false,
                "format": schema,
                "options": {
                    "temperature": NPC_SAMPLING.temperature,
                    "top_p": NPC_SAMPLING.top_p,
                    "repeat_penalty": NPC_SAMPLING.repeat_penalty,
                    "seed": 42
                },
                "messages": [
                    { "role": "system", "content": "SYS" },
                    { "role": "user", "content": "USR" }
                ]
            })
        );
    }

    #[test]
    fn payload_includes_num_ctx_when_present() {
        let schema = serde_json::json!({});
        let payload = build_reroll_payload("m", &ITEM_SAMPLING, Some(4096), 7, &schema, "s", "u");
        assert_eq!(payload["options"]["num_ctx"], serde_json::json!(4096));
    }

    // --- loop control-flow tests through the ChatClient seam (hermetic).

    #[tokio::test]
    async fn run_reroll_attempts_retries_on_empty_then_accepts() {
        let client = MockChatClient::new(vec![
            Ok(None),                                  // attempt 0: empty -> retry
            Ok(Some("{\"value\":\"x\"}".to_string())), // attempt 1: accepted
        ]);
        let out: Result<String, String> = run_reroll_attempts(
            &client,
            "m",
            &NPC_SAMPLING,
            None,
            "sys",
            "usr",
            &serde_json::json!({}),
            || "not produced".to_string(),
            |parsed, _attempt| match parsed.get("value").and_then(|v| v.as_str()) {
                Some(value) => RerollStep::Accept(value.to_string()),
                None => RerollStep::Retry,
            },
        )
        .await;
        assert_eq!(out.unwrap(), "x");
        assert_eq!(client.captured().len(), 2);
    }

    #[tokio::test]
    async fn run_reroll_attempts_propagates_fail_immediately() {
        let client = MockChatClient::with_contents(&["{\"value\":\"bad\"}"]);
        let out: Result<String, String> = run_reroll_attempts(
            &client,
            "m",
            &NPC_SAMPLING,
            None,
            "sys",
            "usr",
            &serde_json::json!({}),
            || "not produced".to_string(),
            |_parsed, _attempt| RerollStep::Fail("boom".to_string()),
        )
        .await;
        assert_eq!(out.unwrap_err(), "boom");
        assert_eq!(client.captured().len(), 1);
    }

    #[tokio::test]
    async fn run_reroll_attempts_exhausts_after_four_attempts() {
        let client = MockChatClient::new(vec![]); // queue empty -> always Ok(None)
        let out: Result<String, String> = run_reroll_attempts(
            &client,
            "m",
            &NPC_SAMPLING,
            None,
            "sys",
            "usr",
            &serde_json::json!({}),
            || "not produced".to_string(),
            |_parsed, _attempt| RerollStep::Retry,
        )
        .await;
        assert_eq!(out.unwrap_err(), "not produced");
        assert_eq!(client.captured().len(), 4);
    }

    // --- accept-side: the per-field normalize + dedup nuances the generic accept
    // must preserve byte-for-byte across the five kinds.

    #[test]
    fn scalar_norm_closed_enums_fail_but_coercible_enums_retry() {
        // Closed enums (no custom/other) surface a hard error to the user.
        assert!(matches!(
            normalize_reroll_scalar(EntityKind::Npc, "sex", "zorp"),
            ScalarNorm::Fail(_)
        ));
        assert!(matches!(
            normalize_reroll_scalar(EntityKind::Item, "category", "zorp"),
            ScalarNorm::Fail(_)
        ));
        assert!(matches!(
            normalize_reroll_scalar(EntityKind::Item, "rarity", "zorp"),
            ScalarNorm::Fail(_)
        ));
        // Coercible enums just retry the attempt.
        for (kind, field) in [
            (EntityKind::Location, "kind_type"),
            (EntityKind::Location, "danger_level"),
            (EntityKind::Faction, "kind_type"),
            (EntityKind::God, "rank"),
            (EntityKind::God, "alignment"),
        ] {
            assert!(
                matches!(
                    normalize_reroll_scalar(kind, field, "zorp"),
                    ScalarNorm::Retry
                ),
                "{kind:?} {field} should retry on a bad enum"
            );
        }
    }

    #[test]
    fn scalar_norm_free_text_is_infallible() {
        assert!(matches!(
            normalize_reroll_scalar(EntityKind::Npc, "background", "A tense backstory."),
            ScalarNorm::Value(_)
        ));
        assert!(matches!(
            normalize_reroll_scalar(EntityKind::Npc, "sex", "Male"),
            ScalarNorm::Value(value) if value == "male"
        ));
    }

    #[test]
    fn list_norm_rejects_only_oversized_location_exports() {
        assert!(
            normalize_reroll_list(
                EntityKind::Location,
                "exports",
                vec!["a".into(), "b".into(), "c".into(), "d".into()]
            )
            .is_none()
        );
        assert!(
            normalize_reroll_list(
                EntityKind::Location,
                "exports",
                vec!["a".into(), "b".into()]
            )
            .is_some()
        );
        // Other lists accept any count (the schema enforces minItems).
        assert!(
            normalize_reroll_list(
                EntityKind::Faction,
                "allies",
                vec![
                    "a".into(),
                    "b".into(),
                    "c".into(),
                    "d".into(),
                    "e".into(),
                    "f".into()
                ]
            )
            .is_some()
        );
    }

    #[test]
    fn dedup_scalar_item_enums_exact_others_case_insensitive() {
        assert!(dedup_scalar_matches(
            EntityKind::Item,
            "category",
            "weapon",
            "weapon"
        ));
        assert!(!dedup_scalar_matches(
            EntityKind::Item,
            "category",
            "Weapon",
            "weapon"
        ));
        assert!(dedup_scalar_matches(
            EntityKind::Npc,
            "name",
            "Bob",
            "  bob "
        ));
        assert!(!dedup_scalar_matches(
            EntityKind::Npc,
            "name",
            "Alice",
            "bob"
        ));
    }

    #[test]
    fn dedup_list_equality_holds_for_materials_and_other_lists() {
        assert!(dedup_list_matches(
            EntityKind::Item,
            "materials",
            &["steel".to_string()],
            &["steel".to_string()]
        ));
        assert!(dedup_list_matches(
            EntityKind::Faction,
            "allies",
            &["x".to_string()],
            &["x".to_string()]
        ));
        assert!(!dedup_list_matches(
            EntityKind::Item,
            "materials",
            &["steel".to_string()],
            &["iron".to_string()]
        ));
    }

    #[test]
    fn snapshot_reads_scalar_and_list_with_sane_fallbacks() {
        let snapshot = serde_json::json!({
            "name": "Bob",
            "carrying": ["sword", "shield"],
            "kind_custom": null
        });
        let map = snapshot.as_object().unwrap();
        assert_eq!(snapshot_scalar(map, "name"), "Bob");
        assert_eq!(snapshot_scalar(map, "kind_custom"), ""); // null -> ""
        assert_eq!(snapshot_scalar(map, "absent"), "");
        assert_eq!(
            snapshot_list(map, "carrying"),
            vec!["sword".to_string(), "shield".to_string()]
        );
        assert!(snapshot_list(map, "absent").is_empty());
    }

    #[tokio::test]
    async fn run_field_reroll_retries_when_value_equals_current() {
        // attempt 0 returns the current value (a no-op -> retry); attempt 1 differs.
        let client =
            MockChatClient::with_contents(&["{\"value\":\"Bob\"}", "{\"value\":\"Alice\"}"]);
        let snapshot = serde_json::json!({ "name": "Bob" });
        let value = run_field_reroll(
            &client,
            "m",
            &NPC_SAMPLING,
            EntityKind::Npc,
            "name",
            false,
            "carrying",
            snapshot.as_object().unwrap(),
            "sys",
            "usr",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
        assert!(matches!(value, RerollValue::Scalar(name) if name == "Alice"));
        assert_eq!(client.captured().len(), 2);
    }

    #[tokio::test]
    async fn run_field_reroll_closed_enum_failure_aborts() {
        let client = MockChatClient::with_contents(&["{\"value\":\"notasex\"}"]);
        let snapshot = serde_json::json!({ "sex": "male" });
        let err = run_field_reroll(
            &client,
            "m",
            &NPC_SAMPLING,
            EntityKind::Npc,
            "sex",
            false,
            "carrying",
            snapshot.as_object().unwrap(),
            "sys",
            "usr",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap_err();
        assert!(err.contains("sex must be one of"), "got: {err}");
        assert_eq!(client.captured().len(), 1); // failed immediately, no retries
    }

    #[tokio::test]
    async fn run_field_reroll_returns_list_under_the_key() {
        let client = MockChatClient::with_contents(&["{\"carrying\":[\"rope\",\"torch\"]}"]);
        let snapshot = serde_json::json!({ "carrying": ["sword"] });
        let value = run_field_reroll(
            &client,
            "m",
            &NPC_SAMPLING,
            EntityKind::Npc,
            "carrying",
            true,
            "carrying",
            snapshot.as_object().unwrap(),
            "sys",
            "usr",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
        assert!(
            matches!(value, RerollValue::List(items) if items == vec!["rope".to_string(), "torch".to_string()])
        );
    }
}
