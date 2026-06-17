use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{
    attempt_seed, build_chat_client, detail_directive, load_generation_config, post_chat_for_content,
};
use crate::services::vault_ref::{
    VaultReferenceEntry, extract_prompt_reference_keys, load_vault_reference_entries,
};
use crate::utils::{
    estimate_tokens, normalize_faction_seed, normalize_god_seed, normalize_item_category,
    normalize_item_rarity, normalize_location_seed, normalize_relative_path_for_storage,
    normalize_sex, normalize_unknown_list, normalize_unknown_text, validate_faction_details,
    validate_god_details, validate_location_details,
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
use std::path::{Path, PathBuf};

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
pub(crate) fn build_reference_context(
    config: &AppConfig,
    user_prompt: &str,
    workspace_root: &Path,
) -> PromptReferenceContext {
    let Some(vault_path) = config.vault.path.clone() else {
        return PromptReferenceContext::default();
    };
    let vault = Vault::new(vault_path);
    if vault.ensure_root_exists().is_err() {
        return PromptReferenceContext::default();
    }
    match load_vault_reference_entries(&vault) {
        Ok(entries) => build_prompt_reference_context(user_prompt, &entries, &vault, workspace_root),
        Err(err) => {
            eprintln!("reference context warning: {err}");
            PromptReferenceContext::default()
        }
    }
}

pub struct AiGenerationService;

impl AiGenerationService {
    pub async fn generate_npc_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<NpcSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one D&D NPC for a fantasy campaign.");

        let reference_context = build_reference_context(&config, user_prompt, workspace_root);

        let recent_payloads = generation_repo
            .recent_prompts(database, "npc_seed", 20)
            .await?;
        let recent_seeds = parse_recent_npc_seeds(recent_payloads);
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

        let (client, url) = build_chat_client(&config)?;

        let mut seen_attempt_names = HashSet::new();
        let mut seen_attempt_occupation_anchors = HashSet::new();

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names and occupations." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.1, "top_p": 0.92, "repeat_penalty": 1.15, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying. carrying must be an array of item strings. Age must be numeric text with no commas, separators, or trailing punctuation (e.g., '133', not '1,133' or '133,'). Height should be imperial like 5'11\", weight_lbs should be lbs as text like 180 with no commas. Prefer occupations different from recent occupations and avoid occupation roots in this list unless explicitly requested: {}. Avoid these recent seeds: {}.{}{}{}",
                        recent_occupation_context, recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) },
                        detail_directive(config.generation.verbosity)
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<NpcSeed, _> = serde_json::from_str(&content);
            let Ok(mut seed) = parsed else { continue };

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

            if seed.name.is_empty() || seed.race.is_empty() { continue; }

            let normalized_name = seed.name.to_ascii_lowercase();
            if recent_names.contains(&normalized_name) || seen_attempt_names.contains(&normalized_name) { continue; }
            let occupation_anchor = occupation_anchor(&seed.occupation);
            if occupation_anchor != "unknown" && (recent_occupation_anchors.contains(&occupation_anchor) || seen_attempt_occupation_anchors.contains(&occupation_anchor)) { continue; }
            seen_attempt_names.insert(normalized_name);
            seen_attempt_occupation_anchors.insert(occupation_anchor);

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "npc_seed", None, &serialized_seed)
                .await?;

            return Ok(SeedGeneration { seed, notice: notice.clone() });
        }

        Err("failed to generate valid structured NPC output from ollama".to_string())
    }

    pub async fn generate_location_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<LocationSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy location for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt, workspace_root);

        let recent_payloads = generation_repo
            .recent_prompts(database, "location_seed", 20)
            .await?;
        let recent_seeds = parse_recent_location_seeds(recent_payloads);
        let recent_names = recent_location_name_set(&recent_seeds);
        let recent_context = describe_recent_location_seeds(&recent_seeds);

        let estimated_tokens = SYSTEM_BOILERPLATE_TOKENS
            + estimate_tokens(&reference_context.system_context)
            + estimate_tokens(&recent_context)
            + estimate_tokens(user_prompt);
        let notice = capacity_notice(estimated_tokens, config.ollama.num_ctx);

        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "kind_type", "visual_description", "history_background", "exports", "tone", "authority", "danger_level", "current_tension"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "kind_type": { "type": "string", "enum": LOCATION_KIND_TYPES },
                "kind_custom": { "type": ["string", "null"] },
                "visual_description": { "type": "string", "minLength": 1 },
                "history_background": { "type": "string", "minLength": 1 },
                "exports": { "type": "array", "minItems": 1, "maxItems": 3, "items": { "type": "string", "minLength": 1 } },
                "tone": { "type": "string", "minLength": 1 },
                "authority": { "type": "string", "minLength": 1 },
                "danger_level": { "type": "string", "enum": LOCATION_DANGER_LEVELS },
                "current_tension": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        });

        let (client, url) = build_chat_client(&config)?;

        let mut seen_attempt_names = HashSet::new();

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.08, "top_p": 0.93, "repeat_penalty": 1.14, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate usable D&D location seeds. Return only JSON with fields name, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension. exports must have 1-3 short items. tone must be 2-5 words. If kind_type is not other, kind_custom must be null. If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any region, settlement, or landmark instead of inventing new ones. Avoid these recent seeds: {}.{}{}{}",
                        recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) },
                        detail_directive(config.generation.verbosity)
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<LocationSeed, _> = serde_json::from_str(&content);
            let Ok(seed) = parsed else { continue };

            let seed = match normalize_location_seed(seed) {
                Ok(seed) => seed,
                Err(_) => continue,
            };
            if validate_location_details(&seed).is_err() { continue; }

            let normalized_name = seed.name.to_ascii_lowercase();
            if recent_names.contains(&normalized_name) || seen_attempt_names.contains(&normalized_name) { continue; }
            seen_attempt_names.insert(normalized_name);

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "location_seed", None, &serialized_seed)
                .await?;

            return Ok(SeedGeneration { seed, notice: notice.clone() });
        }

        Err("failed to generate valid structured location output from ollama".to_string())
    }

    pub async fn generate_faction_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<FactionSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy faction for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt, workspace_root);

        let recent_payloads = generation_repo
            .recent_prompts(database, "faction_seed", 20)
            .await?;
        let recent_seeds = parse_recent_faction_seeds(recent_payloads);
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

        let (client, url) = build_chat_client(&config)?;

        let mut seen_attempt_names = HashSet::new();

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.08, "top_p": 0.93, "repeat_penalty": 1.12, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate usable D&D faction seeds. Return only JSON with fields name, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description. symbol_description should be exactly 1 sentence describing symbol/sigil/colors/banner/iconography. If kind_type is not other, kind_custom must be null. If referenced vault metadata includes an established name for an organization, group, guild, or house, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {}.{}{}{}",
                        recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) },
                        detail_directive(config.generation.verbosity)
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<FactionSeed, _> = serde_json::from_str(&content);
            let Ok(seed) = parsed else { continue };

            let seed = match normalize_faction_seed(seed) {
                Ok(seed) => seed,
                Err(_) => continue,
            };
            if validate_faction_details(&seed).is_err() { continue; }

            let normalized_name = seed.name.to_ascii_lowercase();
            if enforce_unique_name && (recent_names.contains(&normalized_name) || seen_attempt_names.contains(&normalized_name)) { continue; }
            if enforce_unique_name { seen_attempt_names.insert(normalized_name); }

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "faction_seed", None, &serialized_seed)
                .await?;

            return Ok(SeedGeneration { seed, notice: notice.clone() });
        }

        Err("failed to generate valid structured faction output from ollama".to_string())
    }

    pub async fn generate_god_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<GodSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy deity for a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt, workspace_root);

        let recent_payloads = generation_repo
            .recent_prompts(database, "god_seed", 20)
            .await?;
        let recent_seeds = parse_recent_god_seeds(recent_payloads);
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

        let (client, url) = build_chat_client(&config)?;

        let mut seen_attempt_names = HashSet::new();

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.08, "top_p": 0.93, "repeat_penalty": 1.12, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate usable D&D deity seeds. Return only JSON with fields name, epithet, rank, rank_custom, alignment, domains, symbol, appearance, dogma, realm, worshippers, clergy, allies, rivals. rank must be one of: {}. alignment must be one of: {}. If rank is not other, rank_custom must be null. symbol should be exactly 1 sentence describing the holy symbol/sigil/iconography. domains is a list of spheres the deity governs (e.g. war, death, harvest). If referenced vault metadata includes an established name for a god or power, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {}.{}{}{}",
                        GOD_RANKS.join(", "), GOD_ALIGNMENTS.join(", "),
                        recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) },
                        detail_directive(config.generation.verbosity)
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<GodSeed, _> = serde_json::from_str(&content);
            let Ok(seed) = parsed else { continue };

            let seed = match normalize_god_seed(seed) {
                Ok(seed) => seed,
                Err(_) => continue,
            };
            if validate_god_details(&seed).is_err() { continue; }

            let normalized_name = seed.name.to_ascii_lowercase();
            if enforce_unique_name && (recent_names.contains(&normalized_name) || seen_attempt_names.contains(&normalized_name)) { continue; }
            if enforce_unique_name { seen_attempt_names.insert(normalized_name); }

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "god_seed", None, &serialized_seed)
                .await?;

            return Ok(SeedGeneration { seed, notice: notice.clone() });
        }

        Err("failed to generate valid structured god output from ollama".to_string())
    }

    pub async fn generate_item_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<ItemSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one magical or legendary item.");

        let reference_context = build_reference_context(&config, user_prompt, workspace_root);

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

        let (client, url) = build_chat_client(&config)?;

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid or repeated. Return only valid JSON."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.05, "top_p": 0.92, "repeat_penalty": 1.1, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate tabletop RPG items. Category choices: {}. Rarity choices: {}. Provide appearance, abilities, drawbacks (or 'None'), history, value in format like '1000gp' or '250sp' or '50cp', and location. If referenced vault metadata is provided, treat it as authoritative setting context and reuse established canonical names for any person, place, or organization instead of inventing new ones.{}{}{}",
                        ITEM_CATEGORIES.join(", "),
                        ITEM_RARITIES.join(", "),
                        repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) },
                        detail_directive(config.generation.verbosity)
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<ItemSeed, _> = serde_json::from_str(&content);
            let Ok(mut seed) = parsed else { continue };

            seed.name = seed.name.trim().to_string();
            seed.category = normalize_item_category(&seed.category)?;
            seed.rarity = normalize_item_rarity(&seed.rarity)?;
            seed.attunement = normalize_unknown_text(&seed.attunement);
            seed.materials = normalize_unknown_list(seed.materials);
            seed.appearance = normalize_unknown_text(&seed.appearance);
            seed.abilities = normalize_unknown_text(&seed.abilities);
            seed.drawbacks = normalize_unknown_text(&seed.drawbacks);
            seed.history = normalize_unknown_text(&seed.history);
            seed.value = normalize_unknown_text(&seed.value);
            seed.location = normalize_unknown_text(&seed.location);

            if seed.name.is_empty() {
                continue;
            }

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "item_seed", None, &serialized_seed)
                .await?;

            return Ok(SeedGeneration { seed, notice: notice.clone() });
        }

        Err("failed to generate valid structured item output from ollama".to_string())
    }

    pub async fn generate_event_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<EventSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Write a short piece of lore about a notable event in a D&D campaign.");

        let reference_context = build_reference_context(&config, user_prompt, workspace_root);

        let recent_payloads = generation_repo
            .recent_prompts(database, "event_seed", 20)
            .await?;
        let recent_seeds = parse_recent_event_seeds(recent_payloads);
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

        let (client, url) = build_chat_client(&config)?;

        let mut seen_attempt_titles = HashSet::new();

        for attempt in 0..5 {
            let run_seed = attempt_seed(attempt);
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior titles."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.05, "top_p": 0.93, "repeat_penalty": 1.12, "seed": run_seed, "num_ctx": config.ollama.num_ctx },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You write evocative D&D campaign lore about an event — a battle, a betrayal, a founding, a disaster, a discovery. Return only JSON with fields title and body. title is a short evocative name for the event. body is several paragraphs of narrative prose (separated by blank lines) telling the story of what happened, who was involved, and why it matters. Write it as flowing narrative lore, not as bullet points or labeled attributes. If referenced vault metadata is provided, treat it as authoritative setting context and weave in those established people, places, and organizations by their exact canonical names instead of inventing new ones. Avoid these recent event titles: {}.{}{}{}",
                        recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) },
                        detail_directive(config.generation.verbosity)
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let Some(content) = post_chat_for_content(&client, &url, &payload).await? else {
                continue;
            };

            let parsed: Result<EventSeed, _> = serde_json::from_str(&content);
            let Ok(mut seed) = parsed else { continue };

            seed.title = seed.title.trim().to_string();
            seed.body = seed.body.trim().to_string();

            if seed.title.is_empty() || seed.body.is_empty() {
                continue;
            }

            let normalized_title = seed.title.to_ascii_lowercase();
            if recent_titles.contains(&normalized_title)
                || seen_attempt_titles.contains(&normalized_title)
            {
                continue;
            }
            seen_attempt_titles.insert(normalized_title);

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "event_seed", None, &serialized_seed)
                .await?;

            return Ok(SeedGeneration { seed, notice: notice.clone() });
        }

        Err("failed to generate valid structured event output from ollama".to_string())
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
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<DungeonStory>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

        let premise = premise
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let context = context.trim();
        let extra = extra_prompt
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());

        let reference_probe = format!("{} {}", premise.unwrap_or(""), context);
        let reference_context =
            build_reference_context(&config, reference_probe.trim(), workspace_root);

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
            None => "Invent a small, self-contained story that needs nothing outside this one place.".to_string(),
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
                format!("The space is shaped like {shape}; let that guide how the party moves deeper. ")
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

            return Ok(SeedGeneration { seed: story, notice });
        }

        Err("failed to generate a valid dungeon story from ollama".to_string())
    }

    /// Pass 2 of dungeon generation: structure the LOCKED story into the five beat
    /// cards. Extractive — the model maps the story it is given, applies the field
    /// leashes, and writes a one-line spine. The per-beat `content_type` is NOT
    /// requested; it is injected from the deterministic `plan` so the tag can never
    /// disagree with the content. `function` is assigned by position in `into_beats`.
    #[allow(clippy::too_many_arguments)]
    pub async fn structure_dungeon_story(
        &self,
        plan: &DungeonContentPlan,
        story: &DungeonStory,
        tone: &str,
        twist: &str,
        topology: &str,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<SeedGeneration<DungeonSeed>, String> {
        let (config, model) = load_generation_config(workspace_root)?;

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
                format!("Spatial layout: {shape}; let it inform how the beats connect (especially whether the Setback loops the party back toward the entrance). ")
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocationSeed {
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
    pub resources_assets: String,
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
    /// assigned later in `into_beats`, not here, so the skeleton stays ours.
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
    pub fn into_beats(&self) -> Vec<DungeonBeat> {
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
        "puzzle" => "a sealed way forward — a barred door or mechanism — that opens only once the party finds the right key or condition",
        "offshoot" => "an optional branching path: a side chamber, a hidden room, or a tempting dead end",
        "sidekick" => "a lone ally met here who joins the party and travels deeper with them through the rest of this place",
        "oddity" => "a strange and significant object that is the very reason this place exists",
        "ability_check" => "a feat of skill or nerve to get past — a climb, a leap, a steady hand, or a test of will",
        _ => "something noteworthy",
    }
}

/// Mechanical meaning of an anchor type, given to Pass 2 so the card's idea
/// actually delivers that type's function. Also reused by the single-beat reroll,
/// which holds the rolled type fixed and only regenerates the prose.
pub(crate) fn anchor_mechanic(content_type: &str) -> &'static str {
    match content_type {
        "combat" => "a fight; convey the enemy's tactics, behavior, and use of terrain, and NEVER name specific creatures (the GM picks them)",
        "cache" => "a stash of loot or rewards",
        "forge" => "a place to craft or repair magic items; the idea must involve that crafting or repair",
        "puzzle" => "a locked-door->key obstacle of one or more steps; never a riddle or logic puzzle",
        "offshoot" => "an optional side passage, hidden room, or dead end off the main path",
        "sidekick" => "a dungeon-only ally introduced here who joins the party and stays with them through the later beats, leaving only when the dungeon ends",
        "oddity" => "the world-significant object that is the reason this dungeon exists",
        "ability_check" => "an ability/skill check the party must pass — name the check (athletics, perception, persuasion, sleight of hand…) and what failure costs; not a riddle",
        _ => "a noteworthy room",
    }
}

fn overlay_phrase(overlay_type: &str) -> &'static str {
    match overlay_type {
        "foreshadowing" => "a hint of something still to come, here or out in the wider campaign",
        "history" => "a piece of lore about this place, its people, or its makers",
        "map" => "a glimpse of the surrounding world — a route, a landmark, or a link to somewhere else",
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
        "The V for Vendetta" => Some("two passages branching in opposite directions from the entrance"),
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
        "false_victory" => "in the middle, hand the party an apparent win that then curdles — they think they've succeeded, then lose it",
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
        if let Some(overlay) = &plan.overlay {
            if overlay.beat_index == i {
                out.push_str(&format!(
                    " Also layer in {}: {}.",
                    overlay.overlay_type,
                    overlay_phrase(&overlay.overlay_type)
                ));
            }
        }
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct PromptReferenceContext {
    pub system_context: String,
}


pub(crate) fn parse_recent_npc_seeds(payloads: Vec<String>) -> Vec<NpcSeed> {
    payloads.into_iter().filter_map(|payload| serde_json::from_str::<NpcSeed>(&payload).ok()).collect()
}

fn parse_recent_location_seeds(payloads: Vec<String>) -> Vec<LocationSeed> {
    payloads.into_iter().filter_map(|payload| serde_json::from_str::<LocationSeed>(&payload).ok()).collect()
}

fn parse_recent_faction_seeds(payloads: Vec<String>) -> Vec<FactionSeed> {
    payloads.into_iter().filter_map(|payload| serde_json::from_str::<FactionSeed>(&payload).ok()).collect()
}

fn parse_recent_event_seeds(payloads: Vec<String>) -> Vec<EventSeed> {
    payloads.into_iter().filter_map(|payload| serde_json::from_str::<EventSeed>(&payload).ok()).collect()
}

fn recent_event_title_set(seeds: &[EventSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.title.trim().to_ascii_lowercase()).filter(|title| !title.is_empty()).collect()
}

fn describe_recent_event_seeds(seeds: &[EventSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| seed.title.clone()).collect::<Vec<_>>().join("; ")
}

fn recent_faction_name_set(seeds: &[FactionSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.name.trim().to_ascii_lowercase()).filter(|name| !name.is_empty()).collect()
}

fn describe_recent_faction_seeds(seeds: &[FactionSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.reputation)).collect::<Vec<_>>().join("; ")
}

fn parse_recent_god_seeds(payloads: Vec<String>) -> Vec<GodSeed> {
    payloads.into_iter().filter_map(|payload| serde_json::from_str::<GodSeed>(&payload).ok()).collect()
}

fn recent_god_name_set(seeds: &[GodSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.name.trim().to_ascii_lowercase()).filter(|name| !name.is_empty()).collect()
}

fn describe_recent_god_seeds(seeds: &[GodSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| format!("{} | {} | {}", seed.name, seed.rank, seed.alignment)).collect::<Vec<_>>().join("; ")
}

fn describe_recent_location_seeds(seeds: &[LocationSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.danger_level)).collect::<Vec<_>>().join("; ")
}

