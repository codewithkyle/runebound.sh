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
    CommandResponse, InlineNode, OutputDoc, StatusTone, command_ref, doc, heading, list,
    paragraph_text, paragraph_with_inlines, status, text_node,
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

    // Everything after the root token is a free-form monster name — unless it
    // carries `--cr` / `--type` filters, which switch to a clickable results list.
    let query = trimmed["monster".len()..].trim();
    match parse_monster_filters(query) {
        Ok(Some(filters)) => return monster_filtered(invocation.state.inner(), &filters).await,
        Err(message) => return Ok(Some(ok_response(message, None))),
        Ok(None) => {}
    }
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

/// Parsed `monster --cr <range> --type <kind> [name]` filters. `None` CR bounds use
/// the open sentinels (`0.0` / `f64::MAX`); `summary` is the human-readable recap.
struct MonsterFilters {
    name: Option<String>,
    creature_type: Option<String>,
    cr_min: f64,
    cr_max: f64,
    summary: String,
}

/// Parse filter flags out of a `monster` query. Returns `Ok(None)` when no `--`
/// flag is present (the normal single-name lookup), `Ok(Some(_))` for a filtered
/// search, or `Err(message)` for an unknown flag or unreadable value.
fn parse_monster_filters(query: &str) -> Result<Option<MonsterFilters>, String> {
    if !query
        .split_whitespace()
        .any(|token| token.starts_with("--"))
    {
        return Ok(None);
    }
    let mut name_parts: Vec<&str> = Vec::new();
    let mut cr_value: Option<String> = None;
    let mut type_value: Option<String> = None;
    let mut tokens = query.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if let Some(value) = flag_value(token, "--cr", &mut tokens)? {
            cr_value = Some(value);
        } else if let Some(value) = flag_value(token, "--type", &mut tokens)? {
            type_value = Some(value);
        } else if token.starts_with("--") {
            return Err(format!(
                "Unknown filter \"{token}\". Filter monsters with --cr and --type, e.g. \
                 `monster --type dragon --cr 10-17`."
            ));
        } else {
            name_parts.push(token);
        }
    }

    let (cr_min, cr_max) = match &cr_value {
        Some(value) => parse_cr_range(value)?,
        None => (0.0, f64::MAX),
    };
    let creature_type = type_value.filter(|value| !value.is_empty());
    let name = (!name_parts.is_empty()).then(|| name_parts.join(" "));

    let mut summary_parts: Vec<String> = Vec::new();
    if let Some(kind) = &creature_type {
        summary_parts.push(capitalize(kind));
    }
    if let Some(cr) = &cr_value {
        summary_parts.push(format!("CR {}", cr.replace('-', "–")));
    }
    if let Some(name) = &name {
        summary_parts.push(format!("\"{name}\""));
    }

    Ok(Some(MonsterFilters {
        name,
        creature_type,
        cr_min,
        cr_max,
        summary: summary_parts.join(", "),
    }))
}

/// If `token` is `flag` (consuming the next token as its value) or `flag=value`,
/// return the value. `Ok(None)` if `token` is a different flag/word.
fn flag_value<'a>(
    token: &'a str,
    flag: &str,
    tokens: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
) -> Result<Option<String>, String> {
    let Some(rest) = token.strip_prefix(flag) else {
        return Ok(None);
    };
    if rest.is_empty() {
        return tokens
            .next()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| format!("{flag} needs a value, e.g. `{flag} 5`."));
    }
    if let Some(value) = rest.strip_prefix('=') {
        return Ok(Some(value.to_string()));
    }
    Ok(None) // e.g. `--crazy` when matching `--cr`
}

/// Parse a CR filter: a single rating (`5`, `1/4`) or an inclusive range
/// (`10-17`, `1/4-2`). Returns `(min, max)` on `cr_sort`'s numeric scale.
fn parse_cr_range(value: &str) -> Result<(f64, f64), String> {
    let invalid = || {
        format!(
            "Couldn't read CR \"{value}\". Use a number (5), a fraction (1/4), or a range (10-17)."
        )
    };
    if let Some((lo, hi)) = value.split_once('-') {
        let lo = parse_cr_token(lo).ok_or_else(invalid)?;
        let hi = parse_cr_token(hi).ok_or_else(invalid)?;
        Ok((lo.min(hi), lo.max(hi)))
    } else {
        let cr = parse_cr_token(value).ok_or_else(invalid)?;
        Ok((cr, cr))
    }
}

