//! The wizard engine's core contracts: the `Wizard` and `WizardStep` traits plus
//! the value types the engine walks (`WizardChoice`, `WizardTransition`).
//!
//! A wizard is *declarative data*: an ordered list of `WizardStep`s the engine
//! drives. The engine owns dispatch, navigation (`continue`/`back`/`cancel`),
//! `command_ref` rendering, and the structured spinner signal; an author writes
//! only the per-step `prompt`/`choices`/`accept` and the wizard's `finalize`.
//!
//! `accept()` and `finalize()` are the *only* host-coupling points (they take
//! `&AppState`); everything else here is host-agnostic so the engine can later be
//! promoted to a shared crate (see docs/onboarding-wizard-port.md §3.1).

use std::sync::Arc;

use async_trait::async_trait;
use runebound_models::output::OutputDoc;

use crate::app_state::AppState;
use crate::entities::common::CommandResult;

use super::session::WizardData;

/// A clickable/typeable choice on a step: the visible `label` and the literal
/// `token` submitted when it is clicked or completed. `{label:"1: Tragedy",
/// token:"1"}` renders as `command_ref("1: Tragedy", "1")`.
#[derive(Debug, Clone)]
pub struct WizardChoice {
    pub label: String,
    pub token: String,
}

impl WizardChoice {
    pub fn new(label: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            token: token.into(),
        }
    }
}

/// Where the engine goes after a step's `accept()`. The full set is the engine's
/// navigation vocabulary; a given wizard need not exercise every variant (the
/// dungeon wizard drives forward + review loops, while `Back`/`Cancel` are
/// step-requested forms the runtime also handles and the onboarding port uses).
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
}

/// One declarative step in a wizard.
#[async_trait]
pub trait WizardStep: Send + Sync {
    /// Stable id, e.g. "tone", "plan_review".
    fn id(&self) -> &'static str;

    /// Build the step prompt. MUST emit `command_ref` for actionable tokens — use
    /// the `super::prompt` helpers so clickability is by construction.
    fn prompt(&self, data: &WizardData) -> OutputDoc;

    /// Choices that should autocomplete and be clickable for this step. Default:
    /// none (free-text steps).
    fn choices(&self, _data: &WizardData) -> Vec<WizardChoice> {
        Vec::new()
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
        state: &AppState,
    ) -> Result<WizardTransition, String>;
}

/// A registerable, multi-step wizard.
#[async_trait]
pub trait Wizard: Send + Sync {
    /// Stable id used as the `InputContext::Wizard` tag, e.g. "dungeon".
    fn id(&self) -> &'static str;

    /// Human title, e.g. "Create Dungeon" (used in cancel/reset messages).
    fn title(&self) -> &'static str;

    /// The ordered steps the engine walks.
    fn steps(&self) -> &[Arc<dyn WizardStep>];

    /// The initial accumulator when the wizard starts. (The onboarding spike will
    /// generalize this to `seed(ctx)` for config-seeded wizards.)
    fn seed(&self) -> WizardData;

    /// Called on the terminal step's `Complete`: build the artifact (open a draft,
    /// write config) and hand off. The engine resets the session afterward.
    async fn finalize(&self, state: &AppState, data: &WizardData) -> CommandResult;
}
