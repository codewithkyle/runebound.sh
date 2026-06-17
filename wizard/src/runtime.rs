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

use async_trait::async_trait;
use tokio::sync::Mutex;

use runebound_models::output::{OutputDoc, doc, paragraph_text};
use runebound_models::{CommandResponse, OutputSegment, OutputSegmentKind, WizardView};

use crate::CommandResult;
use crate::prompt::doc_to_plain_text;
use crate::registry::WizardRegistry;
use crate::session::{WizardData, WizardSession};
use crate::wizard::{
    NativeAction, NativeOutcome, Wizard, WizardChoice, WizardStep, WizardTransition,
};

/// A host that can drive wizards: it owns the registry of registered wizards and
/// the live session, and is itself the context passed to steps' `accept()` and
/// `finalize()`. Implemented by the desktop `AppState` (and, in the onboarding
/// port, by a core-side context). `Self` is the wizard host type `H`.
#[async_trait]
pub trait WizardHost: Send + Sync + Sized {
    /// The registry of wizards available to this host.
    fn wizard_registry(&self) -> &WizardRegistry<Self>;
    /// The live wizard session (active id, cursor, history, accumulator).
    fn wizard_session(&self) -> &Mutex<WizardSession>;
    /// Fulfill a step-requested native capability (e.g. a folder picker). The
    /// engine calls this when a step's `accept()` returns
    /// `WizardTransition::Native`, then resumes the wizard with the outcome.
    /// Hosts without the capability use the default (`Cancelled`), so the engine
    /// re-renders the requesting step — the headless/CLI degradation path.
    async fn perform_native(&self, _action: &NativeAction) -> NativeOutcome {
        NativeOutcome::Cancelled
    }
}

