use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

pub type HandlerFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait CommandHandler: Send + Sync {
    type Output;
    type Invocation<'a>: 'a
    where
        Self: 'a;

    fn handles(&self) -> &'static str;
    fn metadata(&self) -> &HandlerMetadata;
    fn execute<'a>(&'a self, invocation: Self::Invocation<'a>) -> HandlerFuture<'a, Self::Output>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTarget {
    Core,
    Desktop,
}

#[derive(Debug, Clone)]
pub struct HandlerMetadata {
    pub summary: String,
    pub examples: Vec<String>,
    pub show_in_autocomplete: bool,
    pub requires_subcommand: bool,
    pub execution: ExecutionTarget,
    pub canonical_help: Option<String>,
    pub aliases: Vec<String>,
}

impl HandlerMetadata {
    pub fn new(summary: impl Into<String>, execution: ExecutionTarget) -> Self {
        Self {
            summary: summary.into(),
            examples: Vec::new(),
            show_in_autocomplete: true,
            requires_subcommand: false,
            execution,
            canonical_help: None,
            aliases: Vec::new(),
        }
    }

    pub fn with_examples<I, S>(mut self, examples: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.examples = examples
            .into_iter()
            .map(|item| item.as_ref().to_string())
            .collect();
        self
    }

    pub fn with_aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.aliases = aliases
            .into_iter()
            .map(|item| item.as_ref().to_string())
            .collect();
        self
    }

    pub fn hide(mut self) -> Self {
        self.show_in_autocomplete = false;
        self
    }

    pub fn requires_subcommand(mut self) -> Self {
        self.requires_subcommand = true;
        self
    }

    pub fn with_canonical_help(mut self, canonical: impl Into<String>) -> Self {
        self.canonical_help = Some(canonical.into());
        self
    }
}

pub trait HandlerBridge: Send + Sync {
    type Output;
    type Invocation<'a>: 'a
    where
        Self: 'a;

    fn invoke<'a>(&'a self, invocation: Self::Invocation<'a>) -> HandlerFuture<'a, Self::Output>;
}

pub struct HandlerEntry<B: HandlerBridge> {
    pub name: &'static str,
    pub metadata: HandlerMetadata,
    pub handler: B,
}

impl<B: HandlerBridge> HandlerEntry<B> {
    pub fn new(name: &'static str, metadata: HandlerMetadata, handler: B) -> Self {
        Self {
            name,
            metadata,
            handler,
        }
    }
}

impl<B: HandlerBridge> std::fmt::Debug for HandlerEntry<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerEntry")
            .field("name", &self.name)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl<B: HandlerBridge> CommandHandler for HandlerEntry<B> {
    type Output = B::Output;
    type Invocation<'a>
        = B::Invocation<'a>
    where
        Self: 'a;

    fn handles(&self) -> &'static str {
        self.name
    }

    fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    fn execute<'a>(&'a self, invocation: Self::Invocation<'a>) -> HandlerFuture<'a, Self::Output> {
        self.handler.invoke(invocation)
    }
}

#[derive(Debug)]
pub struct HandlerRegistry<B: HandlerBridge> {
    entries: HashMap<&'static str, HandlerEntry<B>>,
}

impl<B: HandlerBridge> Default for HandlerRegistry<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: HandlerBridge> HandlerRegistry<B> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn register(&mut self, entry: HandlerEntry<B>) -> Option<HandlerEntry<B>> {
        self.entries.insert(entry.name, entry)
    }

    pub fn get(&self, name: &str) -> Option<&HandlerEntry<B>> {
        self.entries.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut HandlerEntry<B>> {
        self.entries.get_mut(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &HandlerEntry<B>> {
        self.entries.values()
    }

    // P7.5 (cleanup-0.5.0): `into_iter` is intentionally an inherent method that
    // consumes the registry into owned entries; whether to rename it to
    // `into_values` or implement `IntoIterator` is the deliberate API decision
    // deferred to Phase 7. Remove this allow when P7.5 lands.
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter(self) -> impl Iterator<Item = HandlerEntry<B>> {
        self.entries.into_values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
