use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{OllamaChatClient, detail_directive, load_generation_config};
use crate::utils::{
    estimate_tokens, normalize_item_category, normalize_item_rarity, normalize_name,
    normalize_unknown_list, normalize_unknown_text,
};
use runebound_models::utils::{ITEM_CATEGORIES, ITEM_RARITIES};

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

impl AiGenerationService {
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
                seed.name = normalize_name(&seed.name);
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
}
