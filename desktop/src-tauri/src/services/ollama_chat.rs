//! Shared Ollama `/api/chat` plumbing for the generation and reroll services.
//!
//! Seed generation (`ai_generation`) and field reroll (`entity_reroll`) both drive
//! the same retry-loop shape: load+validate config, build a timed client, derive a
//! per-attempt RNG seed, POST a structured-output payload, and pull the assistant
//! message content back out. Only the schema, prompt, and post-processing differ
//! per entity kind/field, so that plumbing lives here once.

use std::path::Path;
use std::time::Duration;

use dnd_core::config::{AppConfig, Verbosity, load_effective, validate_for_runtime};

/// The detail-level directive appended to every generation and reroll system
/// prompt. It deliberately overrides the "concise" framing and the per-field
/// sentence counts baked into those prompts, so a single config switch
/// (`generation.verbosity`) controls how much prose the model writes for
/// narrative/descriptive fields. Returned with a leading space so it appends
/// cleanly to an existing prompt string.
pub(crate) fn detail_directive(verbosity: Verbosity) -> &'static str {
    match verbosity {
        Verbosity::Brief => {
            " DETAIL LEVEL: brief. Keep every narrative or descriptive field to 1-2 tight, \
             high-signal sentences."
        }
        Verbosity::Medium => {
            " DETAIL LEVEL: medium. Write each narrative or descriptive field as 3-4 substantive \
             sentences that give a game master at least one concrete, usable hook. Prefer this \
             over any shorter per-field sentence counts mentioned elsewhere in these instructions."
        }
        Verbosity::Verbose => {
            " DETAIL LEVEL: verbose. Despite any instruction to be 'concise' or any shorter \
             per-field sentence counts mentioned elsewhere, write each narrative or descriptive \
             field as 5-7 vivid, specific sentences a game master can use directly. Add concrete \
             names, details, and hooks rather than filler."
        }
    }
}

/// Load + validate the runtime config and extract the configured model. This is the
/// common preamble for every generation/reroll call; errors mirror the prior inline
/// messages so user-facing output is unchanged.
pub(crate) fn load_generation_config(workspace_root: &Path) -> Result<(AppConfig, String), String> {
    let loaded = load_effective(workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let config = loaded.effective;
    let model = config
        .ollama
        .model
        .clone()
        .ok_or_else(|| "ollama.model is not configured; run start setup".to_string())?;
    Ok((config, model))
}

/// Build the chat endpoint URL and an HTTP client honoring the configured timeout.
pub(crate) fn build_chat_client(config: &AppConfig) -> Result<(reqwest::Client, String), String> {
    let url = format!("{}/api/chat", config.ollama.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.ollama.timeout_seconds))
        .build()
        .map_err(|err| err.to_string())?;
    Ok((client, url))
}

/// Per-attempt RNG seed derived from wall-clock micros plus the attempt index, so
/// retries within a single call diverge from one another.
pub(crate) fn attempt_seed(attempt: i32) -> i32 {
    let base_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros() as i64)
        .unwrap_or(0);
    (base_seed + i64::from(attempt)) as i32
}

/// POST a chat payload and return the assistant message content.
///
/// `Err` for transport, non-success status, or unreadable JSON (these abort the
/// whole call); `Ok(None)` when the response carried no usable content, which the
/// caller treats as a retryable attempt.
pub(crate) async fn post_chat_for_content(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
) -> Result<Option<String>, String> {
    let response = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "ollama chat failed with status {}",
            response.status()
        ));
    }

    let value: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
    Ok(value
        .get("message")
        .and_then(|msg| msg.get("content"))
        .and_then(|content| content.as_str())
        .map(|content| content.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_directive_differs_per_level_and_names_it() {
        let brief = detail_directive(Verbosity::Brief);
        let medium = detail_directive(Verbosity::Medium);
        let verbose = detail_directive(Verbosity::Verbose);

        assert!(brief.contains("brief"));
        assert!(medium.contains("medium"));
        assert!(verbose.contains("verbose"));

        // The three levels must produce distinct guidance.
        assert_ne!(brief, medium);
        assert_ne!(medium, verbose);
        assert_ne!(brief, verbose);

        // Appended to a prompt, so each starts with a separating space.
        for directive in [brief, medium, verbose] {
            assert!(directive.starts_with(' '));
        }
    }
}
