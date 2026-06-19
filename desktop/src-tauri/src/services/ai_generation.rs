use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{
    ChatClient, OllamaChatClient, attempt_seed, build_chat_client, detail_directive,
    load_generation_config, post_chat_for_content,
};
use crate::services::vault_ref::{
    VaultReferenceEntry, extract_prompt_reference_keys, load_vault_reference_entries,
};
use crate::utils::{
    estimate_tokens, normalize_exports, normalize_faction_seed, normalize_god_seed,
    normalize_item_category, normalize_item_rarity, normalize_location_danger_level,
    normalize_location_seed, normalize_relative_path_for_storage, normalize_sex,
    normalize_unknown_list, normalize_unknown_text, validate_faction_details, validate_god_details,
    validate_location_details, validate_location_prose,
};
use dnd_core::config::AppConfig;
use dnd_core::entity_store::EntityStore;
use dnd_core::vault::Vault;
use runebound_models::DungeonBeat;
use runebound_models::dungeon_plan::DungeonContentPlan;
use runebound_models::utils::{
    DUNGEON_FUNCTIONS, FACTION_KIND_TYPES, GOD_ALIGNMENTS, GOD_RANKS, ITEM_CATEGORIES,
    ITEM_RARITIES, LOCATION_DANGER_LEVELS, LOCATION_KIND_TYPES,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Tokens reserved within the context window for the model's own output, so the
/// capacity warning fires before the prompt crowds out room to respond.
const OUTPUT_RESERVE_TOKENS: usize = 512;
/// Flat allowance for each generator's fixed system instructions / schema framing.
const SYSTEM_BOILERPLATE_TOKENS: usize = 160;

/// A generated seed plus an optional non-blocking notice (e.g. the assembled
/// prompt is near the configured context window and output may drift).
#[derive(Debug, Clone)]
pub struct SeedGeneration<T> {
    pub seed: T,
    pub notice: Option<String>,
}

/// Returns a user-facing warning when the estimated prompt is close enough to the
/// configured `num_ctx` that there is little room left for the response.
fn capacity_notice(estimated_tokens: usize, num_ctx: u32) -> Option<String> {
    let budget = (num_ctx as usize).saturating_sub(OUTPUT_RESERVE_TOKENS);
    if estimated_tokens > budget {
        Some(format!(
            "⚠️ This prompt is ~{estimated_tokens} tokens, near your model's configured context window (ollama.num_ctx = {num_ctx}). Large referenced documents may cause the model to lose detail or drift. Consider referencing fewer or smaller documents, or raising ollama.num_ctx in your config."
        ))
    } else {
        None
    }
}

/// Assemble the `@reference` prompt context from the configured vault, tolerating a
/// missing or unreadable vault by returning empty context. Shared by seed
/// generation and field reroll so a custom prompt's `@references` resolve the same
/// way in both flows.
pub(crate) async fn build_reference_context(
    config: &AppConfig,
    user_prompt: &str,
) -> PromptReferenceContext {
    let Some(vault_path) = config.vault.path.clone() else {
        return PromptReferenceContext::default();
    };
    let user_prompt = user_prompt.to_string();
    // read_dir + TOML loads + referenced-file reads are blocking IO; keep them off
    // the async runtime worker (P6.2). A panicked task degrades to empty context.
    tokio::task::spawn_blocking(move || {
        let vault = Vault::new(vault_path);
        if vault.ensure_root_exists().is_err() {
            return PromptReferenceContext::default();
        }
        match load_vault_reference_entries(&vault) {
            Ok(entries) => build_prompt_reference_context(&user_prompt, &entries, &vault),
            Err(err) => {
                eprintln!("reference context warning: {err}");
                PromptReferenceContext::default()
            }
        }
    })
    .await
    .unwrap_or_default()
}

/// LLM sampling knobs for a seed-generation request. Hoisted from the per-kind
/// literals each generator inlined in its payload; the values differ by kind on
/// purpose (NPCs run hottest, items coolest).
struct SeedSampling {
    temperature: f64,
    top_p: f64,
    repeat_penalty: f64,
}

const NPC_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.1,
    top_p: 0.92,
    repeat_penalty: 1.15,
};
const LOCATION_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.08,
    top_p: 0.93,
    repeat_penalty: 1.14,
};
const FACTION_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.08,
    top_p: 0.93,
    repeat_penalty: 1.12,
};
const GOD_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.08,
    top_p: 0.93,
    repeat_penalty: 1.12,
};
const ITEM_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.05,
    top_p: 0.92,
    repeat_penalty: 1.1,
};
const EVENT_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.05,
    top_p: 0.93,
    repeat_penalty: 1.12,
};

/// The `\n\n`-prefixed reference block appended to a generator's system prompt, or
/// empty when no `@references` resolved — the formatting every generator repeated
/// inline.
fn reference_system_suffix(reference_context: &PromptReferenceContext) -> String {
    if reference_context.system_context.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", reference_context.system_context)
    }
}

/// Build the Ollama `/api/chat` request body for one seed-generation attempt. Pure
/// (no I/O) so the payload shape is unit-testable; `run_seed` is the only
/// per-attempt-varying input.
fn build_seed_payload(
    model: &str,
    sampling: &SeedSampling,
    num_ctx: u32,
    run_seed: i32,
    schema: &serde_json::Value,
    system: &str,
    user_prompt: &str,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "stream": false,
        "format": schema,
        "options": {
            "temperature": sampling.temperature,
            "top_p": sampling.top_p,
            "repeat_penalty": sampling.repeat_penalty,
            "seed": run_seed,
            "num_ctx": num_ctx,
        },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user_prompt }
        ]
    })
}

/// An attempt's verdict from the parsed seed: a good seed to persist, a soft miss to
/// retry, or a hard failure (a closed-enum violation) to surface.
enum SeedStep<T> {
    Accept(T),
    Retry,
    Fail(String),
}

/// The shared 0..5 seed-generation attempt loop. For each attempt it builds the
/// payload (a fresh RNG seed and, after the first, the per-kind `repair_note`), POSTs
/// it through the [`ChatClient`] seam, parses the reply into `T`, and hands it to
/// `accept` for per-kind normalize/validate/dedup. On `Accept` it persists the seed
/// under `entity_key` (so future generations dedup against it) and returns it; after
/// five misses it returns `not_produced()`. This is the loop every `generate_*_seed`
/// used to inline verbatim. (Payload construction is split into the pure, testable
/// [`build_seed_payload`]; the loop's control flow mirrors `run_reroll_attempts`.)
#[allow(clippy::too_many_arguments)]
async fn run_seed_attempts<T: serde::Serialize + serde::de::DeserializeOwned>(
    client: &dyn ChatClient,
    model: &str,
    sampling: &SeedSampling,
    num_ctx: u32,
    schema: &serde_json::Value,
    user_prompt: &str,
    repair_note: &str,
    entity_key: &str,
    database: &Database,
    generation_repo: &dyn GenerationRepository,
    system_prompt: impl Fn(&str) -> String,
    not_produced: impl Fn() -> String,
    mut accept: impl FnMut(T) -> SeedStep<T>,
) -> Result<T, String> {
    for attempt in 0..5 {
        let run_seed = attempt_seed(attempt);
        let note = if attempt == 0 { "" } else { repair_note };
        let system = system_prompt(note);
        let payload = build_seed_payload(
            model,
            sampling,
            num_ctx,
            run_seed,
            schema,
            &system,
            user_prompt,
        );

        let Some(content) = client.post_chat(&payload).await? else {
            continue;
        };
        let Ok(seed) = serde_json::from_str::<T>(&content) else {
            continue;
        };
        match accept(seed) {
            SeedStep::Accept(seed) => {
                let serialized = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
                generation_repo
                    .insert(database, entity_key, None, &serialized)
                    .await?;
                return Ok(seed);
            }
            SeedStep::Fail(err) => return Err(err),
            SeedStep::Retry => continue,
        }
    }
    Err(not_produced())
}

pub struct AiGenerationService;

