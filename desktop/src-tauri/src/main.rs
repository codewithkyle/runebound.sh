#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod repositories;
mod router;
mod services;
mod utils;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::path::MAIN_SEPARATOR;

use dnd_core::command::{CommandClientEvent, CommandResponse};
use dnd_core::command_manifest::{CommandManifest, CommandSpec};
use dnd_core::command_parse::{ParseResult, ParseStage, normalize_command_input, parse_command_input};
use dnd_core::config::{load_effective, validate_for_runtime};
use dnd_core::db;
use dnd_core::npc::{
    LocationFrontmatter, UNKNOWN_LOCATION, make_entity_id, now_timestamp, render_location_markdown,
    slugify, unique_slug_for_dir,
};
use dnd_core::vault::Vault;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::app_state::{AppState, EditorSession};
use crate::repositories::{
    DocumentRepository, FactionRepository, GenerationRepository, LocationRepository, NpcRepository,
    ProdDocumentRepository, ProdFactionRepository, ProdGenerationRepository, ProdLocationRepository,
    ProdNpcRepository, ProdSoftDeleteRepository, ProdVaultRepository, SoftDeleteRepository,
    VaultRepository,
};
use crate::services::vault_sync::{
    move_vault_file, unique_markdown_path_for_name, unique_trash_path, VaultSyncService,
};
use crate::utils::normalize_relative_path_for_storage;


#[derive(Debug, Clone, Deserialize)]
struct EnsureLocationInput {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
struct EnsureLocationResult {
    name: String,
    slug: String,
    vault_path: String,
    created_file: bool,
    created_record: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum EntityType {
    Npc,
    Location,
    Faction,
}

#[derive(Debug, Clone, Serialize)]
struct EntitySuggestion {
    entity_type: EntityType,
    name: String,
    slug: String,
}

#[derive(Debug, Clone, Serialize)]
struct CommandSuggestion {
    label: String,
    completion: String,
    helper_text: Option<SuggestionHelperText>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum SuggestionHelperText {
    Command,
    Npc,
    Location,
    Faction,
    Reference,
}

#[derive(Debug, Clone, Serialize)]
struct EntityDetails {
    id: String,
    entity_type: EntityType,
    name: String,
    slug: String,
    race: Option<String>,
    occupation: Option<String>,
    sex: Option<String>,
    age: Option<String>,
    height: Option<String>,
    weight_lbs: Option<String>,
    background: Option<String>,
    want_need: Option<String>,
    secret_obstacle: Option<String>,
    carrying: Option<Vec<String>>,
    location: Option<String>,
    vault_path: String,
    kind_type: Option<String>,
    kind_custom: Option<String>,
    visual_description: Option<String>,
    history_background: Option<String>,
    exports: Option<Vec<String>>,
    tone: Option<String>,
    authority: Option<String>,
    danger_level: Option<String>,
    current_tension: Option<String>,
    public_description: Option<String>,
    true_agenda: Option<String>,
    methods: Option<String>,
    leadership: Option<String>,
    headquarters: Option<String>,
    sphere_of_influence: Option<String>,
    resources_assets: Option<String>,
    allies: Option<Vec<String>>,
    rivals_enemies: Option<Vec<String>>,
    reputation: Option<String>,
    goals_short_term: Option<Vec<String>>,
    goals_long_term: Option<Vec<String>>,
    symbol_description: Option<String>,
    created_at: Option<String>,
}

#[derive(Debug, Clone)]
struct VaultReferenceEntry {
    key: String,
    key_lower: String,
    #[allow(dead_code)]
    markdown_path: Option<String>,
    is_dir: bool,
}

#[derive(Debug, Clone)]
struct ActiveReferenceQuery {
    at_index: usize,
    query: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SoftDeleteEntityInput {
    target: String,
}

#[derive(Debug, Clone, Serialize)]
struct SoftDeleteEntityResult {
    entity_type: EntityType,
    id: String,
    name: String,
    slug: String,
    trash_vault_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct UndoSoftDeleteResult {
    entity_type: EntityType,
    id: String,
    name: String,
    slug: String,
    vault_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpcDeletePayload {
    id: String,
    slug: String,
    name: String,
    race: String,
    occupation: String,
    sex: String,
    age: String,
    height: String,
    weight_lbs: String,
    background: String,
    want_need: String,
    secret_obstacle: String,
    carrying: String,
    location: String,
    vault_path: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocationDeletePayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
    kind_type: String,
    kind_custom: Option<String>,
    visual_description: String,
    history_background: String,
    exports: String,
    tone: String,
    authority: String,
    danger_level: String,
    current_tension: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FactionDeletePayload {
    id: String,
    slug: String,
    name: String,
    vault_path: String,
    kind_type: String,
    kind_custom: Option<String>,
    public_description: String,
    true_agenda: String,
    methods: String,
    leadership: String,
    headquarters: String,
    sphere_of_influence: String,
    resources_assets: String,
    allies: String,
    rivals_enemies: String,
    reputation: String,
    current_tension: String,
    goals_short_term: String,
    goals_long_term: String,
    symbol_description: String,
    created_at: String,
    updated_at: String,
}

fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();

    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

fn parse_carrying_csv(value: &str) -> Vec<String> {
    let items: Vec<String> = value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    normalize_unknown_list(items)
}

fn parse_list_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn normalize_exports(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

pub(crate) fn exports_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

pub(crate) fn exports_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_exports(items),
        Err(_) => normalize_exports(parse_list_csv(value)),
    }
}


pub(crate) fn carrying_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

pub(crate) fn carrying_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_unknown_list(items),
        Err(_) => parse_carrying_csv(value),
    }
}

pub(crate) fn faction_list_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

pub(crate) fn faction_list_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_unknown_list(items),
        Err(_) => normalize_unknown_list(parse_list_csv(value)),
    }
}

#[cfg(test)]
fn is_reference_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"')
}

fn can_start_reference_at(input: &str, at_index: usize) -> bool {
    if at_index == 0 {
        return true;
    }

    let before = input[..at_index].chars().next_back();
    before.is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\''))
}

