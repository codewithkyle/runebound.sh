//! The single generic wizard dispatch route. `try_execute_active_wizard` is the
//! one interceptor that replaces every bespoke per-flow interceptor: it reads the
//! active wizard, handles the global nav verbs (`cancel`/`back`/`help`), delegates
//! to the active step's `accept()`, applies the resulting `WizardTransition`, and
//! renders the next prompt (or runs `finalize`).
//!
//! Generic over a host `H: WizardHost` — the host owns the registry and the live
//! session, and is itself the context passed to `accept()`/`finalize()`. The
//! desktop app implements `WizardHost for AppState`; core/CLI will implement it
//! for its own onboarding context.

use tokio::sync::Mutex;

use runebound_models::output::{OutputDoc, doc, paragraph_text};
use runebound_models::{CommandResponse, OutputSegment, OutputSegmentKind, WizardView};

use crate::CommandResult;
use crate::prompt::doc_to_plain_text;
use crate::registry::WizardRegistry;
use crate::session::{WizardData, WizardSession};
use crate::wizard::{Wizard, WizardChoice, WizardStep, WizardTransition};

/// A host that can drive wizards: it owns the registry of registered wizards and
/// the live session, and is itself the context passed to steps' `accept()` and
/// `finalize()`. Implemented by the desktop `AppState` (and, in the onboarding
/// port, by a core-side context). `Self` is the wizard host type `H`.
pub trait WizardHost: Send + Sync + Sized {
    /// The registry of wizards available to this host.
    fn wizard_registry(&self) -> &WizardRegistry<Self>;
    /// The live wizard session (active id, cursor, history, accumulator).
    fn wizard_session(&self) -> &Mutex<WizardSession>;
}

