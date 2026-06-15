use dnd_core::command::CommandClientEvent;
use runebound_models::CommandResponse;

use crate::commands::ok_response;
use crate::entities::domain::EntityDomainResult;
use crate::entities::kind::EntityKind;
use crate::entities::schema::{FieldAccess, format_field_help};
use crate::utils::normalize_optional_prompt;

pub use crate::utils::{
    normalize_unknown_list,
    normalize_unknown_text,
    parse_list_csv,
};

pub type CommandResult = Result<Option<CommandResponse>, String>;

pub fn merge_seed_and_reroll_prompt(
    seed_prompt: &Option<String>,
    reroll_prompt: Option<String>,
) -> Option<String> {
    let seed_prompt = seed_prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let reroll_prompt = reroll_prompt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    match (seed_prompt, reroll_prompt) {
        (Some(seed), Some(reroll)) => Some(format!(
            "Seed context from original create command:\n{}\n\nReroll request:\n{}",
            seed, reroll
        )),
        (Some(seed), None) => Some(seed.to_string()),
        (None, Some(reroll)) => Some(reroll.to_string()),
        (None, None) => None,
    }
}

pub fn no_active_draft_message(kind: EntityKind) -> String {
    let root = kind.command_root();
    format!("no active {} draft. run create {} or load <name>.", root, root)
}

pub fn command_message_response(message: impl Into<String>) -> CommandResult {
    Ok(Some(ok_response(message.into(), None)))
}

pub fn command_response_with_event(
    message: impl Into<String>,
    event: CommandClientEvent,
) -> CommandResult {
    Ok(Some(ok_response(message.into(), Some(event))))
}

pub fn command_no_active_draft(kind: EntityKind) -> CommandResult {
    command_message_response(no_active_draft_message(kind))
}

/// Help for `<entity> set` listing the settable fields and their descriptions.
pub fn entity_set_field_help(kind: EntityKind) -> CommandResult {
    command_message_response(format_field_help(kind, FieldAccess::Set))
}

/// Help for `<entity> reroll` listing the rerollable fields and their descriptions.
pub fn entity_reroll_field_help(kind: EntityKind) -> CommandResult {
    command_message_response(format_field_help(kind, FieldAccess::Reroll))
}

pub fn entity_ok_response(
    message: impl Into<String>,
    event: Option<CommandClientEvent>,
) -> EntityDomainResult {
    Ok(Some(ok_response(message.into(), event)))
}

pub fn entity_message_response(message: impl Into<String>) -> EntityDomainResult {
    entity_ok_response(message, None)
}

pub fn entity_response_with_event(
    message: impl Into<String>,
    event: CommandClientEvent,
) -> EntityDomainResult {
    entity_ok_response(message, Some(event))
}

pub fn parse_reroll_field_and_prompt(
    trimmed: &str,
    prefix: &str,
    usage: &str,
) -> Result<(String, Option<String>), CommandResult> {
    let prefix_lower = prefix.to_ascii_lowercase();
    let trimmed_lower = trimmed.to_ascii_lowercase();

    if trimmed_lower == prefix_lower {
        return Err(command_message_response(usage));
    }

    let prefix_with_space = format!("{prefix_lower} ");
    if !trimmed_lower.starts_with(&prefix_with_space) {
        return Err(command_message_response(usage));
    }

    if trimmed.len() <= prefix.len() + 1 {
        return Err(command_message_response(usage));
    }

    let args = trimmed[prefix.len() + 1..].trim();
    if args.is_empty() {
        return Err(command_message_response(usage));
    }

    let mut split = args.splitn(2, char::is_whitespace);
    let field = split.next().unwrap_or_default().trim().to_string();
    if field.is_empty() {
        return Err(command_message_response(usage));
    }

    let prompt = normalize_optional_prompt(split.next().map(|value| value.to_string()));
    Ok((field, prompt))
}
