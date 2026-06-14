use crate::repositories::{Database, GenerationRepository};
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::npc::{slugify, UNKNOWN_LOCATION};
use dnd_core::vault::Vault;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

const LOCATION_KIND_TYPES: [&str; 10] = [
    "hamlet", "town", "city", "dungeon", "hideout", "ruin", "guildhall", "landmark", "wilderness", "other",
];

const LOCATION_DANGER_LEVELS: [&str; 5] = ["Unknown", "safe", "guarded", "risky", "deadly"];

const FACTION_KIND_TYPES: [&str; 10] = [
    "guild", "cult", "military_order", "noble_house", "criminal_syndicate", "mercantile_league",
    "religious_order", "arcane_circle", "revolutionary_cell", "other",
];

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

    pub async fn reroll_npc_field(
        &self,
        input: RerollNpcFieldInput,
        workspace_root: &PathBuf,
        database: &Database,
        generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollNpcFieldResult, String> {
        let field = canonical_npc_reroll_field(&input.field)?;
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
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

    pub async fn reroll_location_field(
        &self,
        input: RerollLocationFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollLocationFieldResult, String> {
        let field = canonical_location_reroll_field(&input.field)?;
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
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

    pub async fn reroll_faction_field(
        &self,
        input: RerollFactionFieldInput,
        workspace_root: &PathBuf,
        _database: &Database,
        _generation_repo: &dyn GenerationRepository,
    ) -> Result<RerollFactionFieldResult, String> {
        let field = canonical_faction_reroll_field(&input.field)?;
        let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
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

#[derive(Debug, Clone)]
pub struct VaultReferenceEntry {
    pub key: String,
    pub key_lower: String,
    pub markdown_path: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveReferenceQuery {
    pub at_index: usize,
    pub query: String,
}

#[derive(Debug, Clone, Default)]
pub struct PromptReferenceContext {
    pub system_context: String,
}

fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" { Ok(normalized) } else { Err("sex must be one of: male, female".to_string()) }
}

fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() { "Unknown".to_string() } else { trimmed.to_string() }
}

fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values.into_iter().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect();
    if cleaned.is_empty() { vec!["Unknown".to_string()] } else { cleaned }
}

fn parse_carrying_csv(value: &str) -> Vec<String> {
    let items: Vec<String> = value.split(',').map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect();
    normalize_unknown_list(items)
}

fn normalize_location_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if LOCATION_KIND_TYPES.contains(&normalized.as_str()) { Ok(normalized) } else { Err(format!("kind_type must be one of: {}", LOCATION_KIND_TYPES.join(", "))) }
}

fn normalize_location_danger_level(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let normalized = if trimmed.eq_ignore_ascii_case("unknown") { "Unknown".to_string() } else { trimmed.to_ascii_lowercase() };
    if LOCATION_DANGER_LEVELS.contains(&normalized.as_str()) { Ok(normalized) } else { Err(format!("danger_level must be one of: {}", LOCATION_DANGER_LEVELS.join(", "))) }
}

fn parse_list_csv(value: &str) -> Vec<String> {
    value.split(',').map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect()
}

fn normalize_exports(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values.into_iter().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect();
    if cleaned.is_empty() { vec!["Unknown".to_string()] } else { cleaned }
}

fn normalize_faction_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if FACTION_KIND_TYPES.contains(&normalized.as_str()) { Ok(normalized) } else { Err(format!("kind_type must be one of: {}", FACTION_KIND_TYPES.join(", "))) }
}

fn sentence_count(value: &str) -> usize {
    value.split_terminator(['.', '!', '?']).filter(|part| !part.trim().is_empty()).count()
}

fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

fn validate_sentence_range(value: &str, min: usize, max: usize, field: &str) -> Result<(), String> {
    let count = sentence_count(value);
    if count < min || count > max { return Err(format!("{field} must be {min}-{max} sentences; got {count}")); }
    Ok(())
}

fn normalize_location_seed(mut seed: LocationSeed) -> Result<LocationSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_location_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed.kind_custom.as_ref().is_none_or(|value| value.trim().is_empty()) { return Err("kind_custom is required when kind_type is other".to_string()); }
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
    if seed.name.trim().is_empty() { return Err("location name cannot be empty".to_string()); }
    if seed.visual_description != "Unknown" { validate_sentence_range(&seed.visual_description, 1, 3, "visual_description")?; }
    if seed.history_background != "Unknown" { validate_sentence_range(&seed.history_background, 2, 5, "history_background")?; }
    if seed.current_tension != "Unknown" { validate_sentence_range(&seed.current_tension, 1, 2, "current_tension")?; }
    if seed.exports.is_empty() || seed.exports.len() > 3 { return Err("exports must have 1-3 items".to_string()); }
    if !(seed.exports.len() == 1 && seed.exports[0] == "Unknown") {
        let empty_item = seed.exports.iter().any(|item| item.trim().is_empty());
        if empty_item { return Err("exports cannot contain empty items".to_string()); }
    }
    if seed.tone != "Unknown" {
        let tone_words = word_count(&seed.tone);
        if !(2..=5).contains(&tone_words) { return Err(format!("tone must be 2-5 words; got {tone_words}")); }
    }
    Ok(())
}