fn extract_active_reference_query(input: &str) -> Option<ActiveReferenceQuery> {
    for (idx, ch) in input.char_indices().rev() {
        if ch != '@' {
            continue;
        }
        if !can_start_reference_at(input, idx) {
            continue;
        }

        return Some(ActiveReferenceQuery {
            at_index: idx,
            query: input[idx + 1..].to_string(),
        });
    }

    None
}

fn should_ignore_reference_component(component: &str) -> bool {
    component
        .split('/')
        .any(|part| part.starts_with('.') || part.eq_ignore_ascii_case("target"))
}

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

fn is_top_level_reference_key(key: &str, is_dir: bool) -> bool {
    if is_dir {
        let trimmed = key.trim_end_matches('/');
        !trimmed.is_empty() && !trimmed.contains('/')
    } else {
        !key.contains('/')
    }
}

fn load_vault_reference_entries(vault: &Vault) -> Result<Vec<VaultReferenceEntry>, String> {
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

fn build_reference_suggestions_from_entries(
    input: &str,
    active: &ActiveReferenceQuery,
    entries: &[VaultReferenceEntry],
) -> Vec<CommandSuggestion> {
    let query_lower = normalize_relative_path_for_storage(&active.query).to_lowercase();
    let mut ranked: Vec<&VaultReferenceEntry> = entries
        .iter()
        .filter(|entry| {
            if query_lower.is_empty() {
                return is_top_level_reference_key(&entry.key, entry.is_dir);
            }
            entry.key_lower.starts_with(&query_lower)
        })
        .collect();

    ranked.sort_by(|left, right| left.key_lower.cmp(&right.key_lower));
    ranked
        .into_iter()
        .take(12)
        .map(|entry| {
            let completion_suffix = if entry.is_dir { "" } else { " " };
            CommandSuggestion {
                label: format!("@{}", entry.key),
                completion: format!(
                    "{}@{}{}",
                    &input[..active.at_index],
                    entry.key,
                    completion_suffix
                ),
                helper_text: Some(SuggestionHelperText::Reference),
            }
        })
        .collect()
}

#[cfg(test)]
#[cfg(test)]
fn extract_prompt_reference_keys(prompt: &str, entries: &[VaultReferenceEntry]) -> Vec<String> {
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


fn npc_travel_location_query(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "npc travel to" {
        return Some(String::new());
    }
    if lowered.starts_with("npc travel to ") {
        return Some(trimmed[14..].trim().to_string());
    }

    None
}

#[tauri::command]
async fn suggest_command_input(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CommandSuggestion>, String> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Some(active_ref) = extract_active_reference_query(&input) {
        if active_ref
            .query
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
        {
            return Ok(Vec::new());
        }

        if !active_ref.query.trim().starts_with('-') {
            let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
            if let Some(vault_path) = loaded.effective.vault.path {
                let vault = Vault::new(vault_path);
                if vault.ensure_root_exists().is_ok() {
                    let entries = load_vault_reference_entries(&vault)?;
                    let suggestions =
                        build_reference_suggestions_from_entries(&input, &active_ref, &entries);
                    return Ok(suggestions);
                }
            }

            return Ok(Vec::new());
        }
    }

    let manifest = dnd_core::command_manifest::command_manifest();
    let parsed = dnd_core::command_parse::parse_command_input(&input);
    let mut suggestions = build_command_suggestions(&manifest, &parsed, &input);

    let mode = {
        let editor = state.editor_session.lock().await;
        editor.mode
    };

    suggestions.retain(|suggestion| {
        let completion = suggestion.completion.trim().to_ascii_lowercase();
        let label = suggestion.label.trim().to_ascii_lowercase();

        if mode != app_state::EditorMode::Npc {
            if completion == "npc"
                || completion.starts_with("npc ")
                || label == "npc"
                || label.starts_with("npc ")
            {
                return false;
            }
            if mode != app_state::EditorMode::Location
                && mode != app_state::EditorMode::Faction
                && (completion == "reroll" || label == "reroll")
            {
                return false;
            }
        }

        if mode != app_state::EditorMode::Location
            && (completion == "location"
                || completion.starts_with("location ")
                || label == "location"
                || label.starts_with("location "))
        {
            return false;
        }

        if mode != app_state::EditorMode::Faction
            && (completion == "faction"
                || completion.starts_with("faction ")
                || label == "faction"
                || label.starts_with("faction "))
        {
            return false;
        }

        if mode == app_state::EditorMode::None && (completion == "cancel" || label == "cancel") {
            return false;
        }

        true
    });

    let trimmed = input.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let is_load_context = lowered == "load" || lowered.starts_with("load ");
    let is_delete_context = lowered == "delete" || lowered.starts_with("delete ");
    let is_show_context = lowered == "show" || lowered.starts_with("show ");
    let is_preview_context = lowered == "preview" || lowered.starts_with("preview ");
    let search_query = if is_load_context {
        trimmed[4..].trim()
    } else if is_delete_context {
        trimmed[6..].trim()
    } else if is_show_context {
        trimmed[4..].trim()
    } else if is_preview_context {
        trimmed[7..].trim()
    } else {
        trimmed
    };

    if !search_query.is_empty()
        && (is_load_context
            || is_delete_context
            || is_show_context
            || is_preview_context
            || !starts_with_known_command_root(trimmed, &manifest))
    {
        let entity_results = search_entities(state.inner(), search_query.to_string(), Some(6)).await?;
        let prefix = if is_load_context {
            Some("load")
        } else if is_delete_context {
            Some("delete")
        } else if is_show_context {
            Some("show")
        } else if is_preview_context {
            Some("preview")
        } else {
            None
        };

        for entity in entity_results {
            let completion = match prefix {
                Some(value) => format!("{value} {}", entity.name),
                None => entity.name.clone(),
            };
            suggestions.push(CommandSuggestion {
                label: entity.name,
                completion,
                helper_text: Some(match entity.entity_type {
                    EntityType::Npc => SuggestionHelperText::Npc,
                    EntityType::Location => SuggestionHelperText::Location,
                    EntityType::Faction => SuggestionHelperText::Faction,
                }),
            });
        }
    }

    if mode == app_state::EditorMode::Npc {
        if let Some(location_query) = npc_travel_location_query(trimmed) {
            let location_names =
                search_location_names(state.inner(), location_query, Some(8)).await?;
            for location_name in location_names {
                suggestions.push(CommandSuggestion {
                    label: location_name.clone(),
                    completion: format!("npc travel to {} ", location_name),
                    helper_text: Some(SuggestionHelperText::Location),
                });
            }
        }
    }

    let mut seen = HashSet::new();
    suggestions.retain(|suggestion| {
        let key = suggestion.completion.trim().to_ascii_lowercase();
        seen.insert(key)
    });

    Ok(suggestions)
}

fn build_command_suggestions(
    manifest: &CommandManifest,
    parsed: &ParseResult,
    input: &str,
) -> Vec<CommandSuggestion> {
    if matches!(parsed.completion.stage, ParseStage::Root) {
        return build_root_suggestions(manifest, &parsed.completion.current_token);
    }

    if matches!(parsed.completion.stage, ParseStage::Subcommand) {
        return build_subcommand_suggestions(
            manifest,
            parsed.completion.root.as_deref(),
            input,
            &parsed.completion.current_token,
        );
    }

    build_argument_suggestions(manifest, parsed, input)
}

fn build_root_suggestions(manifest: &CommandManifest, token: &str) -> Vec<CommandSuggestion> {
    let prefix = token.to_ascii_lowercase();
    manifest
        .commands
        .iter()
        .filter(|cmd| cmd.show_in_autocomplete)
        .filter(|cmd| cmd.name.starts_with(&prefix))
        .map(|cmd| CommandSuggestion {
            label: cmd.name.clone(),
            completion: format!("{}{}", cmd.name, completion_suffix(cmd)),
            helper_text: Some(SuggestionHelperText::Command),
        })
        .collect()
}

fn build_subcommand_suggestions(
    manifest: &CommandManifest,
    root: Option<&str>,
    input: &str,
    token: &str,
) -> Vec<CommandSuggestion> {
    let Some(root) = root else {
        return Vec::new();
    };
    let Some(command) = find_command(manifest, root) else {
        return Vec::new();
    };

    let prefix = token.to_ascii_lowercase();
    let base = replace_current_token(input, token);
    command
        .subcommands
        .iter()
        .filter(|subcommand| subcommand.name.starts_with(&prefix))
        .map(|subcommand| CommandSuggestion {
            label: format!("{} {}", command.name, subcommand.name),
            completion: format!("{base}{} ", subcommand.name),
            helper_text: Some(SuggestionHelperText::Command),
        })
        .collect()
}

fn build_argument_suggestions(
    manifest: &CommandManifest,
    parsed: &ParseResult,
    input: &str,
) -> Vec<CommandSuggestion> {
    let Some(root) = parsed.completion.root.as_deref() else {
        return Vec::new();
    };
    let Some(command) = find_command(manifest, root) else {
        return Vec::new();
    };

    let subcommand = parsed
        .completion
        .subcommand
        .as_ref()
        .and_then(|item| command.subcommands.iter().find(|sub| sub.name == *item));

    if command.name == "npc" && subcommand.is_some_and(|item| item.name == "travel") {
        let normalized: Vec<String> = parsed
            .normalized_tokens
            .iter()
            .map(|token| token.to_ascii_lowercase())
            .collect();
        let has_to = normalized.len() >= 3 && normalized[2] == "to";
        if !has_to {
            return vec![CommandSuggestion {
                label: "npc travel to".to_string(),
                completion: "npc travel to ".to_string(),
                helper_text: Some(SuggestionHelperText::Command),
            }];
        }
    }

    if command.name == "npc"
        && subcommand.is_some_and(|item| item.name == "set" || item.name == "reroll")
    {
        let field_names = [
            "name",
            "race",
            "occupation",
            "sex",
            "age",
            "height",
            "weight",
            "background",
            "want",
            "secret",
            "carrying",
        ];
        let args = &parsed.normalized_tokens[2..];
        let should_suggest_fields =
            args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);

        if should_suggest_fields {
            let prefix = parsed.completion.current_token.to_ascii_lowercase();
            let base = replace_current_token(input, &parsed.completion.current_token);
            let prefix_label = if subcommand.is_some_and(|item| item.name == "set") {
                "npc set"
            } else {
                "npc reroll"
            };

            return field_names
                .iter()
                .filter(|field| field.starts_with(&prefix))
                .map(|field| CommandSuggestion {
                    label: format!("{prefix_label} {field}"),
                    completion: format!("{base}{field} "),
                    helper_text: Some(SuggestionHelperText::Command),
                })
                .collect();
        }
    }

    if command.name == "location"
        && subcommand.is_some_and(|item| item.name == "set" || item.name == "reroll")
    {
        let field_names = [
            "name",
            "kind",
            "kind_custom",
            "visual",
            "history",
            "exports",
            "tone",
            "authority",
            "danger",
            "tension",
        ];
        let args = &parsed.normalized_tokens[2..];
        let should_suggest_fields =
            args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);

        if should_suggest_fields {
            let prefix = parsed.completion.current_token.to_ascii_lowercase();
            let base = replace_current_token(input, &parsed.completion.current_token);
            let prefix_label = if subcommand.is_some_and(|item| item.name == "set") {
                "location set"
            } else {
                "location reroll"
            };

            return field_names
                .iter()
                .filter(|field| field.starts_with(&prefix))
                .map(|field| CommandSuggestion {
                    label: format!("{prefix_label} {field}"),
                    completion: format!("{base}{field} "),
                    helper_text: Some(SuggestionHelperText::Command),
                })
                .collect();
        }
    }

    if command.name == "faction"
        && subcommand.is_some_and(|item| item.name == "set" || item.name == "reroll")
    {
        let field_names = [
            "name",
            "kind",
            "kind_custom",
            "public",
            "agenda",
            "methods",
            "leadership",
            "headquarters",
            "influence",
            "resources",
            "allies",
            "rivals",
            "reputation",
            "tension",
            "goals_short",
            "goals_long",
            "symbol",
        ];
        let args = &parsed.normalized_tokens[2..];
        let should_suggest_fields =
            args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);

        if should_suggest_fields {
            let prefix = parsed.completion.current_token.to_ascii_lowercase();
            let base = replace_current_token(input, &parsed.completion.current_token);
            let prefix_label = if subcommand.is_some_and(|item| item.name == "set") {
                "faction set"
            } else {
                "faction reroll"
            };

            return field_names
                .iter()
                .filter(|field| field.starts_with(&prefix))
                .map(|field| CommandSuggestion {
                    label: format!("{prefix_label} {field}"),
                    completion: format!("{base}{field} "),
                    helper_text: Some(SuggestionHelperText::Command),
                })
                .collect();
        }
    }

    let options = match subcommand {
        Some(item) => &item.options,
        None => &command.options,
    };
    if options.is_empty() {
        return Vec::new();
    }

    let current = parsed.completion.current_token.to_ascii_lowercase();
    let used: std::collections::HashSet<String> = parsed
        .normalized_tokens
        .iter()
        .filter(|token| token.starts_with('-'))
        .cloned()
        .collect();
    let base = replace_current_token(input, &parsed.completion.current_token);
    let should_filter_prefix = current.starts_with('-') || !current.is_empty();

    options
        .iter()
        .filter(|option| !used.contains(&option.name) || option.takes_value)
        .filter(|option| !should_filter_prefix || option.name.starts_with(&current))
        .map(|option| {
            let label = match subcommand {
                Some(item) => format!("{} {} {}", command.name, item.name, option.name),
                None => format!("{} {}", command.name, option.name),
            };
            let suffix = if option.takes_value { " " } else { "" };
            CommandSuggestion {
                label,
                completion: format!("{base}{}{suffix}", option.name),
                helper_text: Some(SuggestionHelperText::Command),
            }
        })
        .collect()
}

