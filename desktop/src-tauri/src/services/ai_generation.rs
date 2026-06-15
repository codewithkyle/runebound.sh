use crate::repositories::{Database, GenerationRepository};
use crate::utils::{
    normalize_faction_seed, normalize_item_category, normalize_item_rarity, normalize_location_seed,
    normalize_sex, normalize_unknown_list, normalize_unknown_text, validate_faction_details,
    validate_location_details,
};
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::vault::Vault;
use runebound_models::utils::{
    FACTION_KIND_TYPES, ITEM_CATEGORIES, ITEM_RARITIES, LOCATION_DANGER_LEVELS,
    LOCATION_KIND_TYPES,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

pub struct AiGenerationService;

impl AiGenerationService {
    pub async fn generate_npc_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<NpcSeed, String> {
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
        validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
        let config = loaded.effective;
        let model = config.ollama.model.clone().ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one D&D NPC for a fantasy campaign.");

        let reference_context = if let Some(vault_path) = config.vault.path.clone() {
            let vault = Vault::new(vault_path);
            if vault.ensure_root_exists().is_ok() {
                match load_vault_reference_entries(&vault) {
                    Ok(entries) => build_prompt_reference_context(user_prompt, &entries, &vault),
                    Err(err) => { eprintln!("reference context warning: {err}"); PromptReferenceContext::default() }
                }
            } else {
                PromptReferenceContext::default()
            }
        } else {
            PromptReferenceContext::default()
        };

        let recent_payloads = generation_repo
            .recent_prompts(database, "npc_seed", 20)
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
                "carrying": { "type": "array", "minItems": 1, "items": { "type": "string", "minLength": 1 } }
            },
            "additionalProperties": false
        });

        let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.ollama.timeout_seconds))
            .build().map_err(|err| err.to_string())?;

        let mut seen_attempt_names = HashSet::new();
        let mut seen_attempt_occupation_anchors = HashSet::new();

        for attempt in 0..5 {
            let base_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_micros() as i64).unwrap_or(0);
            let run_seed = (base_seed + i64::from(attempt)) as i32;
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names and occupations." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.1, "top_p": 0.92, "repeat_penalty": 1.15, "seed": run_seed },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate concise D&D NPC seeds for a game master. Each result must be novel and different from recent NPCs. Return only JSON with fields name, race, occupation, sex, age, height, weight_lbs, background, want_need, secret_obstacle, carrying. Background must be 1-3 coherent sentences. carrying must be an array of item strings. Age should be years, height should be imperial like 5'11\", weight_lbs should be lbs as text like 180. Prefer occupations different from recent occupations and avoid occupation roots in this list unless explicitly requested: {}. Avoid these recent seeds: {}.{}{}",
                        recent_occupation_context, recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) }
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let response = client.post(&url).json(&payload).send().await.map_err(|err| err.to_string())?;
            if !response.status().is_success() {
                return Err(format!("ollama chat failed with status {}", response.status()));
            }

            let value: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
            let Some(content) = value.get("message").and_then(|msg| msg.get("content")).and_then(|content| content.as_str()) else { continue };

            let parsed: Result<NpcSeed, _> = serde_json::from_str(content);
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

            return Ok(seed);
        }

        Err("failed to generate valid structured NPC output from ollama".to_string())
    }

    pub async fn generate_location_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<LocationSeed, String> {
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
        validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
        let config = loaded.effective;
        let model = config.ollama.model.clone().ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy location for a D&D campaign.");

        let recent_payloads = generation_repo
            .recent_prompts(database, "location_seed", 20)
            .await?;
        let recent_seeds = parse_recent_location_seeds(recent_payloads);
        let recent_names = recent_location_name_set(&recent_seeds);
        let recent_context = describe_recent_location_seeds(&recent_seeds);

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

        let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.ollama.timeout_seconds))
            .build().map_err(|err| err.to_string())?;

        let mut seen_attempt_names = HashSet::new();

        for attempt in 0..5 {
            let base_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_micros() as i64).unwrap_or(0);
            let run_seed = (base_seed + i64::from(attempt)) as i32;
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.08, "top_p": 0.93, "repeat_penalty": 1.14, "seed": run_seed },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate concise, usable D&D location seeds. Return only JSON with fields name, kind_type, kind_custom, visual_description, history_background, exports, tone, authority, danger_level, current_tension. visual_description must be 1-3 sentences. history_background must be 2-5 sentences. exports must have 1-3 short items. tone must be 2-5 words. current_tension must be 1-2 sentences. If kind_type is not other, kind_custom must be null. Avoid these recent seeds: {}.{}",
                        recent_context, repair_note
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let response = client.post(&url).json(&payload).send().await.map_err(|err| err.to_string())?;
            if !response.status().is_success() {
                return Err(format!("ollama chat failed with status {}", response.status()));
            }

            let value: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
            let Some(content) = value.get("message").and_then(|msg| msg.get("content")).and_then(|content| content.as_str()) else { continue };

            let parsed: Result<LocationSeed, _> = serde_json::from_str(content);
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

            return Ok(seed);
        }

        Err("failed to generate valid structured location output from ollama".to_string())
    }

    pub async fn generate_faction_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<FactionSeed, String> {
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
        validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
        let config = loaded.effective;
        let model = config.ollama.model.clone().ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

        let user_prompt = prompt.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
            .unwrap_or("Generate one distinct fantasy faction for a D&D campaign.");

        let reference_context = if let Some(vault_path) = config.vault.path.clone() {
            let vault = Vault::new(vault_path);
            if vault.ensure_root_exists().is_ok() {
                match load_vault_reference_entries(&vault) {
                    Ok(entries) => build_prompt_reference_context(user_prompt, &entries, &vault),
                    Err(err) => { eprintln!("reference context warning: {err}"); PromptReferenceContext::default() }
                }
            } else {
                PromptReferenceContext::default()
            }
        } else {
            PromptReferenceContext::default()
        };

        let recent_payloads = generation_repo
            .recent_prompts(database, "faction_seed", 20)
            .await?;
        let recent_seeds = parse_recent_faction_seeds(recent_payloads);
        let recent_names = recent_faction_name_set(&recent_seeds);
        let recent_context = describe_recent_faction_seeds(&recent_seeds);
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

        let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.ollama.timeout_seconds))
            .build().map_err(|err| err.to_string())?;

        let mut seen_attempt_names = HashSet::new();

        for attempt in 0..5 {
            let base_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_micros() as i64).unwrap_or(0);
            let run_seed = (base_seed + i64::from(attempt)) as i32;
            let repair_note = if attempt == 0 { "" } else { " Previous response was invalid or repeated. Return only valid JSON that matches the schema and avoid prior names." };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.08, "top_p": 0.93, "repeat_penalty": 1.12, "seed": run_seed },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate concise, usable D&D faction seeds. Return only JSON with fields name, kind_type, kind_custom, public_description, true_agenda, methods, leadership, headquarters, sphere_of_influence, resources_assets, allies, rivals_enemies, reputation, current_tension, goals_short_term, goals_long_term, symbol_description. public_description, true_agenda, and methods should be 1-3 sentences. current_tension should be 1-2 sentences. symbol_description should be exactly 1 sentence describing symbol/sigil/colors/banner/iconography. If kind_type is not other, kind_custom must be null. If referenced vault metadata includes an established name for an organization, group, guild, or house, reuse that exact canonical name instead of inventing a new one. Avoid these recent seeds: {}.{}{}",
                        recent_context, repair_note,
                        if reference_context.system_context.is_empty() { String::new() } else { format!("\n\n{}", reference_context.system_context) }
                    )
                }, { "role": "user", "content": user_prompt }]
            });

            let response = client.post(&url).json(&payload).send().await.map_err(|err| err.to_string())?;
            if !response.status().is_success() {
                return Err(format!("ollama chat failed with status {}", response.status()));
            }

            let value: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
            let Some(content) = value.get("message").and_then(|msg| msg.get("content")).and_then(|content| content.as_str()) else { continue };

            let parsed: Result<FactionSeed, _> = serde_json::from_str(content);
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

            return Ok(seed);
        }

        Err("failed to generate valid structured faction output from ollama".to_string())
    }

    pub async fn generate_item_seed(
        &self,
        prompt: Option<String>,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<ItemSeed, String> {
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
        validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
        let config = loaded.effective;
        let model = config
            .ollama
            .model
            .clone()
            .ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;

        let user_prompt = prompt
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("Generate one magical or legendary item.");

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
                "value_gp",
                "current_owner",
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
                "value_gp": { "type": "string", "minLength": 1 },
                "current_owner": { "type": "string", "minLength": 1 },
                "location": { "type": "string", "minLength": 1 }
            },
            "additionalProperties": false
        });

        let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.ollama.timeout_seconds))
            .build()
            .map_err(|err| err.to_string())?;

        for attempt in 0..5 {
            let base_seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_micros() as i64)
                .unwrap_or(0);
            let run_seed = (base_seed + i64::from(attempt)) as i32;
            let repair_note = if attempt == 0 {
                ""
            } else {
                " Previous response was invalid or repeated. Return only valid JSON."
            };

            let payload = serde_json::json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 1.05, "top_p": 0.92, "repeat_penalty": 1.1, "seed": run_seed },
                "messages": [{
                    "role": "system",
                    "content": format!(
                        "You generate concise tabletop RPG items. Category choices: {}. Rarity choices: {}. Provide appearance (1-2 sentences), abilities (1-3 sentences), drawbacks (0-2 sentences, or 'None'), history (1-3 sentences), value text, current owner, and location.{}",
                        ITEM_CATEGORIES.join(", "),
                        ITEM_RARITIES.join(", "),
                        repair_note
                    )
                }, { "role": "user", "content": user_prompt }]
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

            let parsed: Result<ItemSeed, _> = serde_json::from_str(content);
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
            seed.value_gp = normalize_unknown_text(&seed.value_gp);
            seed.current_owner = normalize_unknown_text(&seed.current_owner);
            seed.location = normalize_unknown_text(&seed.location);

            if seed.name.is_empty() {
                continue;
            }

            let serialized_seed = serde_json::to_string(&seed).map_err(|err| err.to_string())?;
            generation_repo
                .insert(database, "item_seed", None, &serialized_seed)
                .await?;

            return Ok(seed);
        }

        Err("failed to generate valid structured item output from ollama".to_string())
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
    pub value_gp: String,
    pub current_owner: String,
    pub location: String,
}

