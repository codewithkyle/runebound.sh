use dnd_core::calendar::{self, format_date_conversational, CalendarDelta, StoredCalendar};

use crate::commands::{ok_response, DesktopHandlerInvocation};

use super::date_commands::CommandResult;

const DELTA_USAGE_HINT: &str = "Units: m=minutes, h=hours, d=days, w=weeks, y=years";

pub async fn handle_time_delta(invocation: DesktopHandlerInvocation<'_>) -> CommandResult {
    let sign_char = invocation
        .lowered
        .get(0)
        .and_then(|token| token.chars().next())
        .unwrap_or('+');

    if invocation.tokens.len() != 2 {
        return Ok(Some(ok_response(usage_message(sign_char), None)));
    }

    let payload = invocation.tokens[1].as_str();
    let mut stored = match load_calendar_state()? {
        Some(calendar) => calendar,
        None => {
            return Ok(Some(ok_response(
                "No calendar loaded. Use `calendar import` to import a calendar.".to_string(),
                None,
            )));
        }
    };

    let delta = match CalendarDelta::from_parts(sign_char, payload) {
        Ok(delta) => delta,
        Err(err) => {
            return Ok(Some(ok_response(
                format!("{}\n{}", err, DELTA_USAGE_HINT),
                None,
            )));
        }
    };

    if let Err(err) = calendar::apply_calendar_delta(&mut stored.state, &stored.definition, delta) {
        return Ok(Some(ok_response(format!("{}", err), None)));
    }

    if let Err(err) = calendar::save_calendar(&stored) {
        return Ok(Some(ok_response(
            format!("failed to save calendar: {}", err),
            None,
        )));
    }

    let formatted = format_date_conversational(&stored);

    Ok(Some(ok_response(formatted, None)))
}

fn usage_message(sign_char: char) -> String {
    format!(
        "usage: {sign}<amount><unit>\n{hint}",
        sign = sign_char,
        hint = DELTA_USAGE_HINT
    )
}

fn load_calendar_state() -> Result<Option<StoredCalendar>, String> {
    calendar::load_calendar().map_err(|err| format!("failed to load calendar: {}", err))
}