impl AiGenerationService {
    pub async fn generate_npc_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<NpcSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one D&D NPC for a fantasy campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "npc_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<NpcSeed>(recent_payloads);
        let recent_names = recent_name_set(&recent_seeds);
        let recent_context = describe_recent_npc_seeds(&recent_seeds);
        let recent_occupation_anchors = recent_occupation_anchor_set(&recent_seeds);
        let recent_occupation_context = describe_recent_npc_occupation_anchors(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(&recent_occupation_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

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
                "carrying": { "type": "array", "minItems": 1, "items": { "type": "string", "minLength": 1 } }
            },
            "additionalProperties": false
        });

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();
        let mut seen_attempt_occupation_anchors = HashSet::new();

        let seed = run_seed_attempts(
            &client,
            &model,
            &NPC_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names and occupations.",
            "npc_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying. carrying must be an array of item strings. Age must be numeric text with no commas, separators, or trailing punctuation (e.g., '133', not '1,133' or '133,'). Height should be imperial like 5'11\", weight_lbs should be lbs as text like 180 with no commas. Prefer occupations different from recent occupations and avoid occupation roots in this list unless explicitly requested: {}. Avoid these recent seeds: {}.{}{}{}",
                recent_occupation_context, recent_context, note, reference_suffix, detail_directive(verbosity)
            ),
            || "failed to generate valid structured NPC output from ollama".to_string(),
            |mut seed: NpcSeed| {
                seed.name = seed.name.trim().to_string();
                seed.race = seed.race.trim().to_string();
                seed.occupation = normalize_unknown_text(&seed.occupation);
                seed.sex = match normalize_sex(&seed.sex) {
                    Ok(value) => value,
                    Err(err) => return SeedStep::Fail(err),
                };
                seed.age = normalize_unknown_text(&seed.age);
                seed.height = normalize_unknown_text(&seed.height);
                seed.weight_lbs = normalize_unknown_text(&seed.weight_lbs);
                seed.background = normalize_unknown_text(&seed.background);
                seed.want_need = normalize_unknown_text(&seed.want_need);
                seed.secret_obstacle = normalize_unknown_text(&seed.secret_obstacle);
                seed.carrying = normalize_unknown_list(seed.carrying);

                if seed.name.is_empty() || seed.race.is_empty() {
                    return SeedStep::Retry;
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if recent_names.contains(&normalized_name)
                    || seen_attempt_names.contains(&normalized_name)
                {
                    return SeedStep::Retry;
                }
                let anchor = occupation_anchor(&seed.occupation);
                if anchor != "unknown"
                    && (recent_occupation_anchors.contains(&anchor)
                        || seen_attempt_occupation_anchors.contains(&anchor))
                {
                    return SeedStep::Retry;
                }
                seen_attempt_names.insert(normalized_name);
                seen_attempt_occupation_anchors.insert(anchor);
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    pub async fn generate_location_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<LocationSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy location for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "location_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<LocationSeed>(recent_payloads);
        let recent_names = recent_location_name_set(&recent_seeds);
        let recent_context = describe_recent_location_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        // The one-shot mirrors the ruin/site output shape: prose-first, no exports
        // or settlement-economy modelling. `kind_type` + `danger_level` stay
        // model-derived (no GM to lock them, unlike the wizard's Site branch).
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "kind_type", "visual_description", "history_background", "tone", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "kind_type": { "type": "string", "enum": LOCATION_KIND_TYPES },
                "kind_custom": { "type": ["string", "null"] },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "tone": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        });

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        let seed = run_seed_attempts(
            &client,
            &model,
            &LOCATION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "location_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate one usable D&D location seed for a game master — describe the place by its look, its history, and the tension there now, the way you would a ruin, landmark, or remote site (not a modelled settlement economy). Return only JSON with fields name, kind_type, kind_custom, visual_description, history_background, tone, danger_level, current_tension. Pick the kind_type that best fits. tone must be 2-5 words. If kind_type is not other, kind_custom must be null. Do not invent exports, trade goods, rulers, or governments. danger_level must be one of: {danger}. If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any region, settlement, or landmark instead of inventing new ones. Avoid these recent seeds: {recent_context}.{note}{reference_suffix}{detail}",
                danger = LOCATION_DANGER_LEVELS.join(", "),
                detail = detail_directive(verbosity),
            ),
            || "failed to generate valid structured location output from ollama".to_string(),
            |seed: LocationSeed| {
                let mut seed = match normalize_location_seed(seed) {
                    Ok(seed) => seed,
                    Err(_) => return SeedStep::Retry,
                };
                // Mirror the ruin/site shape: suppress exports and authority (no
                // economy or rulership modelling) and validate prose only, leaving
                // kind_type + danger_level as the model's choices (normalized above).
                seed.exports = Vec::new();
                seed.authority = String::new();
                if validate_location_prose(&seed).is_err() {
                    return SeedStep::Retry;
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if recent_names.contains(&normalized_name)
                    || seen_attempt_names.contains(&normalized_name)
                {
                    return SeedStep::Retry;
                }
                seen_attempt_names.insert(normalized_name);
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    /// The location wizard's kind-aware generation. Mirrors `generate_location_seed`
    /// but (1) the JSON schema is shaped per branch — Settlement keeps
    /// `exports`/`danger_level`, Site/Hideout drop both (GM-locked / suppressed) — and
    /// (2) the GM's locked answers (control, resources, export mode, focus, owner,
    /// protection, purpose, geography) are embedded as authoritative context, so the
    /// model fills the LLM-derived fields *under* them. `kind_type`/`kind_custom` are
    /// never requested; they are overwritten from the accumulator afterward. Only the
    /// freeform custom-kind lane does NOT use this method — it stays on the one-shot
    /// `generate_location_seed`.
    pub async fn generate_location_seed_for_wizard(
        &self,
        inputs: &LocationWizardInputs,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<LocationSeed>, String> {
        let (config, model) = load_generation_config()?;

        let branch = location_branch(&inputs.kind_type);
        let user_prompt = build_wizard_user_prompt(inputs);

        let reference_context = build_reference_context(&config, &user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "location_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<LocationSeed>(recent_payloads);
        let recent_names = recent_location_name_set(&recent_seeds);
        let recent_context = describe_recent_location_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(&user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = wizard_location_schema(branch);
        let system_prompt_base = wizard_location_system_prompt(inputs, branch);

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        // Locked answers copied out for the (synchronous) accept closure.
        let kind_type = inputs.kind_type.clone();
        let kind_custom = inputs.kind_custom.clone();
        let danger_lock = inputs.danger_lock.clone();
        let faction_name = inputs.faction_name.clone();

        let seed = run_seed_attempts(
            &client,
            &model,
            &LOCATION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            &user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "location_seed",
            database,
            generation_repo,
            |note| format!(
                "{system_prompt_base} If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any region, settlement, or landmark instead of inventing new ones. Avoid reusing these recent location names: {recent_context}.{note}{reference_suffix}{detail}",
                detail = detail_directive(verbosity),
            ),
            || "failed to generate valid structured location output from ollama".to_string(),
            |mut seed: LocationSeed| {
                seed.name = seed.name.trim().to_string();
                if seed.name.is_empty() {
                    return SeedStep::Retry;
                }
                seed.visual_description = normalize_unknown_text(&seed.visual_description);
                seed.history_background = normalize_unknown_text(&seed.history_background);
                seed.tone = normalize_unknown_text(&seed.tone);
                seed.authority = normalize_unknown_text(&seed.authority);
                seed.current_tension = normalize_unknown_text(&seed.current_tension);

                // Kind is GM-locked at step 1 — never the model's pick.
                seed.kind_type = kind_type.clone();
                seed.kind_custom = kind_custom.clone();

                let validation = match branch {
                    LocationBranch::Settlement => {
                        seed.exports = normalize_exports(seed.exports);
                        seed.danger_level = match normalize_location_danger_level(&seed.danger_level)
                        {
                            Ok(value) => value,
                            Err(_) => return SeedStep::Retry,
                        };
                        validate_location_details(&seed)
                    }
                    LocationBranch::Site | LocationBranch::Hideout => {
                        // Exports suppressed; danger is the GM's locked answer.
                        seed.exports = Vec::new();
                        seed.danger_level =
                            danger_lock.clone().unwrap_or_else(|| "Unknown".to_string());
                        validate_location_prose(&seed)
                    }
                    LocationBranch::Guildhall => {
                        // A public HQ: exports suppressed, but danger is LLM-derived
                        // (incidental, like a settlement's) so it must validate.
                        seed.exports = Vec::new();
                        seed.danger_level = match normalize_location_danger_level(&seed.danger_level)
                        {
                            Ok(value) => value,
                            Err(_) => return SeedStep::Retry,
                        };
                        validate_location_prose(&seed)
                    }
                };
                if validation.is_err() {
                    return SeedStep::Retry;
                }

                // A linked faction is the known house; force authority to its name.
                if let Some(name) = &faction_name {
                    seed.authority = name.clone();
                }

                let normalized_name = seed.name.to_ascii_lowercase();
                if recent_names.contains(&normalized_name)
                    || seen_attempt_names.contains(&normalized_name)
                {
                    return SeedStep::Retry;
                }
                seen_attempt_names.insert(normalized_name);
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    pub async fn generate_faction_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<FactionSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy faction for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "faction_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<FactionSeed>(recent_payloads);
        let recent_names = recent_faction_name_set(&recent_seeds);
        let recent_context = describe_recent_faction_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);
        let enforce_unique_name = reference_context.system_context.is_empty();

        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "kind_type", "public_description", "true_agenda", "methods", "leadership", "headquarters", "sphere_of_influence", "resources_assets", "allies", "rivals_enemies", "reputation", "current_tension", "goals_short_term", "goals_long_term", "symbol_description"],
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
                "resources_assets": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
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

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        let seed = run_seed_attempts(
            &client,
            &model,
            &FACTION_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "faction_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate usable D&D faction seeds. Return only JSON with fields name, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description. symbol_description should be exactly 1 sentence describing symbol/sigil/colors/banner/iconography. If kind_type is not other, kind_custom must be null. If referenced vault metadata includes an established name for an organization, group, guild, or house, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {}.{}{}{}",
                recent_context, note, reference_suffix, detail_directive(verbosity)
            ),
            || "failed to generate valid structured faction output from ollama".to_string(),
            |seed: FactionSeed| {
                let seed = match normalize_faction_seed(seed) {
                    Ok(seed) => seed,
                    Err(_) => return SeedStep::Retry,
                };
                if validate_faction_details(&seed).is_err() {
                    return SeedStep::Retry;
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if enforce_unique_name
                    && (recent_names.contains(&normalized_name)
                        || seen_attempt_names.contains(&normalized_name))
                {
                    return SeedStep::Retry;
                }
                if enforce_unique_name {
                    seen_attempt_names.insert(normalized_name);
                }
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    pub async fn generate_god_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<GodSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy deity for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "god_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<GodSeed>(recent_payloads);
        let recent_names = recent_god_name_set(&recent_seeds);
        let recent_context = describe_recent_god_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);
        let enforce_unique_name = reference_context.system_context.is_empty();

        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "epithet", "rank", "alignment", "domains", "symbol", "appearance", "dogma", "realm", "worshippers", "clergy", "allies", "rivals"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "epithet": { "type": "string", "minLength": 1 },
                "rank": { "type": "string", "enum": GOD_RANKS },
                "rank_custom": { "type": ["string", "null"] },
                "alignment": { "type": "string", "enum": GOD_ALIGNMENTS },
                "domains": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
                "symbol": { "type": "string", "minLength": 1 },
                "appearance": { "type": "string", "minLength": 1 },
                "dogma": { "type": "string", "minLength": 1 },
                "realm": { "type": "string", "minLength": 1 },
                "worshippers": { "type": "string", "minLength": 1 },
                "clergy": { "type": "string", "minLength": 1 },
                "allies": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } },
                "rivals": { "type": "array", "minItems": 1, "maxItems": 5, "items": { "type": "string", "minLength": 1 } }
            },
            "additionalProperties": false
        });

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_names = HashSet::new();

        let seed = run_seed_attempts(
            &client,
            &model,
            &GOD_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names.",
            "god_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate usable D&D deity seeds. Return only JSON with fields name, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals. rank must be one of: {}. alignment must be one of: {}. If rank is not other, rank_custom must be null. symbol should be exactly 1 sentence describing the holy symbol/sigil/iconography. domains is a list of spheres the deity governs (e.g. war, death, harvest). If referenced vault metadata includes an established name for a god or power, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {}.{}{}{}",
                GOD_RANKS.join(", "), GOD_ALIGNMENTS.join(", "),
                recent_context, note, reference_suffix, detail_directive(verbosity)
            ),
            || "failed to generate valid structured god output from ollama".to_string(),
            |seed: GodSeed| {
                let seed = match normalize_god_seed(seed) {
                    Ok(seed) => seed,
                    Err(_) => return SeedStep::Retry,
                };
                if validate_god_details(&seed).is_err() {
                    return SeedStep::Retry;
                }
                let normalized_name = seed.name.to_ascii_lowercase();
                if enforce_unique_name
                    && (recent_names.contains(&normalized_name)
                        || seen_attempt_names.contains(&normalized_name))
                {
                    return SeedStep::Retry;
                }
                if enforce_unique_name {
                    seen_attempt_names.insert(normalized_name);
                }
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    pub async fn generate_item_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<ItemSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one magical or legendary item.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = serde_json::json!({
            "type": "object",
            "required": [
                "name",
                "category",
                "rarity",
                "attunement",
                "materials",
                "appearance",
                "abilities",
                "drawbacks",
                "history",
                "value",
                "location"
            ],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "category": { "type": "string", "enum": ITEM_CATEGORIES },
                "rarity": { "type": "string", "enum": ITEM_RARITIES },
                "attunement": { "type": "string", "minLength": 1 },
                "materials": { "type": "array", "minItems": 1, "maxItems": 4, "items": { "type": "string", "minLength": 1 } },
                "appearance": { "type": "string", "minLength": 1 },
                "abilities": { "type": "string", "minLength": 1 },
                "drawbacks": { "type": "string" },
                "history": { "type": "string", "minLength": 1 },
                "value": { "type": "string", "minLength": 1 },
                "location": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        });

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;

        let seed = run_seed_attempts(
            &client,
            &model,
            &ITEM_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON.",
            "item_seed",
            database,
            generation_repo,
            |note| format!(
                "You generate tabletop RPG items. Category choices: {}. Rarity choices: {}. Provide appearance, abilities, drawbacks (or 'None'), history, value in format like '1000gp' or '250sp' or '50cp', and location. If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any person, place, or organization instead of inventing new ones.{}{}{}",
                ITEM_CATEGORIES.join(", "), ITEM_RARITIES.join(", "), note, reference_suffix, detail_directive(verbosity)
            ),
            || "failed to generate valid structured item output from ollama".to_string(),
            |mut seed: ItemSeed| {
                seed.name = seed.name.trim().to_string();
                seed.category = match normalize_item_category(&seed.category) {
                    Ok(value) => value,
                    Err(err) => return SeedStep::Fail(err),
                };
                seed.rarity = match normalize_item_rarity(&seed.rarity) {
                    Ok(value) => value,
                    Err(err) => return SeedStep::Fail(err),
                };
                seed.attunement = normalize_unknown_text(&seed.attunement);
                seed.materials = normalize_unknown_list(seed.materials);
                seed.appearance = normalize_unknown_text(&seed.appearance);
                seed.abilities = normalize_unknown_text(&seed.abilities);
                seed.drawbacks = normalize_unknown_text(&seed.drawbacks);
                seed.history = normalize_unknown_text(&seed.history);
                seed.value = normalize_unknown_text(&seed.value);
                seed.location = normalize_unknown_text(&seed.location);

                if seed.name.is_empty() {
                    return SeedStep::Retry;
                }
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    pub async fn generate_event_seed(
        &self,
        prompt: Option<String>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<EventSeed>, String> {
        let (config, model) = load_generation_config()?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Write a short piece of lore about a notable event in a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "event_seed", 20)
            .await?;
        let recent_seeds = parse_recent_seeds::<EventSeed>(recent_payloads);
        let recent_titles = recent_event_title_set(&recent_seeds);
        let recent_context = describe_recent_event_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = serde_json::json!({
            "type": "object",
            "required": ["title", "body"],
            "properties": {
                "title": { "type": "string", "minLength": 1 },
                "body": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        });

        let client = OllamaChatClient::from_config(&config)?;
        let reference_suffix = reference_system_suffix(&reference_context);
        let verbosity = config.generation.verbosity;
        let mut seen_attempt_titles = HashSet::new();

        let seed = run_seed_attempts(
            &client,
            &model,
            &EVENT_GEN_SAMPLING,
            config.ollama.num_ctx,
            &schema,
            user_prompt,
            " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior titles.",
            "event_seed",
            database,
            generation_repo,
            |note| format!(
                "You write evocative D&D campaign lore about an event — a battle, a betrayal, a founding, a disaster, a discovery. Return only JSON with fields title and body. title is a short evocative name for the event. body is several paragraphs of narrative prose (separated by blank lines) telling the story of what happened, who was involved, and why it matters. Write it as flowing narrative lore, not as bullet points or labeled attributes. If referenced vault metadata is provided, treat it as authoritative setting context and weave in those established people, places, and organizations by their exact canonical names instead of inventing new ones. Avoid these recent event titles: {}.{}{}{}",
                recent_context, note, reference_suffix, detail_directive(verbosity)
            ),
            || "failed to generate valid structured event output from ollama".to_string(),
            |mut seed: EventSeed| {
                seed.title = seed.title.trim().to_string();
                seed.body = seed.body.trim().to_string();

                if seed.title.is_empty() || seed.body.is_empty() {
                    return SeedStep::Retry;
                }
                let normalized_title = seed.title.to_ascii_lowercase();
                if recent_titles.contains(&normalized_title)
                    || seen_attempt_titles.contains(&normalized_title)
                {
                    return SeedStep::Retry;
                }
                seen_attempt_titles.insert(normalized_title);
                SeedStep::Accept(seed)
            },
        )
        .await?;

        Ok(SeedGeneration { seed, notice })
    }

    /// Pass 1 of dungeon generation: write the short story the GM reviews. The
    /// rolled content `plan` is fed in as plain-language story ingredients (never
    /// jargon), and the single-location anchor lives here because the story is
    /// where sprawl happens. `extra_prompt` carries the GM's optional reroll steer.
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_dungeon_story(
        &self,
        plan: &DungeonContentPlan,
        premise: Option<String>,
        context: &str,
        tone: &str,
        twist: &str,
        topology: &str,
        extra_prompt: Option<&str>,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<DungeonStory>, String> {
        let (config, model) = load_generation_config()?;

        let premise = premise
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let context = context.trim();
        let extra = extra_prompt
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());

        let reference_probe = format!("{} {}", premise.unwrap_or(""), context);
        let reference_context = build_reference_context(&config, reference_probe.trim()).await;

        let recent_payloads = generation_repo
            .recent_prompts(database, "dungeon_story", 12)
            .await?;
        let recent_context = describe_recent_dungeon_stories(recent_payloads);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(&reference_probe);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "location", "story"],
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "location": { "type": "string", "minLength": 1 },
                "story": { "type": "string", "minLength": 1 }
            }
        });

        let premise_directive = match premise {
            Some(value) => format!("Build the story to honor this premise: \"{value}\"."),
            None => {
                "Invent a small, self-contained story that needs nothing outside this one place."
                    .to_string()
            }
        };
        let context_directive = if context.is_empty() {
            String::new()
        } else {
            format!("Weave in these GM-supplied details where natural: {context}.")
        };
        let faction_directive = if plan.factions {
            "Rival factions contest this place; thread their conflict through the story, and make the peak a confrontation between forces rather than a lone monster. ".to_string()
        } else {
            String::new()
        };
        let overlay_directive = match &plan.overlay {
            Some(overlay) => format!(
                "In movement {}, also plant {}, lightly — a layer on the scene, not its center. ",
                overlay.beat_index + 1,
                overlay_phrase(&overlay.overlay_type)
            ),
            None => String::new(),
        };
        let topology_directive = match topology_shape(topology) {
            Some(shape) => {
                format!(
                    "The space is shaped like {shape}; let that guide how the party moves deeper. "
                )
            }
            None => String::new(),
        };
        let steer_directive = match extra {
            Some(value) => format!("The GM asked for this in the retold version: {value}. "),
            None => String::new(),
        };
        let sidekick_directive = match plan.anchors.iter().position(|a| a == "sidekick") {
            Some(idx) => format!(
                "The ally introduced in movement {} is a companion, not a place: they join the party and travel with them through the movements that follow, until the dungeon ends — keep them present in those later movements rather than forgetting them after their first scene. ",
                idx + 1
            ),
            None => String::new(),
        };

        let elements = pass1_elements_block(plan);

        let system_prompt = format!(
            "You are a master storyteller seeding a dungeon for a tabletop game master. Your goal is a COMPLETE, self-contained micro-story in two short paragraphs — a real tale that runs from a clear beginning to a definite END, not a fragment, a mood piece, or a description of a place. Things must HAPPEN and someone must ACT; carry the tale all the way to its ending and never trail off in atmosphere. A GM should read it in fifteen seconds and see the whole shape of an adventure. The north star is SPECIFIC BUT UNRESOLVED: concrete, evocative sparks that raise questions, even as the tale itself reaches a complete arc.\n\n\
ONE LOCATION. The whole story happens inside a single bounded place the party enters and moves DEEPER into — e.g. \"a drowned bell-foundry\", \"a hijacked customs house\". Name that place. They never travel to another region, town, or building; they go further in, not elsewhere. Keep the cast and threats consistent from first line to last.\n\n\
Move through five movements in order — a setup, an inciting turn, rising tension, a peak, and a resolution — and actually REACH the fifth (the ending); do not stop after the setup or the descent. Do NOT label the parts; let it read as two flowing paragraphs, roughly six to ten sentences total. Pace the movements so the confrontation lands at the FOURTH movement (the peak), never earlier. Each element below belongs to exactly ONE movement — build that movement around it and keep it out of the others:\n\n{elements}\n\
Tone: {tone} — let it color the whole arc. Twist: {twist}. {faction_directive}{sidekick_directive}{overlay_directive}{topology_directive}{steer_directive}\n\n\
{premise_directive} {context_directive}\n\n\
Avoid retelling these recent stories: {recent}.{reference}\n\n\
Return only JSON: name (a short evocative title), location (the one place, a short phrase), and story (the complete two-paragraph tale, beginning to end).",
            elements = elements,
            tone = tone,
            twist = twist_directive(twist),
            faction_directive = faction_directive,
            sidekick_directive = sidekick_directive,
            overlay_directive = overlay_directive,
            topology_directive = topology_directive,
            steer_directive = steer_directive,
            premise_directive = premise_directive,
            context_directive = context_directive,
            recent = recent_context,
            reference = if reference_context.system_context.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", reference_context.system_context)
            },
        );

        let user_prompt = match premise {
            Some(value) => value.to_string(),
            None => "Write the story.".to_string(),
        };

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid. Return only valid JSON matching the schema."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.05, "top_p": 0.92, "repeat_penalty": 1.1, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [
                    { "role": "system", "content": format!("{system_prompt}{repair_note}") },
                    { "role": "user", "content": user_prompt }
                ]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<DungeonStory, _> = serde_json::from_str(&content);
            let Ok(mut story) = parsed else { continue };
            story.normalize();
            if story.name.is_empty() || story.location == "Unknown" || story.story.is_empty() {
                continue;
            }

            let serialized = serde_json::to_string(&story).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "dungeon_story", None, &serialized)
                .await?;

            return Ok(SeedGeneration {
                seed: story,
                notice,
            });
        }

        Err("failed to generate a valid dungeon story from ollama".to_string())
    }

    /// Pass 2 of dungeon generation: structure the LOCKED story into the five beat
    /// cards. Extractive — the model maps the story it is given, applies the field
    /// leashes, and writes a one-line spine. The per-beat `content_type` is NOT
    /// requested; it is injected from the deterministic `plan` so the tag can never
    /// disagree with the content. `function` is assigned by position in `to_beats`.
    #[allow(clippy::too_many_arguments)]
    pub async fn structure_dungeon_story(
        &self,
        plan: &DungeonContentPlan,
        story: &DungeonStory,
        tone: &str,
        twist: &str,
        topology: &str,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<DungeonSeed>, String> {
        let (config, model) = load_generation_config()?;

        let beat_schema = serde_json::json!({
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
        let schema = serde_json::json!({
            "type": "object",
            "required": ["premise", "beats"],
            "additionalProperties": false,
            "properties": {
                "premise": { "type": "string", "minLength": 1 },
                "beats": { "type": "array", "minItems": 5, "maxItems": 5, "items": beat_schema }
            }
        });

        let assignment = pass2_assignment_block(plan);
        let faction_note = if plan.factions {
            "These beats sit inside a faction struggle; let it tint the relevant beats and render the peak as a confrontation between forces. ".to_string()
        } else {
            String::new()
        };
        let topology_note = match topology_shape(topology) {
            Some(shape) => {
                format!(
                    "Spatial layout: {shape}; let it inform how the beats connect (especially whether the Setback loops the party back toward the entrance). "
                )
            }
            None => String::new(),
        };
        let sidekick_note = match plan.anchors.iter().position(|a| a == "sidekick") {
            Some(idx) => format!(
                "The sidekick beat (beat {}) introduces a companion who then accompanies the party: where the story shows them, let the idea or lever of the beats AFTER it involve that ally rather than dropping them after their introduction. ",
                idx + 1
            ),
            None => String::new(),
        };

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&assignment)
            + estimate_tokens(&story.story);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let system_prompt = format!(
            "You are structuring a finished story into a game master's index cards. The story below is LOCKED — do not invent new events, places, characters, or items; only express what is already there. The north star is SPECIFIC BUT UNRESOLVED: each field is a concrete spark that never states the final answer.\n\n\
The story has five movements in order. Produce exactly five beats in the same order; beat N renders movement N. SCOPE EACH BEAT TO ITSELF: every field describes ONLY what happens in that one beat — never summarize the whole dungeon in a single beat, never name the final confrontation or ending before its own beat, and do not let beat 1 preview the climax. For each beat write four fields:\n\
- idea: 1-2 sentences — what happens in THIS beat only.\n\
- player_goals: 1 sentence — the clear, concrete goal for the players in THIS beat: what they must learn, do, reach, or overcome to complete it (not the goal of the whole dungeon).\n\
- lever: ONE complication, question, or hook the GM can pull, in 1-2 sentences.\n\
- loot: a conditional reward line, OR null (see each beat's rule below).\n\
- design_note: 1 sentence to the GM (out of fiction) — how this beat fits the overall dungeon and story: what it sets up, pays off, or escalates.\n\n\
Each beat has a fixed role and content type, written here. Honor them EXACTLY — every beat must deliver its listed content type's mechanic, even where that movement of the story is brief: lead the idea with that mechanic, recasting the story's own props (a chain, a hook, a ledger) to serve it. Never change a beat's type. A beat typed combat MUST stage an actual fight (convey tactics and behavior) — never render a combat beat as a choice, a conversation, or a quiet decision. A non-combat beat must NOT be turned into a fight. The FINAL beat is the payoff — a reward, a revelation, or humble pie — NOT a second battle:\n\n{assignment}\n\
{faction_note}{sidekick_note}{topology_note}Tone: {tone}. Twist shape: {twist}.\n\n\
Also produce premise: a single-line spine summarizing the whole dungeon (one sentence; specific but unresolved).\n\n\
Keep every field tight — 1-2 sentences; a paragraph of boxed text is over-generating. Return only JSON: premise, and the five beats (idea, player_goals, lever, loot, design_note) in order.",
            assignment = assignment,
            faction_note = faction_note,
            sidekick_note = sidekick_note,
            topology_note = topology_note,
            tone = tone,
            twist = twist,
        );

        let user_prompt = format!(
            "Title: {}\nLocation: {}\n\nStory:\n{}",
            story.name, story.location, story.story
        );

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid. Return only valid JSON matching the schema with exactly five beats."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 0.7, "top_p": 0.9, "repeat_penalty": 1.1, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [
                    { "role": "system", "content": format!("{system_prompt}{repair_note}") },
                    { "role": "user", "content": user_prompt }
                ]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<DungeonStructured, _> = serde_json::from_str(&content);
            let Ok(structured) = parsed else { continue };
            if structured.beats.len() != DUNGEON_FUNCTIONS.len() {
                continue;
            }

            // Inject content_type per beat from the deterministic plan; carry
            // name/location from Pass 1; take the spine from Pass 2.
            let beats = structured
                .beats
                .into_iter()
                .enumerate()
                .map(|(i, beat)| DungeonBeatSeed {
                    content_type: plan.anchors[i].clone(),
                    idea: beat.idea,
                    player_goals: beat.player_goals,
                    lever: beat.lever,
                    loot: beat.loot,
                    design_note: beat.design_note,
                })
                .collect();
            let mut seed = DungeonSeed {
                name: story.name.clone(),
                location: story.location.clone(),
                premise: structured.premise,
                beats,
            };
            seed.normalize();

            return Ok(SeedGeneration { seed, notice });
        }

        Err("failed to structure the dungeon story into cards from ollama".to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// Which structured location branch a kind routes to. Drives the per-kind schema
/// shape and prompt in [`AiGenerationService::generate_location_seed_for_wizard`].
/// (Only freeform custom kinds are *not* structured — they stay on the one-shot path.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationBranch {
    Settlement,
    Site,
    Hideout,
    /// A faction's public headquarters. Exports suppressed, `authority` locked to
    /// the linked faction, `danger_level` LLM-derived (a public hall's danger is
    /// usually incidental, like a settlement's).
    Guildhall,
}

/// Map a GM-locked kind to its branch. Only the structured kinds reach the wizard
/// generation method; anything else falls back to Settlement defensively.
pub fn location_branch(kind_type: &str) -> LocationBranch {
    match kind_type {
        "ruin" | "landmark" | "wilderness" => LocationBranch::Site,
        "hideout" => LocationBranch::Hideout,
        "guildhall" => LocationBranch::Guildhall,
        _ => LocationBranch::Settlement,
    }
}

/// Under-`locations/` subfolder for a structured wizard kind, or `None` for
/// other/freeform/unknown (stays flat). Unlike [`location_branch`], `other` must map
/// to `None` rather than a default branch, so match the kind strings directly. Only
/// shapes the readable `.md` vault_path — the TOML store and DB projection stay flat.
pub fn location_subfolder(kind_type: &str) -> Option<&'static str> {
    match kind_type {
        "hamlet" | "town" | "city" => Some("settlements"),
        "ruin" | "landmark" | "wilderness" => Some("sites"),
        "hideout" => Some("hideouts"),
        "guildhall" => Some("guildhalls"),
        _ => None, // other / freeform / unknown -> flat
    }
}

