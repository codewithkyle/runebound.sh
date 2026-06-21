use command_handler::CommandHandler;
use dnd_core::command_manifest::command_manifest;
use runebound_models::{CommandResponse, OutputDoc};
use tauri::State;

use crate::app_state::AppState;
use crate::commands::entity_commands::build_load_response;
use crate::commands::monster_commands::resolve_monster_doc;
use crate::commands::spell_commands::resolve_spell_doc;
use crate::commands::{
    DesktopHandlerInvocation, desktop_handler_registry, ok_response, ok_response_with_doc,
};
use crate::services::entity_admin::EntityAdminService;
use crate::services::suggestions::starts_with_known_command_root;

pub(crate) async fn dispatch_desktop_command(
    input: &str,
    tokens: &[String],
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Option<CommandResponse>, String> {
    if tokens.is_empty() {
        return Ok(None);
    }

    let lowered: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();

    let registry = desktop_handler_registry();
    if let Some(entry) = registry.get(lowered[0].as_str()) {
        let invocation = DesktopHandlerInvocation {
            raw_input: input,
            tokens,
            lowered: &lowered,
            state: state.clone(),
            app_handle: app_handle.clone(),
        };
        return entry.execute(invocation).await;
    }

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let manifest = command_manifest();
    if !starts_with_known_command_root(trimmed, &manifest) {
        let admin = EntityAdminService;
        // A bare name with no command root is resolved in BARE_NAME_PRECEDENCE order
        // — first hit wins, so a saved entity beats a spell, and a spell beats a
        // monster, on a name collision. The order lives in one named list (pinned by
        // a test) so a reorder can't silently change which card a collision returns.
        for source in BARE_NAME_PRECEDENCE {
            match source {
                BareNameSource::Entity => {
                    if let Some(entity) = admin
                        .resolve_entity(trimmed.to_string(), state.inner())
                        .await?
                    {
                        let (output, event) = build_load_response(entity, state).await;
                        return Ok(Some(ok_response(output, event)));
                    }
                }
                BareNameSource::Spell => {
                    if let Some(card) = resolve_spell_doc(state.inner(), trimmed).await? {
                        return Ok(Some(card_response(card)));
                    }
                }
                BareNameSource::Monster => {
                    if let Some(card) = resolve_monster_doc(state.inner(), trimmed).await? {
                        return Ok(Some(card_response(card)));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// A reference card kind reachable by typing a bare name (no command root).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BareNameSource {
    Entity,
    Spell,
    Monster,
}

/// The bare-name fallback resolution order — first hit wins. Pinned by
/// `bare_name_precedence_is_entity_then_spell_then_monster`; reorder this list and
/// that test together, because the order decides which card a name collision (a
/// name that exists as more than one kind) resolves to.
const BARE_NAME_PRECEDENCE: [BareNameSource; 3] = [
    BareNameSource::Entity,
    BareNameSource::Spell,
    BareNameSource::Monster,
];

/// Wrap a resolved reference card (spell/monster) as a `CommandResponse`.
fn card_response(card: OutputDoc) -> CommandResponse {
    ok_response_with_doc(card.to_plain_text(), Some(card), None)
}

#[cfg(test)]
mod tests {
    use super::{BARE_NAME_PRECEDENCE, BareNameSource};

    #[test]
    fn bare_name_precedence_is_entity_then_spell_then_monster() {
        // Guard: a name that exists as more than one kind resolves entity → spell →
        // monster (spell wins over monster). A silent reorder of the router fallback
        // would change collision resolution; this fails if the order is swapped.
        assert_eq!(
            BARE_NAME_PRECEDENCE,
            [
                BareNameSource::Entity,
                BareNameSource::Spell,
                BareNameSource::Monster,
            ]
        );
    }
}