/// `create dungeon` and friends call this to launch a wizard and return its first
/// prompt. Resets any prior session.
pub async fn start_wizard<H: WizardHost>(id: &'static str, host: &H) -> CommandResult {
    let Some(wizard) = host.wizard_registry().get(id) else {
        return Err(format!("unknown wizard: {id}"));
    };
    let mut session = host.wizard_session().lock().await;
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

/// The always-available wizard verbs, appended to every step's suggestions and
/// listed in `help`. `back` only when there is somewhere to go back to.
fn global_verbs(has_history: bool) -> Vec<WizardChoice> {
    let mut verbs = Vec::new();
    if has_history {
        verbs.push(WizardChoice::new("back", "back").with_help("Return to the previous step"));
    }
    verbs.push(WizardChoice::new("cancel", "cancel").with_help("Discard this wizard and exit"));
    verbs.push(WizardChoice::new("help", "help").with_help("Show the commands available here"));
    verbs
}

/// Dedupe choices by lowercased token, keeping the first occurrence (so a step's
/// own `cancel` wins over the global one).
fn dedupe_choices(choices: Vec<WizardChoice>) -> Vec<WizardChoice> {
    let mut seen = std::collections::HashSet::new();
    choices
        .into_iter()
        .filter(|choice| seen.insert(choice.token.to_ascii_lowercase()))
        .collect()
}

/// Input-aware typeahead for the active step: the step's `suggest()` (per-step
/// tokens + staged args like `set room <room> <type>`) plus the always-available
/// global verbs, prefix-filtered and deduped. Empty when no wizard is active.
/// The suggestion service calls this and maps the result to `CommandSuggestion`.
pub async fn active_step_suggestions<H: WizardHost>(host: &H, input: &str) -> Vec<WizardChoice> {
    let session = host.wizard_session().lock().await;
    let Some(id) = session.active_id else {
        return Vec::new();
    };
    let Some(wizard) = host.wizard_registry().get(id) else {
        return Vec::new();
    };
    let Some(step) = wizard.steps().get(session.cursor) else {
        return Vec::new();
    };
    let Some(data) = session.data.as_ref() else {
        return Vec::new();
    };
    let mut out = step.suggest(input, data);
    out.extend(crate::prompt::filter_choices(
        &global_verbs(!session.history.is_empty()),
        input,
    ));
    dedupe_choices(out)
}

/// Intercepted before registry dispatch while a wizard is active. Returns
/// `Ok(None)` when no wizard is active (fall through to normal dispatch).
pub async fn try_execute_active_wizard<H: WizardHost>(line: &str, host: &H) -> CommandResult {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();

    let mut session = host.wizard_session().lock().await;
    let Some(id) = session.active_id else {
        return Ok(None);
    };
    let Some(wizard) = host.wizard_registry().get(id) else {
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

    // Global verb: help — render the current step's commands without advancing.
    if lowered == "help" || lowered == format!("help {id}") {
        return Ok(Some(render_step_help(wizard.as_ref(), &session)));
    }

    // Delegate to the active step.
    let cursor = session.cursor;
    let Some(step) = wizard.steps().get(cursor).cloned() else {
        *session = WizardSession::default();
        return Ok(None);
    };
    let transition = {
        let data = session.data.as_mut().expect("active wizard data");
        step.accept(trimmed, data, host).await?
    };

    match transition {
        WizardTransition::Stay => Ok(Some(render_current(wizard.as_ref(), &session))),
        WizardTransition::Next => {
            let current = session.cursor;
            session.history.push(current);
            session.cursor = current + 1;
            if session.cursor >= wizard.steps().len() {
                return complete(wizard.as_ref(), &mut session, host).await;
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
        WizardTransition::Complete => complete(wizard.as_ref(), &mut session, host).await,
        WizardTransition::Cancel => {
            *session = WizardSession::default();
            cancelled(wizard.as_ref())
        }
    }
}

/// Run the wizard's `finalize` on the accumulated data, then reset the session.
async fn complete<H: WizardHost>(
    wizard: &dyn Wizard<H>,
    session: &mut WizardSession,
    host: &H,
) -> CommandResult {
    let data = session.data.take().expect("active wizard data");
    let result = wizard.finalize(host, &data).await;
    *session = WizardSession::default();
    result
}

/// Render the step the cursor currently points at.
fn render_current<H: Send + Sync>(wizard: &dyn Wizard<H>, session: &WizardSession) -> CommandResponse {
    let step = &wizard.steps()[session.cursor];
    let data = session.data.as_ref().expect("active wizard data");
    render_step(wizard, step.as_ref(), data)
}

/// Build a `CommandResponse` for a step prompt: the clickable `output_doc`, a
/// plain-text fallback, and the structured `WizardView` spinner signal.
fn render_step<H: Send + Sync>(
    wizard: &dyn Wizard<H>,
    step: &dyn WizardStep<H>,
    data: &WizardData,
) -> CommandResponse {
    let document = step.prompt(data);
    let text = doc_to_plain_text(&document);
    let mut response = ok_response_with_doc(text, document);
    response.wizard = Some(WizardView {
        id: wizard.id().to_string(),
        step_id: step.id().to_string(),
        awaiting_llm_label: step.awaiting_llm_label().map(str::to_string),
    });
    response
}

/// Render the current step's `help`: its summary plus a clickable, described
/// line per command (step choices + global verbs). Keeps the `WizardView` so the
/// context (and the next submission's spinner) stays intact.
fn render_step_help<H: Send + Sync>(
    wizard: &dyn Wizard<H>,
    session: &WizardSession,
) -> CommandResponse {
    let step = wizard.steps()[session.cursor].as_ref();
    let data = session.data.as_ref().expect("active wizard data");
    let mut commands = step.choices(data);
    commands.extend(global_verbs(!session.history.is_empty()));
    let commands = dedupe_choices(commands);
    let document = crate::prompt::step_help_doc(step.summary(), &commands);
    let text = doc_to_plain_text(&document);
    let mut response = ok_response_with_doc(text, document);
    response.wizard = Some(WizardView {
        id: wizard.id().to_string(),
        step_id: step.id().to_string(),
        awaiting_llm_label: step.awaiting_llm_label().map(str::to_string),
    });
    response
}

fn cancelled<H: Send + Sync>(wizard: &dyn Wizard<H>) -> CommandResult {
    let message = format!("{} cancelled.", wizard.title());
    Ok(Some(ok_response_with_doc(
        message.clone(),
        doc().with_block(paragraph_text(message)),
    )))
}

/// The engine's own `CommandResponse` builder: a successful response carrying a
/// structured `output_doc` and its plain-text fallback, with no client event
/// (steps never emit one — only a wizard's `finalize`, which is host code, does).
fn ok_response_with_doc(output: String, output_doc: OutputDoc) -> CommandResponse {
    CommandResponse {
        ok: true,
        output: output.clone(),
        error: None,
        exit_code: 0,
        segments: vec![OutputSegment {
            kind: OutputSegmentKind::Text,
            text: output,
            command_ref: None,
        }],
        output_doc: Some(output_doc),
        client_event: None,
        wizard: None,
    }
}
