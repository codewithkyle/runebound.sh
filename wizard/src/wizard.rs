//! The wizard engine's core contracts: the `Wizard` and `WizardStep` traits plus
//! the value types the engine walks (`WizardChoice`, `WizardTransition`).
//!
//! A wizard is *declarative data*: an ordered list of `WizardStep`s the engine
//! drives. The engine owns dispatch, navigation (`continue`/`back`/`cancel`),
//! `command_ref` rendering, and the structured spinner signal; an author writes
//! only the per-step `prompt`/`choices`/`accept` and the wizard's `finalize`.
//!
//! Both traits are generic over a host type `H` (the host context — `AppState`
//! for the desktop app, an `OnboardingCtx` for core/CLI). `accept()` and
//! `finalize()` are the *only* host-coupling points (they take `&H`); everything
//! else here is host-agnostic.

use std::sync::Arc;

use async_trait::async_trait;
use runebound_models::output::OutputDoc;

use crate::CommandResult;
use crate::session::WizardData;

/// A clickable/typeable choice on a step: the visible `label`, the literal
/// `token` submitted when it is clicked or completed, and an optional one-line
/// `help` description shown in the step's `help`. `{label:"1: Tragedy",
/// token:"1"}` renders as `command_ref("1: Tragedy", "1")`.
#[derive(Debug, Clone)]
pub struct WizardChoice {
    pub label: String,
    pub token: String,
    pub help: Option<String>,
}

impl WizardChoice {
    pub fn new(label: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            token: token.into(),
            help: None,
        }
    }

    /// Attach a one-line description, shown next to the choice in `help`.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// Where the engine goes after a step's `accept()`. The full set is the engine's
/// navigation vocabulary; a given wizard need not exercise every variant (the
/// dungeon wizard drives forward + review loops, while `Back`/`Cancel`/`Native`
/// are step-requested forms the runtime also handles and the onboarding port uses).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum WizardTransition {
    /// Re-render the current step (invalid input, or a local edit like `reroll`).
    Stay,
    /// Advance to the next step in order. Off the end runs `finalize`.
    Next,
    /// Jump to a step by id (supports review loops).
    Goto(&'static str),
    /// Step backward to the previous step (restoring its accumulated answer).
    Back,
    /// Run `Wizard::finalize` and exit.
    Complete,
    /// Reset and exit.
    Cancel,
    /// Hand off to a host-native capability (e.g. a folder picker). The engine
    /// calls `WizardHost::perform_native`; on success it submits the result to
    /// the action's target step, on cancel it re-renders the requesting step.
    Native(NativeAction),
}

/// A host-native capability a step can request mid-flow that the host-agnostic
/// engine cannot perform itself. The engine routes it through
/// `WizardHost::perform_native`, then resumes the wizard with the result.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum NativeAction {
    /// Open a native folder picker. On success the chosen path is submitted to
    /// the step with id `resubmit_to` (as if the user had typed it there); on
    /// cancel the requesting step is re-rendered.
    PickFolder { resubmit_to: &'static str },
}

/// The result of a `WizardHost::perform_native` call. `Provided` carries the
/// value to feed back into the wizard; `Cancelled` (the default for a host
/// without the capability) re-renders the requesting step.
#[derive(Debug, Clone)]
pub enum NativeOutcome {
    Provided(String),
    Cancelled,
}

/// One declarative step in a wizard, generic over the host type `H`.
#[async_trait]
pub trait WizardStep<H: Send + Sync>: Send + Sync {
    /// Stable id, e.g. "tone", "plan_review".
    fn id(&self) -> &'static str;

    /// Build the step prompt. MUST emit `command_ref` for actionable tokens — use
    /// the `crate::prompt` helpers so clickability is by construction.
    fn prompt(&self, data: &WizardData) -> OutputDoc;

    /// One-line summary of what this step is for and what free-text/parameterized
    /// input it accepts, shown at the top of the step's `help`. Default: none.
    fn summary(&self) -> &'static str {
        ""
    }

    /// Choices that should autocomplete and be clickable for this step. Default:
    /// none (free-text steps). These are the simple single-token verbs; staged
    /// multi-token commands (`set room <room> <type>`) are produced by `suggest`.
    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        Vec::new()
    }

    /// Input-aware typeahead for this step. Default: the step's `choices()`
    /// prefix-filtered by the current input. Override to stage multi-token
    /// commands (e.g. suggest rooms after `set room `, then types). The global
    /// verbs (`back`/`cancel`/`help`) are appended by the runtime, not here.
    fn suggest(&self, input: &str, data: &WizardData) -> Vec<WizardChoice> {
        crate::prompt::filter_choices(&self.choices(data), input)
    }

    /// Spinner label to show when submitting from this step triggers an LLM call.
    /// `None` = instant.
    fn awaiting_llm_label(&self) -> Option<&'static str> {
        None
    }

    /// Validate + apply input, decide where to go next. May call services (LLM).
    /// Returning `Err` surfaces the error to the user and leaves the step active.
    async fn accept(
        &self,
        input: &str,
        data: &mut WizardData,
        host: &H,
    ) -> Result<WizardTransition, String>;
}

/// A registerable, multi-step wizard, generic over the host type `H`.
#[async_trait]
pub trait Wizard<H: Send + Sync>: Send + Sync {
    /// Stable id used as the `InputContext::Wizard` tag, e.g. "dungeon".
    fn id(&self) -> &'static str;

    /// Human title, e.g. "Create Dungeon" (used in cancel/reset messages).
    fn title(&self) -> &'static str;

    /// The ordered steps the engine walks.
    fn steps(&self) -> &[Arc<dyn WizardStep<H>>];

    /// The initial accumulator when the wizard starts. Receives the host context
    /// so config-seeded wizards (e.g. onboarding) can pre-fill from effective
    /// config; the dungeon wizard ignores it. Async + fallible so a wizard can do
    /// I/O on entry (e.g. probe a server) and refuse to start (`Err`) if a
    /// precondition fails.
    async fn seed(&self, host: &H) -> Result<WizardData, String>;

    /// Called on the terminal step's `Complete`: build the artifact (open a draft,
    /// write config) and hand off. The engine resets the session afterward.
    async fn finalize(&self, host: &H, data: &WizardData) -> CommandResult;
}
