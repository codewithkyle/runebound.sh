use crate::services::vault_ref::{
    VaultReferenceEntry, extract_prompt_reference_keys, load_vault_reference_entries,
};
use crate::utils::normalize_relative_path_for_storage;
use dnd_core::config::AppConfig;
use dnd_core::entity_store::EntityStore;
use dnd_core::vault::Vault;
use std::collections::HashMap;
use std::path::Path;

/// Assemble the `@reference` prompt context from the configured vault, tolerating a
/// missing or unreadable vault by returning empty context. Shared by seed
/// generation and field reroll so a custom prompt's `@references` resolve the same
/// way in both flows.
pub(crate) async fn build_reference_context(
    config: &AppConfig,
    user_prompt: &str,
) -> PromptReferenceContext {
    let Some(vault_path) = config.vault.path.clone() else {
        return PromptReferenceContext::default();
    };
    let user_prompt = user_prompt.to_string();
    // read_dir + TOML loads + referenced-file reads are blocking IO; keep them off
    // the async runtime worker (P6.2). A panicked task degrades to empty context.
    tokio::task::spawn_blocking(move || {
        let vault = Vault::new(vault_path);
        if vault.ensure_root_exists().is_err() {
            return PromptReferenceContext::default();
        }
        match load_vault_reference_entries(&vault) {
            Ok(entries) => build_prompt_reference_context(&user_prompt, &entries, &vault),
            Err(err) => {
                eprintln!("reference context warning: {err}");
                PromptReferenceContext::default()
            }
        }
    })
    .await
    .unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
pub struct PromptReferenceContext {
    pub system_context: String,
}

fn build_prompt_reference_context(
    prompt: &str,
    entries: &[VaultReferenceEntry],
    vault: &Vault,
) -> PromptReferenceContext {
    let keys = extract_prompt_reference_keys(prompt, entries);
    if keys.is_empty() {
        return PromptReferenceContext::default();
    }

    let path_by_key: std::collections::HashMap<String, String> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .markdown_path
                .as_ref()
                .map(|path| (entry.key.to_lowercase(), path.clone()))
        })
        .collect();
    let mut blocks = Vec::new();

    let canonical_metadata = match EntityStore::new() {
        Ok(store) => canonical_metadata_map(&store),
        Err(err) => {
            eprintln!("reference context warning: failed to load canonical entities: {err}");
            HashMap::new()
        }
    };

    for key in keys.into_iter() {
        let Some(path) = path_by_key.get(&key.to_lowercase()) else {
            continue;
        };
        let normalized_path = normalize_relative_path_for_storage(path);
        let metadata = if let Some(canonical) = canonical_metadata.get(&normalized_path) {
            canonical.clone()
        } else {
            let contents = match vault.read_relative(Path::new(path)) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!(
                        "reference context warning: failed reading {}: {}",
                        path, err
                    );
                    continue;
                }
            };
            match reference_payload_from_markdown(&contents) {
                Some(value) => value,
                None => continue,
            }
        };
        blocks.push(format!("@{key}\npath: {path}\n```toml\n{metadata}\n```"));
    }

    if blocks.is_empty() {
        return PromptReferenceContext::default();
    }
    PromptReferenceContext {
        system_context: format!(
            "Referenced vault metadata (treat as authoritative setting context):\n\n{}",
            blocks.join("\n\n")
        ),
    }
}

fn canonical_metadata_map(store: &EntityStore) -> HashMap<String, String> {
    let mut map = HashMap::new();

    if let Ok(npcs) = store.list_npcs() {
        for npc in npcs {
            if let Ok(serialized) = toml::to_string_pretty(&npc) {
                map.insert(
                    normalize_relative_path_for_storage(&npc.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(locations) = store.list_locations() {
        for location in locations {
            if let Ok(serialized) = toml::to_string_pretty(&location) {
                map.insert(
                    normalize_relative_path_for_storage(&location.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(factions) = store.list_factions() {
        for faction in factions {
            if let Ok(serialized) = toml::to_string_pretty(&faction) {
                map.insert(
                    normalize_relative_path_for_storage(&faction.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(items) = store.list_items() {
        for item in items {
            if let Ok(serialized) = toml::to_string_pretty(&item) {
                map.insert(
                    normalize_relative_path_for_storage(&item.vault_path),
                    serialized,
                );
            }
        }
    }

    if let Ok(events) = store.list_events() {
        for event in events {
            if let Ok(serialized) = toml::to_string_pretty(&event) {
                map.insert(
                    normalize_relative_path_for_storage(&event.vault_path),
                    serialized,
                );
            }
        }
    }

    map
}

fn extract_runebound_toml(contents: &str) -> Option<String> {
    let start = contents.find("```runebound")?;
    let mut body = &contents[start + "```runebound".len()..];
    if let Some(rest) = body.strip_prefix("\r\n") {
        body = rest;
    } else if let Some(rest) = body.strip_prefix('\n') {
        body = rest;
    }
    let end = body.find("\n```").or_else(|| body.find("```"))?;
    let block = body[..end].trim();
    if block.is_empty() {
        None
    } else {
        Some(block.to_string())
    }
}

fn reference_payload_from_markdown(contents: &str) -> Option<String> {
    if let Some(block) = extract_runebound_toml(contents) {
        return Some(block);
    }
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_payload_prefers_runebound_block() {
        let markdown =
            "# Notes\n\n```runebound\ntype = \"npc\"\nname = \"Jimmy\"\n```\n\nExtra text";
        let payload = reference_payload_from_markdown(markdown).expect("payload");
        assert!(payload.contains("type = \"npc\""));
        assert!(!payload.contains("Extra text"));
    }
    #[test]
    fn reference_payload_falls_back_to_full_file() {
        let markdown = "# Notes about Jimmy\nNo runebound block present.";
        let payload = reference_payload_from_markdown(markdown).expect("payload");
        assert!(payload.contains("Notes about Jimmy"));
        assert!(payload.contains("No runebound block"));
    }
}
