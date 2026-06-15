use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use dnd_core::command_manifest::{self, CommandManifest, CommandSpec};
use dnd_core::command_parse::{self, ParseResult, ParseStage};
use dnd_core::config::load_effective;
use dnd_core::vault::Vault;
use serde::Serialize;

use crate::app_state::{self, AppState};
use crate::services::entity_admin::EntityType;
use crate::utils::normalize_relative_path_for_storage;

pub struct SuggestionService;

impl SuggestionService {
    pub async fn build_suggestions(
        &self,
        input: String,
        state: &AppState,
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

        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input(&input);
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
            let entity_results = search_entities(state, search_query.to_string(), Some(6)).await?;
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
                let location_names = search_location_names(state, location_query, Some(8)).await?;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandSuggestion {
    pub label: String,
    pub completion: String,
    pub helper_text: Option<SuggestionHelperText>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionHelperText {
    Command,
    Npc,
    Location,
    Faction,
    Reference,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntitySuggestion {
    pub entity_type: EntityType,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone)]
struct ActiveReferenceQuery {
    at_index: usize,
    query: String,
}

#[derive(Debug, Clone)]
struct VaultReferenceEntry {
    key: String,
    key_lower: String,
    #[allow(dead_code)]
    markdown_path: Option<String>,
    is_dir: bool,
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
    let used: HashSet<String> = parsed
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
    if !command.subcommands.is_empty() || !command.options.is_empty() || command.requires_subcommand {
        " "
    } else {
        ""
    }
}

pub(crate) fn starts_with_known_command_root(input: &str, manifest: &CommandManifest) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{
        build_reference_suggestions_from_entries, extract_active_reference_query,
        extract_prompt_reference_keys, load_vault_reference_entries, npc_travel_location_query,
        ActiveReferenceQuery, CommandSuggestion, SuggestionHelperText, VaultReferenceEntry,
    };

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

    #[test]
    fn reference_suggestions_include_completion_suffix() {
        let entries = vec![VaultReferenceEntry {
            key: "locations/Aegis".to_string(),
            key_lower: "locations/aegis".to_string(),
            markdown_path: Some("locations/Aegis.md".to_string()),
            is_dir: false,
        }];
        let active = ActiveReferenceQuery {
            at_index: 5,
            query: "locations".to_string(),
        };
        let results = build_reference_suggestions_from_entries("test @", &active, &entries);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].completion, "test @locations/Aegis ");
    }

    #[test]
    fn build_command_suggestions_filters_duplicates() {
        let mut manifest = command_manifest::command_manifest();
        manifest.commands.retain(|cmd| cmd.name == "npc");
        let parsed = command_parse::parse_command_input("npc");
        let suggestions = build_command_suggestions(&manifest, &parsed, "npc");
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn command_suggestions_handle_unknown_root() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("unknown");
        let suggestions = build_command_suggestions(&manifest, &parsed, "unknown");
        assert!(suggestions.is_empty());
    }
}

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
