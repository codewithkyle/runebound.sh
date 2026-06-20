use super::reference::*;

use crate::repositories::{Database, GenerationRepository};
use crate::services::ollama_chat::{ChatClient, attempt_seed};

/// Tokens reserved within the context window for the model's own output, so the
/// capacity warning fires before the prompt crowds out room to respond.
const OUTPUT_RESERVE_TOKENS: usize = 512;

/// Flat allowance for each generator's fixed system instructions / schema framing.
pub(super) const SYSTEM_BOILERPLATE_TOKENS: usize = 160;

/// `Option<String>` → trimmed `Option<&str>`, dropping blank/whitespace-only values.
/// Shared by the faction and location wizard prompt builders.
pub(super) fn opt_clause(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// A generated seed plus an optional non-blocking notice (e.g. the assembled
/// prompt is near the configured context window and output may drift).
#[derive(Debug, Clone)]
pub struct SeedGeneration<T> {
    pub seed: T,
    pub notice: Option<String>,
}

/// Returns a user-facing warning when the estimated prompt is close enough to the
/// configured `num_ctx` that there is little room left for the response.
pub(super) fn capacity_notice(estimated_tokens: usize, num_ctx: u32) -> Option<String> {
    let budget = (num_ctx as usize).saturating_sub(OUTPUT_RESERVE_TOKENS);
    if estimated_tokens > budget {
        Some(format!(
            "⚠️ This prompt is ~{estimated_tokens} tokens, near your model's configured context window (ollama.num_ctx = {num_ctx}). Large referenced documents may cause the model to lose detail or drift. Consider referencing fewer or smaller documents, or raising ollama.num_ctx in your config."
        ))
    } else {
        None
    }
}

/// LLM sampling knobs for a seed-generation request. Hoisted from the per-kind
/// literals each generator inlined in its payload; the values differ by kind on
/// purpose (NPCs run hottest, items coolest).
pub(super) struct SeedSampling {
    temperature: f64,
    top_p: f64,
    repeat_penalty: f64,
}

pub(super) const NPC_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.1,
    top_p: 0.92,
    repeat_penalty: 1.15,
};

pub(super) const LOCATION_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.08,
    top_p: 0.93,
    repeat_penalty: 1.14,
};

pub(super) const FACTION_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.08,
    top_p: 0.93,
    repeat_penalty: 1.12,
};

pub(super) const GOD_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.08,
    top_p: 0.93,
    repeat_penalty: 1.12,
};

pub(super) const ITEM_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.05,
    top_p: 0.92,
    repeat_penalty: 1.1,
};

pub(super) const EVENT_GEN_SAMPLING: SeedSampling = SeedSampling {
    temperature: 1.05,
    top_p: 0.93,
    repeat_penalty: 1.12,
};

/// The `\n\n`-prefixed reference block appended to a generator's system prompt, or
/// empty when no `@references` resolved — the formatting every generator repeated
/// inline.
pub(super) fn reference_system_suffix(reference_context: &PromptReferenceContext) -> String {
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
pub(super) enum SeedStep<T> {
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
pub(super) async fn run_seed_attempts<T: serde::Serialize + serde::de::DeserializeOwned>(
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

/// Parse the recent-seed payloads (raw JSON strings from the generation log) into
/// typed seeds for dedup/context, dropping any that no longer deserialize. One
/// generic replaces the former per-kind `parse_recent_*_seeds` fan-out.
pub(crate) fn parse_recent_seeds<T: serde::de::DeserializeOwned>(payloads: Vec<String>) -> Vec<T> {
    payloads
        .into_iter()
        .filter_map(|payload| serde_json::from_str::<T>(&payload).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