#[derive(Debug, Clone)]
pub struct VaultReferenceEntry {
    pub key: String,
    pub key_lower: String,
    pub markdown_path: Option<String>,
    pub is_dir: bool,
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

fn recent_faction_name_set(seeds: &[FactionSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.name.trim().to_ascii_lowercase()).filter(|name| !name.is_empty()).collect()
}

fn describe_recent_faction_seeds(seeds: &[FactionSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| format!("{} | {} | {}", seed.name, seed.kind_type, seed.reputation)).collect::<Vec<_>>().join("; ")
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


fn is_reference_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"')
}

fn can_start_reference_at(input: &str, at_index: usize) -> bool {
    if at_index == 0 { return true; }
    let before = input[..at_index].chars().next_back();
    before.is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\''))
}

fn should_ignore_reference_component(component: &str) -> bool {
    component.split('/').any(|part| part.starts_with('.') || part.eq_ignore_ascii_case("target"))
}

fn markdown_reference_key(relative_path: &str) -> Option<String> {
    let normalized = relative_path.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    let ext = path.extension().and_then(|value| value.to_str())?;
    if !ext.eq_ignore_ascii_case("md") { return None; }
    let stem = path.file_stem().and_then(|value| value.to_str()).map(str::trim).filter(|value| !value.is_empty())?;
    let parent = path.parent().and_then(|value| value.to_str()).unwrap_or("");
    if parent.is_empty() { Some(stem.to_string()) } else { Some(format!("{parent}/{stem}")) }
}

