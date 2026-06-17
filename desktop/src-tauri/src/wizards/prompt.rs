//! Sanctioned builders for wizard step prompts. The point of routing every step
//! through these is that it is *impossible* to author a non-clickable choice: a
//! `WizardChoice` always renders as a `command_ref`, so clicking submits its
//! token. This closes the clickability regression by construction.
//!
//! Host-agnostic (no `AppState`).

use runebound_models::output::{
    InlineNode, OutputBlock, OutputDoc, command_ref, doc, heading, list, paragraph_text,
    paragraph_with_inlines, text_node,
};

use super::wizard::WizardChoice;

/// A single choice as a clickable inline ref: label shown, token submitted.
pub fn choice_ref(choice: &WizardChoice) -> InlineNode {
    command_ref(choice.label.clone(), choice.token.clone())
}

/// Prefix-filter choices by the typed input's lowercased token. The default
/// `WizardStep::suggest` and the runtime's global-verb pass both use this so
/// single-token typeahead behaves identically everywhere.
pub fn filter_choices(choices: &[WizardChoice], input: &str) -> Vec<WizardChoice> {
    let prefix = input.trim().to_ascii_lowercase();
    choices
        .iter()
        .filter(|choice| choice.token.to_ascii_lowercase().starts_with(&prefix))
        .cloned()
        .collect()
}

/// The `help` body for a step: the summary, then one clickable line per command
/// (step choices followed by the global verbs) with its description. Callers pass
/// an already-deduped command list.
pub fn step_help_doc(summary: &str, commands: &[WizardChoice]) -> OutputDoc {
    let mut document = doc().with_block(heading(3, "Commands available here"));
    if !summary.is_empty() {
        document = document.with_block(paragraph_text(summary.to_string()));
    }
    let entries: Vec<Vec<InlineNode>> = commands
        .iter()
        .map(|choice| {
            let mut line = vec![choice_ref(choice)];
            if let Some(help) = &choice.help {
                line.push(text_node(format!(" — {help}")));
            }
            line
        })
        .collect();
    document.with_block(list(entries))
}

/// A numbered/option menu: title, intro, then one clickable choice per line.
/// Renders `{label:"1: Tragedy", token:"1"}` as `command_ref("1: Tragedy", "1")`.
pub fn wizard_menu(title: &str, intro: &str, choices: &[WizardChoice]) -> OutputDoc {
    doc()
        .with_block(heading(2, title.to_string()))
        .with_block(paragraph_text(intro.to_string()))
        .with_block(choice_lines(choices))
}

/// A paragraph with each choice on its own line, all clickable. Used as the menu
/// body and as the action row on review screens.
pub fn choice_lines(choices: &[WizardChoice]) -> OutputBlock {
    let mut inlines: Vec<InlineNode> = Vec::new();
    for (i, choice) in choices.iter().enumerate() {
        if i > 0 {
            inlines.push(text_node("\n"));
        }
        inlines.push(choice_ref(choice));
    }
    paragraph_with_inlines(inlines)
}

/// An inline action row: choices joined by " · ", all clickable. Use for the
/// `continue · reroll · cancel` line on review screens.
pub fn action_row(choices: &[WizardChoice]) -> OutputBlock {
    let mut inlines: Vec<InlineNode> = Vec::new();
    for (i, choice) in choices.iter().enumerate() {
        if i > 0 {
            inlines.push(text_node(" · "));
        }
        inlines.push(choice_ref(choice));
    }
    paragraph_with_inlines(inlines)
}

/// Flatten a doc to plain text for the `output` fallback field (terminal history
/// and the no-`output_doc` render path). Keeps headings, renders command refs as
/// their label, and skips images.
pub fn doc_to_plain_text(document: &OutputDoc) -> String {
    let mut out = String::new();
    for block in &document.blocks {
        match block {
            OutputBlock::Heading { text, .. } => push_block(&mut out, text),
            OutputBlock::Paragraph { inlines } => push_block(&mut out, &inlines_text(inlines)),
            OutputBlock::List { items } => {
                for item in items {
                    out.push_str(&inlines_text(item));
                    out.push('\n');
                }
                out.push('\n');
            }
            OutputBlock::Code { text, .. } => push_block(&mut out, text),
            OutputBlock::Status { text, .. } => push_block(&mut out, text),
            OutputBlock::Spinner { text, .. } => push_block(&mut out, text),
            OutputBlock::EntityCard { title, .. } => push_block(&mut out, title),
            OutputBlock::Image { .. } => {}
        }
    }
    out.trim_end().to_string()
}

fn push_block(out: &mut String, text: &str) {
    out.push_str(text);
    out.push_str("\n\n");
}

fn inlines_text(inlines: &[InlineNode]) -> String {
    inlines
        .iter()
        .map(|node| match node {
            InlineNode::Text { text } => text.as_str(),
            InlineNode::CommandRef { label, .. } => label.as_str(),
            InlineNode::Emphasis { text } => text.as_str(),
            InlineNode::Strong { text } => text.as_str(),
            InlineNode::Code { text } => text.as_str(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_choices_matches_token_prefix_case_insensitively() {
        let choices = vec![
            WizardChoice::new("continue", "continue"),
            WizardChoice::new("cancel", "cancel"),
            WizardChoice::new("reroll", "reroll"),
        ];
        let tokens: Vec<String> = filter_choices(&choices, "C")
            .into_iter()
            .map(|choice| choice.token)
            .collect();
        assert_eq!(tokens, vec!["continue".to_string(), "cancel".to_string()]);
    }

    #[test]
    fn filter_choices_empty_prefix_keeps_all() {
        let choices = vec![WizardChoice::new("generate", "generate")];
        assert_eq!(filter_choices(&choices, "").len(), 1);
    }
}
