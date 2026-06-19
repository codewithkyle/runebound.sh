//! Shared Ollama `/api/chat` plumbing for the generation and reroll services.
//!
//! Seed generation (`ai_generation`) and field reroll (`entity_reroll`) both drive
//! the same retry-loop shape: load+validate config, build a timed client, derive a
//! per-attempt RNG seed, POST a structured-output payload, and pull the assistant
//! message content back out. Only the schema, prompt, and post-processing differ
//! per entity kind/field, so that plumbing lives here once.

use std::time::Duration;

use async_trait::async_trait;
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
pub(crate) fn load_generation_config() -> Result<(AppConfig, String), String> {
    let loaded = load_effective().map_err(|err| err.to_string())?;
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
/// retries within a single call diverge from one another. The `as i32` truncation
/// is intentional: only the low bits need to differ between attempts of one call,
/// and Ollama accepts any 32-bit seed — the wider clock value would just overflow
/// the field, so we keep the cheap wrap rather than hashing.
pub(crate) fn attempt_seed(attempt: i32) -> i32 {
    let base_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros() as i64)
        .unwrap_or(0);
    (base_seed + i64::from(attempt)) as i32
}

/// POST a chat payload and return the assistant message content.
///
/// `Err` for transport, non-success status, unreadable JSON, a top-level error
/// body, or a truncated response (these abort the whole call); `Ok(None)` when the
/// response carried no usable content, which the caller treats as a retryable
/// attempt. The body interpretation lives in [`interpret_chat_response`].
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
    interpret_chat_response(&value)
}

/// Decide what a successful (`2xx`) Ollama `/api/chat` JSON body means. Split out
/// from [`post_chat_for_content`] so the (transport-free) decision logic is
/// unit-testable.
///
/// Two failure modes hide behind a 200 and must not be swallowed into the retry
/// loop's generic "failed to generate" outcome:
///
/// - a top-level `{"error": …}` body (e.g. model not found / out of memory) —
///   surfaced verbatim, since no retry fixes it; and
/// - `done_reason == "length"`, which means the model hit its context/output limit
///   and the content was cut off mid-token. Any JSON it carries is truncated and
///   would fail to parse, and this recurs at the same prompt size — so we report it
///   as an actionable capacity error rather than retrying and then blaming a parse
///   miss.
fn interpret_chat_response(value: &serde_json::Value) -> Result<Option<String>, String> {
    if let Some(error) = value.get("error").and_then(|err| err.as_str()) {
        return Err(format!("ollama chat returned an error: {error}"));
    }

    if value.get("done_reason").and_then(|reason| reason.as_str()) == Some("length") {
        return Err(
            "ollama truncated its response at the context/output limit (done_reason = \
             \"length\"); raise ollama.num_ctx or lower generation.verbosity / reference \
             fewer documents, then try again."
                .to_string(),
        );
    }

    Ok(value
        .get("message")
        .and_then(|msg| msg.get("content"))
        .and_then(|content| content.as_str())
        .map(|content| content.to_string()))
}

/// The single network seam every generation/reroll attempt POSTs through. Taking
/// the loops' dependency on Ollama as a trait (rather than a bare `reqwest::Client`
/// + URL) lets the deterministic part of generation/reroll — payload construction
/// and the dedup/retry control flow — be unit-tested with a mock, without a live
/// model. The production driver still goes over HTTP via [`OllamaChatClient`].
#[async_trait]
pub(crate) trait ChatClient: Send + Sync {
    /// POST one chat payload, returning the assistant message content. Semantics
    /// mirror [`post_chat_for_content`]: `Err` aborts the whole call, `Ok(None)` is
    /// a retryable empty attempt, `Ok(Some(content))` is the reply body.
    async fn post_chat(&self, payload: &serde_json::Value) -> Result<Option<String>, String>;
}

/// The production [`ChatClient`]: a timed `reqwest` client bound to the configured
/// `/api/chat` URL. Built from config so callers hold one object instead of the
/// `(client, url)` pair the loops used to thread separately.
pub(crate) struct OllamaChatClient {
    client: reqwest::Client,
    url: String,
}

impl OllamaChatClient {
    /// Build the client + endpoint from runtime config (same wiring as the former
    /// inline [`build_chat_client`] call in every loop).
    pub(crate) fn from_config(config: &AppConfig) -> Result<Self, String> {
        let (client, url) = build_chat_client(config)?;
        Ok(Self { client, url })
    }
}

