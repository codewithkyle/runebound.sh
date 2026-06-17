//! Tier 2 link generation: best-effort LLM recognition of entities that exist
//! *nowhere yet*.
//!
//! Tiers 0/1 link relational fields and mentions of entities already known (in
//! the canonical store or the vault). This module catches the remaining case —
//! a proper noun the generator invented ("Captain Vex", "the Sundering") that a
//! GM might want to prep later. At publish time we ask the LLM to *recognize*
//! such names in the entity's prose; the deterministic [`super::publish::EntityLinker`]
//! then stubs them as `[[wikilinks]]`.
//!
//! This is strictly best-effort: any failure (Ollama down, no model, bad JSON)
//! yields an empty list, so a publish never depends on the LLM being reachable.
//! Callers gate the call on the cached boot health to avoid latency when the
//! server is known to be down.

use std::collections::HashSet;
use std::path::Path;

use crate::services::ollama_chat::{
    build_chat_client, load_generation_config, post_chat_for_content,
};

/// Characters that can't appear in a clean `[[wikilink]]` target (mirrors the
/// guard in [`super::publish`]).
const LINK_UNSAFE: &[char] = &['[', ']', '|', '#', '^'];

/// Recognize proper-noun entity names in `prose` that are not already in
/// `known_lower` (a set of lowercased known names). Returns canonical-cased
/// names ready to be fed to the linker. Never errors — returns `[]` on any
/// failure so publishing is unaffected.
pub async fn extract_unknown_mentions(
    workspace_root: &Path,
    prose: &str,
    known_lower: &HashSet<String>,
) -> Vec<String> {
    if prose.trim().is_empty() {
        return Vec::new();
    }
    match request_mentions(workspace_root, prose).await {
        Ok(raw) => filter_mentions(raw, prose, known_lower),
        Err(_) => Vec::new(),
    }
}

async fn request_mentions(workspace_root: &Path, prose: &str) -> Result<Vec<String>, String> {
    let (config, model) = load_generation_config(workspace_root)?;
    let (client, url) = build_chat_client(&config)?;

    let schema = serde_json::json!({
        "type": "object",
        "required": ["mentions"],
        "properties": {
            "mentions": { "type": "array", "items": { "type": "string" } }
        },
        "additionalProperties": false
    });

    let payload = serde_json::json!({
        "model": model,
        "stream": false,
        "format": schema,
        // Deterministic, recognition-only — no creativity wanted here.
        "options": { "temperature": 0.0, "num_ctx": config.ollama.num_ctx },
        "messages": [
            {
                "role": "system",
                "content": "You extract named entities from Dungeons & Dragons prose. Return only JSON of the form {\"mentions\": [...]}. List the proper names of distinct people, places, factions, or items that are explicitly named in the text and that a game master might want as their own page. Copy each name using the exact spelling that appears in the text. Exclude generic descriptions (e.g. 'the old bridge', 'a band of bandits'), pronouns, titles without a name, and common nouns. If nothing qualifies, return an empty array."
            },
            { "role": "user", "content": prose }
        ]
    });

    let content = post_chat_for_content(&client, &url, &payload)
        .await?
        .ok_or_else(|| "ollama returned no content".to_string())?;

    #[derive(serde::Deserialize)]
    struct MentionsResponse {
        mentions: Vec<String>,
    }

    let parsed: MentionsResponse = serde_json::from_str(&content).map_err(|err| err.to_string())?;
    Ok(parsed.mentions)
}

/// Clean and validate the raw LLM name list. Pure so it can be unit-tested
/// without the model. Drops names that are too short, link-unsafe, already
/// known, hallucinated (not actually present in the prose), or duplicated.
fn filter_mentions(raw: Vec<String>, prose: &str, known_lower: &HashSet<String>) -> Vec<String> {
    let prose_lower = prose.to_ascii_lowercase();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();

    for name in raw {
        let trimmed = name.trim();
        if trimmed.len() < 2 {
            continue;
        }
        if trimmed.contains(LINK_UNSAFE) {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        // Ground every name in the actual text — guards against the model
        // inventing names that were never written.
        if !prose_lower.contains(&lower) {
            continue;
        }
        // Already-known names are handled by the store/vault candidate set.
        if known_lower.contains(&lower) {
            continue;
        }
        if !seen.insert(lower) {
            continue;
        }
        out.push(trimmed.to_string());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_ascii_lowercase()).collect()
    }

    #[test]
    fn keeps_a_grounded_unknown_name() {
        let prose = "Her brother Captain Vex rode for the coast.";
        let out = filter_mentions(vec!["Captain Vex".to_string()], prose, &known(&[]));
        assert_eq!(out, vec!["Captain Vex".to_string()]);
    }

    #[test]
    fn drops_names_not_present_in_the_prose() {
        // The model hallucinated a name that never appears in the text.
        let prose = "A quiet village by the river.";
        let out = filter_mentions(vec!["Captain Vex".to_string()], prose, &known(&[]));
        assert!(out.is_empty());
    }

    #[test]
    fn drops_already_known_names() {
        let prose = "She fled to Waterdeep at dawn.";
        let out = filter_mentions(vec!["Waterdeep".to_string()], prose, &known(&["waterdeep"]));
        assert!(
            out.is_empty(),
            "known names are handled by store/vault candidates"
        );
    }

    #[test]
    fn dedupes_case_insensitively() {
        let prose = "The Sundering broke the sky; the sundering still echoes.";
        let out = filter_mentions(
            vec!["The Sundering".to_string(), "the sundering".to_string()],
            prose,
            &known(&[]),
        );
        assert_eq!(out, vec!["The Sundering".to_string()]);
    }

    #[test]
    fn drops_link_unsafe_and_too_short_names() {
        let prose = "Notes mention X and [bracketed] and Aldric Vane here.";
        let out = filter_mentions(
            vec![
                "X".to_string(),
                "[bracketed]".to_string(),
                "Aldric Vane".to_string(),
            ],
            prose,
            &known(&[]),
        );
        assert_eq!(out, vec!["Aldric Vane".to_string()]);
    }
}
