use std::path::PathBuf;

use dnd_core::calendar::{self, StoredCalendar};
use runebound_models::{
    CommandResponse, OutputDoc, doc, entity_card, entity_row, heading, paragraph_text,
};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{
    DesktopHandlerInvocation, command_action_response, ok_response, ok_response_with_doc,
};

pub type CommandResult = Result<Option<CommandResponse>, String>;

pub async fn handle_calendar(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "calendar" || lowered == "calendar help" {
        return calendar_help();
    }

    if lowered.starts_with("calendar import") {
        return calendar_import(trimmed, invocation).await;
    }

    Ok(Some(command_action_response(
        "unknown calendar command. use ",
        "calendar help",
        "",
    )))
}

fn calendar_help() -> CommandResult {
    let output = r#"Import a JSON calendar file from donjon.bin.sh/fantasy/calendar/

Usage:
  calendar import <path>
  calendar import path/to/calendar.json

The calendar will be normalized to TOML format and stored at:
  ~/.config/runebound.sh/calendar.toml

Note: Importing a calendar will reset the active date to year 0, first month, day 1.

Examples:
  calendar import ~/Downloads/calendar.json
  calendar import /path/to/my-fantasy-calendar.json"#;

    Ok(Some(ok_response(output.to_string(), None)))
}

async fn calendar_import(trimmed: &str, invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let path = if trimmed.len() > "calendar import".len() {
        let remainder = trimmed["calendar import".len()..].trim();
        if remainder.is_empty() {
            None
        } else {
            Some(remainder.to_string())
        }
    } else {
        None
    };

    let path = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let picked = invocation
                .app_handle
                .dialog()
                .file()
                .add_filter("JSON files", &["json"])
                .set_title("Select Calendar File")
                .blocking_pick_file();

            match picked {
                Some(fp) => {
                    use tauri_plugin_dialog::FilePath;
                    match fp {
                        FilePath::Path(p) => p,
                        FilePath::Url(u) => PathBuf::from(u.as_str()),
                    }
                }
                None => {
                    return Ok(Some(ok_response(
                        "Calendar import cancelled.".to_string(),
                        None,
                    )));
                }
            }
        }
    };

    let path_string = path.to_string_lossy().into_owned();
    let expanded =
        shellexpand::full(&path_string).map_err(|e| format!("failed to expand path: {}", e))?;
    let path = PathBuf::from(expanded.as_ref());

    if !path.exists() {
        return Ok(Some(ok_response(
            format!("file not found: {}", path.display()),
            None,
        )));
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return Ok(Some(ok_response(
                format!("failed to read file: {}", e),
                None,
            )));
        }
    };

    let stored: StoredCalendar = match calendar::import_donjon_json(&content) {
        Ok(c) => c,
        Err(e) => {
            return Ok(Some(ok_response(
                format!("failed to parse calendar JSON: {}", e),
                None,
            )));
        }
    };

    let month_count = stored.definition.months.len();
    let year_len = stored.definition.year_len;
    let week_len = stored.definition.week_len;
    let moon_count = stored.definition.moons.len();
    let first_day = stored.definition.first_day;

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let output = format!(
        "Calendar imported successfully.\n\n\
        Year length: {} days\n\
        Months: {} ({})\n\
        Week length: {} days\n\
        Moons: {}\n\
        First day: {} (weekday index)\n\n\
        Active calendar state has been reset to:\n\
        Year: 0, Month: {}, Day: 1\n\n\
        Previous calendar state (if any) has been replaced.",
        year_len,
        month_count,
        stored.definition.months.join(", "),
        week_len,
        if moon_count > 0 {
            stored.definition.moons.join(", ")
        } else {
            "none".to_string()
        },
        first_day,
        stored
            .definition
            .months
            .first()
            .unwrap_or(&"Unknown".to_string())
    );

    let doc = build_calendar_import_doc(&stored);

    Ok(Some(ok_response_with_doc(output, Some(doc), None)))
}

fn build_calendar_import_doc(calendar: &StoredCalendar) -> OutputDoc {
    let mut rows = vec![
        entity_row(
            "year length",
            format!("{} days", calendar.definition.year_len),
        ),
        entity_row("months", format!("{}", calendar.definition.months.len())),
        entity_row("month names", calendar.definition.months.join(", ")),
        entity_row(
            "week length",
            format!("{} days", calendar.definition.week_len),
        ),
        entity_row("weekdays", calendar.definition.weekdays.join(", ")),
    ];

    if !calendar.definition.moons.is_empty() {
        rows.push(entity_row("moons", calendar.definition.moons.join(", ")));
    }

    rows.push(entity_row(
        "first day",
        format!("{}", calendar.definition.first_day),
    ));
    rows.push(entity_row(
        "state",
        format!(
            "Year {}, {} {}, Day {}",
            calendar.state.year,
            calendar
                .definition
                .months
                .get(calendar.state.month_index)
                .unwrap_or(&"Unknown".to_string()),
            calendar.state.day,
            format!("{:02}:{:02}", calendar.state.hour_24, calendar.state.minute)
        ),
    ));

    doc()
        .with_block(entity_card("Calendar Imported", rows))
        .with_block(heading(3, "Active State Reset"))
        .with_block(paragraph_text(
            "The calendar state has been reset to year 0, first month, day 1, midnight.",
        ))
}
