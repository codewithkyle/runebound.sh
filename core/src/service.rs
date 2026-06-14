use std::path::{Path, PathBuf};

use crate::command::{self, CommandResponse};
use crate::command_manifest::{CommandManifest, command_manifest};
use crate::command_parse::{ParseResult, parse_command_input};
use crate::session::SessionState;

pub struct CommandService {
    workspace_root: PathBuf,
    session: SessionState,
}

impl CommandService {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            session: SessionState::default(),
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
        command::execute_line_with_session(&self.workspace_root, input, &mut self.session).await
    }
}