fn recent_name_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.name.trim().to_ascii_lowercase()).filter(|name| !name.is_empty()).collect()
}

fn occupation_tokens(value: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &["a", "an", "and", "as", "at", "by", "deceased", "ex", "for", "former", "from", "in", "of", "on", "retired", "the", "to", "under", "with"];
    value.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' }).collect::<String>()
        .split_whitespace().map(|token| token.trim().to_ascii_lowercase()).filter(|token| !token.is_empty() && !STOP_WORDS.contains(&token.as_str())).collect()
}

pub(crate) fn occupation_anchor(value: &str) -> String {
    occupation_tokens(value).into_iter().next().unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn recent_occupation_anchor_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| occupation_anchor(&seed.occupation)).filter(|anchor| !anchor.is_empty() && anchor != "unknown").collect()
}

fn recent_location_name_set(seeds: &[LocationSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.name.trim().to_ascii_lowercase()).filter(|name| !name.is_empty()).collect()
}

fn describe_recent_npc_seeds(seeds: &[NpcSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| format!("{} | {} | {} | {}", seed.name, seed.race, seed.sex, seed.occupation)).collect::<Vec<_>>().join("; ")
}

pub(crate) fn describe_recent_npc_occupation_anchors(seeds: &[NpcSeed]) -> String {
    let mut anchors: Vec<String> = recent_occupation_anchor_set(seeds).into_iter().collect();
    if anchors.is_empty() { return "none".to_string(); }
    anchors.sort();
    anchors.truncate(12);
    anchors.join(", ")
}