/// One CR token to its numeric value (the fractions match `cr_sort`'s mapping).
fn parse_cr_token(token: &str) -> Option<f64> {
    match token.trim() {
        "1/8" => Some(0.125),
        "1/4" => Some(0.25),
        "1/2" => Some(0.5),
        other => other.parse::<f64>().ok(),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Run a filtered monster search and render the matches as a clickable list. Each
/// row links to its own card (`monster <name>`); results are capped so a broad
/// filter can't flood the terminal.
async fn monster_filtered(state: &AppState, filters: &MonsterFilters) -> CommandResult {
    const CAP: usize = 100;
    let rows = state
        .monster_repo()
        .search_filtered(
            state.database().as_ref(),
            filters.name.as_deref(),
            filters.creature_type.as_deref(),
            filters.cr_min,
            filters.cr_max,
            CAP as i64 + 1,
        )
        .await?;

    if rows.is_empty() {
        return Ok(Some(ok_response(
            format!(
                "No monsters match {}. Try widening the filters.",
                filters.summary
            ),
            None,
        )));
    }

    let truncated = rows.len() > CAP;
    let shown = &rows[..rows.len().min(CAP)];
    let mut document = doc().with_block(heading(2, format!("Monsters — {}", filters.summary)));
    let items: Vec<Vec<InlineNode>> = shown
        .iter()
        .map(|row| {
            // The stored `cr` is the verbose display ("1/4 (XP 50; PB +2)"); the list
            // wants just the rating, which is always its leading token.
            let rating = row.cr.split_whitespace().next().unwrap_or("—");
            vec![
                command_ref(row.name.clone(), format!("monster {}", row.name)),
                text_node(format!("  —  CR {rating} · {}", row.creature_type)),
            ]
        })
        .collect();
    document = document.with_block(list(items));

    let footer = if truncated {
        format!("Showing the first {CAP} matches — narrow with --cr / --type.")
    } else {
        let count = shown.len();
        format!("{count} match{}.", if count == 1 { "" } else { "es" })
    };
    document = document.with_block(paragraph_text(footer));

    Ok(Some(ok_response_with_doc(
        document.to_plain_text(),
        Some(document),
        None,
    )))
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
            text_node("Filter by challenge rating and type: "),
            command_ref(
                "monster --type dragon --cr 10-17",
                "monster --type dragon --cr 10-17",
            ),
            text_node(" lists every match (CR accepts 5, 1/4, or a range like 10-17)."),
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
    // Surface how many derived (`_copy`) stat blocks were materialized.
    if summary.resolved_copy > 0 {
        document = document.with_block(paragraph_text(format!(
            "Resolved {} variant monsters from their base stat blocks.",
            summary.resolved_copy
        )));
    }
    // A copy whose base could not be found is dropped — never silently.
    if summary.skipped_copy > 0 {
        document = document.with_block(paragraph_text(format!(
            "Skipped {} variant monsters (base stat block not found).",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_name_is_not_filter_mode() {
        assert!(parse_monster_filters("goblin warrior").unwrap().is_none());
        assert!(parse_monster_filters("").unwrap().is_none());
    }

    #[test]
    fn parses_type_and_cr_range() {
        let f = parse_monster_filters("--type dragon --cr 10-17")
            .unwrap()
            .expect("filter mode");
        assert_eq!(f.creature_type.as_deref(), Some("dragon"));
        assert_eq!((f.cr_min, f.cr_max), (10.0, 17.0));
        assert_eq!(f.name, None);
        assert_eq!(f.summary, "Dragon, CR 10–17");
    }

    #[test]
    fn name_fragment_composes_with_filters() {
        let f = parse_monster_filters("red dragon --cr 5")
            .unwrap()
            .expect("filter mode");
        assert_eq!(f.name.as_deref(), Some("red dragon"));
        assert_eq!((f.cr_min, f.cr_max), (5.0, 5.0));
    }

    #[test]
    fn cr_accepts_fractions_equals_form_and_swapped_bounds() {
        let f = parse_monster_filters("--cr=1/4").unwrap().unwrap();
        assert_eq!((f.cr_min, f.cr_max), (0.25, 0.25));
        // A reversed range is normalized low→high.
        let f = parse_monster_filters("--type fiend --cr 5-1")
            .unwrap()
            .unwrap();
        assert_eq!((f.cr_min, f.cr_max), (1.0, 5.0));
    }

    #[test]
    fn rejects_unknown_flag_and_bad_cr() {
        assert!(parse_monster_filters("--bogus x").is_err());
        assert!(parse_monster_filters("--cr abc").is_err());
        assert!(parse_monster_filters("--cr").is_err());
    }

    #[test]
    fn parse_cr_range_handles_values_and_ranges() {
        assert_eq!(parse_cr_range("1/2").unwrap(), (0.5, 0.5));
        assert_eq!(parse_cr_range("10-17").unwrap(), (10.0, 17.0));
        assert_eq!(parse_cr_range("1/8-1").unwrap(), (0.125, 1.0));
        assert!(parse_cr_range("oops").is_err());
    }
}
