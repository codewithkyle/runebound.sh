use dnd_core::calendar::{self, StoredCalendar, format_date_conversational};
use runebound_models::CommandResponse;
use runebound_models::output::{StatusTone, doc, status};

use crate::commands::{
    DesktopHandlerInvocation, command_action_response, ok_response, ok_response_with_doc,
};

pub type CommandResult = Result<Option<CommandResponse>, String>;

/// Echo the current calendar date as a structured status line, with the plain
/// conversational sentence as the fallback `output`.
pub(crate) fn date_response(formatted: String) -> CommandResult {
    let document = doc().with_block(status(StatusTone::Info, formatted.clone()));
    Ok(Some(ok_response_with_doc(formatted, Some(document), None)))
}

pub async fn handle_date(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "date help" {
        return date_help();
    }

    if lowered.starts_with("date set") {
        return date_set(invocation.tokens).await;
    }

    if lowered == "date" {
        return date_show(invocation).await;
    }

    Ok(Some(command_action_response(
        "unknown date command. use ",
        "date help",
        "",
    )))
}

async fn date_show(_invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let stored = match calendar::load_calendar() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(Some(command_action_response(
                "No calendar loaded. Use ",
                "calendar import",
                " to import a calendar.",
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
    date_response(formatted)
}

fn date_help() -> CommandResult {
    let output = r#"Inspect and modify the current calendar date.

Usage:
  date
  date set year <number>
  date set month <month-name>
  date set day <number>
  date set time <HH:MM> [AM|PM]

Requirements:
  - A calendar must be imported first (see `calendar import`)
  - Time values default to AM when no suffix is provided; 24-hour inputs (e.g., `13:30`) are also supported

Examples:
  date
  date set year 5
  date set month Emberwane
  date set day 14
  date set time 12:15 PM
  date set 1:00"#;

    Ok(Some(ok_response(output.to_string(), None)))
}

async fn date_set(tokens: &[String]) -> CommandResult {
    let stored = match calendar::load_calendar() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(Some(command_action_response(
                "No calendar loaded. Use ",
                "calendar import",
                " to import a calendar.",
            )));
        }
        Err(e) => {
            return Ok(Some(ok_response(
                format!("failed to load calendar: {}", e),
                None,
            )));
        }
    };

    // Tokens are `["date", "set", <component>, <value>...]`; skip the command words.
    let parts: Vec<&str> = tokens.iter().skip(2).map(String::as_str).collect();

    if parts.is_empty() {
        return Ok(Some(ok_response(
            "usage: date set <year|month|day|time> <value>".to_string(),
            None,
        )));
    }

    let component_raw = parts[0];
    let component = component_raw.to_lowercase();

    if component == "time" {
        if parts.len() < 2 {
            return Ok(Some(ok_response(
                "usage: date set time <HH:MM> [AM|PM]".to_string(),
                None,
            )));
        }
        if parts.len() > 3 {
            return Ok(Some(ok_response(
                "too many arguments. usage: date set time <HH:MM> [AM|PM]".to_string(),
                None,
            )));
        }
        return date_set_time(stored, parts[1], parts.get(2).copied());
    }

    if component_raw.contains(':') {
        if parts.len() > 2 {
            return Ok(Some(ok_response(
                "too many arguments. usage: date set <HH:MM> [AM|PM]".to_string(),
                None,
            )));
        }
        return date_set_time(stored, component_raw, parts.get(1).copied());
    }

    if parts.len() < 2 {
        return Ok(Some(ok_response(
            format!(
                "missing value for '{}'. usage: date set {} <value>",
                component, component
            ),
            None,
        )));
    }

    let value = parts[1];

    match component.as_str() {
        "year" => date_set_year(stored, value),
        "month" => date_set_month(stored, value),
        "day" => date_set_day(stored, value),
        _ => Ok(Some(ok_response(
            format!(
                "Unknown date component '{}'. Valid options: year, month, day, time.",
                component
            ),
            None,
        ))),
    }
}

fn date_set_year(mut stored: StoredCalendar, value: &str) -> CommandResult {
    let year: i32 = match value.parse() {
        Ok(y) => y,
        Err(_) => {
            return Ok(Some(ok_response(
                format!(
                    "'{}' is not a valid year number. Year must be an integer (0 or greater).",
                    value
                ),
                None,
            )));
        }
    };

    if let Err(e) = stored.state.set_year(year) {
        return Ok(Some(ok_response(format!("{}", e), None)));
    }

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    date_response(formatted)
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

    // `set_month_index` clamps the day into the new month's range for us.
    if let Err(e) = stored
        .state
        .set_month_index(month_index, &stored.definition)
    {
        return Ok(Some(ok_response(format!("invalid month: {}", e), None)));
    }

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    date_response(formatted)
}

fn date_set_day(mut stored: StoredCalendar, value: &str) -> CommandResult {
    let day: u32 = match value.parse() {
        Ok(d) => d,
        Err(_) => {
            return Ok(Some(ok_response(
                format!(
                    "'{}' is not a valid day number. Day must be a positive integer.",
                    value
                ),
                None,
            )));
        }
    };

    if let Err(e) = stored.state.set_day(day, &stored.definition) {
        return Ok(Some(ok_response(format!("{}", e), None)));
    }

    if let Err(e) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", e),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    date_response(formatted)
}

fn date_set_time(
    mut stored: StoredCalendar,
    time_value: &str,
    suffix: Option<&str>,
) -> CommandResult {
    let (hour, minute) = match parse_time_input(time_value, suffix) {
        Ok(result) => result,
        Err(err) => {
            return Ok(Some(ok_response(err, None)));
        }
    };

    if let Err(err) = stored.state.set_hour(hour) {
        return Ok(Some(ok_response(format!("{}", err), None)));
    }
    if let Err(err) = stored.state.set_minute(minute) {
        return Ok(Some(ok_response(format!("{}", err), None)));
    }

    if let Err(err) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", err),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);
    date_response(formatted)
}

fn parse_time_input(value: &str, suffix: Option<&str>) -> Result<(u8, u8), String> {
    let trimmed_value = value.trim();
    let Some((hour_part, minute_part)) = trimmed_value.split_once(':') else {
        return Err("invalid time. expected HH:MM format".to_string());
    };

    if hour_part.trim().is_empty() {
        return Err("hour is required".to_string());
    }
    if minute_part.trim().is_empty() {
        return Err("minute is required".to_string());
    }

    let hour: u32 = hour_part
        .trim()
        .parse()
        .map_err(|_| "hour must be a number".to_string())?;
    let minute: u32 = minute_part
        .trim()
        .parse()
        .map_err(|_| "minute must be a number".to_string())?;
    if minute > 59 {
        return Err("minute must be between 0 and 59".to_string());
    }

    let suffix_normalized = suffix.map(|s| s.trim().to_ascii_uppercase());

    let hour_24 = if let Some(sfx) = suffix_normalized.as_deref() {
        match sfx {
            "AM" => convert_to_24_hour_am_pm(hour, false)?,
            "PM" => convert_to_24_hour_am_pm(hour, true)?,
            _ => {
                return Err("unknown suffix. use AM or PM".to_string());
            }
        }
    } else {
        convert_without_suffix(hour)?
    };

    Ok((hour_24, minute as u8))
}

fn convert_to_24_hour_am_pm(hour: u32, is_pm: bool) -> Result<u8, String> {
    if hour == 0 || hour > 12 {
        return Err("hour must be between 1 and 12 when using AM/PM".to_string());
    }

    let hour_24 = if is_pm {
        if hour == 12 { 12 } else { hour + 12 }
    } else if hour == 12 {
        0
    } else {
        hour
    };

    Ok(hour_24 as u8)
}

fn convert_without_suffix(hour: u32) -> Result<u8, String> {
    if hour > 23 {
        return Err("hour must be between 0 and 23 when no AM/PM is provided".to_string());
    }

    if hour >= 13 {
        Ok(hour as u8)
    } else if hour == 12 {
        // Default to AM when ambiguous per requirements
        Ok(0)
    } else {
        Ok(hour as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::{convert_to_24_hour_am_pm, convert_without_suffix, parse_time_input};

    #[test]
    fn parses_time_with_pm_suffix() {
        let (hour, minute) = parse_time_input("12:15", Some("PM")).expect("should parse");
        assert_eq!((hour, minute), (12, 15));
    }

    #[test]
    fn parses_time_defaults_to_am() {
        let (hour, minute) = parse_time_input("1:05", None).expect("should parse");
        assert_eq!((hour, minute), (1, 5));
    }

    #[test]
    fn parses_24_hour_time_without_suffix() {
        let (hour, minute) = parse_time_input("13:30", None).expect("should parse");
        assert_eq!((hour, minute), (13, 30));
    }

    #[test]
    fn convert_without_suffix_defaults_midnight_at_twelve() {
        assert_eq!(convert_without_suffix(12).unwrap(), 0);
    }

    #[test]
    fn convert_to_24_hour_am_pm_handles_midnight_and_noon() {
        assert_eq!(convert_to_24_hour_am_pm(12, false).unwrap(), 0);
        assert_eq!(convert_to_24_hour_am_pm(12, true).unwrap(), 12);
    }
}
