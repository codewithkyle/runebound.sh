//! Shared helpers for wizard "link an existing entity" steps: case-insensitive
//! typeahead and match resolution over `(name, slug)` pairs, plus recovery of
//! published `.md` notes from the vault (those were reaped from the DB at publish
//! time and now live only as files).
//!
//! First grown by the location wizard's faction/location pickers and lifted here so
//! other wizards reuse the exact same matching + loading behavior rather than
//! re-implementing it (e.g. a faction wizard linking allies/rivals, or a
//! headquarters location). The location-anchor specifics — subfoldered `.md` paths
//! and `LocationAnchorChoice` — stay in `wizards/location.rs`; only the generic,
//! flat `(name, slug)` machinery is here.

use std::collections::HashSet;

use dnd_core::config::load_effective;
use dnd_core::npc::slugify;
use dnd_core::vault::Vault;
use wizard::WizardChoice;

use crate::app_state::AppState;
use crate::services::vault_ref::{VaultReferenceEntry, load_vault_reference_entries};

/// How many matches the entity typeaheads (the faction and location pickers) list at
/// once, mirroring the `@reference` autocomplete cap so a huge campaign never floods
/// the suggestion box.
pub(crate) const ENTITY_SUGGESTION_LIMIT: usize = 12;

/// The outcome of resolving typed input against a `(name, slug)` entry set.
pub(crate) enum EntityMatch {
    /// A real entry: its display name plus the slug it carried.
    Found(String, String),
    /// Nothing matched — the caller decides whether to accept it as free text.
    None,
    /// A substring matched more than one entry; the caller asks the GM to narrow it.
    Ambiguous,
}

/// Case-insensitive typeahead over `(name, slug)` entries: substring match on the
/// name, prefix matches ranked first, capped. The submitted token is the display
/// name (readable in the input). Shared by the faction and location pickers.
pub(crate) fn entity_suggestions(entities: &[(String, String)], input: &str) -> Vec<WizardChoice> {
    let query = input.trim().to_ascii_lowercase();
    let mut matches: Vec<&(String, String)> = entities
        .iter()
        .filter(|(name, _)| query.is_empty() || name.to_ascii_lowercase().contains(&query))
        .collect();
    matches.sort_by(|(left, _), (right, _)| {
        let left = left.to_ascii_lowercase();
        let right = right.to_ascii_lowercase();
        // Prefix matches outrank mid-word matches; ties break alphabetically.
        right
            .starts_with(&query)
            .cmp(&left.starts_with(&query))
            .then(left.cmp(&right))
    });
    matches
        .into_iter()
        .take(ENTITY_SUGGESTION_LIMIT)
        .map(|(name, _)| WizardChoice::new(name.clone(), name.clone()))
        .collect()
}

/// Resolve typed input against `(name, slug)` entries: an exact name/slug match wins,
/// else a unique case-insensitive substring match. Shared by both pickers.
pub(crate) fn match_entity(entities: &[(String, String)], trimmed: &str) -> EntityMatch {
    if let Some((name, slug)) = entities
        .iter()
        .find(|(name, slug)| {
            slug.eq_ignore_ascii_case(trimmed) || name.eq_ignore_ascii_case(trimmed)
        })
        .cloned()
    {
        return EntityMatch::Found(name, slug);
    }
    let needle = trimmed.to_ascii_lowercase();
    let mut hits = entities
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().contains(&needle));
    match (hits.next().cloned(), hits.next()) {
        (Some((name, slug)), None) => EntityMatch::Found(name, slug),
        (Some(_), Some(_)) => EntityMatch::Ambiguous,
        _ => EntityMatch::None,
    }
}

/// Resolve a submitted link name against a loaded `(name, slug)` set: an exact/unique
/// match resolves to its canonical name; an unmatched name is accepted as free text
/// (it renders as a `[[wikilink]]` that resolves by name in Obsidian, even before the
/// entity exists); an ambiguous one asks the GM to narrow it. `kind` only labels the
/// error message. Shared by the faction and location pickers.
pub(crate) fn resolve_link_name(
    entries: &[(String, String)],
    trimmed: &str,
    kind: &str,
) -> Result<String, String> {
    match match_entity(entries, trimmed) {
        EntityMatch::Found(name, _) => Ok(name),
        EntityMatch::None => Ok(trimmed.to_string()),
        EntityMatch::Ambiguous => Err(format!(
            "Several {kind} match \"{trimmed}\" — pick one from the list or keep typing."
        )),
    }
}

