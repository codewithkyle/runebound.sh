use super::AiGenerationService;
use super::engine::*;
use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{OllamaChatClient, detail_directive, load_generation_config};
use crate::utils::{estimate_tokens, normalize_name};
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventSeed {
    pub title: String,
    pub body: String,
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

impl AiGenerationService {
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
                seed.title = normalize_name(&seed.title);
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
}