#[async_trait]
impl ChatClient for OllamaChatClient {
    async fn post_chat(&self, payload: &serde_json::Value) -> Result<Option<String>, String> {
        post_chat_for_content(&self.client, &self.url, payload).await
    }
}

/// A [`ChatClient`] for tests: records every payload it is asked to POST and
/// replays a queue of canned outcomes. Lets characterization tests assert the exact
/// request built for an (entity[, field]) and exercise the dedup/retry loop with
/// scripted collisions, all without a live model.
#[cfg(test)]
pub(crate) struct MockChatClient {
    responses: std::sync::Mutex<std::collections::VecDeque<Result<Option<String>, String>>>,
    captured: std::sync::Mutex<Vec<serde_json::Value>>,
}

#[cfg(test)]
impl MockChatClient {
    /// A mock returning each outcome in order; once exhausted it yields `Ok(None)`
    /// (a retryable empty attempt).
    pub(crate) fn new(responses: Vec<Result<Option<String>, String>>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses.into()),
            captured: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Convenience: a mock that returns each content string as `Ok(Some(..))`.
    pub(crate) fn with_contents(contents: &[&str]) -> Self {
        Self::new(
            contents
                .iter()
                .map(|content| Ok(Some((*content).to_string())))
                .collect(),
        )
    }

    /// Every payload POSTed so far, in order.
    pub(crate) fn captured(&self) -> Vec<serde_json::Value> {
        self.captured.lock().expect("mock captured lock").clone()
    }
}

#[cfg(test)]
#[async_trait]
impl ChatClient for MockChatClient {
    async fn post_chat(&self, payload: &serde_json::Value) -> Result<Option<String>, String> {
        self.captured
            .lock()
            .expect("mock captured lock")
            .push(payload.clone());
        self.responses
            .lock()
            .expect("mock responses lock")
            .pop_front()
            .unwrap_or(Ok(None))
    }
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

    #[test]
    fn interpret_returns_content_on_normal_completion() {
        let body = serde_json::json!({
            "done_reason": "stop",
            "message": { "role": "assistant", "content": "{\"name\":\"Lirael\"}" }
        });
        assert_eq!(
            interpret_chat_response(&body).unwrap().as_deref(),
            Some("{\"name\":\"Lirael\"}")
        );
    }

    #[test]
    fn interpret_returns_none_when_content_missing() {
        // A 200 with no assistant content is a retryable empty attempt, not an abort.
        let body = serde_json::json!({ "done_reason": "stop", "message": { "role": "assistant" } });
        assert_eq!(interpret_chat_response(&body).unwrap(), None);
    }

    #[test]
    fn interpret_surfaces_top_level_error_body() {
        // Ollama can answer 200 with an error body; it must abort with that message.
        let body = serde_json::json!({ "error": "model 'llama9' not found" });
        let err = interpret_chat_response(&body).unwrap_err();
        assert!(err.contains("model 'llama9' not found"), "got: {err}");
    }

    #[tokio::test]
    async fn mock_chat_client_captures_payloads_and_replays_in_order() {
        // The mock is the test seam used by the reroll/seed characterization tests:
        // it records every payload and replays canned outcomes, falling back to a
        // retryable empty attempt once exhausted.
        let mock =
            MockChatClient::with_contents(&["{\"value\":\"first\"}", "{\"value\":\"second\"}"]);

        let first_payload = serde_json::json!({ "model": "m", "messages": [] });
        assert_eq!(
            mock.post_chat(&first_payload).await.unwrap().as_deref(),
            Some("{\"value\":\"first\"}")
        );
        assert_eq!(
            mock.post_chat(&serde_json::json!({ "model": "m2" }))
                .await
                .unwrap()
                .as_deref(),
            Some("{\"value\":\"second\"}")
        );
        // Exhausted queue -> Ok(None), the loop's "retryable empty attempt".
        assert_eq!(mock.post_chat(&serde_json::json!({})).await.unwrap(), None);

        let captured = mock.captured();
        assert_eq!(captured.len(), 3);
        assert_eq!(captured[0], first_payload);
    }

    #[test]
    fn interpret_surfaces_truncation_distinctly() {
        // done_reason == "length" means the content was cut off: abort with an
        // actionable capacity message rather than letting the truncated (invalid)
        // JSON read as a generic parse miss.
        let body = serde_json::json!({
            "done_reason": "length",
            "message": { "role": "assistant", "content": "{\"name\":\"Lir" }
        });
        let err = interpret_chat_response(&body).unwrap_err();
        assert!(err.contains("truncated"), "got: {err}");
        assert!(err.contains("num_ctx"), "got: {err}");
    }
}
