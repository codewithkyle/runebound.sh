//! `spell <name>` lookup + `spellbook import <path>` handlers.
//!
//! Lookup resolves a spell from the canonical TOML store (by slug, then by a DB
//! name search) and renders its card. Import reads the user's own local 5etools
//! data and (re)builds the library. A bare spell name typed with no command prefix
//! reaches [`resolve_spell_doc`] via the router's fallback.

use std::path::PathBuf;

use dnd_core::spell_import::slugify;
use dnd_core::spell_store::SpellStore;
use runebound_models::spells::spell_card;
use runebound_models::{
    CommandResponse, OutputDoc, StatusTone, command_ref, doc, heading, paragraph_text,
    paragraph_with_inlines, status, text_node,
};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::commands::{
    DesktopHandlerInvocation, command_action_response, ok_response, ok_response_with_doc,
};
use crate::services::spell_library::SpellLibraryService;

pub type CommandResult = Result<Option<CommandResponse>, String>;

// ---------------------------------------------------------------------------
// `spell <name>` — lookup + render
// ---------------------------------------------------------------------------

pub async fn handle_spell(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "spell" || lowered == "spell help" {
        return spell_help();
    }

    // Everything after the root token is a free-form spell name.
    let query = trimmed["spell".len()..].trim();
    match resolve_spell_doc(invocation.state.inner(), query).await? {
        Some(card) => Ok(Some(ok_response_with_doc(
            card.to_plain_text(),
            Some(card),
            None,
        ))),
        None => Ok(Some(spell_not_found(invocation.state.inner(), query).await)),
    }
}

/// Resolve a spell by name/slug and build its card, or `None` if no spell matches.
/// Shared by `spell <name>` and the router's bare-name fallback.
pub async fn resolve_spell_doc(state: &AppState, query: &str) -> Result<Option<OutputDoc>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }

    let store = SpellStore::new().map_err(|err| err.to_string())?;
    // Fast path: a typed name slugifies straight to the stored card.
    if let Some(spell) = store
        .load_spell(&slugify(query))
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(spell_card(&spell)));
    }
    // Otherwise recover the slug via a DB name search (handles partial/odd input).
    let rows = state
        .spell_repo()
        .search_by_name(state.database().as_ref(), query, 1)
        .await?;
    if let Some(row) = rows.into_iter().next()
        && let Some(spell) = store.load_spell(&row.slug).map_err(|err| err.to_string())?
    {
        return Ok(Some(spell_card(&spell)));
    }
    Ok(None)
}

async fn spell_not_found(state: &AppState, query: &str) -> CommandResponse {
    let count = state
        .spell_repo()
        .count(state.database().as_ref())
        .await
        .unwrap_or(0);
    if count == 0 {
        return command_action_response(
            "No spells imported yet. Run ",
            "spellbook import",
            " to build your spell library from a local 5etools copy.",
        );
    }
    ok_response(
        format!("No spell found for \"{query}\". Check the spelling or try a different name."),
        None,
    )
}

fn spell_help() -> CommandResult {
    let document = doc()
        .with_block(heading(2, "Spell Lookup"))
        .with_block(paragraph_text(
            "Type a spell name to render its card — with or without the `spell` prefix.",
        ))
        .with_block(paragraph_with_inlines(vec![
            text_node("Examples: "),
            command_ref("spell fireball", "spell fireball"),
            text_node(", or just "),
            command_ref("Fireball", "Fireball"),
            text_node("."),
        ]))
        .with_block(paragraph_with_inlines(vec![
            text_node("Run "),
            command_ref("spellbook import", "spellbook import"),
            text_node(" first to build your library from a local 5etools copy."),
        ]));
    Ok(Some(ok_response_with_doc(
        document.to_plain_text(),
        Some(document),
        None,
    )))
}

// ---------------------------------------------------------------------------
// `spellbook import <path>` — build the library
// ---------------------------------------------------------------------------

pub async fn handle_spellbook(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "spellbook" || lowered == "spellbook help" {
        return spellbook_help();
    }
    if lowered.starts_with("spellbook import") {
        return spellbook_import(trimmed, invocation).await;
    }
    Ok(Some(command_action_response(
        "unknown spellbook command. use ",
        "spellbook help",
        "",
    )))
}

async fn spellbook_import(
    trimmed: &str,
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let remainder = trimmed["spellbook import".len()..].trim();

    let path = if remainder.is_empty() {
        // No path argument → open the native folder picker.
        let picked = invocation
            .app_handle
            .dialog()
            .file()
            .set_title("Select your 5etools data folder")
            .blocking_pick_folder();
        match picked {
            Some(file_path) => {
                use tauri_plugin_dialog::FilePath;
                match file_path {
                    FilePath::Path(path) => path,
                    FilePath::Url(url) => PathBuf::from(url.as_str()),
                }
            }
            None => {
                return Ok(Some(ok_response(
                    "Spell import cancelled.".to_string(),
                    None,
                )));
            }
        }
    } else {
        let expanded =
            shellexpand::full(remainder).map_err(|err| format!("failed to expand path: {err}"))?;
        PathBuf::from(expanded.as_ref())
    };

    if !path.exists() {
        return Ok(Some(ok_response(
            format!("path not found: {}", path.display()),
            None,
        )));
    }

    let count = match SpellLibraryService
        .import_from_dir(&path, invocation.state.inner())
        .await
    {
        Ok(count) => count,
        Err(err) => {
            return Ok(Some(ok_response(
                format!("Spell import failed: {err}"),
                None,
            )));
        }
    };

    let message = format!("Imported {count} spells.");
    let document = doc().with_block(status(StatusTone::Success, message.clone()));
    Ok(Some(ok_response_with_doc(message, Some(document), None)))
}

fn spellbook_help() -> CommandResult {
    let document = doc()
        .with_block(heading(2, "Spellbook Import"))
        .with_block(paragraph_text(
            "Build the spell library using your own local copy of https://5e.tools/",
        ))
        .with_block(paragraph_with_inlines(vec![
            text_node("Usage: "),
            command_ref("spellbook import", "spellbook import"),
            text_node(" <path>, or "),
            command_ref("spellbook import", "spellbook import"),
            text_node(" with no path to pick a folder."),
        ]));
    Ok(Some(ok_response_with_doc(
        document.to_plain_text(),
        Some(document),
        None,
    )))
}
