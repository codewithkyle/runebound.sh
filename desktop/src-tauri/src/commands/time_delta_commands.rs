use dnd_core::calendar::{self, format_date_conversational, CalendarDelta, StoredCalendar};

use crate::commands::{DesktopHandlerInvocation, command_action_response, ok_response};

use super::date_commands::{CommandResult, date_response};

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
            return Ok(Some(command_action_response(
                "No calendar loaded. Use ",
                "calendar import",
                " to import a calendar.",
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

    date_response(formatted)
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