/// `create dungeon` and friends call this to launch a wizard and return its first
/// prompt. Resets any prior session.
pub async fn start_wizard<H: WizardHost>(id: &'static str, host: &H) -> CommandResult {
    let Some(wizard) = host.wizard_registry().get(id) else {
        return Err(format!("unknown wizard: {id}"));
    };
    // Seed before claiming the session so a failed seed (e.g. a probe error)
    // leaves any prior session untouched and surfaces the error.
    let data = wizard.seed(host).await?;
    let mut session = host.wizard_session().lock().await;
    *session = WizardSession {
        active_id: Some(id),
        cursor: 0,
        history: Vec::new(),
        data: Some(data),
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

    run_transition(host, &mut session, wizard.as_ref(), transition).await
}

/// Apply a `WizardTransition` to the live session and produce the response. Runs
/// as a loop so a `Native` action — once the host fulfills it and we re-submit
/// the result to the target step — can drive the *next* transition without
/// recursion or releasing the session lock.
async fn run_transition<H: WizardHost>(
    host: &H,
    session: &mut WizardSession,
    wizard: &dyn Wizard<H>,
    mut transition: WizardTransition,
) -> CommandResult {
    loop {
        match transition {
            WizardTransition::Stay => return Ok(Some(render_current(wizard, session))),
            WizardTransition::Next => {
                let current = session.cursor;
                session.history.push(current);
                session.cursor = current + 1;
                if session.cursor >= wizard.steps().len() {
                    return complete(wizard, session, host).await;
                }
                return Ok(Some(render_current(wizard, session)));
            }
            WizardTransition::Goto(target) => {
                let Some(idx) = wizard.steps().iter().position(|s| s.id() == target) else {
                    return Err(format!("wizard {}: unknown step '{target}'", wizard.id()));
                };
                let current = session.cursor;
                session.history.push(current);
                session.cursor = idx;
                return Ok(Some(render_current(wizard, session)));
            }
            WizardTransition::Back => {
                if let Some(prev) = session.history.pop() {
                    session.cursor = prev;
                }
                return Ok(Some(render_current(wizard, session)));
            }
            WizardTransition::Complete => return complete(wizard, session, host).await,
            WizardTransition::Cancel => {
                *session = WizardSession::default();
                return cancelled(wizard);
            }
            WizardTransition::Native(action) => {
                match host.perform_native(&action).await {
                    // No capability / user cancelled: re-render the requesting step.
                    NativeOutcome::Cancelled => return Ok(Some(render_current(wizard, session))),
                    // Feed the produced value to the action's target step, as if
                    // the user had typed it there, and apply its transition next.
                    NativeOutcome::Provided(value) => {
                        let NativeAction::PickFolder { resubmit_to } = action;
                        let Some(idx) = wizard.steps().iter().position(|s| s.id() == resubmit_to)
                        else {
                            return Err(format!(
                                "wizard {}: native resubmit to unknown step '{resubmit_to}'",
                                wizard.id()
                            ));
                        };
                        let current = session.cursor;
                        session.history.push(current);
                        session.cursor = idx;
                        let step = wizard.steps()[idx].clone();
                        let data = session.data.as_mut().expect("active wizard data");
                        transition = step.accept(&value, data, host).await?;
                    }
                }
            }
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
fn render_current<H: Send + Sync>(
    wizard: &dyn Wizard<H>,
    session: &WizardSession,
) -> CommandResponse {
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

#[cfg(test)]
mod tests {
    //! Exercises the engine's host-coupling contracts against a fake host: the
    //! native-action round-trip (fulfilled + degraded) and the `seed(host)` hook.
    use std::sync::Arc;

    use async_trait::async_trait;
    use runebound_models::output::{doc, paragraph_text};

    use super::*;
    use crate::session::WizardData;
    use crate::wizard::{Wizard, WizardStep};

    /// The fake host's seed value + the canned outcome its `perform_native`
    /// returns, so a test can drive both the fulfilled and degraded paths.
    struct FakeHost {
        registry: WizardRegistry<FakeHost>,
        session: Mutex<WizardSession>,
        seed_path: String,
        native: NativeOutcome,
    }

    impl FakeHost {
        fn new(native: NativeOutcome) -> Self {
            let mut registry = WizardRegistry::new();
            registry.register(Arc::new(TestWizard::new()));
            Self {
                registry,
                session: Mutex::new(WizardSession::default()),
                seed_path: "seeded".to_string(),
                native,
            }
        }
    }

    #[async_trait]
    impl WizardHost for FakeHost {
        fn wizard_registry(&self) -> &WizardRegistry<FakeHost> {
            &self.registry
        }
        fn wizard_session(&self) -> &Mutex<WizardSession> {
            &self.session
        }
        async fn perform_native(&self, _action: &NativeAction) -> NativeOutcome {
            self.native.clone()
        }
    }

    #[derive(Default)]
    struct TestData {
        picked: Option<String>,
    }

    fn data(d: &WizardData) -> &TestData {
        d.downcast_ref::<TestData>().expect("test data")
    }

    /// "menu": `1` requests the native picker (resubmit to "path"); anything else
    /// stays.
    struct MenuStep;
    #[async_trait]
    impl WizardStep<FakeHost> for MenuStep {
        fn id(&self) -> &'static str {
            "menu"
        }
        fn prompt(&self, _data: &WizardData) -> OutputDoc {
            doc().with_block(paragraph_text("menu"))
        }
        async fn accept(
            &self,
            input: &str,
            _data: &mut WizardData,
            _host: &FakeHost,
        ) -> Result<WizardTransition, String> {
            if input == "1" {
                Ok(WizardTransition::Native(NativeAction::PickFolder {
                    resubmit_to: "path",
                }))
            } else {
                Ok(WizardTransition::Stay)
            }
        }
    }

    /// "path": stores the submitted value and completes.
    struct PathStep;
    #[async_trait]
    impl WizardStep<FakeHost> for PathStep {
        fn id(&self) -> &'static str {
            "path"
        }
        fn prompt(&self, _data: &WizardData) -> OutputDoc {
            doc().with_block(paragraph_text("path"))
        }
        async fn accept(
            &self,
            input: &str,
            d: &mut WizardData,
            _host: &FakeHost,
        ) -> Result<WizardTransition, String> {
            d.downcast_mut::<TestData>().expect("test data").picked = Some(input.to_string());
            Ok(WizardTransition::Complete)
        }
    }

    struct TestWizard {
        steps: Vec<Arc<dyn WizardStep<FakeHost>>>,
    }
    impl TestWizard {
        fn new() -> Self {
            Self {
                steps: vec![Arc::new(MenuStep), Arc::new(PathStep)],
            }
        }
    }
    #[async_trait]
    impl Wizard<FakeHost> for TestWizard {
        fn id(&self) -> &'static str {
            "t"
        }
        fn title(&self) -> &'static str {
            "Test"
        }
        fn steps(&self) -> &[Arc<dyn WizardStep<FakeHost>>] {
            &self.steps
        }
        async fn seed(&self, host: &FakeHost) -> Result<WizardData, String> {
            // Exercise the seed(host) hook: pull a value from the host context.
            Ok(WizardData::new(TestData {
                picked: Some(host.seed_path.clone()),
            }))
        }
        async fn finalize(&self, _host: &FakeHost, d: &WizardData) -> CommandResult {
            let picked = data(d).picked.clone().unwrap_or_default();
            Ok(Some(ok_response_with_doc(picked, doc())))
        }
    }

    #[tokio::test]
    async fn seed_hook_receives_the_host_context() {
        let host = FakeHost::new(NativeOutcome::Cancelled);
        start_wizard("t", &host)
            .await
            .expect("start")
            .expect("prompt");
        let session = host.session.lock().await;
        let picked = data(session.data.as_ref().unwrap()).picked.clone();
        assert_eq!(picked.as_deref(), Some("seeded"));
    }

    #[tokio::test]
    async fn native_fulfilled_resubmits_value_to_target_step_and_completes() {
        let host = FakeHost::new(NativeOutcome::Provided("/vault".to_string()));
        start_wizard("t", &host).await.expect("start");
        // `1` at the menu triggers the native picker; the fake host provides a
        // path, which is resubmitted to "path", which stores it and completes.
        let response = try_execute_active_wizard("1", &host)
            .await
            .expect("handled")
            .expect("response");
        assert_eq!(response.output, "/vault");
        // Completing resets the session.
        assert!(host.session.lock().await.active_id.is_none());
    }

    #[tokio::test]
    async fn native_degraded_reprompts_requesting_step_without_advancing() {
        let host = FakeHost::new(NativeOutcome::Cancelled);
        start_wizard("t", &host).await.expect("start");
        let response = try_execute_active_wizard("1", &host)
            .await
            .expect("handled")
            .expect("response");
        // Cancelled native action re-renders the requesting "menu" step.
        assert_eq!(response.wizard.expect("wizard view").step_id, "menu");
        let session = host.session.lock().await;
        assert_eq!(session.active_id, Some("t"));
        assert_eq!(session.cursor, 0);
        // The seeded value is untouched; no path was resubmitted.
        assert_eq!(
            data(session.data.as_ref().unwrap()).picked.as_deref(),
            Some("seeded")
        );
    }
}
