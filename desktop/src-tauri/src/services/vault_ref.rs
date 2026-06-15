//! Shared `@reference` index used by both AI prompt-context building
//! (`ai_generation`) and autocomplete (`suggestions`).
//!
//! Both consumers need the same view of the vault: the set of markdown files and
//! directories that can be addressed with an `@path/to/Entity` reference, plus the
//! parsing rules for detecting those references in free text. Keeping one copy here
//! avoids the drift that previously crept in (e.g. ad-hoc `\\`→`/` replacement vs.
//! the canonical [`normalize_relative_path_for_storage`]).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use dnd_core::vault::Vault;

use crate::utils::normalize_relative_path_for_storage;

/// A single addressable `@reference` target discovered in the vault: either a
/// markdown file (`markdown_path` is set) or a directory (`is_dir`).
#[derive(Debug, Clone)]
pub struct VaultReferenceEntry {
    pub key: String,
    pub key_lower: String,
    pub markdown_path: Option<String>,
    pub is_dir: bool,
}

/// Characters that terminate an `@reference` token in free text.
fn is_reference_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"')
}

/// Whether an `@` at `at_index` can begin a reference (start of input or preceded
/// by whitespace / an opening bracket/quote), so we don't match mid-word `@`s
/// such as email addresses.
pub fn can_start_reference_at(input: &str, at_index: usize) -> bool {
    if at_index == 0 {
        return true;
    }

    let before = input[..at_index].chars().next_back();
    before.is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\''))
}

/// Skip hidden directories (dotfiles) and the build `target/` directory.
fn should_ignore_reference_component(component: &str) -> bool {
    component
        .split('/')
        .any(|part| part.starts_with('.') || part.eq_ignore_ascii_case("target"))
}

/// Build the reference key for a markdown file (`parent/Stem`, or just `Stem` at
/// the vault root). Returns `None` for non-markdown paths.
fn markdown_reference_key(relative_path: &str) -> Option<String> {
    let normalized = normalize_relative_path_for_storage(relative_path);
    let path = Path::new(&normalized);
    let ext = path.extension().and_then(|value| value.to_str())?;
    if !ext.eq_ignore_ascii_case("md") {
        return None;
    }

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let parent = path.parent().and_then(|value| value.to_str()).unwrap_or("");
    if parent.is_empty() {
        Some(stem.to_string())
    } else {
        Some(format!("{parent}/{stem}"))
    }
}

/// Walk the vault and collect every addressable reference target (markdown files
/// and directories), keyed by their `@reference` path.
pub fn load_vault_reference_entries(vault: &Vault) -> Result<Vec<VaultReferenceEntry>, String> {
    vault.ensure_root_exists().map_err(|err| err.to_string())?;

    let mut entries: HashMap<String, VaultReferenceEntry> = HashMap::new();
    let mut stack = vec![PathBuf::new()];

    while let Some(relative_dir) = stack.pop() {
        let full_dir = vault
            .resolve_relative(&relative_dir)
            .map_err(|err| err.to_string())?;
        let dir_entries = fs::read_dir(&full_dir)
            .map_err(|err| format!("failed to read directory {}: {}", full_dir.display(), err))?;

        for dir_entry in dir_entries {
            let dir_entry = match dir_entry {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("reference index warning: failed to read directory entry: {err}");
                    continue;
                }
            };
            let entry_path = dir_entry.path();
            let relative = match entry_path.strip_prefix(vault.root()) {
                Ok(value) => normalize_relative_path_for_storage(&value.to_string_lossy()),
                Err(_) => continue,
            };
            if should_ignore_reference_component(&relative) {
                continue;
            }

            if entry_path.is_dir() {
                let mut key = relative.trim_matches('/').to_string();
                if key.is_empty() {
                    continue;
                }
                key.push('/');
                entries.entry(key.clone()).or_insert_with(|| VaultReferenceEntry {
                    key: key.clone(),
                    key_lower: key.to_lowercase(),
                    markdown_path: None,
                    is_dir: true,
                });
                stack.push(PathBuf::from(relative));
                continue;
            }

            let Some(key) = markdown_reference_key(&relative) else {
                continue;
            };
            entries.entry(key.clone()).or_insert_with(|| VaultReferenceEntry {
                key: key.clone(),
                key_lower: key.to_lowercase(),
                markdown_path: Some(relative),
                is_dir: false,
            });
        }
    }

    let mut out: Vec<VaultReferenceEntry> = entries.into_values().collect();
    out.sort_by(|left, right| left.key_lower.cmp(&right.key_lower));
    Ok(out)
}

/// Find the markdown reference keys explicitly mentioned via `@reference` in a
/// prompt. Longest keys win so `@npcs/Lirael Drake` is preferred over `@npcs`.
pub fn extract_prompt_reference_keys(prompt: &str, entries: &[VaultReferenceEntry]) -> Vec<String> {
    let mut candidates: Vec<&VaultReferenceEntry> = entries
        .iter()
        .filter(|entry| !entry.is_dir && entry.markdown_path.is_some())
        .collect();
    candidates.sort_by(|left, right| right.key_lower.len().cmp(&left.key_lower.len()));

    let prompt_lower = prompt.to_lowercase();
    let mut cursor = 0;
    let mut matched = Vec::new();

    while cursor < prompt.len() {
        let next_at = match prompt[cursor..].find('@') {
            Some(offset) => cursor + offset,
            None => break,
        };
        if !can_start_reference_at(prompt, next_at) {
            cursor = next_at + 1;
            continue;
        }

        let tail_start = next_at + 1;
        let tail = &prompt_lower[tail_start..];
        let mut best: Option<&VaultReferenceEntry> = None;

        for candidate in &candidates {
            if !tail.starts_with(&candidate.key_lower) {
                continue;
            }
            let boundary_index = tail_start + candidate.key.len();
            let boundary_ok = prompt[boundary_index..]
                .chars()
                .next()
                .is_none_or(is_reference_boundary_char);
            if !boundary_ok {
                continue;
            }
            best = Some(*candidate);
            break;
        }

        if let Some(candidate) = best {
            matched.push(candidate.key.clone());
            cursor = tail_start + candidate.key.len();
            continue;
        }

        cursor = next_at + 1;
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for key in matched {
        let lowered = key.to_lowercase();
        if seen.insert(lowered) {
            unique.push(key);
        }
    }
    unique
}
