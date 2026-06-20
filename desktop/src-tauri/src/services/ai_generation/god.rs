use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{OllamaChatClient, detail_directive, load_generation_config};
use crate::utils::{estimate_tokens, normalize_god_seed, validate_god_details};
use runebound_models::utils::{GOD_ALIGNMENTS, GOD_RANKS};
use std::collections::HashSet;

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

impl AiGenerationService {
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
}
