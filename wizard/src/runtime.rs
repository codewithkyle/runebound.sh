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
use crate::session::{ActiveWizard, WizardData, WizardSession};
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
    // Seed + resolve the first step *before* claiming the session, so a failure (a
    // probe error in `seed`, or a wizard with no steps) leaves any prior session
    // untouched. Rendering the prompt before the move also avoids re-borrowing the
    // session for the seeded data.
    let data = wizard.seed(host).await?;
    let Some(step) = wizard.steps().first().cloned() else {
        return Err(format!("wizard {id} has no steps"));
    };
    let response = render_step(wizard.as_ref(), step.as_ref(), &data);

    let mut session = host.wizard_session().lock().await;
    *session = WizardSession::Active(ActiveWizard {
        id,
        cursor: 0,
        history: Vec::new(),
        data,
    });
    Ok(Some(response))
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
    let WizardSession::Active(active) = &*session else {
        return Vec::new();
    };
    let Some(wizard) = host.wizard_registry().get(active.id) else {
        return Vec::new();
    };
    let Some(step) = wizard.steps().get(active.cursor) else {
        return Vec::new();
    };
    let mut out = step.suggest(input, &active.data);
    out.extend(crate::prompt::filter_choices(
        &global_verbs(!active.history.is_empty()),
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
    let Some(id) = session.active_id() else {
        return Ok(None);
    };
    let Some(wizard) = host.wizard_registry().get(id) else {
        // Defensive: active id with no registered wizard — drop the session.
        *session = WizardSession::Inactive;
        return Ok(None);
    };

    // Global verb: cancel (the desktop `cancel` handler never runs mid-wizard,
    // same invariant as setup). Both `cancel` and `cancel <id>` exit.
    if lowered == "cancel" || lowered == format!("cancel {id}") {
        *session = WizardSession::Inactive;
        return cancelled(wizard.as_ref());
    }

    // Global verb: back — pop to the previous step, keeping accumulated answers.
    if lowered == "back" {
        let WizardSession::Active(active) = &mut *session else {
            return Ok(None);
        };
        if let Some(prev) = active.history.pop() {
            active.cursor = prev;
        }
        return Ok(Some(render_current(wizard.as_ref(), active)));
    }

    // Global verb: help — render the current step's commands without advancing.
    if lowered == "help" || lowered == format!("help {id}") {
        let WizardSession::Active(active) = &*session else {
            return Ok(None);
        };
        return Ok(Some(render_step_help(wizard.as_ref(), active)));
    }

    // Delegate to the active step. Resolve the step from a copied cursor first so
    // the "step gone" reset doesn't fight the `&mut data` borrow taken below.
    let WizardSession::Active(active) = &*session else {
        return Ok(None);
    };
    let cursor = active.cursor;
    let Some(step) = wizard.steps().get(cursor).cloned() else {
        *session = WizardSession::Inactive;
        return Ok(None);
    };
    let WizardSession::Active(active) = &mut *session else {
        return Ok(None);
    };
    let transition = step.accept(trimmed, &mut active.data, host).await?;

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
            WizardTransition::Stay => {
                let WizardSession::Active(active) = &*session else {
                    return Ok(None);
                };
                return Ok(Some(render_current(wizard, active)));
            }
            WizardTransition::Next => {
                let WizardSession::Active(active) = &mut *session else {
                    return Ok(None);
                };
                active.history.push(active.cursor);
                active.cursor += 1;
                if active.cursor >= wizard.steps().len() {
                    return complete(wizard, session, host).await;
                }
                return Ok(Some(render_current(wizard, active)));
            }
            WizardTransition::Goto(target) => {
                let Some(idx) = wizard.steps().iter().position(|s| s.id() == target) else {
                    return Err(format!("wizard {}: unknown step '{target}'", wizard.id()));
                };
                let WizardSession::Active(active) = &mut *session else {
                    return Ok(None);
                };
                active.history.push(active.cursor);
                active.cursor = idx;
                return Ok(Some(render_current(wizard, active)));
            }
            WizardTransition::Back => {
                let WizardSession::Active(active) = &mut *session else {
                    return Ok(None);
                };
                if let Some(prev) = active.history.pop() {
                    active.cursor = prev;
                }
                return Ok(Some(render_current(wizard, active)));
            }
            WizardTransition::Complete => return complete(wizard, session, host).await,
            WizardTransition::Cancel => {
                *session = WizardSession::Inactive;
                return cancelled(wizard);
            }
            WizardTransition::Native(action) => {
                match host.perform_native(&action).await {
                    // No capability / user cancelled: re-render the requesting step.
                    NativeOutcome::Cancelled => {
                        let WizardSession::Active(active) = &*session else {
                            return Ok(None);
                        };
                        return Ok(Some(render_current(wizard, active)));
                    }
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
                        let step = wizard.steps()[idx].clone();
                        let WizardSession::Active(active) = &mut *session else {
                            return Ok(None);
                        };
                        active.history.push(active.cursor);
                        active.cursor = idx;
                        transition = step.accept(&value, &mut active.data, host).await?;
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
    // Deactivate and take ownership of the run in one move: the accumulator comes
    // out with it (no `Option::take` + `expect`), and the session is left inactive.
    let WizardSession::Active(active) = std::mem::take(session) else {
        return Ok(None);
    };
    wizard.finalize(host, &active.data).await
}

/// Render the step the cursor currently points at.
fn render_current<H: Send + Sync>(
    wizard: &dyn Wizard<H>,
    active: &ActiveWizard,
) -> CommandResponse {
    let step = &wizard.steps()[active.cursor];
    render_step(wizard, step.as_ref(), &active.data)
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
    active: &ActiveWizard,
) -> CommandResponse {
    let step = wizard.steps()[active.cursor].as_ref();
    let mut commands = step.choices(&active.data);
    commands.extend(global_verbs(!active.history.is_empty()));
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
            registry.register(Arc::new(StayWizard::new()));
            registry.register(Arc::new(BackWizard::new()));
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

    fn active(session: &WizardSession) -> &ActiveWizard {
        match session {
            WizardSession::Active(active) => active,
            WizardSession::Inactive => panic!("expected an active wizard session"),
        }
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
        let picked = data(&active(&session).data).picked.clone();
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
        assert!(host.session.lock().await.active_id().is_none());
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
        assert_eq!(session.active_id(), Some("t"));
        assert_eq!(active(&session).cursor, 0);
        // The seeded value is untouched; no path was resubmitted.
        assert_eq!(
            data(&active(&session).data).picked.as_deref(),
            Some("seeded")
        );
    }

    /// A resubmit target that records its input and *stays* (no advance) — so a
    /// Native resubmit into it exercises the Stay-after-Native + history path
    /// (P7.4), unlike `PathStep` which completes. Reuses `MenuStep`'s
    /// `resubmit_to: "path"` id so the menu's native action lands here.
    struct StayStep;
    #[async_trait]
    impl WizardStep<FakeHost> for StayStep {
        fn id(&self) -> &'static str {
            "path"
        }
        fn prompt(&self, _data: &WizardData) -> OutputDoc {
            doc().with_block(paragraph_text("stay"))
        }
        async fn accept(
            &self,
            input: &str,
            d: &mut WizardData,
            _host: &FakeHost,
        ) -> Result<WizardTransition, String> {
            d.downcast_mut::<TestData>().expect("test data").picked = Some(input.to_string());
            Ok(WizardTransition::Stay)
        }
    }

    /// `menu` then a staying `path`: `1` fires the picker (resubmit_to "path") and
    /// the provided value lands on `StayStep`, which records it and stays.
    struct StayWizard {
        steps: Vec<Arc<dyn WizardStep<FakeHost>>>,
    }
    impl StayWizard {
        fn new() -> Self {
            Self {
                steps: vec![Arc::new(MenuStep), Arc::new(StayStep)],
            }
        }
    }
    #[async_trait]
    impl Wizard<FakeHost> for StayWizard {
        fn id(&self) -> &'static str {
            "stays"
        }
        fn title(&self) -> &'static str {
            "StayW"
        }
        fn steps(&self) -> &[Arc<dyn WizardStep<FakeHost>>] {
            &self.steps
        }
        async fn seed(&self, _host: &FakeHost) -> Result<WizardData, String> {
            Ok(WizardData::new(TestData::default()))
        }
        async fn finalize(&self, _host: &FakeHost, _d: &WizardData) -> CommandResult {
            Ok(Some(ok_response_with_doc("done".to_string(), doc())))
        }
    }

    #[tokio::test]
    async fn native_resubmit_that_stays_lands_on_target_and_records_history() {
        let host = FakeHost::new(NativeOutcome::Provided("/picked".to_string()));
        start_wizard("stays", &host).await.expect("start");
        // `1` at the menu fires the picker; the host provides a path, resubmitted to
        // "path" (StayStep), which stores it and stays (no complete).
        let response = try_execute_active_wizard("1", &host)
            .await
            .expect("handled")
            .expect("response");
        assert_eq!(response.wizard.expect("wizard view").step_id, "path");
        let session = host.session.lock().await;
        // Still active, parked on the resubmit target, with the menu step in history.
        assert_eq!(session.active_id(), Some("stays"));
        assert_eq!(active(&session).cursor, 1);
        assert_eq!(active(&session).history, vec![0]);
        // The resubmitted value reached the staying step.
        assert_eq!(
            data(&active(&session).data).picked.as_deref(),
            Some("/picked")
        );
    }

    /// `a` then `b`: `a` records its input and advances (`Next`); `b` records and stays.
    /// Lets a test build history via `Next`, then `back`, to pin the engine's contract
    /// that `Back` restores the cursor but does *not* roll back the accumulator.
    struct AStep;
    #[async_trait]
    impl WizardStep<FakeHost> for AStep {
        fn id(&self) -> &'static str {
            "a"
        }
        fn prompt(&self, _data: &WizardData) -> OutputDoc {
            doc().with_block(paragraph_text("a"))
        }
        async fn accept(
            &self,
            input: &str,
            d: &mut WizardData,
            _host: &FakeHost,
        ) -> Result<WizardTransition, String> {
            d.downcast_mut::<TestData>().expect("test data").picked = Some(input.to_string());
            Ok(WizardTransition::Next)
        }
    }

    struct BStep;
    #[async_trait]
    impl WizardStep<FakeHost> for BStep {
        fn id(&self) -> &'static str {
            "b"
        }
        fn prompt(&self, _data: &WizardData) -> OutputDoc {
            doc().with_block(paragraph_text("b"))
        }
        async fn accept(
            &self,
            input: &str,
            d: &mut WizardData,
            _host: &FakeHost,
        ) -> Result<WizardTransition, String> {
            d.downcast_mut::<TestData>().expect("test data").picked = Some(input.to_string());
            Ok(WizardTransition::Stay)
        }
    }

    struct BackWizard {
        steps: Vec<Arc<dyn WizardStep<FakeHost>>>,
    }
    impl BackWizard {
        fn new() -> Self {
            Self {
                steps: vec![Arc::new(AStep), Arc::new(BStep)],
            }
        }
    }
    #[async_trait]
    impl Wizard<FakeHost> for BackWizard {
        fn id(&self) -> &'static str {
            "backw"
        }
        fn title(&self) -> &'static str {
            "BackW"
        }
        fn steps(&self) -> &[Arc<dyn WizardStep<FakeHost>>] {
            &self.steps
        }
        async fn seed(&self, _host: &FakeHost) -> Result<WizardData, String> {
            Ok(WizardData::new(TestData::default()))
        }
        async fn finalize(&self, _host: &FakeHost, _d: &WizardData) -> CommandResult {
            Ok(Some(ok_response_with_doc("done".to_string(), doc())))
        }
    }

    #[tokio::test]
    async fn back_restores_the_cursor_but_not_the_accumulator() {
        let host = FakeHost::new(NativeOutcome::Cancelled);
        start_wizard("backw", &host).await.expect("start");
        // Advance off step `a` (records "first", `Next` → cursor 1, history [0]).
        try_execute_active_wizard("first", &host)
            .await
            .expect("handled")
            .expect("response");
        {
            let session = host.session.lock().await;
            assert_eq!(active(&session).cursor, 1);
            assert_eq!(
                data(&active(&session).data).picked.as_deref(),
                Some("first")
            );
        }
        // `back` pops history to step `a`...
        try_execute_active_wizard("back", &host)
            .await
            .expect("handled")
            .expect("response");
        let session = host.session.lock().await;
        assert_eq!(active(&session).cursor, 0);
        assert!(active(&session).history.is_empty());
        // ...but the accumulator is NOT rolled back — the value recorded on `a` survives.
        // This documents the engine contract relied on by the wizards' reset-on-entry.
        assert_eq!(
            data(&active(&session).data).picked.as_deref(),
            Some("first")
        );
    }
}