fn load_vault_reference_entries(vault: &Vault) -> Result<Vec<VaultReferenceEntry>, String> {
    use std::collections::HashMap;
    use std::fs;
    vault.ensure_root_exists().map_err(|err| err.to_string())?;
    let mut entries: HashMap<String, VaultReferenceEntry> = HashMap::new();
    let mut stack = vec![PathBuf::new()];

    while let Some(relative_dir) = stack.pop() {
        let full_dir = vault.resolve_relative(&relative_dir).map_err(|err| err.to_string())?;
        let dir_entries = fs::read_dir(&full_dir).map_err(|err| format!("failed to read directory {}: {}", full_dir.display(), err))?;

        for dir_entry in dir_entries {
            let dir_entry = match dir_entry { Ok(value) => value, Err(err) => { eprintln!("reference index warning: failed to read directory entry: {err}"); continue; } };
            let entry_path = dir_entry.path();
            let relative = match entry_path.strip_prefix(vault.root()) { Ok(value) => value.to_string_lossy().to_string().replace('\\', "/"), Err(_) => continue };
            if should_ignore_reference_component(&relative) { continue; }

            if entry_path.is_dir() {
                let mut key = relative.trim_matches('/').to_string();
                if key.is_empty() { continue; }
                key.push('/');
                entries.entry(key.clone()).or_insert_with(|| VaultReferenceEntry { key: key.clone(), key_lower: key.to_lowercase(), markdown_path: None, is_dir: true });
                stack.push(PathBuf::from(relative));
                continue;
            }

            let Some(key) = markdown_reference_key(&relative) else { continue };
            entries.entry(key.clone()).or_insert_with(|| VaultReferenceEntry { key: key.clone(), key_lower: key.to_lowercase(), markdown_path: Some(relative), is_dir: false });
        }
    }

    let mut out: Vec<VaultReferenceEntry> = entries.into_values().collect();
    out.sort_by(|left, right| left.key_lower.cmp(&right.key_lower));
    Ok(out)
}

