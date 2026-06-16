use std::collections::HashSet;

use dnd_core::command_manifest::{self, CommandManifest, CommandSpec};
use dnd_core::command_parse::{self, ParseResult, ParseStage};
use dnd_core::config::{Verbosity, load_effective};
use dnd_core::session::{OllamaStepState, OnboardingFlow, VaultStepState};
use dnd_core::vault::Vault;
use serde::Serialize;

use crate::app_state::AppState;
use crate::entities::{EntityKind, rerollable_fields, settable_fields};
use crate::services::entity_admin::EntityType;
use crate::services::vault_ref::{
    VaultReferenceEntry, can_start_reference_at, load_vault_reference_entries,
};
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

        let active_kind = {
            let editor = state.editor_session.lock().await;
            editor.active_kind()
        };
        let is_npc = matches!(active_kind, Some(EntityKind::Npc));

        // Resolve the current input context (entity editor takes precedence over an
        // in-progress setup wizard) and keep only commands the manifest declares
        // visible there. This replaces the former hard-coded per-kind blacklist.
        let context = match active_kind {
            Some(kind) => command_manifest::InputContext::EntityEditor(kind.as_str().to_string()),
            None => {
                let onboarding_active = {
                    let service = state.command_service.lock().await;
                    service.session().onboarding.active
                };
                if onboarding_active {
                    command_manifest::InputContext::ConfigEditor
                } else {
                    command_manifest::InputContext::Default
                }
            }
        };

        suggestions.retain(|suggestion| {
            let root = suggestion
                .completion
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            match find_command(&manifest, &root) {
                // Known command roots are gated by their declared availability.
                Some(command) => {
                    command_manifest::command_availability(&command.name).is_visible_in(&context)
                }
                // Non-command suggestions (entity-name search, etc.) are left alone.
                None => true,
            }
        });

        let trimmed = input.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let is_load_context = lowered == "load" || lowered.starts_with("load ");
        let is_delete_context = lowered == "delete" || lowered.starts_with("delete ");
        let is_show_context = lowered == "show" || lowered.starts_with("show ");
        let is_preview_context = lowered == "preview" || lowered.starts_with("preview ");
        let is_publish_help = lowered.starts_with("publish help");
        let is_publish_context = !is_publish_help
            && (lowered == "publish" || lowered.starts_with("publish "));
        let search_query = if is_load_context {
            trimmed[4..].trim()
        } else if is_delete_context {
            trimmed[6..].trim()
        } else if is_show_context {
            trimmed[4..].trim()
        } else if is_preview_context {
            trimmed[7..].trim()
        } else if is_publish_context {
            trimmed["publish".len()..].trim()
        } else {
            trimmed
        };

        if !search_query.is_empty()
            && (is_load_context
                || is_delete_context
                || is_show_context
                || is_preview_context
                || is_publish_context
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
            } else if is_publish_context {
                Some("publish")
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
                        EntityType::Item => SuggestionHelperText::Item,
                    }),
                });
            }
        }

        if is_npc {
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

        // `continue` is only meaningful in specific onboarding contexts (the
        // Ollama/vault menus and the LLM model step). Surface it for typeahead
        // there so it is discoverable, but never in normal command entry.
        let suggest_continue = {
            let service = state.command_service.lock().await;
            let onboarding = &service.session().onboarding;
            onboarding.active
                && (onboarding.ollama_substate == OllamaStepState::MenuShown
                    || (onboarding.vault_substate == VaultStepState::MenuShown
                        && !onboarding.vault_path.trim().is_empty())
                    || (onboarding.flow == OnboardingFlow::Llm && onboarding.step == 3))
        };
        if suggest_continue {
            let query = trimmed.to_ascii_lowercase();
            if !query.is_empty() && "continue".starts_with(&query) {
                suggestions.insert(
                    0,
                    CommandSuggestion {
                        label: "continue".to_string(),
                        completion: "continue".to_string(),
                        helper_text: Some(SuggestionHelperText::Command),
                    },
                );
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
pub enum SuggestionHelperText {
    Command,
    Npc,
    Location,
    Faction,
    Item,
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
    let item_repo = state.item_repo();

    let npcs = npc_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let locations = location_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let factions = faction_repo
        .search_by_name(database.as_ref(), trimmed, limit)
        .await?;
    let items = item_repo
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
        .chain(items.into_iter().map(|item| EntitySuggestion {
            entity_type: EntityType::Item,
            name: item.name,
            slug: item.slug,
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
        if let Some(root_name) = parsed.completion.root.as_deref() {
            if let Some(command) = find_command(manifest, root_name) {
                if command.requires_subcommand
                    && parsed
                        .completion
                        .current_token
                        .eq_ignore_ascii_case(root_name)
                {
                    let mut hydrated_input = input.to_string();
                    if !hydrated_input.ends_with(' ') {
                        hydrated_input.push(' ');
                    }
                    return build_subcommand_suggestions(
                        manifest,
                        parsed.completion.root.as_deref(),
                        &hydrated_input,
                        "",
                    );
                }
            }
        }

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
    let subcommand_name = subcommand.as_ref().map(|item| item.name.as_str());

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

    if command.name == "date" {
        if let Some(suggestions) = build_date_argument_suggestions(subcommand_name, parsed, input) {
            return suggestions;
        }
    }

    if command.name == "setup" {
        if let Some(suggestions) = build_setup_argument_suggestions(subcommand_name, parsed, input) {
            return suggestions;
        }
    }

    if let Some(kind) = entity_kind_for_root(command.name.as_str()) {
        if let Some(suggestions) = build_entity_field_argument_suggestions(
            kind,
            command,
            subcommand_name,
            parsed,
            input,
        ) {
            return suggestions;
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

fn build_entity_field_argument_suggestions(
    kind: EntityKind,
    command: &CommandSpec,
    subcommand: Option<&str>,
    parsed: &ParseResult,
    input: &str,
) -> Option<Vec<CommandSuggestion>> {
    let subcommand = subcommand?;
    if subcommand != "set" && subcommand != "reroll" {
        return None;
    }

    let args = &parsed.normalized_tokens[2..];
    let should_suggest_fields =
        args.is_empty() || (args.len() == 1 && !parsed.completion.ends_with_space);
    if !should_suggest_fields {
        return None;
    }

    let prefix = parsed.completion.current_token.to_ascii_lowercase();
    let base = replace_current_token(input, &parsed.completion.current_token);
    let field_names: Vec<&'static str> = if subcommand == "set" {
        settable_fields(kind).map(|spec| spec.display_name).collect()
    } else {
        rerollable_fields(kind)
            .map(|spec| spec.display_name)
            .collect()
    };

    let prefix_label = format!("{} {}", command.name, subcommand);
    Some(
        field_names
            .into_iter()
            .filter(|field| field.starts_with(&prefix))
            .map(|field| CommandSuggestion {
                label: format!("{prefix_label} {field}"),
                completion: format!("{base}{field} "),
                helper_text: Some(SuggestionHelperText::Command),
            })
            .collect(),
    )
}

fn entity_kind_for_root(root: &str) -> Option<EntityKind> {
    match root {
        "npc" => Some(EntityKind::Npc),
        "location" => Some(EntityKind::Location),
        "faction" => Some(EntityKind::Faction),
        "item" => Some(EntityKind::Item),
        _ => None,
    }
}

fn build_date_argument_suggestions(
    subcommand: Option<&str>,
    parsed: &ParseResult,
    input: &str,
) -> Option<Vec<CommandSuggestion>> {
    let subcommand = subcommand?;

    if subcommand != "set" {
        return None;
    }

    let normalized = &parsed.normalized_tokens;
    if normalized.len() < 2 {
        return None;
    }

    let typed_after_set = normalized.len().saturating_sub(2);
    let ends_with_space = parsed.completion.ends_with_space;
    let target_token_lower = normalized.get(2).map(|token| token.to_ascii_lowercase());
    let has_target_token = typed_after_set >= 1;
    let is_known_target = target_token_lower
        .as_deref()
        .is_some_and(|value| matches!(value, "year" | "month" | "day" | "time"));
    let selecting_component = !has_target_token || !is_known_target || (typed_after_set == 1 && !ends_with_space);

    if selecting_component {
        let base = base_for_date_component_selection(input, typed_after_set);
        let prefix = if has_target_token {
            target_token_lower.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let suggestions = build_date_component_suggestions(&base, &prefix);
        return Some(suggestions);
    }

    let target_name = target_token_lower.expect("expected target token");
    let mut base = replace_current_token(input, &parsed.completion.current_token);
    if !base.ends_with(' ') {
        base.push(' ');
    }
    let value_prefix = if ends_with_space {
        String::new()
    } else {
        parsed.completion.current_token.to_ascii_lowercase()
    };

    if target_name == "month" {
        if let Ok(Some(calendar)) = dnd_core::calendar::load_calendar() {
            return Some(
                calendar
                    .definition
                    .months
                    .iter()
                    .filter(|month| month.to_ascii_lowercase().starts_with(&value_prefix))
                    .map(|month| CommandSuggestion {
                        label: month.clone(),
                        completion: format!("{}{} ", base, month),
                        helper_text: Some(SuggestionHelperText::Command),
                    })
                    .collect(),
            );
        }
        return None;
    }

    if target_name == "year" || target_name == "day" || target_name == "time" {
        return Some(Vec::new());
    }

    None
}

/// Typeahead for `setup verbosity <brief|medium|verbose>`. Suggests the three
/// known levels (prefix-filtered) but the handler still accepts free text.
fn build_setup_argument_suggestions(
    subcommand: Option<&str>,
    parsed: &ParseResult,
    input: &str,
) -> Option<Vec<CommandSuggestion>> {
    if subcommand? != "verbosity" {
        return None;
    }

    let mut base = replace_current_token(input, &parsed.completion.current_token);
    if !base.ends_with(' ') {
        base.push(' ');
    }
    let value_prefix = if parsed.completion.ends_with_space {
        String::new()
    } else {
        parsed.completion.current_token.to_ascii_lowercase()
    };

    let suggestions = Verbosity::ALL
        .iter()
        .map(|level| level.as_str())
        .filter(|value| value.starts_with(&value_prefix))
        .map(|value| CommandSuggestion {
            label: format!("setup verbosity {value}"),
            completion: format!("{base}{value} "),
            helper_text: Some(SuggestionHelperText::Command),
        })
        .collect();
    Some(suggestions)
}

fn base_for_date_component_selection(input: &str, tokens_to_remove: usize) -> String {
    if tokens_to_remove == 0 {
        let mut base = input.to_string();
        if !base.ends_with(' ') {
            base.push(' ');
        }
        return base;
    }

    let mut trimmed = input.trim_end().to_string();
    for _ in 0..tokens_to_remove {
        trimmed = strip_last_token_segment(&trimmed);
    }
    if !trimmed.ends_with(' ') {
        trimmed.push(' ');
    }
    trimmed
}

fn strip_last_token_segment(value: &str) -> String {
    let trimmed = value.trim_end();
    match trimmed.rfind(char::is_whitespace) {
        Some(index) => trimmed[..index + 1].to_string(),
        None => String::new(),
    }
}

fn build_date_component_suggestions(base: &str, prefix: &str) -> Vec<CommandSuggestion> {
    const COMPONENTS: [&str; 4] = ["year", "month", "day", "time"];
    COMPONENTS
        .iter()
        .filter(|component| component.starts_with(prefix))
        .map(|component| CommandSuggestion {
            label: format!("date set {}", component),
            completion: format!("{}{} ", base, component),
            helper_text: Some(SuggestionHelperText::Command),
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

fn is_top_level_reference_key(key: &str, is_dir: bool) -> bool {
    if is_dir {
        let trimmed = key.trim_end_matches('/');
        !trimmed.is_empty() && !trimmed.contains('/')
    } else {
        !key.contains('/')
    }
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
#[cfg(test)]
mod tests {
    use super::{
        build_command_suggestions, build_entity_field_argument_suggestions,
        build_reference_suggestions_from_entries, entity_kind_for_root, extract_active_reference_query,
        find_command, npc_travel_location_query, ActiveReferenceQuery, VaultReferenceEntry,
    };
    use crate::entities::EntityKind;
    use crate::services::vault_ref::extract_prompt_reference_keys;
    use dnd_core::{command_manifest, command_parse};

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
    fn date_command_suggests_set_without_trailing_space() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("date");
        let suggestions = build_command_suggestions(&manifest, &parsed, "date");
        assert!(
            suggestions
                .iter()
                .any(|suggestion| suggestion.completion == "date set "),
            "missing date set suggestion"
        );
    }

    #[test]
    fn date_set_suggests_components_without_trailing_space() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("date set");
        let suggestions = build_command_suggestions(&manifest, &parsed, "date set");
        assert!(
            suggestions
                .iter()
                .any(|suggestion| suggestion.completion == "date set year "),
            "missing component suggestion"
        );
    }

    #[test]
    fn date_set_component_prefix_filters_results() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("date set y");
        let suggestions = build_command_suggestions(&manifest, &parsed, "date set y");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].completion, "date set year ");
    }

    #[test]
    fn setup_verbosity_suggests_all_three_levels() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("setup verbosity ");
        let suggestions = build_command_suggestions(&manifest, &parsed, "setup verbosity ");
        let completions: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.completion.as_str())
            .collect();
        assert!(completions.contains(&"setup verbosity brief "));
        assert!(completions.contains(&"setup verbosity medium "));
        assert!(completions.contains(&"setup verbosity verbose "));
    }

    #[test]
    fn setup_verbosity_prefix_filters_results() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("setup verbosity ver");
        let suggestions = build_command_suggestions(&manifest, &parsed, "setup verbosity ver");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].completion, "setup verbosity verbose ");
    }

    #[test]
    fn date_set_year_does_not_suggest_numeric_values() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("date set year ");
        let suggestions = build_command_suggestions(&manifest, &parsed, "date set year ");
        assert!(suggestions.is_empty(), "expected no year value suggestions");
    }

    #[test]
    fn date_set_day_does_not_suggest_numeric_values() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("date set day 12");
        let suggestions = build_command_suggestions(&manifest, &parsed, "date set day 12");
        assert!(suggestions.is_empty(), "expected no day value suggestions");
    }

    #[test]
    fn command_suggestions_handle_unknown_root() {
        let manifest = command_manifest::command_manifest();
        let parsed = command_parse::parse_command_input("unknown");
        let suggestions = build_command_suggestions(&manifest, &parsed, "unknown");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn entity_kind_for_item_root_maps_to_item_kind() {
        assert_eq!(entity_kind_for_root("item"), Some(EntityKind::Item));
    }

    #[test]
    fn item_set_field_suggestions_include_rarity() {
        let manifest = command_manifest::command_manifest();
        let command = find_command(&manifest, "item").expect("missing item command");
        let parsed = command_parse::parse_command_input("item set r");
        let suggestions = build_entity_field_argument_suggestions(
            EntityKind::Item,
            command,
            Some("set"),
            &parsed,
            "item set r",
        )
        .expect("expected suggestions");

        assert!(
            suggestions
                .iter()
                .any(|suggestion| suggestion.completion == "item set rarity "),
            "missing rarity suggestion"
        );
    }

    #[test]
    fn item_reroll_field_suggestions_include_materials() {
        let manifest = command_manifest::command_manifest();
        let command = find_command(&manifest, "item").expect("missing item command");
        let parsed = command_parse::parse_command_input("item reroll m");
        let suggestions = build_entity_field_argument_suggestions(
            EntityKind::Item,
            command,
            Some("reroll"),
            &parsed,
            "item reroll m",
        )
        .expect("expected suggestions");

        assert!(
            suggestions
                .iter()
                .any(|suggestion| suggestion.completion == "item reroll materials "),
            "missing materials suggestion"
        );
    }
}
