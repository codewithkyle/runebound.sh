//! Sanctioned builders for wizard step prompts. The point of routing every step
//! through these is that it is *impossible* to author a non-clickable choice: a
//! `WizardChoice` always renders as a `command_ref`, so clicking submits its
//! token. This closes the clickability regression by construction.
//!
//! Host-agnostic (no `AppState`).

use runebound_models::output::{
    InlineNode, OutputBlock, OutputDoc, command_ref, doc, heading, paragraph_text,
    paragraph_with_inlines, text_node,
};

use super::wizard::WizardChoice;

/// A single choice as a clickable inline ref: label shown, token submitted.
pub fn choice_ref(choice: &WizardChoice) -> InlineNode {
    command_ref(choice.label.clone(), choice.token.clone())
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