fn find_command<'a>(manifest: &'a CommandManifest, root: &str) -> Option<&'a CommandSpec> {
    let normalized = root.to_ascii_lowercase();
    manifest
        .commands
        .iter()
        .find(|command| command.name == normalized)
}

fn replace_current_token(input: &str, current_token: &str) -> String {
    if current_token.is_empty() {
        return input.to_string();
    }

    let suffix_len = current_token.len();
    if input.len() < suffix_len {
        return input.to_string();
    }

    input[..input.len() - suffix_len].to_string()
}

fn completion_suffix(command: &CommandSpec) -> &'static str {
    if !command.subcommands.is_empty() || !command.options.is_empty() || command.requires_subcommand
    {
        " "
    } else {
        ""
    }
}

fn starts_with_known_command_root(input: &str, manifest: &CommandManifest) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    let Some(first) = trimmed.split_whitespace().next() else {
        return false;
    };
    let lowered = first.to_ascii_lowercase();
    manifest.commands.iter().any(|command| command.name == lowered)
}

#[tauri::command]
async fn run_command(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommandResponse, String> {
    let normalized_input = normalize_input_for_dispatch(&input);
    let parsed = parse_command_input(&normalized_input);
    if !parsed.valid {
        let has_unknown_command = parsed
            .diagnostics
            .iter()
            .any(|diag| diag.code == "unknown_command");

        if !has_unknown_command {
            if let Some(diag) = parsed.diagnostics.first() {
                return Err(diag.message.clone());
            }
            return Err("invalid command".to_string());
        }
    }

    if let Some(response) =
        router::dispatch_desktop_command(&normalized_input, &parsed.normalized_tokens, state.clone())
            .await?
    {
        let skip_history_push = matches!(
            response.client_event,
            Some(CommandClientEvent::ClearTerminal {
                clear_history: true
            })
        );
        if !skip_history_push {
            let trimmed = normalized_input.trim();
            if !trimmed.is_empty() {
                let mut service = state.command_service.lock().await;
                service.session_mut().push_history(trimmed, 50);
            }
        }
        return Ok(response);
    }

    let mut service = state.command_service.lock().await;
    Ok(service.execute_line(&normalized_input).await)
}

fn normalize_input_for_dispatch(input: &str) -> String {
    normalize_command_input(input)
}



async fn ensure_location_exists(
    input: EnsureLocationInput,
    state: tauri::State<'_, AppState>,
) -> Result<EnsureLocationResult, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let raw_name = input.name.trim();
    if raw_name.is_empty() {
        return Err("location name cannot be empty".to_string());
    }
    if raw_name.eq_ignore_ascii_case(UNKNOWN_LOCATION) {
        return Ok(EnsureLocationResult {
            name: UNKNOWN_LOCATION.to_string(),
            slug: slugify(UNKNOWN_LOCATION),
            vault_path: String::new(),
            created_file: false,
            created_record: false,
        });
    }

    let database = state.database();
    let location_repo = state.location_repo();
    let document_repo = state.document_repo();
    let slug = slugify(raw_name);
    let existing = location_repo
        .find_by_slug(database.as_ref(), &slug)
        .await?;

    let mut created_file = false;
    let mut created_record = false;
    let now = now_timestamp();
    let id = existing
        .as_ref()
        .map(|row| row.id.clone())
        .unwrap_or_else(|| make_entity_id("loc"));
    let canonical_name = existing
        .as_ref()
        .map(|row| row.name.clone())
        .unwrap_or_else(|| raw_name.to_string());
    let created_at = existing
        .as_ref()
        .map(|row| row.created_at.clone())
        .unwrap_or_else(|| now.clone());

    let relative_path = if let Some(row) = existing.as_ref() {
        normalize_relative_path_for_storage(&row.vault_path)
    } else {
        unique_markdown_path_for_name(&vault, "locations", &canonical_name, None)?
    };
    let file_exists = vault
        .resolve_relative(&PathBuf::from(&relative_path))
        .map_err(|err| err.to_string())?
        .exists();

    if !file_exists {
        let default_exports = vec!["Unknown".to_string()];
        let content = render_location_markdown(&LocationFrontmatter {
            doc_type: "location".to_string(),
            id: id.clone(),
            slug: slug.clone(),
            name: canonical_name.clone(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: default_exports.clone(),
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
            created_at: created_at.clone(),
            updated_at: now.clone(),
        })
        .map_err(|err| err.to_string())?;
        vault
            .write_relative(&PathBuf::from(&relative_path), &content)
            .map_err(|err| err.to_string())?;
        created_file = true;
    }

    if existing.is_none() {
        created_record = true;
    }

    let row = db::LocationRow {
        id,
        slug: slug.clone(),
        name: canonical_name.clone(),
        vault_path: relative_path,
        kind_type: existing
            .as_ref()
            .map(|row| row.kind_type.clone())
            .unwrap_or_else(|| "other".to_string()),
        kind_custom: existing
            .as_ref()
            .and_then(|row| row.kind_custom.clone())
            .or_else(|| Some("Unknown".to_string())),
        visual_description: existing
            .as_ref()
            .map(|row| row.visual_description.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        history_background: existing
            .as_ref()
            .map(|row| row.history_background.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        exports: existing
            .as_ref()
            .map(|row| row.exports.clone())
            .unwrap_or_else(|| "[\"Unknown\"]".to_string()),
        tone: existing
            .as_ref()
            .map(|row| row.tone.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        authority: existing
            .as_ref()
            .map(|row| row.authority.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        danger_level: existing
            .as_ref()
            .map(|row| row.danger_level.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        current_tension: existing
            .as_ref()
            .map(|row| row.current_tension.clone())
            .unwrap_or_else(|| "Unknown".to_string()),
        created_at,
        updated_at: now.clone(),
    };

    location_repo
        .upsert(database.as_ref(), &row)
        .await?;
    document_repo
        .upsert_index(
            database.as_ref(),
            "location",
            &row.slug,
            Some(&row.name),
            &row.vault_path,
            &row.created_at,
            &row.updated_at,
        )
        .await?;

    Ok(EnsureLocationResult {
        name: canonical_name,
        slug,
        vault_path: row.vault_path,
        created_file,
        created_record,
    })
}







async fn search_entities(
    state: &AppState,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<EntitySuggestion>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let limit = i64::from(limit.unwrap_or(8)).clamp(1, 20);
    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();

    let npcs = npc_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let locations = location_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let factions = faction_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;

    let mut items: Vec<EntitySuggestion> = npcs
        .into_iter()
        .map(|npc| EntitySuggestion {
            entity_type: EntityType::Npc,
            name: npc.name,
            slug: npc.slug,
        })
        .chain(locations.into_iter().map(|location| EntitySuggestion {
            entity_type: EntityType::Location,
            name: location.name,
            slug: location.slug,
        }))
        .chain(factions.into_iter().map(|faction| EntitySuggestion {
            entity_type: EntityType::Faction,
            name: faction.name,
            slug: faction.slug,
        }))
        .collect();

    items.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    items.truncate(limit as usize);
    Ok(items)
}

async fn search_location_names(
    state: &AppState,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<String>, String> {
    let limit = i64::from(limit.unwrap_or(8)).clamp(1, 20);
    let database = state.database();
    let location_repo = state.location_repo();
    let rows = location_repo
        .search_by_name(database.as_ref(), query.trim(), limit)
        .await?;

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let name = row.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let key = name.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(name);
        }
    }

    Ok(out)
}

async fn resolve_entity(input: String, state: &AppState) -> Result<Option<EntityDetails>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();

    if let Some(npc) = npc_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: npc.id,
            entity_type: EntityType::Npc,
            name: npc.name,
            slug: npc.slug,
            race: Some(npc.race),
            occupation: Some(npc.occupation),
            sex: Some(npc.sex),
            age: Some(npc.age),
            height: Some(npc.height),
            weight_lbs: Some(npc.weight_lbs),
            background: Some(npc.background),
            want_need: Some(npc.want_need),
            secret_obstacle: Some(npc.secret_obstacle),
            carrying: Some(carrying_from_db_text(&npc.carrying)),
            location: Some(npc.location),
            vault_path: normalize_relative_path_for_storage(&npc.vault_path),
            kind_type: None,
            kind_custom: None,
            visual_description: None,
            history_background: None,
            exports: None,
            tone: None,
            authority: None,
            danger_level: None,
            current_tension: None,
            public_description: None,
            true_agenda: None,
            methods: None,
            leadership: None,
            headquarters: None,
            sphere_of_influence: None,
            resources_assets: None,
            allies: None,
            rivals_enemies: None,
            reputation: None,
            goals_short_term: None,
            goals_long_term: None,
            symbol_description: None,
            created_at: Some(npc.created_at),
        }));
    }

    if let Some(location) = location_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: location.id,
            entity_type: EntityType::Location,
            name: location.name,
            slug: location.slug,
            race: None,
            occupation: None,
            sex: None,
            age: None,
            height: None,
            weight_lbs: None,
            background: None,
            want_need: None,
            secret_obstacle: None,
            carrying: None,
            location: None,
            vault_path: normalize_relative_path_for_storage(&location.vault_path),
            kind_type: Some(location.kind_type),
            kind_custom: location.kind_custom,
            visual_description: Some(location.visual_description),
            history_background: Some(location.history_background),
            exports: Some(exports_from_db_text(&location.exports)),
            tone: Some(location.tone),
            authority: Some(location.authority),
            danger_level: Some(location.danger_level),
            current_tension: Some(location.current_tension),
            public_description: None,
            true_agenda: None,
            methods: None,
            leadership: None,
            headquarters: None,
            sphere_of_influence: None,
            resources_assets: None,
            allies: None,
            rivals_enemies: None,
            reputation: None,
            goals_short_term: None,
            goals_long_term: None,
            symbol_description: None,
            created_at: Some(location.created_at),
        }));
    }

    if let Some(faction) = faction_repo
        .find_by_name_or_slug(database.as_ref(), trimmed)
        .await?
    {
        return Ok(Some(EntityDetails {
            id: faction.id,
            entity_type: EntityType::Faction,
            name: faction.name,
            slug: faction.slug,
            race: None,
            occupation: None,
            sex: None,
            age: None,
            height: None,
            weight_lbs: None,
            background: None,
            want_need: None,
            secret_obstacle: None,
            carrying: None,
            location: None,
            vault_path: normalize_relative_path_for_storage(&faction.vault_path),
            kind_type: Some(faction.kind_type),
            kind_custom: faction.kind_custom,
            visual_description: None,
            history_background: None,
            exports: None,
            tone: None,
            authority: None,
            danger_level: None,
            current_tension: Some(faction.current_tension),
            public_description: Some(faction.public_description),
            true_agenda: Some(faction.true_agenda),
            methods: Some(faction.methods),
            leadership: Some(faction.leadership),
            headquarters: Some(faction.headquarters),
            sphere_of_influence: Some(faction.sphere_of_influence),
            resources_assets: Some(faction.resources_assets),
            allies: Some(faction_list_from_db_text(&faction.allies)),
            rivals_enemies: Some(faction_list_from_db_text(&faction.rivals_enemies)),
            reputation: Some(faction.reputation),
            goals_short_term: Some(faction_list_from_db_text(&faction.goals_short_term)),
            goals_long_term: Some(faction_list_from_db_text(&faction.goals_long_term)),
            symbol_description: Some(faction.symbol_description),
            created_at: Some(faction.created_at),
        }));
    }

    Ok(None)
}