/// Merge DB drafts with published-note entries, deduping by slug (drafts win) and
/// sorting by display name. The flat counterpart for any `(name, slug)` entity; the
/// location picker keeps its own subfolder-preserving merge.
pub(crate) fn merge_linkable(
    mut drafts: Vec<(String, String)>,
    published: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let mut seen: HashSet<String> = drafts
        .iter()
        .map(|(_, slug)| slug.to_ascii_lowercase())
        .collect();
    for (name, slug) in published {
        if seen.insert(slug.to_ascii_lowercase()) {
            drafts.push((name, slug));
        }
    }
    drafts.sort_by(|left, right| {
        left.0
            .to_ascii_lowercase()
            .cmp(&right.0.to_ascii_lowercase())
    });
    drafts
}

/// Every faction the GM can link, read-only: unpublished drafts from the DB plus
/// published notes recovered from the vault (those were reaped from the DB at
/// publish time and now live only as `.md` files). Deduped by slug, sorted by name.
pub(crate) async fn load_linkable_factions(
    state: &AppState,
) -> Result<Vec<(String, String)>, String> {
    let database = state.database();
    let rows = state.faction_repo().list_all(database.as_ref()).await?;
    let drafts = rows.into_iter().map(|row| (row.name, row.slug)).collect();

    // Reading the vault is recursive, blocking IO; keep it off the async runtime.
    let published = tokio::task::spawn_blocking(|| load_published_entity_names("factions"))
        .await
        .map_err(|err| err.to_string())??;
    Ok(merge_linkable(drafts, published))
}

/// Every NPC the GM can link as a faction's leader, read-only: unpublished drafts
/// from the DB plus published notes recovered from the vault. Deduped by slug,
/// sorted by name. Sibling of [`load_linkable_factions`] (swaps the repo + folder).
pub(crate) async fn load_linkable_npcs(state: &AppState) -> Result<Vec<(String, String)>, String> {
    let database = state.database();
    let rows = state.npc_repo().list_all(database.as_ref()).await?;
    let drafts = rows.into_iter().map(|row| (row.name, row.slug)).collect();

    let published = tokio::task::spawn_blocking(|| load_published_entity_names("npcs"))
        .await
        .map_err(|err| err.to_string())??;
    Ok(merge_linkable(drafts, published))
}

/// Every god the GM can link to a temple/cult, read-only: unpublished drafts from
/// the DB plus published notes recovered from the vault. Deduped by slug, sorted by
/// name. Sibling of [`load_linkable_factions`] (swaps the repo + folder).
pub(crate) async fn load_linkable_gods(state: &AppState) -> Result<Vec<(String, String)>, String> {
    let database = state.database();
    let rows = state.god_repo().list_all(database.as_ref()).await?;
    let drafts = rows.into_iter().map(|row| (row.name, row.slug)).collect();

    let published = tokio::task::spawn_blocking(|| load_published_entity_names("gods"))
        .await
        .map_err(|err| err.to_string())??;
    Ok(merge_linkable(drafts, published))
}

/// Load the vault's reference entries, or an empty set when no vault is configured.
/// Blocking IO (recursive `read_dir`) — only call inside `spawn_blocking`.
pub(crate) fn load_vault_entries_blocking() -> Result<Vec<VaultReferenceEntry>, String> {
    let loaded = load_effective().map_err(|err| err.to_string())?;
    let Some(vault_path) = loaded.effective.vault.path else {
        return Ok(Vec::new());
    };
    let vault = Vault::new(vault_path);
    if vault.ensure_root_exists().is_err() {
        return Ok(Vec::new());
    }
    load_vault_reference_entries(&vault)
}

/// Recover published `(name, slug)` notes from one of the vault's *flat* entity
/// folders (e.g. `factions/`). Drops the subfolder the locations folder may carry.
/// Blocking IO — only call inside `spawn_blocking`.
pub(crate) fn load_published_entity_names(folder: &str) -> Result<Vec<(String, String)>, String> {
    let entries = load_vault_entries_blocking()?;
    Ok(entries_from_refs(&entries, folder)
        .into_iter()
        .map(|(name, slug, _sub)| (name, slug))
        .collect())
}