fn build_prompt_reference_context(
    prompt: &str,
    entries: &[VaultReferenceEntry],
    vault: &Vault,
    workspace_root: &Path,
) -> PromptReferenceContext {
    let keys = extract_prompt_reference_keys(prompt, entries);
    if keys.is_empty() { return PromptReferenceContext::default(); }

    let path_by_key: std::collections::HashMap<String, String> = entries.iter().filter_map(|entry| entry.markdown_path.as_ref().map(|path| (entry.key.to_lowercase(), path.clone()))).collect();
    let mut blocks = Vec::new();

    let canonical_metadata = match EntityStore::new(workspace_root) {
        Ok(store) => canonical_metadata_map(&store),
        Err(err) => {
            eprintln!("reference context warning: failed to load canonical entities: {err}");
            HashMap::new()
        }
    };

    for key in keys.into_iter() {
        let Some(path) = path_by_key.get(&key.to_lowercase()) else { continue };
        let normalized_path = normalize_relative_path_for_storage(path);
        let metadata = if let Some(canonical) = canonical_metadata.get(&normalized_path) {
            canonical.clone()
        } else {
            let contents = match vault.read_relative(Path::new(path)) { Ok(value) => value, Err(err) => { eprintln!("reference context warning: failed reading {}: {}", path, err); continue; } };
            match reference_payload_from_markdown(&contents) { Some(value) => value, None => continue }
        };
        blocks.push(format!("@{key}\npath: {path}\n```toml\n{metadata}\n```"));
    }

    if blocks.is_empty() { return PromptReferenceContext::default(); }
    PromptReferenceContext { system_context: format!("Referenced vault metadata (treat as authoritative setting context):\n\n{}", blocks.join("\n\n")) }
}

