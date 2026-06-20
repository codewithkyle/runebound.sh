use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{OllamaChatClient, detail_directive, load_generation_config};
use crate::utils::{
    estimate_tokens, normalize_name, normalize_sex, normalize_unknown_list, normalize_unknown_text,
};
use std::collections::HashSet;

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
                seed.name = normalize_name(&seed.name);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
