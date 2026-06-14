use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTarget {
    Core,
    Desktop,
}

#[derive(Debug, Clone, Copy)]
pub struct HandlerMetadata {
    pub summary: &'static str,
    pub examples: &'static [&'static str],
    pub show_in_autocomplete: bool,
    pub requires_subcommand: bool,
    pub execution: ExecutionTarget,
    pub canonical_help: Option<&'static str>,
    pub aliases: &'static [&'static str],
}

impl HandlerMetadata {
    pub const fn new(summary: &'static str, execution: ExecutionTarget) -> Self {
        Self {
            summary,
            examples: &[],
            show_in_autocomplete: true,
            requires_subcommand: false,
            execution,
            canonical_help: None,
            aliases: &[],
        }
    }

    pub const fn with_examples(mut self, examples: &'static [&'static str]) -> Self {
        self.examples = examples;
        self
    }

    pub const fn with_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    pub const fn hide(mut self) -> Self {
        self.show_in_autocomplete = false;
        self
    }

    pub const fn requires_subcommand(mut self) -> Self {
        self.requires_subcommand = true;
        self
    }

    pub const fn with_canonical_help(mut self, canonical: &'static str) -> Self {
        self.canonical_help = Some(canonical);
        self
    }
}

#[derive(Debug, Clone)]
pub struct HandlerEntry<H> {
    pub name: &'static str,
    pub metadata: HandlerMetadata,
    pub handler: H,
}

impl<H> HandlerEntry<H> {
    pub const fn new(name: &'static str, metadata: HandlerMetadata, handler: H) -> Self {
        Self {
            name,
            metadata,
            handler,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct HandlerRegistry<H> {
    entries: HashMap<&'static str, HandlerEntry<H>>,
}

impl<H> HandlerRegistry<H> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn register(&mut self, entry: HandlerEntry<H>) -> Option<HandlerEntry<H>> {
        self.entries.insert(entry.name, entry)
    }

    pub fn get(&self, name: &str) -> Option<&HandlerEntry<H>> {
        self.entries.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut HandlerEntry<H>> {
        self.entries.get_mut(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &HandlerEntry<H>> {
        self.entries.values()
    }

    pub fn into_iter(self) -> impl Iterator<Item = HandlerEntry<H>> {
        self.entries.into_values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
