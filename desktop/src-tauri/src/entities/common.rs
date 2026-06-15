use dnd_core::command::CommandClientEvent;

use crate::commands::ok_response;
use crate::entities::domain::EntityDomainResult;
use crate::entities::kind::EntityKind;

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

pub fn normalize_unknown_text(value: &str) -> String {
    dnd_core::npc::normalize_unknown_text(value)
}

pub fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    dnd_core::npc::normalize_unknown_list(values)
}

pub fn parse_list_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