fn canonical_metadata_map(store: &EntityStore) -> HashMap<String, String> {
    let mut map = HashMap::new();

    if let Ok(npcs) = store.list_npcs() {
        for npc in npcs {
            if let Ok(serialized) = toml::to_string_pretty(&npc) {
                map.insert(normalize_relative_path_for_storage(&npc.vault_path), serialized);
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
                map.insert(normalize_relative_path_for_storage(&item.vault_path), serialized);
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
    if let Some(rest) = body.strip_prefix("\r\n") { body = rest; } else if let Some(rest) = body.strip_prefix('\n') { body = rest; }
    let end = body.find("\n```").or_else(|| body.find("```"))?;
    let block = body[..end].trim();
    if block.is_empty() { None } else { Some(block.to_string()) }
}

fn reference_payload_from_markdown(contents: &str) -> Option<String> {
    if let Some(block) = extract_runebound_toml(contents) {
        return Some(block);
    }
    let trimmed = contents.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::{capacity_notice, describe_recent_npc_occupation_anchors, occupation_anchor, recent_occupation_anchor_set, reference_payload_from_markdown, NpcSeed, OUTPUT_RESERVE_TOKENS};

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
        assert_eq!(occupation_anchor("Cartographer & explorer (deceased)"), "cartographer");
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
        let markdown = "# Notes\n\n```runebound\ntype = \"npc\"\nname = \"Jimmy\"\n```\n\nExtra text";
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