/// Full relative dir for a NEW location save: `locations/<sub>` for a structured
/// wizard kind, or flat `locations` otherwise.
pub fn location_dir_for_kind(base: &str, kind_type: &str) -> String {
    match location_subfolder(kind_type) {
        Some(sub) => format!("{base}/{sub}"),
        None => base.to_string(),
    }
}

/// The GM's locked wizard answers, flattened into a borrow-friendly struct the
/// wizard fills and passes to generation. Keeps `ai_generation.rs` free of the
/// wizard's own accumulator type.
#[derive(Debug, Clone, Default)]
pub struct LocationWizardInputs {
    pub kind_type: String,
    pub kind_custom: Option<String>,
    // Settlement (Q-A…Q-D)
    pub control: Option<String>,
    pub resources: Option<String>,
    pub export_mode: Option<String>,
    // Site (Q-S1…Q-S3)
    pub site_focus: Option<String>,
    pub site_draw: Option<String>,
    // Hideout (Q-H1…Q-H4)
    pub base_owner: Option<String>,
    pub base_protection: Option<String>,
    pub base_purpose: Option<String>,
    // GM-locked danger for Site + Hideout (Q-S2 / Q-H3)
    pub danger_lock: Option<String>,
    // Guildhall (faction-locked public HQ): its public-facing function, and the
    // existing location it stands within (or a free-typed place name).
    pub public_role: Option<String>,
    pub location_anchor: Option<String>,
    // The anchor location's under-`locations/` subfolder (e.g. "settlements"), so the
    // `@locations/<sub>/<anchor>` seed resolves to the right path-keyed note. Empty/None
    // for a flat note or a free-typed place; falls back to the name-only `@locations/<anchor>`.
    pub location_anchor_sub: Option<String>,
    // Shared optional map anchor (Q-D / Q-S4 / Q-H5)
    pub geography: Option<String>,
    // A linked faction's canonical name (read-only), forces `authority`.
    pub faction_name: Option<String>,
    // Optional reroll steer from the review screen.
    pub hint: Option<String>,
}