/// Extract `(display name, slug, subfolder)` for each note under the vault's
/// `<folder>/` directory, accepting the flat `<folder>/<Name>.md` (`sub` == "") and
/// exactly one nesting level `<folder>/<sub>/<Name>.md` (`sub` == "<sub>"); anything
/// deeper is rejected. The display name is the file stem; the slug is derived from
/// it, since published notes carry no DB row. Pure, so it's unit-testable.
pub(crate) fn entries_from_refs(
    entries: &[VaultReferenceEntry],
    folder: &str,
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let Some(path) = entry.markdown_path.as_deref() else {
            continue;
        };
        let Some((dir, rest)) = path.split_once('/') else {
            continue;
        };
        if !dir.eq_ignore_ascii_case(folder) {
            continue;
        }
        // Flat note, or exactly one subfolder level; reject anything nested deeper.
        let (sub, file) = match rest.split_once('/') {
            None => ("", rest),
            Some((sub, file)) if !file.contains('/') => (sub, file),
            Some(_) => continue,
        };
        let name = std::path::Path::new(file)
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(name) = name {
            out.push((name.to_string(), slugify(name), sub.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ref_file(key: &str) -> VaultReferenceEntry {
        VaultReferenceEntry {
            key: key.to_string(),
            key_lower: key.to_ascii_lowercase(),
            markdown_path: Some(format!("{key}.md")),
            is_dir: false,
        }
    }

    #[test]
    fn entries_from_refs_accepts_one_level_and_captures_subfolder() {
        let entries = vec![
            ref_file("factions/Crimson Lanterns"),
            ref_file("locations/Silverhall"), // flat → sub ""
            ref_file("locations/settlements/Ashford"), // one level → sub "settlements"
            ref_file("locations/sites/Mirecairn"), // one level → sub "sites"
            ref_file("locations/sites/deep/TooDeep"), // two levels → ignored
            ref_file("npcs/Lirael Drake"),    // wrong folder → ignored
            VaultReferenceEntry {
                key: "factions/".to_string(),
                key_lower: "factions/".to_string(),
                markdown_path: None,
                is_dir: true, // directory → ignored
            },
        ];
        // Factions never nest: a flat note keeps an empty subfolder.
        assert_eq!(
            entries_from_refs(&entries, "factions"),
            vec![(
                "Crimson Lanterns".to_string(),
                "crimson-lanterns".to_string(),
                String::new(),
            )]
        );
        // Locations capture the one allowed nesting level; deeper paths are dropped.
        assert_eq!(
            entries_from_refs(&entries, "locations"),
            vec![
                (
                    "Silverhall".to_string(),
                    "silverhall".to_string(),
                    String::new()
                ),
                (
                    "Ashford".to_string(),
                    "ashford".to_string(),
                    "settlements".to_string()
                ),
                (
                    "Mirecairn".to_string(),
                    "mirecairn".to_string(),
                    "sites".to_string()
                ),
            ]
        );
    }

    #[test]
    fn match_entity_resolves_exact_unique_ambiguous_and_none() {
        let entities = vec![
            (
                "Crimson Lanterns".to_string(),
                "crimson-lanterns".to_string(),
            ),
            ("Crimson Court".to_string(), "crimson-court".to_string()),
            ("Silver Hand".to_string(), "silver-hand".to_string()),
        ];
        // Exact name and exact slug both resolve.
        assert!(matches!(
            match_entity(&entities, "Silver Hand"),
            EntityMatch::Found(name, _) if name == "Silver Hand"
        ));
        assert!(matches!(
            match_entity(&entities, "silver-hand"),
            EntityMatch::Found(name, _) if name == "Silver Hand"
        ));
        // A unique substring resolves; an ambiguous one does not.
        assert!(matches!(
            match_entity(&entities, "silver"),
            EntityMatch::Found(name, _) if name == "Silver Hand"
        ));
        assert!(matches!(
            match_entity(&entities, "crimson"),
            EntityMatch::Ambiguous
        ));
        assert!(matches!(
            match_entity(&entities, "nowhere"),
            EntityMatch::None
        ));
    }

    #[test]
    fn entity_suggestions_filters_ranks_prefix_first_and_excludes_misses() {
        let entities = vec![
            (
                "Crimson Lanterns".to_string(),
                "crimson-lanterns".to_string(),
            ),
            (
                "The Crimson Court".to_string(),
                "the-crimson-court".to_string(),
            ),
            ("Silver Hand".to_string(), "silver-hand".to_string()),
        ];
        let tokens: Vec<String> = entity_suggestions(&entities, "crim")
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        // Both crimson entries match; the prefix match ranks above the mid-word one,
        // and the unrelated entry is excluded.
        assert_eq!(
            tokens,
            vec![
                "Crimson Lanterns".to_string(),
                "The Crimson Court".to_string(),
            ]
        );
    }

    #[test]
    fn merge_linkable_dedupes_by_slug_with_drafts_winning() {
        let drafts = vec![("Silver Hand".to_string(), "silver-hand".to_string())];
        let published = vec![
            // Same slug as a draft → dropped (the draft wins).
            ("Silver Hand (old)".to_string(), "silver-hand".to_string()),
            ("Crimson Court".to_string(), "crimson-court".to_string()),
        ];
        let merged = merge_linkable(drafts, published);
        // Deduped to two, sorted by display name.
        assert_eq!(
            merged,
            vec![
                ("Crimson Court".to_string(), "crimson-court".to_string()),
                ("Silver Hand".to_string(), "silver-hand".to_string()),
            ]
        );
    }
}
