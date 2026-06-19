use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
pub struct OutputDoc {
    pub blocks: Vec<OutputBlock>,
}

impl OutputDoc {
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    pub fn with_block(mut self, block: OutputBlock) -> Self {
        self.blocks.push(block);
        self
    }

    pub fn push(&mut self, block: OutputBlock) {
        self.blocks.push(block);
    }

    /// Render this doc to plain text — the fallback `output` string for hosts that
    /// don't render the structured doc, and the single source the help + entity-card
    /// prose derive from (so the two can't drift). Inline `command_ref`s degrade to
    /// their visible label, headings to `#`-prefixed lines by level, list items to
    /// `- `, and an entity card to its title + `label value` rows.
    pub fn to_plain_text(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for block in &self.blocks {
            match block {
                OutputBlock::Heading { level, text } => {
                    let hashes = "#".repeat((*level).clamp(1, 6) as usize);
                    lines.push(format!("{hashes} {text}"));
                }
                OutputBlock::Paragraph { inlines } => lines.push(inlines_to_text(inlines)),
                OutputBlock::Status { text, .. }
                | OutputBlock::Code { text, .. }
                | OutputBlock::Spinner { text, .. } => lines.push(text.clone()),
                OutputBlock::List { items } => {
                    for item in items {
                        lines.push(format!("- {}", inlines_to_text(item)));
                    }
                }
                OutputBlock::EntityCard { title, rows } => {
                    lines.push(format!("## {title}"));
                    for row in rows {
                        lines.push(format!("{} {}", row.label, row.value));
                    }
                }
                OutputBlock::Image { alt, .. } => lines.push(alt.clone()),
            }
        }
        lines.join("\n")
    }
}

/// Flatten inline nodes to plain text: every styled span yields its text and a
/// `command_ref` yields its visible label (the clickable target is doc-only).
fn inlines_to_text(inlines: &[InlineNode]) -> String {
    inlines
        .iter()
        .map(|node| match node {
            InlineNode::Text { text }
            | InlineNode::Emphasis { text }
            | InlineNode::Strong { text }
            | InlineNode::Code { text } => text.as_str(),
            InlineNode::CommandRef { label, .. } => label.as_str(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputBlock {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        inlines: Vec<InlineNode>,
    },
    List {
        items: Vec<Vec<InlineNode>>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Status {
        tone: StatusTone,
        text: String,
    },
    Spinner {
        state: SpinnerState,
        text: String,
    },
    EntityCard {
        title: String,
        rows: Vec<EntityCardRow>,
    },
    /// A bundled illustration. `src` is a logical asset key the frontend maps to
    /// an imported (hashed) asset URL — not a path — so the backend never needs to
    /// know where the build placed the file.
    Image {
        src: String,
        alt: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct EntityCardRow {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineNode {
    Text { text: String },
    CommandRef { label: String, command: String },
    Emphasis { text: String },
    Strong { text: String },
    Code { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StatusTone {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SpinnerState {
    Running,
    Success,
    Error,
}

pub fn doc() -> OutputDoc {
    OutputDoc::new()
}

pub fn heading(level: u8, text: impl Into<String>) -> OutputBlock {
    OutputBlock::Heading {
        level,
        text: text.into(),
    }
}

pub fn paragraph_text(text: impl Into<String>) -> OutputBlock {
    paragraph_with_inlines(vec![text_node(text)])
}

pub fn paragraph_with_inlines(inlines: Vec<InlineNode>) -> OutputBlock {
    OutputBlock::Paragraph { inlines }
}

pub fn list(items: Vec<Vec<InlineNode>>) -> OutputBlock {
    OutputBlock::List { items }
}

pub fn code_block(language: Option<impl Into<String>>, text: impl Into<String>) -> OutputBlock {
    OutputBlock::Code {
        language: language.map(Into::into),
        text: text.into(),
    }
}

pub fn status(tone: StatusTone, text: impl Into<String>) -> OutputBlock {
    OutputBlock::Status {
        tone,
        text: text.into(),
    }
}

pub fn spinner(state: SpinnerState, text: impl Into<String>) -> OutputBlock {
    OutputBlock::Spinner {
        state,
        text: text.into(),
    }
}

pub fn image(src: impl Into<String>, alt: impl Into<String>) -> OutputBlock {
    OutputBlock::Image {
        src: src.into(),
        alt: alt.into(),
    }
}

pub fn entity_card(title: impl Into<String>, rows: Vec<EntityCardRow>) -> OutputBlock {
    OutputBlock::EntityCard {
        title: title.into(),
        rows,
    }
}

pub fn entity_row(label: impl Into<String>, value: impl Into<String>) -> EntityCardRow {
    EntityCardRow {
        label: label.into(),
        value: value.into(),
    }
}

pub fn text_node(text: impl Into<String>) -> InlineNode {
    InlineNode::Text { text: text.into() }
}

pub fn command_ref(label: impl Into<String>, command: impl Into<String>) -> InlineNode {
    InlineNode::CommandRef {
        label: label.into(),
        command: command.into(),
    }
}

pub fn emphasis(text: impl Into<String>) -> InlineNode {
    InlineNode::Emphasis { text: text.into() }
}

pub fn strong(text: impl Into<String>) -> InlineNode {
    InlineNode::Strong { text: text.into() }
}

pub fn code(text: impl Into<String>) -> InlineNode {
    InlineNode::Code { text: text.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_output_doc() {
        let payload = doc()
            .with_block(heading(2, "System Status"))
            .with_block(paragraph_with_inlines(vec![
                text_node("Type "),
                command_ref("status", "status"),
                text_node(" to run checks."),
            ]))
            .with_block(status(StatusTone::Info, "ready"));

        let serialized = serde_json::to_string(&payload).expect("should serialize output doc");
        assert!(serialized.contains("system status") || serialized.contains("System Status"));
        assert!(serialized.contains("command_ref"));
        assert!(serialized.contains("status"));
    }

    #[test]
    fn to_plain_text_flattens_blocks_and_inlines() {
        // Headings honor their level, list items get `- `, and a command_ref
        // degrades to its visible label (the clickable target is doc-only).
        let payload = doc()
            .with_block(heading(2, "Commands"))
            .with_block(list(vec![vec![
                command_ref("status", "status"),
                text_node(" — Run checks"),
            ]]))
            .with_block(heading(3, "Examples"))
            .with_block(paragraph_text("Use `status help`."));

        assert_eq!(
            payload.to_plain_text(),
            "## Commands\n- status — Run checks\n### Examples\nUse `status help`."
        );
    }
}
