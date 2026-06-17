//! The single generic wizard dispatch route. `try_execute_active_wizard` is the
//! one interceptor that replaces every bespoke per-flow interceptor: it reads the
//! active wizard, handles the global nav verbs (`cancel`/`back`), delegates to the
//! active step's `accept()`, applies the resulting `WizardTransition`, and renders
//! the next prompt (or runs `finalize`).

use runebound_models::CommandResponse;
use runebound_models::WizardView;
use runebound_models::output::{doc, paragraph_text};

use crate::app_state::AppState;
use crate::commands::ok_response_with_doc;
use crate::entities::common::{command_message_response_with_doc, CommandResult};

use super::prompt::doc_to_plain_text;
use super::session::{WizardData, WizardSession};
use super::wizard::{Wizard, WizardChoice, WizardStep, WizardTransition};

/// `create dungeon` and friends call this to launch a wizard and return its first
/// prompt. Resets any prior session.
pub async fn start_wizard(id: &'static str, state: &AppState) -> CommandResult {
    let Some(wizard) = state.wizards().get(id) else {
        return Err(format!("unknown wizard: {id}"));
    };
    let mut session = state.wizard_session.lock().await;
    *session = WizardSession {
        active_id: Some(id),
        cursor: 0,
        history: Vec::new(),
        data: Some(wizard.seed()),
    };
    let Some(step) = wizard.steps().first().cloned() else {
        *session = WizardSession::default();
        return Err(format!("wizard {id} has no steps"));
    };
    let data = session.data.as_ref().expect("seeded wizard data");
    Ok(Some(render_step(wizard.as_ref(), step.as_ref(), data)))
}

/// The active step's declared `choices()`, for autocomplete typeahead. Returns
/// empty when no wizard is active. Read-only snapshot — the suggestion service
/// calls this to offer the current step's tokens (`1`, `generate`, `reroll`, …),
/// the per-step counterpart to the manifest's generic nav verbs.
pub async fn active_step_choices(state: &AppState) -> Vec<WizardChoice> {
    let session = state.wizard_session.lock().await;
    let Some(id) = session.active_id else {
        return Vec::new();
    };
    let Some(wizard) = state.wizards().get(id) else {
        return Vec::new();
    };
    let Some(step) = wizard.steps().get(session.cursor) else {
        return Vec::new();
    };
    let Some(data) = session.data.as_ref() else {
        return Vec::new();
    };
    step.choices(data)
}

/// Intercepted before registry dispatch while a wizard is active. Returns
/// `Ok(None)` when no wizard is active (fall through to normal dispatch).
pub async fn try_execute_active_wizard(line: &str, state: &AppState) -> CommandResult {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();

    let mut session = state.wizard_session.lock().await;
    let Some(id) = session.active_id else {
        return Ok(None);
    };
    let Some(wizard) = state.wizards().get(id) else {
        // Defensive: active id with no registered wizard — drop the session.
        *session = WizardSession::default();
        return Ok(None);
    };

    // Global verb: cancel (the desktop `cancel` handler never runs mid-wizard,
    // same invariant as setup). Both `cancel` and `cancel <id>` exit.
    if lowered == "cancel" || lowered == format!("cancel {id}") {
        *session = WizardSession::default();
        return cancelled(wizard.as_ref());
    }

    // Global verb: back — pop to the previous step, keeping accumulated answers.
    if lowered == "back" {
        if let Some(prev) = session.history.pop() {
            session.cursor = prev;
        }
        return Ok(Some(render_current(wizard.as_ref(), &session)));
    }

    // Delegate to the active step.
    let cursor = session.cursor;
    let Some(step) = wizard.steps().get(cursor).cloned() else {
        *session = WizardSession::default();
        return Ok(None);
    };
    let transition = {
        let data = session.data.as_mut().expect("active wizard data");
        step.accept(trimmed, data, state).await?
    };

    match transition {
        WizardTransition::Stay => Ok(Some(render_current(wizard.as_ref(), &session))),
        WizardTransition::Next => {
            let current = session.cursor;
            session.history.push(current);
            session.cursor = current + 1;
            if session.cursor >= wizard.steps().len() {
                return complete(wizard.as_ref(), &mut session, state).await;
            }
            Ok(Some(render_current(wizard.as_ref(), &session)))
        }
        WizardTransition::Goto(target) => {
            let Some(idx) = wizard.steps().iter().position(|s| s.id() == target) else {
                return Err(format!("wizard {id}: unknown step '{target}'"));
            };
            let current = session.cursor;
            session.history.push(current);
            session.cursor = idx;
            Ok(Some(render_current(wizard.as_ref(), &session)))
        }
        WizardTransition::Back => {
            if let Some(prev) = session.history.pop() {
                session.cursor = prev;
            }
            Ok(Some(render_current(wizard.as_ref(), &session)))
        }
        WizardTransition::Complete => complete(wizard.as_ref(), &mut session, state).await,
        WizardTransition::Cancel => {
            *session = WizardSession::default();
            cancelled(wizard.as_ref())
        }
    }
}

/// Run the wizard's `finalize` on the accumulated data, then reset the session.
async fn complete(
    wizard: &dyn Wizard,
    session: &mut WizardSession,
    state: &AppState,
) -> CommandResult {
    let data = session.data.take().expect("active wizard data");
    let result = wizard.finalize(state, &data).await;
    *session = WizardSession::default();
    result
}

/// Render the step the cursor currently points at.
fn render_current(wizard: &dyn Wizard, session: &WizardSession) -> CommandResponse {
    let step = &wizard.steps()[session.cursor];
    let data = session.data.as_ref().expect("active wizard data");
    render_step(wizard, step.as_ref(), data)
}

/// Build a `CommandResponse` for a step prompt: the clickable `output_doc`, a
/// plain-text fallback, and the structured `WizardView` spinner signal.
fn render_step(wizard: &dyn Wizard, step: &dyn WizardStep, data: &WizardData) -> CommandResponse {
    let document = step.prompt(data);
    let text = doc_to_plain_text(&document);
    let mut response = ok_response_with_doc(text, Some(document), None);
    response.wizard = Some(WizardView {
        id: wizard.id().to_string(),
        step_id: step.id().to_string(),
        awaiting_llm_label: step.awaiting_llm_label().map(str::to_string),
    });
    response
}

fn cancelled(wizard: &dyn Wizard) -> CommandResult {
    let message = format!("{} cancelled.", wizard.title());
    command_message_response_with_doc(
        message.clone(),
        doc().with_block(paragraph_text(message)),
    )
}