fn extract_prompt_reference_keys(prompt: &str, entries: &[VaultReferenceEntry]) -> Vec<String> {
    let mut candidates: Vec<&VaultReferenceEntry> = entries.iter().filter(|entry| !entry.is_dir && entry.markdown_path.is_some()).collect();
    candidates.sort_by(|left, right| right.key_lower.len().cmp(&left.key_lower.len()));

    let prompt_lower = prompt.to_lowercase();
    let mut cursor = 0;
    let mut matched = Vec::new();

    while cursor < prompt.len() {
        let next_at = match prompt[cursor..].find('@') { Some(offset) => cursor + offset, None => break };
        if !can_start_reference_at(prompt, next_at) { cursor = next_at + 1; continue; }

        let tail_start = next_at + 1;
        let tail = &prompt_lower[tail_start..];
        let mut best: Option<&VaultReferenceEntry> = None;

        for candidate in &candidates {
            if !tail.starts_with(&candidate.key_lower) { continue; }
            let boundary_index = tail_start + candidate.key.len();
            let boundary_ok = prompt[boundary_index..].chars().next().is_none_or(is_reference_boundary_char);
            if !boundary_ok { continue; }
            best = Some(*candidate);
            break;
        }

        if let Some(candidate) = best { matched.push(candidate.key.clone()); cursor = tail_start + candidate.key.len(); continue; }
        cursor = next_at + 1;
    }

    let mut unique = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for key in matched { let lowered = key.to_lowercase(); if seen.insert(lowered) { unique.push(key); } }
    unique
}

fn build_prompt_reference_context(prompt: &str, entries: &[VaultReferenceEntry], vault: &Vault) -> PromptReferenceContext {
    const MAX_REFERENCE_DOCS: usize = 5;
    const MAX_METADATA_CHARS_PER_DOC: usize = 1800;

    let keys = extract_prompt_reference_keys(prompt, entries);
    if keys.is_empty() { return PromptReferenceContext::default(); }

    let path_by_key: std::collections::HashMap<String, String> = entries.iter().filter_map(|entry| entry.markdown_path.as_ref().map(|path| (entry.key.to_lowercase(), path.clone()))).collect();
    let mut blocks = Vec::new();

    for key in keys.into_iter().take(MAX_REFERENCE_DOCS) {
        let Some(path) = path_by_key.get(&key.to_lowercase()) else { continue };
        let contents = match vault.read_relative(std::path::Path::new(path)) { Ok(value) => value, Err(err) => { eprintln!("reference context warning: failed reading {}: {}", path, err); continue; } };
        let Some(runebound) = extract_runebound_toml(&contents) else { continue };
        let metadata = if runebound.len() > MAX_METADATA_CHARS_PER_DOC { format!("{}...", &runebound[..MAX_METADATA_CHARS_PER_DOC]) } else { runebound };
        blocks.push(format!("@{key}\npath: {path}\n```toml\n{metadata}\n```"));
    }

    if blocks.is_empty() { return PromptReferenceContext::default(); }
    PromptReferenceContext { system_context: format!("Referenced vault metadata (treat as authoritative setting context):\n\n{}", blocks.join("\n\n")) }
}

fn extract_runebound_toml(contents: &str) -> Option<String> {
    let start = contents.find("```runebound")?;
    let mut body = &contents[start + "```runebound".len()..];
    if let Some(rest) = body.strip_prefix("\r\n") { body = rest; } else if let Some(rest) = body.strip_prefix('\n') { body = rest; }
    let end = body.find("\n```").or_else(|| body.find("```"))?;
    let block = body[..end].trim();
    if block.is_empty() { None } else { Some(block.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::{describe_recent_npc_occupation_anchors, occupation_anchor, recent_occupation_anchor_set, NpcSeed};

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
}
