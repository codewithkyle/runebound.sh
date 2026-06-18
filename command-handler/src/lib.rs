//! Host-agnostic command-handler infrastructure: the registry, the handler
//! traits, and the metadata a handler advertises. Deliberately standalone — it
//! depends on no manifest crate — so both the core CLI and the desktop app build
//! their own `HandlerRegistry` over it.
//!
//! ## Two traits, on purpose
//! [`HandlerBridge`] is the *minimal* thing a handler author writes: one
//! `invoke(invocation) -> future` method. [`CommandHandler`] is the *full* surface
//! the registry dispatches against — `handles()` (the name) + `metadata()` +
//! `execute()`. [`HandlerEntry`] is the adapter: it wraps a `HandlerBridge` with a
//! name and [`HandlerMetadata`] and `impl`s `CommandHandler` by delegating
//! `execute` → `invoke`. So handler code stays a bare closure/bridge, and the
//! name/metadata are supplied once at registration rather than threaded through
//! every handler.
//!
//! ## The `command-specs` bridge is not duplication
//! [`HandlerMetadata`] / [`ExecutionTarget`] live here (the generic layer); the
//! manifest single-source (`command-specs`) declares its own
//! `HandlerMetadataDescriptor` / `CommandExecution` and provides `From` impls that
//! convert *its* records into these types (`handler_metadata_for`). The two type
//! families mirror each other on purpose: this crate must not depend on the
//! manifest crate, so the manifest owns the bridge, not the other way around.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

pub type HandlerFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The full surface the [`HandlerRegistry`] dispatches against: a command's name,
/// its [`HandlerMetadata`], and its async `execute`. Most handlers don't implement
/// this directly — they implement [`HandlerBridge`] and let [`HandlerEntry`] supply
/// the name + metadata. See the module docs for the split.
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

/// The minimal per-handler logic: just `invoke`. A [`HandlerEntry`] wraps one of
/// these with a name + [`HandlerMetadata`] to form a full [`CommandHandler`]. See
/// the module docs for why the surface is split this way.
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

    /// Consume the registry, yielding its owned entries (insertion order is not
    /// preserved — it's a `HashMap`). Named `into_values` rather than `into_iter`
    /// so it doesn't shadow the `IntoIterator` convention (an inherent `into_iter`
    /// that isn't the trait method is a footgun in `for` loops).
    pub fn into_values(self) -> impl Iterator<Item = HandlerEntry<B>> {
        self.entries.into_values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