fn normalize_faction_seed(mut seed: FactionSeed) -> Result<FactionSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_faction_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed.kind_custom.as_ref().is_none_or(|value| value.trim().is_empty()) { return Err("kind_custom is required when kind_type is other".to_string()); }
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
    if seed.name.trim().is_empty() { return Err("faction name cannot be empty".to_string()); }
    if seed.public_description != "Unknown" { validate_sentence_range(&seed.public_description, 1, 3, "public_description")?; }
    if seed.true_agenda != "Unknown" { validate_sentence_range(&seed.true_agenda, 1, 3, "true_agenda")?; }
    if seed.current_tension != "Unknown" { validate_sentence_range(&seed.current_tension, 1, 2, "current_tension")?; }
    if seed.symbol_description != "Unknown" { validate_sentence_range(&seed.symbol_description, 1, 1, "symbol_description")?; }
    Ok(())
}

fn parse_recent_npc_seeds(payloads: Vec<String>) -> Vec<NpcSeed> {
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

fn occupation_anchor(value: &str) -> String {
    occupation_tokens(value).into_iter().next().unwrap_or_else(|| "unknown".to_string())
}

fn recent_occupation_anchor_set(seeds: &[NpcSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| occupation_anchor(&seed.occupation)).filter(|anchor| !anchor.is_empty() && anchor != "unknown").collect()
}

fn recent_location_name_set(seeds: &[LocationSeed]) -> std::collections::HashSet<String> {
    seeds.iter().map(|seed| seed.name.trim().to_ascii_lowercase()).filter(|name| !name.is_empty()).collect()
}

fn describe_recent_npc_seeds(seeds: &[NpcSeed]) -> String {
    if seeds.is_empty() { return "none".to_string(); }
    seeds.iter().take(10).map(|seed| format!("{} | {} | {} | {}", seed.name, seed.race, seed.sex, seed.occupation)).collect::<Vec<_>>().join("; ")
}

fn describe_recent_npc_occupation_anchors(seeds: &[NpcSeed]) -> String {
    let mut anchors: Vec<String> = recent_occupation_anchor_set(seeds).into_iter().collect();
    if anchors.is_empty() { return "none".to_string(); }
    anchors.sort();
    anchors.truncate(12);
    anchors.join(", ")
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
    if at_index == 0 { return true; }
    let before = input[..at_index].chars().next_back();
    before.is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\''))
}

fn extract_active_reference_query(input: &str) -> Option<ActiveReferenceQuery> {
    for (idx, ch) in input.char_indices().rev() {
        if ch != '@' { continue; }
        if !can_start_reference_at(input, idx) { continue; }
        return Some(ActiveReferenceQuery { at_index: idx, query: input[idx + 1..].to_string() });
    }
    None
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

fn is_top_level_reference_key(key: &str, is_dir: bool) -> bool {
    if is_dir { let trimmed = key.trim_end_matches('/'); !trimmed.is_empty() && !trimmed.contains('/') } else { !key.contains('/') }
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

fn build_reference_suggestions_from_entries(input: &str, active: &ActiveReferenceQuery, entries: &[VaultReferenceEntry]) -> Vec<CommandSuggestion> {
    let query_lower = active.query.replace('\\', "/").to_lowercase();
    let mut ranked: Vec<&VaultReferenceEntry> = entries.iter().filter(|entry| {
        if query_lower.is_empty() { return is_top_level_reference_key(&entry.key, entry.is_dir); }
        entry.key_lower.starts_with(&query_lower)
    }).collect();

    ranked.sort_by(|left, right| left.key_lower.cmp(&right.key_lower));
    ranked.into_iter().take(12).map(|entry| {
        let completion_suffix = if entry.is_dir { "" } else { " " };
        CommandSuggestion {
            label: format!("@{}", entry.key),
            completion: format!("{}@{}{}", &input[..active.at_index], entry.key, completion_suffix),
            helper_text: Some(SuggestionHelperText::Reference),
        }
    }).collect()
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandSuggestion {
    pub label: String,
    pub completion: String,
    pub helper_text: Option<SuggestionHelperText>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionHelperText {
    Command,
    Npc,
    Location,
    Faction,
    Reference,
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
