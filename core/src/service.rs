use std::path::{Path, PathBuf};

use runebound_models::{OutputSegment, OutputSegmentKind};
use wizard::{WizardSession, start_wizard, try_execute_active_wizard};

use crate::command::{self, CommandResponse};
use crate::command_manifest::{CommandManifest, command_manifest};
use crate::command_parse::{ParseResult, normalize_command_input, parse_command_input};
use crate::onboarding_wizard::{CoreOnboardingCtx, onboarding_entry_wizard_id};
use crate::session::SessionState;

pub struct CommandService {
    workspace_root: PathBuf,
    session: SessionState,
    /// Live state of the active onboarding wizard. Held here (not in the
    /// serializable `SessionState`) so it persists across commands; moved into a
    /// short-lived `CoreOnboardingCtx` host for each dispatch and restored after.
    wizard_session: WizardSession,
}

impl CommandService {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            session: SessionState::default(),
            wizard_session: WizardSession::default(),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn session(&self) -> &SessionState {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut SessionState {
        &mut self.session
    }

    pub fn manifest(&self) -> CommandManifest {
        command_manifest()
    }

    pub fn parse_input(&self, input: &str) -> ParseResult {
        parse_command_input(input)
    }

    pub async fn execute_line(&mut self, input: &str) -> CommandResponse {
        let normalized = normalize_command_input(input);
        let trimmed = normalized.trim();

        // Onboarding wizard route, ahead of core dispatch: an active wizard
        // consumes the line; otherwise an entry command launches one. The host
        // owns the session for the duration of the call (moved in, restored
        // after); on the CLI the folder picker degrades via the default
        // `perform_native`.
        let active = self.wizard_session.active_id.is_some();
        let entry = onboarding_entry_wizard_id(trimmed);
        if active || entry.is_some() {
            if !trimmed.is_empty() {
                self.session.push_history(trimmed, 50);
            }
            let session = std::mem::take(&mut self.wizard_session);
            let ctx = CoreOnboardingCtx::new(self.workspace_root.clone(), session);
            let result = if active {
                try_execute_active_wizard(trimmed, &ctx).await
            } else {
                start_wizard(entry.expect("entry id"), &ctx).await
            };
            self.wizard_session = ctx.into_session();
            match result {
                Ok(Some(response)) => return response,
                // Not handled (e.g. active id with no registered wizard): fall through.
                Ok(None) => {}
                Err(err) => return error_response(err),
            }
        }

        command::execute_line_with_session(&self.workspace_root, input, &mut self.session).await
    }
}

/// Build a failed `CommandResponse` from an error message, matching the shape the
/// core dispatch path uses for errors.
fn error_response(message: String) -> CommandResponse {
    CommandResponse {
        ok: false,
        output: String::new(),
        error: Some(message.clone()),
        exit_code: 1,
        segments: vec![OutputSegment {
            kind: OutputSegmentKind::Error,
            text: message.clone(),
            command_ref: None,
        }],
        output_doc: Some(command::output_doc_from_error_text(message)),
        client_event: None,
        wizard: None,
    }
}

#[cfg(test)]
mod tests {
    //! The onboarding wizard route through `CommandService`. These drive only the
    //! entry/launch + cancel paths (no `save`), so they read effective config but
    //! never write it.
    use super::*;

    #[tokio::test]
    async fn start_setup_launches_the_onboarding_wizard() {
        let mut service = CommandService::new(std::env::temp_dir());
        let response = service.execute_line("start setup").await;
        assert!(response.ok, "start setup failed: {:?}", response.error);
        assert!(response.output.contains("Vault setup"));
        assert_eq!(
            response.wizard.as_ref().map(|w| w.id.as_str()),
            Some("setup")
        );
        assert_eq!(service.wizard_session.active_id, Some("setup"));
    }

    #[tokio::test]
    async fn setup_vault_launches_the_vault_subflow() {
        let mut service = CommandService::new(std::env::temp_dir());
        let response = service.execute_line("setup vault").await;
        assert_eq!(
            response.wizard.as_ref().map(|w| w.id.as_str()),
            Some("setup-vault")
        );
    }

    #[tokio::test]
    async fn cancel_exits_the_active_onboarding_wizard() {
        let mut service = CommandService::new(std::env::temp_dir());
        service.execute_line("start setup").await;
        let response = service.execute_line("cancel").await;
        assert!(response.output.to_lowercase().contains("cancel"));
        assert_eq!(service.wizard_session.active_id, None);
    }

    #[tokio::test]
    async fn invalid_menu_choice_keeps_the_wizard_active_and_reprompts() {
        let mut service = CommandService::new(std::env::temp_dir());
        service.execute_line("start setup").await;
        let response = service.execute_line("99").await;
        assert!(response.output.contains("invalid choice"));
        assert_eq!(service.wizard_session.active_id, Some("setup"));
    }
}
