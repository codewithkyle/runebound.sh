use dnd_core::command::CommandClientEvent;
use runebound_models::CommandResponse;
use runebound_models::output::{
    InlineNode, OutputDoc, code, command_ref, doc, heading, list, paragraph_text,
    paragraph_with_inlines, strong, text_node,
};

use crate::commands::{ok_response, ok_response_with_doc};
use crate::entities::domain::EntityDomainResult;
use crate::entities::kind::EntityKind;
use crate::entities::schema::{
    FieldAccess, format_field_help, rerollable_fields, settable_fields,
};
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

/// A response carrying a structured `output_doc` plus its plain-text fallback.
pub fn command_message_response_with_doc(
    message: impl Into<String>,
    output_doc: OutputDoc,
) -> CommandResult {
    Ok(Some(ok_response_with_doc(message.into(), Some(output_doc), None)))
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
    command_message_response_with_doc(
        format_field_help(kind, FieldAccess::Set),
        field_help_doc(kind, FieldAccess::Set),
    )
}

/// Help for `<entity> reroll` listing the rerollable fields and their descriptions.
pub fn entity_reroll_field_help(kind: EntityKind) -> CommandResult {
    command_message_response_with_doc(
        format_field_help(kind, FieldAccess::Reroll),
        field_help_doc(kind, FieldAccess::Reroll),
    )
}

/// Structured field-help doc: usage line plus a described list of editable fields.
fn field_help_doc(kind: EntityKind, access: FieldAccess) -> OutputDoc {
    let root = kind.command_root();
    let (title, intro, usage, note) = match access {
        FieldAccess::Set => (
            format!("{root} set"),
            format!("Update a field on the active {root} draft."),
            format!("{root} set <field> <value>"),
            None,
        ),
        FieldAccess::Reroll => (
            format!("{root} reroll"),
            format!("Regenerate a field on the active {root} draft with the LLM."),
            format!("{root} reroll <field> [prompt]"),
            Some("The optional prompt may include @references to vault documents."),
        ),
    };

    let fields: Vec<_> = match access {
        FieldAccess::Set => settable_fields(kind).collect(),
        FieldAccess::Reroll => rerollable_fields(kind).collect(),
    };
    let items: Vec<Vec<InlineNode>> = fields
        .iter()
        .map(|spec| {
            let mut inlines = vec![
                strong(spec.display_name),
                text_node(format!(" — {}", spec.description)),
            ];
            let extra_aliases: Vec<&str> = spec
                .aliases
                .iter()
                .copied()
                .filter(|alias| *alias != spec.display_name)
                .collect();
            if !extra_aliases.is_empty() {
                inlines.push(text_node(format!(" (aliases: {})", extra_aliases.join(", "))));
            }
            inlines
        })
        .collect();

    let mut document = doc()
        .with_block(heading(2, title))
        .with_block(paragraph_text(intro))
        .with_block(paragraph_with_inlines(vec![text_node("Usage: "), code(usage)]));
    if let Some(note) = note {
        document = document.with_block(paragraph_text(note));
    }
    document
        .with_block(heading(3, "Fields"))
        .with_block(list(items))
}

/// Structured `<entity> help` overview: the domain's prose with bare commands made
/// clickable (placeholder forms stay as code), plus clickable field-help links.
pub fn entity_help_doc(kind: EntityKind, prose: &str) -> OutputDoc {
    let root = kind.command_root();
    let mut document = doc();
    let mut command_items: Vec<Vec<InlineNode>> = Vec::new();

    for line in prose.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(title) = line.strip_prefix("## ") {
            document = document.with_block(heading(2, title.to_string()));
            continue;
        }
        // Placeholder forms (`npc set <field> <value>`) can't be executed verbatim,
        // so render those as code; bare commands become clickable command refs.
        if line.contains('<') || line.contains('[') {
            command_items.push(vec![code(line.to_string())]);
        } else {
            command_items.push(vec![command_ref(line.to_string(), line.to_string())]);
        }
    }

    if !command_items.is_empty() {
        document = document.with_block(list(command_items));
    }

    document.with_block(paragraph_with_inlines(vec![
        text_node("Field help: "),
        command_ref(format!("{root} set help"), format!("{root} set help")),
        text_node(" · "),
        command_ref(format!("{root} reroll help"), format!("{root} reroll help")),
    ]))
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