fn opt_clause(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

fn geography_clause(geography: &Option<String>) -> String {
    match opt_clause(geography) {
        Some(geo) => format!(" Ground it on the GM's map: {geo}."),
        None => String::new(),
    }
}

/// The fixed-vs-unspecified danger directive for Site/Hideout. The level itself is
/// injected after generation; the prompt only asks the model to write its *source*.
fn locked_danger_clause(danger: &Option<String>) -> String {
    match danger.as_deref().unwrap_or("Unknown") {
        "Unknown" => " The danger level is deliberately left open; let any tension emerge naturally without forcing a severity.".to_string(),
        level => format!(
            " The danger level is fixed at '{level}'; write the SOURCE of that danger and how it manifests in current_tension and history_background, but do not contradict that level."
        ),
    }
}

const LOCATION_PROSE_LEASH: &str = " visual_description must be 1-3 sentences, history_background 2-5 sentences, current_tension 1-2 sentences, and tone 2-5 words.";

/// Build the branch-specific system prompt that embeds the GM's locked answers.
/// The recent-seed avoidance, repair note, reference block, and detail directive
/// are appended by the caller's closure.
fn wizard_location_system_prompt(inputs: &LocationWizardInputs, branch: LocationBranch) -> String {
    let kind = &inputs.kind_type;
    let geo = geography_clause(&inputs.geography);
    match branch {
        LocationBranch::Settlement => {
            let control = opt_clause(&inputs.control).unwrap_or("a single ruler or house");
            let resources = opt_clause(&inputs.resources)
                .unwrap_or("whatever the surrounding land naturally provides");
            let export_mode = opt_clause(&inputs.export_mode).unwrap_or("mixed");
            format!(
                "You generate one usable D&D settlement seed (a {kind}) for a game master. Return only JSON with fields name, visual_description, history_background, exports, tone, authority, danger_level, current_tension. This settlement is controlled by {control}; the authority field must reflect that. Its natural resources are: {resources}. Its exports are {export_mode} — produce a 1-3 item exports list consistent with that: raw → ship the resource roughly as-is; refined → the processed or finished good made from it; mixed → some of both.{geo} danger_level must be one of: {danger}.{leash}",
                danger = LOCATION_DANGER_LEVELS.join(", "),
                leash = LOCATION_PROSE_LEASH,
            )
        }
        LocationBranch::Site => {
            let focus = match inputs.site_focus.as_deref() {
                Some("past") => {
                    "what this place WAS — weight history_background heavily and keep the present quiet"
                }
                Some("present") => {
                    "what is HERE NOW — weight the current occupant and current_tension, keeping history light"
                }
                _ => "a balance of what it was and what is here now",
            };
            let draw = match opt_clause(&inputs.site_draw) {
                Some(draw) => format!(" The reason players are drawn here: {draw}."),
                None => String::new(),
            };
            let danger = locked_danger_clause(&inputs.danger_lock);
            format!(
                "You generate one usable D&D site seed (a {kind}) for a game master — a place the party stumbles upon, NOT a settlement. Return only JSON with fields name, visual_description, history_background, tone, authority, current_tension. Weight the writing toward {focus}.{draw}{geo} Do not invent rulers, governments, or exports; authority should name the lone occupant or guardian of the place, or 'Unknown' if it stands empty.{danger}{leash}",
                leash = LOCATION_PROSE_LEASH,
            )
        }
        LocationBranch::Hideout => {
            let owner = opt_clause(&inputs.base_owner).unwrap_or("a single operator");
            let protection = opt_clause(&inputs.base_protection).unwrap_or("secrecy");
            let purpose = opt_clause(&inputs.base_purpose).unwrap_or("refuge");
            let danger = locked_danger_clause(&inputs.danger_lock);
            format!(
                "You generate one usable D&D hideout seed (a {kind}) for a game master — someone's deliberately hidden, actively occupied base, NOT a ruin. Return only JSON with fields name, visual_description, history_background, tone, authority, current_tension. The base is owned by {owner}; the authority field must name that owner. It is protected by {protection} and exists for {purpose} — let that drive its defenses and how players might find it. Write it present-tense and do not invent exports.{geo}{danger}{leash}",
                leash = LOCATION_PROSE_LEASH,
            )
        }
        LocationBranch::Guildhall => {
            let faction = opt_clause(&inputs.faction_name).unwrap_or("an established organization");
            let role = match opt_clause(&inputs.public_role) {
                Some(role) => format!(" It functions publicly as {role}."),
                None => String::new(),
            };
            // The hall stands within an existing place (Q-G3); ground it there.
            let anchor = match opt_clause(&inputs.location_anchor) {
                Some(place) => {
                    format!(" This hall stands within {place}; ground it in that place.")
                }
                None => String::new(),
            };
            format!(
                "You generate one usable D&D guildhall seed (a {kind}) for a game master — the PUBLIC headquarters of an established organization, NOT a settlement and NOT a hidden base. Return only JSON with fields name, visual_description, history_background, tone, authority, danger_level, current_tension. This hall is the public seat of {faction}; the authority field must name {faction}, and the visual_description, history_background, tone, and current_tension must reflect that organization's identity, methods, and goals.{role}{anchor} Do not invent exports or trade goods. danger_level must be one of: {danger} — a public hall's danger is usually low unless the organization courts it.{leash}",
                danger = LOCATION_DANGER_LEVELS.join(", "),
                leash = LOCATION_PROSE_LEASH,
            )
        }
    }
}

fn wizard_location_schema(branch: LocationBranch) -> serde_json::Value {
    match branch {
        LocationBranch::Settlement => serde_json::json!({
            "type": "object",
            "required": ["name", "visual_description", "history_background", "exports", "tone", "authority", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "exports": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "type": "string", "minLength": 1 } },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        // Site + Hideout: exports suppressed, danger GM-locked — both omitted so the
        // model can never emit them.
        LocationBranch::Site | LocationBranch::Hideout => serde_json::json!({
            "type": "object",
            "required": ["name", "visual_description", "history_background", "tone", "authority", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
        // Guildhall: exports suppressed (omitted), but danger_level is LLM-derived so
        // it stays in the schema. `authority` is overwritten with the faction after.
        LocationBranch::Guildhall => serde_json::json!({
            "type": "object",
            "required": ["name", "visual_description", "history_background", "tone", "authority", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        }),
    }
}

/// The user-message seed for the wizard request: a concise restatement of the
/// locked answers. It also doubles as the `@reference` probe, so any place names in
/// the geography resolve against the vault. Reused by the wizard's `build_seed_prompt`
/// to persist the GM's intent as reroll bias.
pub(crate) fn build_wizard_user_prompt(inputs: &LocationWizardInputs) -> String {
    let kind = &inputs.kind_type;
    let mut parts = vec![format!("Create a {kind}.")];
    match location_branch(kind) {
        LocationBranch::Settlement => {
            if let Some(control) = opt_clause(&inputs.control) {
                parts.push(format!("Controlled by: {control}."));
            }
            if let Some(resources) = opt_clause(&inputs.resources) {
                parts.push(format!("Natural resources: {resources}."));
            }
            if let Some(mode) = opt_clause(&inputs.export_mode) {
                parts.push(format!("Export mode: {mode}."));
            }
        }
        LocationBranch::Site => {
            if let Some(focus) = opt_clause(&inputs.site_focus) {
                parts.push(format!("Focus: {focus}."));
            }
            if let Some(draw) = opt_clause(&inputs.site_draw) {
                parts.push(format!("Draw: {draw}."));
            }
        }
        LocationBranch::Hideout => {
            if let Some(owner) = opt_clause(&inputs.base_owner) {
                parts.push(format!("Owner: {owner}."));
            }
            if let Some(protection) = opt_clause(&inputs.base_protection) {
                parts.push(format!("Protection: {protection}."));
            }
            if let Some(purpose) = opt_clause(&inputs.base_purpose) {
                parts.push(format!("Purpose: {purpose}."));
            }
        }
        LocationBranch::Guildhall => {
            // `@factions/<name>` resolves to the faction's authoritative metadata when
            // it is a published note (the reference machinery reads it); a draft or
            // free-typed name simply doesn't resolve and the name carries on its own.
            if let Some(faction) = opt_clause(&inputs.faction_name) {
                parts.push(format!(
                    "The organization that runs this hall: @factions/{faction}."
                ));
            }
            if let Some(role) = opt_clause(&inputs.public_role) {
                parts.push(format!("Public role: {role}."));
            }
            // `@locations/<sub>/<anchor>` pulls the containing place's metadata in the
            // same way — the `@reference` system is path-keyed, so a subfoldered note
            // must carry its subfolder or grounding degrades to name-only. A flat note
            // or free-typed place has no subfolder and falls back to `@locations/<anchor>`.
            if let Some(anchor) = opt_clause(&inputs.location_anchor) {
                match opt_clause(&inputs.location_anchor_sub) {
                    Some(sub) => parts.push(format!("It stands within @locations/{sub}/{anchor}.")),
                    None => parts.push(format!("It stands within @locations/{anchor}.")),
                }
            }
        }
    }
    if let Some(geo) = opt_clause(&inputs.geography) {
        parts.push(format!("Geography: {geo}."));
    }
    if let Some(hint) = opt_clause(&inputs.hint) {
        parts.push(format!("Also: {hint}."));
    }
    parts.join(" ")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocationSeed {
    pub name: String,
    // GM-locked in the wizard (omitted from the wizard schema), so default to empty
    // when absent; the one-shot schema still requires it. Overwritten from the
    // accumulator in the wizard path.
    #[serde(default)]
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    // Suppressed (Site/Hideout) or derived (Settlement) in the wizard; the
    // site/hideout schema omits it entirely, so tolerate its absence.
    #[serde(default)]
    pub exports: Vec<String>,
    pub tone: String,
    // Suppressed by the one-shot lane (its schema omits it, emptied after); the
    // wizard schemas still require it, so tolerate its absence here.
    #[serde(default)]
    pub authority: String,
    // GM-locked for Site/Hideout (omitted from their schema, injected after).
    #[serde(default)]
    pub danger_level: String,
    pub current_tension: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FactionSeed {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GodSeed {
    pub name: String,
    pub epithet: String,
    pub rank: String,
    #[serde(default)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ItemSeed {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventSeed {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonBeatSeed {
    pub content_type: String,
    pub idea: String,
    #[serde(default)]
    pub player_goals: String,
    pub lever: String,
    #[serde(default)]
    pub loot: Option<String>,
    #[serde(default)]
    pub design_note: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonSeed {
    pub name: String,
    #[serde(default)]
    pub location: String, // the single bounded place all five beats sit inside
    pub premise: String,
    pub beats: Vec<DungeonBeatSeed>,
}

impl DungeonSeed {
    /// Normalize narrative fields and the conditional loot line. `function` is
    /// assigned later in `to_beats`, not here, so the skeleton stays ours.
    fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.location = normalize_unknown_text(&self.location);
        self.premise = normalize_unknown_text(&self.premise);
        for beat in self.beats.iter_mut() {
            beat.content_type = normalize_unknown_text(&beat.content_type).to_ascii_lowercase();
            beat.idea = normalize_unknown_text(&beat.idea);
            beat.player_goals = normalize_unknown_text(&beat.player_goals);
            beat.lever = normalize_unknown_text(&beat.lever);
            beat.design_note = normalize_unknown_text(&beat.design_note);
            beat.loot = beat
                .loot
                .as_ref()
                .map(|loot| loot.trim().to_string())
                .filter(|loot| !loot.is_empty() && !loot.eq_ignore_ascii_case("none"));
        }
    }

    /// Convert to persistable beats, assigning the fixed function skeleton by
    /// position (beat 0 = Entrance … beat 4 = Resolution).
    pub fn to_beats(&self) -> Vec<DungeonBeat> {
        self.beats
            .iter()
            .enumerate()
            .map(|(i, beat)| DungeonBeat {
                function: DUNGEON_FUNCTIONS
                    .get(i)
                    .copied()
                    .unwrap_or("Beat")
                    .to_string(),
                content_type: beat.content_type.clone(),
                idea: beat.idea.clone(),
                player_goals: beat.player_goals.clone(),
                lever: beat.lever.clone(),
                loot: beat.loot.clone(),
                design_note: beat.design_note.clone(),
                // The plan's overlay/faction tint are stamped on afterward (the seed
                // doesn't carry them); see apply_plan_meta_to_beats.
                overlay: None,
                factions: false,
            })
            .collect()
    }
}

/// Pass 1 output: the prose the GM reviews before structuring (name + the one
/// bounded location + the one-to-two-paragraph story).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DungeonStory {
    pub name: String,
    #[serde(default)]
    pub location: String,
    pub story: String,
}

impl DungeonStory {
    fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.location = normalize_unknown_text(&self.location);
        self.story = self.story.trim().to_string();
    }
}

/// Pass 2 raw output: the spine plus five beats, each MISSING `content_type` —
/// that is injected from the plan, never requested from the model.
#[derive(Debug, Clone, serde::Deserialize)]
struct DungeonStructured {
    premise: String,
    beats: Vec<DungeonStructuredBeat>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct DungeonStructuredBeat {
    idea: String,
    #[serde(default)]
    player_goals: String,
    lever: String,
    #[serde(default)]
    loot: Option<String>,
    #[serde(default)]
    design_note: String,
}

fn describe_recent_dungeon_stories(payloads: Vec<String>) -> String {
    let names: Vec<String> = payloads
        .iter()
        .filter_map(|payload| serde_json::from_str::<DungeonStory>(payload).ok())
        .map(|story| story.name)
        .filter(|name| !name.trim().is_empty())
        .take(10)
        .collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join("; ")
    }
}

/// Plain-language phrase for an anchor type, woven into the Pass-1 story so the
/// rolled content arrives without leaking the internal jargon.
fn anchor_story_phrase(content_type: &str) -> &'static str {
    match content_type {
        "combat" => "a hostile force or dangerous creature that must be fought or slipped past",
        "cache" => "a cache of treasure or reward waiting to be found",
        "forge" => "a forge, crucible, or workshop where something can be made or repaired",
        "puzzle" => {
            "a sealed way forward — a barred door or mechanism — that opens only once the party finds the right key or condition"
        }
        "offshoot" => {
            "an optional branching path: a side chamber, a hidden room, or a tempting dead end"
        }
        "sidekick" => {
            "a lone ally met here who joins the party and travels deeper with them through the rest of this place"
        }
        "oddity" => "a strange and significant object that is the very reason this place exists",
        "ability_check" => {
            "a feat of skill or nerve to get past — a climb, a leap, a steady hand, or a test of will"
        }
        _ => "something noteworthy",
    }
}

/// Mechanical meaning of an anchor type, given to Pass 2 so the card's idea
/// actually delivers that type's function. Also reused by the single-beat reroll,
/// which holds the rolled type fixed and only regenerates the prose.
pub(crate) fn anchor_mechanic(content_type: &str) -> &'static str {
    match content_type {
        "combat" => {
            "a fight; convey the enemy's tactics, behavior, and use of terrain, and NEVER name specific creatures (the GM picks them)"
        }
        "cache" => "a stash of loot or rewards",
        "forge" => {
            "a place to craft or repair magic items; the idea must involve that crafting or repair"
        }
        "puzzle" => {
            "a locked-door->key obstacle of one or more steps; never a riddle or logic puzzle"
        }
        "offshoot" => "an optional side passage, hidden room, or dead end off the main path",
        "sidekick" => {
            "a dungeon-only ally introduced here who joins the party and stays with them through the later beats, leaving only when the dungeon ends"
        }
        "oddity" => "the world-significant object that is the reason this dungeon exists",
        "ability_check" => {
            "an ability/skill check the party must pass — name the check (athletics, perception, persuasion, sleight of hand…) and what failure costs; not a riddle"
        }
        _ => "a noteworthy room",
    }
}

fn overlay_phrase(overlay_type: &str) -> &'static str {
    match overlay_type {
        "foreshadowing" => "a hint of something still to come, here or out in the wider campaign",
        "history" => "a piece of lore about this place, its people, or its makers",
        "map" => {
            "a glimpse of the surrounding world — a route, a landmark, or a link to somewhere else"
        }
        _ => "a telling detail",
    }
}

/// Plain-language layout for a topology, so its SHAPE can inform generation
/// without ever leaking the proper-noun name (e.g. "Foglio's Snail") into the
/// prose, where the model would otherwise reuse it as the dungeon's name.
fn topology_shape(topology: &str) -> Option<&'static str> {
    match topology {
        "The Railroad" => Some("a straight sequence of rooms, each leading to the next"),
        "The Moose" => Some("a short dead-end branch near the entrance off a longer main passage"),
        "The V for Vendetta" => {
            Some("two passages branching in opposite directions from the entrance")
        }
        "The Arrow" => Some("a three-way junction near the entrance"),
        "The Fauchard Fork" => Some("an early fork into one short path and one longer path"),
        "The Evil Mule" => Some("a branch that soon forks again into two"),
        "Foglio's Snail" => Some("two rooms deep, then a split into two hidden side rooms"),
        "The Paw" => Some("a hub that branches into three rooms"),
        "The Cross" => Some("a central hub with rooms opening off every side"),
        _ => None, // "none" / unknown — impose no spatial shape
    }
}

fn twist_directive(twist: &str) -> &'static str {
    match twist {
        "false_victory" => {
            "in the middle, hand the party an apparent win that then curdles — they think they've succeeded, then lose it"
        }
        "false_defeat" => "in the middle, stage an apparent loss the party then claws back from",
        _ => "play the arc straight — no fake-out in the middle",
    }
}

/// The numbered movement list for Pass 1: each beat's rolled anchor rendered as a
/// story ingredient, tied to its place in the arc.
fn pass1_elements_block(plan: &DungeonContentPlan) -> String {
    const LABELS: [&str; 5] = [
        "the way in",
        "the first turn inside",
        "where it costs them",
        "the peak",
        "the payoff",
    ];
    let mut out = String::new();
    for (i, anchor) in plan.anchors.iter().enumerate() {
        out.push_str(&format!(
            "  {}. ({}): {}\n",
            i + 1,
            LABELS[i],
            anchor_story_phrase(anchor)
        ));
    }
    out
}

/// The per-beat assignment block for Pass 2: fixed role + the GIVEN content type
/// and its mechanic + the loot rule, plus any overlay layer to fold in.
fn pass2_assignment_block(plan: &DungeonContentPlan) -> String {
    const ROLES: [&str; 5] = [
        "the way in (what stops a stray wanderer from getting through)",
        "the first obstacle inside (roleplay, a sealed way, or a trap)",
        "the cost, where the party PAYS",
        "the peak: a real confrontation, reversal, or revelation",
        "the payoff — a reward, a revelation, or humble pie, NOT another fight",
    ];
    const LOOT_RULES: [&str; 5] = [
        "Loot: null.",
        "Loot: null.",
        "Loot: null.",
        "Loot: only if it reads as the boss's hoard.",
        "Loot: REQUIRED — name a concrete reward the party claims here.",
    ];
    let mut out = String::new();
    for (i, anchor) in plan.anchors.iter().enumerate() {
        // A cache beat is a reward stash anywhere it lands: loot is mandatory.
        let loot_rule = if anchor == "cache" {
            "Loot: REQUIRED — name a concrete reward the party claims here."
        } else {
            LOOT_RULES[i]
        };
        out.push_str(&format!(
            "Beat {} — {}. Type: {} — {}. {}",
            i + 1,
            ROLES[i],
            anchor.to_uppercase(),
            anchor_mechanic(anchor),
            loot_rule,
        ));
        if let Some(overlay) = &plan.overlay
            && overlay.beat_index == i
        {
            out.push_str(&format!(
                " Also layer in {}: {}.",
                overlay.overlay_type,
                overlay_phrase(&overlay.overlay_type)
            ));
        }
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct PromptReferenceContext {
    pub system_context: String,
}

/// Parse the recent-seed payloads (raw JSON strings from the generation log) into
/// typed seeds for dedup/context, dropping any that no longer deserialize. One
/// generic replaces the former per-kind `parse_recent_*_seeds` fan-out.
pub(crate) fn parse_recent_seeds<T: serde::de::DeserializeOwned>(payloads: Vec<String>) -> Vec<T> {
    payloads
        .into_iter()
        .filter_map(|payload| serde_json::from_str::<T>(&payload).ok())
        .collect()
}

fn recent_event_title_set(seeds: &[EventSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.title.trim().to_ascii_lowercase())
        .filter(|title| !title.is_empty())
        .collect()
}

fn describe_recent_event_seeds(seeds: &[EventSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| seed.title.clone())
        .collect::<Vec<_>>()
        .join("; ")
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

fn recent_god_name_set(seeds: &[GodSeed]) -> std::collections::HashSet<String> {
    seeds
        .iter()
        .map(|seed| seed.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

fn describe_recent_god_seeds(seeds: &[GodSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| format!("{} | {} | {}", seed.name, seed.rank, seed.alignment))
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
        .map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.danger_level))
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
        "a", "an", "and", "as", "at", "by", "deceased", "ex", "for", "former", "from", "in", "of",
        "on", "retired", "the", "to", "under", "with",
    ];
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty() && !STOP_WORDS.contains(&token.as_str()))
        .collect()
}

pub(crate) fn occupation_anchor(value: &str) -> String {
    occupation_tokens(value)
        .into_iter()
        .next()
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn recent_occupation_anchor_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
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

fn describe_recent_npc_seeds(seeds: &[NpcSeed]) -> String {
    if seeds.is_empty() {
        return "none".to_string();
    }
    seeds
        .iter()
        .take(10)
        .map(|seed| {
            format!(
                "{} | {} | {} | {}",
                seed.name, seed.race, seed.sex, seed.occupation
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn describe_recent_npc_occupation_anchors(seeds: &[NpcSeed]) -> String {
    let mut anchors: Vec<String> = recent_occupation_anchor_set(seeds).into_iter().collect();
    if anchors.is_empty() {
        return "none".to_string();
    }
    anchors.sort();
    anchors.truncate(12);
    anchors.join(", ")
}

fn build_prompt_reference_context(
    prompt: &str,
    entries: &[VaultReferenceEntry],
    vault: &Vault,
) -> PromptReferenceContext {
    let keys = extract_prompt_reference_keys(prompt, entries);
    if keys.is_empty() {
        return PromptReferenceContext::default();
    }

    let path_by_key: std::collections::HashMap<String, String> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .markdown_path
                .as_ref()
                .map(|path| (entry.key.to_lowercase(), path.clone()))
        })
        .collect();
    let mut blocks = Vec::new();

    let canonical_metadata = match EntityStore::new() {
        Ok(store) => canonical_metadata_map(&store),
        Err(err) => {
            eprintln!("reference context warning: failed to load canonical entities: {err}");
            HashMap::new()
        }
    };

    for key in keys.into_iter() {
        let Some(path) = path_by_key.get(&key.to_lowercase()) else {
            continue;
        };
        let normalized_path = normalize_relative_path_for_storage(path);
        let metadata = if let Some(canonical) = canonical_metadata.get(&normalized_path) {
            canonical.clone()
        } else {
            let contents = match vault.read_relative(Path::new(path)) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!(
                        "reference context warning: failed reading {}: {}",
                        path, err
                    );
                    continue;
                }
            };
            match reference_payload_from_markdown(&contents) {
                Some(value) => value,
                None => continue,
            }
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

fn canonical_metadata_map(store: &EntityStore) -> HashMap<String, String> {
    let mut map = HashMap::new();

    if let Ok(npcs) = store.list_npcs() {
        for npc in npcs {
            if let Ok(serialized) = toml::to_string_pretty(&npc) {
                map.insert(
                    normalize_relative_path_for_storage(&npc.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(locations) = store.list_locations() {
        for location in locations {
            if let Ok(serialized) = toml::to_string_pretty(&location) {
                map.insert(
                    normalize_relative_path_for_storage(&location.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(factions) = store.list_factions() {
        for faction in factions {
            if let Ok(serialized) = toml::to_string_pretty(&faction) {
                map.insert(
                    normalize_relative_path_for_storage(&faction.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(items) = store.list_items() {
        for item in items {
            if let Ok(serialized) = toml::to_string_pretty(&item) {
                map.insert(
                    normalize_relative_path_for_storage(&item.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(events) = store.list_events() {
        for event in events {
            if let Ok(serialized) = toml::to_string_pretty(&event) {
                map.insert(
                    normalize_relative_path_for_storage(&event.vault_path),
                    serialized,
                );
            }
        }
    }

    map
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

fn reference_payload_from_markdown(contents: &str) -> Option<String> {
    if let Some(block) = extract_runebound_toml(contents) {
        return Some(block);
    }
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LocationWizardInputs, NPC_GEN_SAMPLING, NpcSeed, OUTPUT_RESERVE_TOKENS, build_seed_payload,
        build_wizard_user_prompt, capacity_notice, describe_recent_npc_occupation_anchors,
        location_dir_for_kind, location_subfolder, occupation_anchor, recent_occupation_anchor_set,
        reference_payload_from_markdown,
    };

    #[test]
    fn location_subfolder_maps_each_branch_and_flattens_other() {
        assert_eq!(location_subfolder("hamlet"), Some("settlements"));
        assert_eq!(location_subfolder("town"), Some("settlements"));
        assert_eq!(location_subfolder("city"), Some("settlements"));
        assert_eq!(location_subfolder("ruin"), Some("sites"));
        assert_eq!(location_subfolder("landmark"), Some("sites"));
        assert_eq!(location_subfolder("wilderness"), Some("sites"));
        assert_eq!(location_subfolder("hideout"), Some("hideouts"));
        assert_eq!(location_subfolder("guildhall"), Some("guildhalls"));
        // other / freeform / unknown stay flat (NOT routed into settlements/).
        assert_eq!(location_subfolder("other"), None);
        assert_eq!(location_subfolder(""), None);
        assert_eq!(location_subfolder("village"), None);
    }

    #[test]
    fn location_dir_for_kind_appends_subfolder_or_stays_flat() {
        assert_eq!(
            location_dir_for_kind("locations", "ruin"),
            "locations/sites"
        );
        assert_eq!(
            location_dir_for_kind("locations", "town"),
            "locations/settlements"
        );
        assert_eq!(
            location_dir_for_kind("locations", "guildhall"),
            "locations/guildhalls"
        );
        // other / unknown -> flat (the one-shot lane passes "other" deliberately).
        assert_eq!(location_dir_for_kind("locations", "other"), "locations");
        assert_eq!(location_dir_for_kind("locations", "village"), "locations");
    }

    #[test]
    fn guildhall_prompt_threads_anchor_subfolder_when_present() {
        // A subfoldered anchor must emit the path-keyed `@locations/<sub>/<anchor>`.
        let inputs = LocationWizardInputs {
            kind_type: "guildhall".to_string(),
            location_anchor: Some("Silverhall".to_string()),
            location_anchor_sub: Some("settlements".to_string()),
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&inputs);
        assert!(prompt.contains("@locations/settlements/Silverhall"));
    }

    #[test]
    fn guildhall_prompt_falls_back_to_name_only_when_flat() {
        // A flat note / free-typed place has no subfolder -> name-only `@locations/<anchor>`.
        let inputs = LocationWizardInputs {
            kind_type: "guildhall".to_string(),
            location_anchor: Some("Silverhall".to_string()),
            location_anchor_sub: None,
            ..Default::default()
        };
        let prompt = build_wizard_user_prompt(&inputs);
        assert!(prompt.contains("@locations/Silverhall"));
        assert!(!prompt.contains("@locations/settlements"));
    }

    #[test]
    fn seed_payload_wraps_messages_schema_and_sampling_with_num_ctx() {
        // Generation payloads always carry num_ctx (unlike reroll's optional one).
        let schema = serde_json::json!({ "type": "object" });
        let payload = build_seed_payload(
            "test-model",
            &NPC_GEN_SAMPLING,
            8192,
            42,
            &schema,
            "SYS",
            "USR",
        );
        assert_eq!(
            payload,
            serde_json::json!({
                "model": "test-model",
                "stream": false,
                "format": schema,
                "options": {
                    "temperature": NPC_GEN_SAMPLING.temperature,
                    "top_p": NPC_GEN_SAMPLING.top_p,
                    "repeat_penalty": NPC_GEN_SAMPLING.repeat_penalty,
                    "seed": 42,
                    "num_ctx": 8192
                },
                "messages": [
                    { "role": "system", "content": "SYS" },
                    { "role": "user", "content": "USR" }
                ]
            })
        );
    }

    #[test]
    fn capacity_notice_none_when_comfortably_under_budget() {
        assert!(capacity_notice(100, 8192).is_none());
    }

    #[test]
    fn capacity_notice_fires_when_prompt_crowds_output_reserve() {
        // Exactly at the budget edge: estimate == num_ctx - reserve -> no notice.
        let num_ctx: u32 = 4096;
        let budget = num_ctx as usize - OUTPUT_RESERVE_TOKENS;
        assert!(capacity_notice(budget, num_ctx).is_none());
        // One token over the budget -> notice.
        let notice = capacity_notice(budget + 1, num_ctx).expect("notice expected");
        assert!(notice.contains("num_ctx"));
    }

    #[test]
    fn occupation_anchor_ignores_descriptive_fillers() {
        assert_eq!(
            occupation_anchor("former cartographer, current wanderer"),
            "cartographer"
        );
        assert_eq!(
            occupation_anchor("Cartographer & explorer (deceased)"),
            "cartographer"
        );
    }

    #[test]
    fn recent_occupation_anchor_set_collects_unique_roots() {
        let seeds = vec![
            NpcSeed {
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
            NpcSeed {
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
            NpcSeed {
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
            NpcSeed {
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
    fn reference_payload_prefers_runebound_block() {
        let markdown =
            "# Notes\n\n```runebound\ntype = \"npc\"\nname = \"Jimmy\"\n```\n\nExtra text";
        let payload = reference_payload_from_markdown(markdown).expect("payload");
        assert!(payload.contains("type = \"npc\""));
        assert!(!payload.contains("Extra text"));
    }

    #[test]
    fn reference_payload_falls_back_to_full_file() {
        let markdown = "# Notes about Jimmy\nNo runebound block present.";
        let payload = reference_payload_from_markdown(markdown).expect("payload");
        assert!(payload.contains("Notes about Jimmy"));
        assert!(payload.contains("No runebound block"));
    }
}
