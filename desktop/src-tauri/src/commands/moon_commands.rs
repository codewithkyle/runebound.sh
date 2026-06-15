use dnd_core::calendar::{self, moon_phase_info, MoonPhaseKind, StoredCalendar};
use runebound_models::{doc, entity_card, entity_row};

use crate::commands::{ok_response, ok_response_with_doc, DesktopHandlerInvocation};

use super::date_commands::CommandResult;

pub async fn handle_moon(
    invocation: DesktopHandlerInvocation<'_>,
) -> CommandResult {
    let trimmed = invocation.raw_input.trim();
    let lowered = trimmed.to_ascii_lowercase();

    if lowered == "moon help" {
        return moon_help();
    }

    if lowered != "moon" {
        return Ok(Some(ok_response(
            "unknown moon command. use `moon help`".to_string(),
            None,
        )));
    }

    let stored = match load_calendar_state()? {
        Some(calendar) => calendar,
        None => {
            return Ok(Some(ok_response(
                "No calendar loaded. Use `calendar import` to import a calendar.".to_string(),
                None,
            )));
        }
    };

    if stored.definition.moons.is_empty() {
        return Ok(Some(ok_response(
            "Active calendar does not define any moons.".to_string(),
            None,
        )));
    }

    let phase_info = match moon_phase_info(&stored) {
        Ok(info) => info,
        Err(err) => {
            return Ok(Some(ok_response(
                format!(
                    "Unable to compute moon phases: {}. Ensure the imported calendar includes lunar cycle data (lunar_cyc).",
                    err
                ),
                None,
            )));
        }
    };

    let mut rows = Vec::new();
    let mut lines = Vec::new();
    for info in phase_info {
        let display_phase = phase_name(&info.phase);
        let day_display = info.age + 1;
        lines.push(format!(
            "{}: {} (Day {} of {})",
            info.name, display_phase, day_display, info.cycle_length
        ));
        rows.push(entity_row(
            &info.name,
            format!("{} · Day {} of {}", display_phase, day_display, info.cycle_length),
        ));
    }

    let doc = doc().with_block(entity_card("Moon Phases", rows));

    Ok(Some(ok_response_with_doc(lines.join("\n"), Some(doc), None)))
}

fn moon_help() -> CommandResult {
    let output = r#"Show the current moon phases for the active calendar.

Usage:
  moon

Requirements:
  - A calendar with lunar data must be imported (`calendar import`)

Output:
  Lists each moon, its current phase, and the day count within its cycle."#;

    Ok(Some(ok_response(output.to_string(), None)))
}

fn load_calendar_state() -> Result<Option<StoredCalendar>, String> {
    calendar::load_calendar().map_err(|err| format!("failed to load calendar: {}", err))
}

fn phase_name(kind: &MoonPhaseKind) -> &'static str {
    match kind {
        MoonPhaseKind::New => "New Moon",
        MoonPhaseKind::WaxingCrescent => "Waxing Crescent",
        MoonPhaseKind::FirstQuarter => "First Quarter",
        MoonPhaseKind::WaxingGibbous => "Waxing Gibbous",
        MoonPhaseKind::Full => "Full Moon",
        MoonPhaseKind::WaningGibbous => "Waning Gibbous",
        MoonPhaseKind::LastQuarter => "Last Quarter",
        MoonPhaseKind::WaningCrescent => "Waning Crescent",
    }
}
