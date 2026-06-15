use dnd_core::calendar::{self, format_date_conversational, StoredCalendar};
use runebound_models::{
    CommandResponse,
};

use crate::commands::{ok_response, DesktopHandlerInvocation};

pub type CommandResult = Result<Option<CommandResponse>, String>;

pub async fn handle_date(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "date help" {
        return date_help();
    }

    if lowered.starts_with("date set") {
        return date_set(trimmed).await;
    }

    if lowered == "date" {
        return date_show(invocation).await;
    }

    Ok(Some(ok_response(
        "unknown date command. use `date help`".to_string(),
        None,
    )))
}

async fn date_show(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let stored = match calendar::load_calendar() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(Some(ok_response(
                "No calendar loaded. Use `calendar import` to import a calendar.".to_string(),
                None,
            )));
        }
        Err(e) => {
            return Ok(Some(ok_response(
                format!("failed to load calendar: {}", e),
                None,
            )));
        }
    };

    let formatted = format_date_conversational(&stored);
    Ok(Some(ok_response(formatted, None)))
}

fn date_help() -> CommandResult {
    let output = r#"Inspect and modify the current calendar date.

Usage:
  date
  date set year <number>
  date set month <month-name>
  date set day <number>

Requirements:
  - A calendar must be imported first (see `calendar import`)

Examples:
  date
  date set year 5
  date set month Emberwane
  date set day 14

Output format:
  Current date is shown as: "14th of Emberwane 2:30 PM (Moonday)"
  Weekday is computed from the calendar's first_day and week_len."#;

    Ok(Some(ok_response(output.to_string(), None)))
}

async fn date_set(
    trimmed: &str,
) -> CommandResult {
    let stored = match calendar::load_calendar() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(Some(ok_response(
                "No calendar loaded. Use `calendar import` to import a calendar.".to_string(),
                None,
            )));
        }
        Err(e) => {
            return Ok(Some(ok_response(
                format!("failed to load calendar: {}", e),
                None,
            )));
        }
    };

    let remainder = trimmed["date set".len()..].trim();
    let parts: Vec<&str> = remainder.split_whitespace().collect();

    if parts.is_empty() {
        return Ok(Some(ok_response(
            "usage: date set <year|month|day> <value>".to_string(),
            None,
        )));
    }

    let component = parts[0].to_lowercase();
    if parts.len() < 2 {
        return Ok(Some(ok_response(
            format!("missing value for '{}'. usage: date set {} <value>", component, component),
            None,
        )));
    }

    let value = parts[1];

    match component.as_str() {
        "year" => date_set_year(stored, value),
        "month" => date_set_month(stored, value),
        "day" => date_set_day(stored, value),
        _ => {
            Ok(Some(ok_response(
                format!("Unknown date component '{}'. Valid options: year, month, day.", component),
                None,
            )))
        }
    }
}

fn date_set_year(mut stored: StoredCalendar, value: &str) -> CommandResult {
    let year: i32 = match value.parse() {
        Ok(y) => y,
        Err(_) => {
            return Ok(Some(ok_response(
                format!("'{}' is not a valid year number. Year must be an integer (0 or greater).", value),
                None,
            )));
        }
    };

    if let Err(e) = stored.state.set_year(year) {
        return Ok(Some(ok_response(
            format!("{}", e),
            None,
        )));
    }

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    Ok(Some(ok_response(formatted, None)))
}

fn date_set_month(mut stored: StoredCalendar, value: &str) -> CommandResult {
    let target_lower = value.to_lowercase();
    let month_index = stored
        .definition
        .months
        .iter()
        .position(|m| m.to_lowercase() == target_lower);

    let month_index = match month_index {
        Some(idx) => idx,
        None => {
            let valid_months = stored.definition.months.join(", ");
            return Ok(Some(ok_response(
                format!("Unknown month '{}'. Valid months: {}", value, valid_months),
                None,
            )));
        }
    };

    if let Err(e) = stored.state.set_month_index(month_index, &stored.definition) {
        return Ok(Some(ok_response(
            format!("invalid month: {}", e),
            None,
        )));
    }

    if stored.state.day > stored.definition.month_len.get(&stored.definition.months[month_index]).copied().unwrap_or(0) {
        stored.state.day = stored.definition.month_len.get(&stored.definition.months[month_index]).copied().unwrap_or(1);
    }

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    Ok(Some(ok_response(formatted, None)))
}

fn date_set_day(mut stored: StoredCalendar, value: &str) -> CommandResult {
    let day: u32 = match value.parse() {
        Ok(d) => d,
        Err(_) => {
            return Ok(Some(ok_response(
                format!("'{}' is not a valid day number. Day must be a positive integer.", value),
                None,
            )));
        }
    };

    if let Err(e) = stored.state.set_day(day, &stored.definition) {
        return Ok(Some(ok_response(
            format!("{}", e),
            None,
        )));
    }

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    Ok(Some(ok_response(formatted, None)))
}