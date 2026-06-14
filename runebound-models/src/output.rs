use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCardRow {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InlineNode {
    Text { text: String },
    CommandRef { label: String, command: String },
    Emphasis { text: String },
    Strong { text: String },
    Code { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusTone {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}