async fn soft_delete_entity(
    input: SoftDeleteEntityInput,
    state: tauri::State<'_, AppState>,
) -> Result<SoftDeleteEntityResult, String> {
    let target = input.target.trim();
    if target.is_empty() {
        return Err("usage: delete <npc-or-location-name>".to_string());
    }

    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let soft_delete_repo = state.soft_delete_repo();
    let now = now_timestamp();

    if let Some(npc) = npc_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&npc.vault_path);
        let trash_path = unique_trash_path(&vault, "npcs", &npc.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        npc_repo
            .delete_by_id(database.as_ref(), &npc.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &npc.vault_path)
            .await?;

        let payload = NpcDeletePayload {
            id: npc.id.clone(),
            slug: npc.slug.clone(),
            name: npc.name.clone(),
            race: npc.race,
            occupation: npc.occupation,
            sex: npc.sex,
            age: npc.age,
            height: npc.height,
            weight_lbs: npc.weight_lbs,
            background: npc.background,
            want_need: npc.want_need,
            secret_obstacle: npc.secret_obstacle,
            carrying: npc.carrying,
            location: npc.location,
            vault_path: normalized_vault_path.clone(),
            created_at: npc.created_at,
            updated_at: npc.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "npc".to_string(),
            entity_id: npc.id.clone(),
            name: npc.name.clone(),
            slug: npc.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Npc,
            id: npc.id,
            name: npc.name,
            slug: npc.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(location) = location_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&location.vault_path);
        let trash_path = unique_trash_path(&vault, "locations", &location.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        location_repo
            .delete_by_id(database.as_ref(), &location.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &location.vault_path)
            .await?;

        let payload = LocationDeletePayload {
            id: location.id.clone(),
            slug: location.slug.clone(),
            name: location.name.clone(),
            vault_path: normalized_vault_path.clone(),
            kind_type: location.kind_type,
            kind_custom: location.kind_custom,
            visual_description: location.visual_description,
            history_background: location.history_background,
            exports: location.exports,
            tone: location.tone,
            authority: location.authority,
            danger_level: location.danger_level,
            current_tension: location.current_tension,
            created_at: location.created_at,
            updated_at: location.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "location".to_string(),
            entity_id: location.id.clone(),
            name: location.name.clone(),
            slug: location.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Location,
            id: location.id,
            name: location.name,
            slug: location.slug,
            trash_vault_path: trash_path,
        });
    }

    if let Some(faction) = faction_repo
        .find_by_name_or_slug(database.as_ref(), target)
        .await?
    {
        let normalized_vault_path = normalize_relative_path_for_storage(&faction.vault_path);
        let trash_path = unique_trash_path(&vault, "factions", &faction.slug, &now)?;
        move_vault_file(&vault, &normalized_vault_path, &trash_path)?;

        faction_repo
            .delete_by_id(database.as_ref(), &faction.id)
            .await?;
        document_repo
            .delete_by_vault_path(database.as_ref(), &faction.vault_path)
            .await?;

        let payload = FactionDeletePayload {
            id: faction.id.clone(),
            slug: faction.slug.clone(),
            name: faction.name.clone(),
            vault_path: normalized_vault_path.clone(),
            kind_type: faction.kind_type,
            kind_custom: faction.kind_custom,
            public_description: faction.public_description,
            true_agenda: faction.true_agenda,
            methods: faction.methods,
            leadership: faction.leadership,
            headquarters: faction.headquarters,
            sphere_of_influence: faction.sphere_of_influence,
            resources_assets: faction.resources_assets,
            allies: faction.allies,
            rivals_enemies: faction.rivals_enemies,
            reputation: faction.reputation,
            current_tension: faction.current_tension,
            goals_short_term: faction.goals_short_term,
            goals_long_term: faction.goals_long_term,
            symbol_description: faction.symbol_description,
            created_at: faction.created_at,
            updated_at: faction.updated_at,
        };

        let payload_json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let soft_delete_row = db::SoftDeleteRow {
            id: 0,
            entity_type: "faction".to_string(),
            entity_id: faction.id.clone(),
            name: faction.name.clone(),
            slug: faction.slug.clone(),
            original_vault_path: normalized_vault_path,
            trash_vault_path: trash_path.clone(),
            payload_json,
            created_at: now,
            undone_at: None,
        };
        soft_delete_repo
            .insert(database.as_ref(), &soft_delete_row)
            .await?;

        return Ok(SoftDeleteEntityResult {
            entity_type: EntityType::Faction,
            id: faction.id,
            name: faction.name,
            slug: faction.slug,
            trash_vault_path: trash_path,
        });
    }

    Err(format!("no npc, location, or faction found for: {target}"))
}

