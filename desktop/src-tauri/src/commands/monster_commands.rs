//! `monster <name>` lookup + `bestiary import <path>` handlers.
//!
//! Lookup resolves a monster from the canonical TOML store (by slug, then by a DB
//! name search) and renders its stat-block card. Import reads the user's own local
//! 5etools data and (re)builds the library. A bare monster name typed with no
//! command prefix reaches [`resolve_monster_doc`] via the router's fallback (after
//! entities and spells).

use std::path::PathBuf;

use dnd_core::monster_store::MonsterStore;
use dnd_core::spell_import::slugify;
use runebound_models::monsters::monster_card;
use runebound_models::{
    CommandResponse, OutputDoc, StatusTone, command_ref, doc, heading, paragraph_text,
    paragraph_with_inlines, status, text_node,
};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::commands::{
    DesktopHandlerInvocation, command_action_response, ok_response, ok_response_with_doc,
};
use crate::services::bestiary_library::BestiaryLibraryService;

pub type CommandResult = Result<Option<CommandResponse>, String>;

// ---------------------------------------------------------------------------
// `monster <name>` — lookup + render
// ---------------------------------------------------------------------------

pub async fn handle_monster(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "monster" || lowered == "monster help" {
        return monster_help();
    }
    // Import lives under `bestiary`; nudge a stray `monster import` there.
    if lowered == "monster import" || lowered.starts_with("monster import ") {
        return Ok(Some(command_action_response(
            "Monster import lives under ",
            "bestiary import",
            ".",
        )));
    }

    // Everything after the root token is a free-form monster name.
    let query = trimmed["monster".len()..].trim();
    match resolve_monster_doc(invocation.state.inner(), query).await? {
        Some(card) => Ok(Some(ok_response_with_doc(
            card.to_plain_text(),
            Some(card),
            None,
        ))),
        None => Ok(Some(
            monster_not_found(invocation.state.inner(), query).await,
        )),
    }
}

/// Resolve a monster by name/slug and build its card, or `None` if none matches.
/// Shared by `monster <name>` and the router's bare-name fallback.
pub async fn resolve_monster_doc(
    state: &AppState,
    query: &str,
) -> Result<Option<OutputDoc>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }

    let store = MonsterStore::new().map_err(|err| err.to_string())?;
    // Fast path: a typed name slugifies straight to the stored card.
    if let Some(monster) = store
        .load_monster(&slugify(query))
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(monster_card(&monster)));
    }
    // Otherwise recover the slug via a DB name search (handles partial/odd input).
    let rows = state
        .monster_repo()
        .search_by_name(state.database().as_ref(), query, 1)
        .await?;
    if let Some(row) = rows.into_iter().next()
        && let Some(monster) = store
            .load_monster(&row.slug)
            .map_err(|err| err.to_string())?
    {
        return Ok(Some(monster_card(&monster)));
    }
    Ok(None)
}

async fn monster_not_found(state: &AppState, query: &str) -> CommandResponse {
    let count = state
        .monster_repo()
        .count(state.database().as_ref())
        .await
        .unwrap_or(0);
    if count == 0 {
        return command_action_response(
            "No monsters imported yet. Run ",
            "bestiary import",
            " to build your monster library from a local 5etools copy.",
        );
    }
    ok_response(
        format!("No monster found for \"{query}\". Check the spelling or try a different name."),
        None,
    )
}

fn monster_help() -> CommandResult {
    let document = doc()
        .with_block(heading(2, "Monster Lookup"))
        .with_block(paragraph_text(
            "Type a monster name to render its stat block — with or without the `monster` prefix.",
        ))
        .with_block(paragraph_with_inlines(vec![
            text_node("Examples: "),
            command_ref("monster goblin", "monster goblin"),
            text_node(", or just "),
            command_ref("Goblin Warrior", "Goblin Warrior"),
            text_node("."),
        ]))
        .with_block(paragraph_with_inlines(vec![
            text_node("Run "),
            command_ref("bestiary import", "bestiary import"),
            text_node(" first to build your library from a local 5etools copy."),
        ]));
    Ok(Some(ok_response_with_doc(
        document.to_plain_text(),
        Some(document),
        None,
    )))
}

// ---------------------------------------------------------------------------
// `bestiary import <path>` — build the library
// ---------------------------------------------------------------------------

pub async fn handle_bestiary(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "bestiary" || lowered == "bestiary help" {
        return bestiary_help();
    }
    if lowered.starts_with("bestiary import") {
        return bestiary_import(trimmed, invocation).await;
    }
    Ok(Some(command_action_response(
        "unknown bestiary command. use ",
        "bestiary help",
        "",
    )))
}

async fn bestiary_import(trimmed: &str, invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let remainder = trimmed["bestiary import".len()..].trim();

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
                    "Monster import cancelled.".to_string(),
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

    let summary = match BestiaryLibraryService
        .import_from_dir(&path, invocation.state.inner())
        .await
    {
        Ok(summary) => summary,
        Err(err) => {
            return Ok(Some(ok_response(
                format!("Monster import failed: {err}"),
                None,
            )));
        }
    };

    let count = summary.monsters.len();
    let headline = format!("Imported {count} monsters.");
    let mut document = doc().with_block(status(StatusTone::Success, headline.clone()));
    // Never silently drop the `_copy` variants — report the skipped count.
    if summary.skipped_copy > 0 {
        document = document.with_block(paragraph_text(format!(
            "Skipped {} variant monsters (derived stat blocks not yet supported).",
            summary.skipped_copy
        )));
    }
    document = document.with_block(paragraph_with_inlines(vec![
        text_node("Try "),
        command_ref("monster goblin", "monster goblin"),
        text_node("."),
    ]));
    Ok(Some(ok_response_with_doc(
        document.to_plain_text(),
        Some(document),
        None,
    )))
}

fn bestiary_help() -> CommandResult {
    let document = doc()
        .with_block(heading(2, "Bestiary Import"))
        .with_block(paragraph_text(
            "Build the monster library using your own local copy of https://5e.tools/",
        ))
        .with_block(paragraph_with_inlines(vec![
            text_node("Usage: "),
            command_ref("bestiary import", "bestiary import"),
            text_node(" <path>, or "),
            command_ref("bestiary import", "bestiary import"),
            text_node(" with no path to pick a folder."),
        ]));
    Ok(Some(ok_response_with_doc(
        document.to_plain_text(),
        Some(document),
        None,
    )))
}