async fn undo_last_soft_delete(state: tauri::State<'_, AppState>) -> Result<UndoSoftDeleteResult, String> {
    let loaded = load_effective(&state.workspace_root).map_err(|err| err.to_string())?;
    validate_for_runtime(&loaded.effective).map_err(|err| err.to_string())?;
    let vault_path = loaded
        .effective
        .vault
        .path
        .clone()
        .ok_or_else(|| "vault.path is not configured".to_string())?;
    let vault = Vault::new(vault_path);
    state.vault_repo().ensure_structure(&vault)?;

    let database = state.database();
    let npc_repo = state.npc_repo();
    let location_repo = state.location_repo();
    let faction_repo = state.faction_repo();
    let document_repo = state.document_repo();
    let soft_delete_repo = state.soft_delete_repo();

    let Some(soft_delete) = soft_delete_repo
        .latest_pending(database.as_ref())
        .await?
    else {
        return Err("nothing to undo".to_string());
    };

    let now = now_timestamp();

    if soft_delete.entity_type == "npc" {
        let payload: NpcDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "npcs", &restored_slug);
            restored_vault_path = unique_markdown_path_for_name(&vault, "npcs", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let npc_row = db::NpcRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            race: payload.race,
            occupation: payload.occupation,
            sex: payload.sex,
            age: payload.age,
            height: payload.height,
            weight_lbs: payload.weight_lbs,
            background: payload.background,
            want_need: payload.want_need,
            secret_obstacle: payload.secret_obstacle,
            carrying: payload.carrying,
            location: payload.location,
            vault_path: restored_vault_path.clone(),
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        npc_repo
            .upsert(database.as_ref(), &npc_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "npc",
                &npc_row.slug,
                Some(&npc_row.name),
                &npc_row.vault_path,
                &npc_row.created_at,
                &npc_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Npc,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    if soft_delete.entity_type == "location" {
        let payload: LocationDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "locations", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "locations", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let location_row = db::LocationRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            kind_type: payload.kind_type,
            kind_custom: payload.kind_custom,
            visual_description: payload.visual_description,
            history_background: payload.history_background,
            exports: payload.exports,
            tone: payload.tone,
            authority: payload.authority,
            danger_level: payload.danger_level,
            current_tension: payload.current_tension,
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        location_repo
            .upsert(database.as_ref(), &location_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "location",
                &location_row.slug,
                Some(&location_row.name),
                &location_row.vault_path,
                &location_row.created_at,
                &location_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Location,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    if soft_delete.entity_type == "faction" {
        let payload: FactionDeletePayload =
            serde_json::from_str(&soft_delete.payload_json).map_err(|err| err.to_string())?;

        let mut restored_slug = payload.slug;
        let mut restored_vault_path = normalize_relative_path_for_storage(&payload.vault_path);
        let trash_vault_path = normalize_relative_path_for_storage(&soft_delete.trash_vault_path);
        let preferred_full = vault
            .resolve_relative(&PathBuf::from(&restored_vault_path))
            .map_err(|err| err.to_string())?;
        if preferred_full.exists() {
            restored_slug = unique_slug_for_dir(vault.root(), "factions", &restored_slug);
            restored_vault_path =
                unique_markdown_path_for_name(&vault, "factions", &payload.name, None)?;
        }

        move_vault_file(&vault, &trash_vault_path, &restored_vault_path)?;

        let faction_row = db::FactionRow {
            id: payload.id.clone(),
            slug: restored_slug.clone(),
            name: payload.name.clone(),
            vault_path: restored_vault_path.clone(),
            kind_type: payload.kind_type,
            kind_custom: payload.kind_custom,
            public_description: payload.public_description,
            true_agenda: payload.true_agenda,
            methods: payload.methods,
            leadership: payload.leadership,
            headquarters: payload.headquarters,
            sphere_of_influence: payload.sphere_of_influence,
            resources_assets: payload.resources_assets,
            allies: payload.allies,
            rivals_enemies: payload.rivals_enemies,
            reputation: payload.reputation,
            current_tension: payload.current_tension,
            goals_short_term: payload.goals_short_term,
            goals_long_term: payload.goals_long_term,
            symbol_description: payload.symbol_description,
            created_at: payload.created_at,
            updated_at: now.clone(),
        };

        faction_repo
            .upsert(database.as_ref(), &faction_row)
            .await?;
        document_repo
            .upsert_index(
                database.as_ref(),
                "faction",
                &faction_row.slug,
                Some(&faction_row.name),
                &faction_row.vault_path,
                &faction_row.created_at,
                &faction_row.updated_at,
            )
            .await?;

        soft_delete_repo
            .mark_undone(database.as_ref(), soft_delete.id, &now)
            .await?;

        return Ok(UndoSoftDeleteResult {
            entity_type: EntityType::Faction,
            id: payload.id,
            name: payload.name,
            slug: restored_slug,
            vault_path: restored_vault_path,
        });
    }

    Err(format!(
        "unsupported soft delete entity type: {}",
        soft_delete.entity_type
    ))
}

#[tauri::command]
fn get_command_manifest() -> CommandManifest {
    dnd_core::command_manifest::command_manifest()
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveReferenceQuery, VaultReferenceEntry, build_reference_suggestions_from_entries,
        extract_active_reference_query, extract_prompt_reference_keys, normalize_input_for_dispatch,
        npc_travel_location_query,
    };
    use crate::services::ai_generation::LocationSeed;
    use crate::utils::{normalize_location_seed, validate_location_details};

    #[test]
    fn dispatch_preserves_windows_backslashes() {
        let input = r"set vault C:\Users\andrewk9\Documents\DND";
        assert_eq!(normalize_input_for_dispatch(input), input);
    }

    #[test]
    fn dispatch_only_unwraps_markdown_backticks() {
        let input = "  `set vault C:\\Users\\andrewk9\\Documents\\DND`  ";
        assert_eq!(
            normalize_input_for_dispatch(input),
            r"set vault C:\Users\andrewk9\Documents\DND"
        );
    }

    #[test]
    fn location_seed_requires_custom_kind_for_other() {
        let seed = LocationSeed {
            name: "Gloomreach".to_string(),
            kind_type: "other".to_string(),
            kind_custom: None,
            visual_description: "Moss-slick walls drip in torchlight.".to_string(),
            history_background: "Built by exiles. Later seized by smugglers.".to_string(),
            exports: vec!["amber resin".to_string()],
            tone: "wet tense".to_string(),
            authority: "Smuggler council".to_string(),
            danger_level: "risky".to_string(),
            current_tension: "A rival gang stalks the tunnels.".to_string(),
        };

        let err = normalize_location_seed(seed).expect_err("expected missing kind_custom error");
        assert!(err.contains("kind_custom"));
    }

    #[test]
    fn location_seed_validation_accepts_unknown_backcompat_values() {
        let seed = LocationSeed {
            name: "Unknown Hold".to_string(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: vec!["Unknown".to_string()],
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
        };

        validate_location_details(&seed).expect("expected Unknown defaults to pass validation");
    }

    #[test]
    fn extracts_active_reference_query_from_tail() {
        let input = "create npc a duke for @locations/Aegis";
        let active = extract_active_reference_query(input).expect("expected active reference");
        assert_eq!(active.at_index, 22);
        assert_eq!(active.query, "locations/Aegis");
    }

    #[test]
    fn does_not_treat_email_as_reference_query() {
        let input = "create npc envoy named a@b";
        let active = extract_active_reference_query(input);
        assert!(active.is_none());
    }

    #[test]
    fn prompt_reference_matching_prefers_longest_entry() {
        let entries = vec![
            VaultReferenceEntry {
                key: "locations/Aegis".to_string(),
                key_lower: "locations/aegis".to_string(),
                markdown_path: Some("locations/Aegis.md".to_string()),
                is_dir: false,
            },
            VaultReferenceEntry {
                key: "locations/Aegis Isle".to_string(),
                key_lower: "locations/aegis isle".to_string(),
                markdown_path: Some("locations/Aegis Isle.md".to_string()),
                is_dir: false,
            },
        ];

        let found = extract_prompt_reference_keys(
            "create npc a duke for @locations/Aegis Isle during winter",
            &entries,
        );
        assert_eq!(found, vec!["locations/Aegis Isle"]);
    }

    #[test]
    fn prompt_reference_matching_supports_multiple_mentions() {
        let entries = vec![
            VaultReferenceEntry {
                key: "locations/Aegis Isle".to_string(),
                key_lower: "locations/aegis isle".to_string(),
                markdown_path: Some("locations/Aegis Isle.md".to_string()),
                is_dir: false,
            },
            VaultReferenceEntry {
                key: "npcs/Lady Aisling Everlynn".to_string(),
                key_lower: "npcs/lady aisling everlynn".to_string(),
                markdown_path: Some("npcs/Lady Aisling Everlynn.md".to_string()),
                is_dir: false,
            },
        ];

        let found = extract_prompt_reference_keys(
            "create npc sibling of @npcs/Lady Aisling Everlynn from @locations/Aegis Isle",
            &entries,
        );
        assert_eq!(
            found,
            vec![
                "npcs/Lady Aisling Everlynn".to_string(),
                "locations/Aegis Isle".to_string(),
            ]
        );
    }

    #[test]
    fn empty_reference_query_suggests_top_level_directories() {
        let entries = vec![
            VaultReferenceEntry {
                key: "locations/".to_string(),
                key_lower: "locations/".to_string(),
                markdown_path: None,
                is_dir: true,
            },
            VaultReferenceEntry {
                key: "npcs/".to_string(),
                key_lower: "npcs/".to_string(),
                markdown_path: None,
                is_dir: true,
            },
            VaultReferenceEntry {
                key: "locations/Aegis Isle".to_string(),
                key_lower: "locations/aegis isle".to_string(),
                markdown_path: Some("locations/Aegis Isle.md".to_string()),
                is_dir: false,
            },
        ];

        let active = ActiveReferenceQuery {
            at_index: 11,
            query: String::new(),
        };
        let suggestions = build_reference_suggestions_from_entries("create npc @", &active, &entries);
        let labels: Vec<String> = suggestions.into_iter().map(|item| item.label).collect();

        assert_eq!(labels, vec!["@locations/".to_string(), "@npcs/".to_string()]);
    }

    #[test]
    fn parses_npc_travel_location_query_for_typeahead() {
        assert_eq!(
            npc_travel_location_query("npc travel to Aegis Isle"),
            Some("Aegis Isle".to_string())
        );
        assert_eq!(npc_travel_location_query("npc travel to"), Some(String::new()));
        assert_eq!(npc_travel_location_query("npc travel"), None);
    }
}

fn main() {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let database = tauri::async_runtime::block_on(db::init_database())
        .expect("failed to initialize sqlite database");
    let database = Arc::new(database);

    let vault_repo: Arc<dyn VaultRepository> = Arc::new(ProdVaultRepository);
    let npc_repo: Arc<dyn NpcRepository> = Arc::new(ProdNpcRepository);
    let location_repo: Arc<dyn LocationRepository> = Arc::new(ProdLocationRepository);
    let faction_repo: Arc<dyn FactionRepository> = Arc::new(ProdFactionRepository);
    let document_repo: Arc<dyn DocumentRepository> = Arc::new(ProdDocumentRepository);
    let generation_repo: Arc<dyn GenerationRepository> = Arc::new(ProdGenerationRepository);
    let soft_delete_repo: Arc<dyn SoftDeleteRepository> = Arc::new(ProdSoftDeleteRepository);

    let command_service = dnd_core::service::CommandService::new(workspace_root.clone());

    let app_state = AppState {
        workspace_root,
        command_service: Mutex::new(command_service),
        editor_session: Mutex::new(EditorSession::default()),
        database: database.clone(),
        vault_repo: vault_repo.clone(),
        npc_repo: npc_repo.clone(),
        location_repo: location_repo.clone(),
        faction_repo: faction_repo.clone(),
        document_repo: document_repo.clone(),
        generation_repo: generation_repo.clone(),
        soft_delete_repo: soft_delete_repo.clone(),
    };

    let vault_sync_service = VaultSyncService;
    if let Err(err) = tauri::async_runtime::block_on(vault_sync_service.sync_from_vault(&app_state)) {
        eprintln!("startup vault sync skipped: {err}");
    }

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            run_command,
            suggest_command_input,
            get_command_manifest,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
