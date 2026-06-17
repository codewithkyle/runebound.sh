use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::config::config_paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarDefinition {
    pub year_len: u32,
    pub months: Vec<String>,
    pub month_len: HashMap<String, u32>,
    pub week_len: u32,
    pub weekdays: Vec<String>,
    pub moons: Vec<String>,
    #[serde(default)]
    pub notes: HashMap<String, serde_json::Value>,
    pub first_day: u32,
}

impl CalendarDefinition {
    pub fn month_specs(&self) -> Vec<MonthSpec> {
        self.months
            .iter()
            .map(|name| {
                let len = self.month_len.get(name).copied().unwrap_or(0);
                MonthSpec {
                    name: name.clone(),
                    length: len,
                }
            })
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        if self.year_len == 0 {
            bail!("year_len must be greater than 0");
        }

        if self.months.is_empty() {
            bail!("at least one month is required");
        }

        for month in &self.months {
            let len = self.month_len.get(month).copied().unwrap_or(0);
            if len == 0 {
                bail!("month '{}' has invalid length 0", month);
            }
        }

        let total_month_days: u32 = self
            .months
            .iter()
            .map(|m| self.month_len.get(m).copied().unwrap_or(0))
            .sum();

        if total_month_days != self.year_len {
            bail!(
                "sum of month lengths ({}) does not match year_len ({})",
                total_month_days,
                self.year_len
            );
        }

        if self.week_len == 0 {
            bail!("week_len must be greater than 0");
        }

        if self.weekdays.len() != self.week_len as usize {
            bail!(
                "weekdays count ({}) does not match week_len ({})",
                self.weekdays.len(),
                self.week_len
            );
        }

        if self.first_day >= self.week_len {
            bail!(
                "first_day ({}) must be less than week_len ({})",
                self.first_day,
                self.week_len
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthSpec {
    pub name: String,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCalendar {
    pub definition: CalendarDefinition,
    pub state: CalendarState,
}

impl StoredCalendar {
    pub fn validate(&self) -> Result<()> {
        self.definition.validate()?;

        if self.state.year < 0 {
            bail!("year cannot be negative");
        }

        if self.state.month_index >= self.definition.months.len() {
            bail!(
                "month_index ({}) out of bounds for {} months",
                self.state.month_index,
                self.definition.months.len()
            );
        }

        let month_name = &self.definition.months[self.state.month_index];
        let month_len = self
            .definition
            .month_len
            .get(month_name)
            .copied()
            .unwrap_or(0);
        if self.state.day == 0 || self.state.day > month_len {
            bail!(
                "day ({}) must be between 1 and {} for month '{}'",
                self.state.day,
                month_len,
                month_name
            );
        }

        if self.state.hour_24 >= 24 {
            bail!("hour_24 ({}) must be between 0 and 23", self.state.hour_24);
        }

        if self.state.minute >= 60 {
            bail!("minute ({}) must be between 0 and 59", self.state.minute);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarState {
    pub year: i32,
    pub month_index: usize,
    pub day: u32,
    pub hour_24: u8,
    pub minute: u8,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self {
            year: 0,
            month_index: 0,
            day: 1,
            hour_24: 0,
            minute: 0,
        }
    }
}

impl CalendarState {
    pub fn reset(&mut self) {
        self.year = 0;
        self.month_index = 0;
        self.day = 1;
        self.hour_24 = 0;
        self.minute = 0;
    }

    pub fn set_year(&mut self, year: i32) -> Result<()> {
        if year < 0 {
            bail!("year must be 0 or greater");
        }
        self.year = year;
        Ok(())
    }

    pub fn set_month_index(&mut self, index: usize, definition: &CalendarDefinition) -> Result<()> {
        if index >= definition.months.len() {
            bail!(
                "month index {} is out of bounds ({} months available)",
                index,
                definition.months.len()
            );
        }
        self.month_index = index;
        // Months can have different lengths, so clamp the current day into the new
        // month's range (e.g. moving from a 50-day month on day 50 to a 30-day month).
        let month_len = definition
            .month_len
            .get(&definition.months[index])
            .copied()
            .unwrap_or(0);
        if self.day > month_len {
            self.day = month_len.max(1);
        }
        Ok(())
    }

    pub fn set_day(&mut self, day: u32, definition: &CalendarDefinition) -> Result<()> {
        let month_name = &definition.months[self.month_index];
        let month_len = definition.month_len.get(month_name).copied().unwrap_or(0);
        if day == 0 || day > month_len {
            bail!(
                "day {} must be between 1 and {} for month '{}'",
                day,
                month_len,
                month_name
            );
        }
        self.day = day;
        Ok(())
    }

    pub fn set_hour(&mut self, hour: u8) -> Result<()> {
        if hour >= 24 {
            bail!("hour must be between 0 and 23");
        }
        self.hour_24 = hour;
        Ok(())
    }

    pub fn set_minute(&mut self, minute: u8) -> Result<()> {
        if minute >= 60 {
            bail!("minute must be between 0 and 59");
        }
        self.minute = minute;
        Ok(())
    }

    pub fn total_minutes(&self, definition: &CalendarDefinition) -> i64 {
        let year_minutes = i64::from(self.year) * i64::from(definition.year_len) * MINUTES_PER_DAY;
        let month_days = days_before_month(definition, self.month_index) as i64;
        let day_offset = i64::from(self.day.saturating_sub(1));
        let day_minutes = (month_days + day_offset) * MINUTES_PER_DAY;
        let hour_minutes = i64::from(self.hour_24) * MINUTES_PER_HOUR;
        let minute_component = i64::from(self.minute);
        year_minutes + day_minutes + hour_minutes + minute_component
    }

    fn update_from_total_minutes(
        &mut self,
        definition: &CalendarDefinition,
        total_minutes: i64,
    ) -> Result<()> {
        let clamped = total_minutes.max(0);
        let total_days = clamped / MINUTES_PER_DAY;
        let minute_of_day = clamped % MINUTES_PER_DAY;

        let year_len = i64::from(definition.year_len);
        let year = total_days / year_len;
        if year > i64::from(i32::MAX) {
            bail!("calendar year exceeds supported range");
        }

        let mut day_of_year = (total_days % year_len) as u32;
        let mut month_index = 0usize;
        let mut day_in_month = 1u32;
        for (idx, name) in definition.months.iter().enumerate() {
            let month_len = definition.month_len.get(name).copied().unwrap_or(0);
            if month_len == 0 {
                bail!("month '{}' has invalid length 0", name);
            }
            if day_of_year < month_len {
                month_index = idx;
                day_in_month = day_of_year + 1;
                break;
            }
            day_of_year -= month_len;
        }

        self.year = year as i32;
        self.month_index = month_index;
        self.day = day_in_month;
        self.hour_24 = (minute_of_day / MINUTES_PER_HOUR) as u8;
        self.minute = (minute_of_day % MINUTES_PER_HOUR) as u8;
        Ok(())
    }
}

const MINUTES_PER_HOUR: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const MINUTES_PER_DAY: i64 = MINUTES_PER_HOUR * HOURS_PER_DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarDeltaUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Years,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarDelta {
    sign: i8,
    magnitude: u32,
    unit: CalendarDeltaUnit,
}

impl std::str::FromStr for CalendarDelta {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        if input.is_empty() {
            bail!("delta cannot be empty");
        }
        let mut chars = input.chars();
        let sign_char = chars
            .next()
            .ok_or_else(|| anyhow!("delta cannot be empty"))?;
        let remainder: String = chars.collect();
        Self::from_parts(sign_char, remainder.as_str())
    }
}

impl CalendarDelta {
    pub fn from_parts(sign_char: char, payload: &str) -> Result<Self> {
        if payload.is_empty() {
            bail!("delta is missing an amount and unit");
        }

        let sign = match sign_char {
            '+' => 1,
            '-' => -1,
            _ => bail!("delta must start with '+' or '-'"),
        };

        let (amount_str, unit_char) = split_amount_and_unit(payload)?;
        let magnitude: u32 = amount_str
            .parse()
            .map_err(|_| anyhow!("'{}' is not a valid positive integer", amount_str))?;
        if magnitude == 0 {
            bail!("delta amount must be greater than 0");
        }

        let unit = CalendarDeltaUnit::try_from(unit_char)?;

        Ok(Self {
            sign,
            magnitude,
            unit,
        })
    }

    pub fn is_positive(&self) -> bool {
        self.sign >= 0
    }

    pub fn magnitude(&self) -> u32 {
        self.magnitude
    }

    pub fn unit(&self) -> CalendarDeltaUnit {
        self.unit
    }

    pub fn unit_label(&self, amount: u32) -> &'static str {
        match self.unit {
            CalendarDeltaUnit::Minutes => {
                if amount == 1 {
                    "minute"
                } else {
                    "minutes"
                }
            }
            CalendarDeltaUnit::Hours => {
                if amount == 1 {
                    "hour"
                } else {
                    "hours"
                }
            }
            CalendarDeltaUnit::Days => {
                if amount == 1 {
                    "day"
                } else {
                    "days"
                }
            }
            CalendarDeltaUnit::Weeks => {
                if amount == 1 {
                    "week"
                } else {
                    "weeks"
                }
            }
            CalendarDeltaUnit::Years => {
                if amount == 1 {
                    "year"
                } else {
                    "years"
                }
            }
        }
    }

    fn minutes(&self, definition: &CalendarDefinition) -> Result<i64> {
        let magnitude = i64::from(self.magnitude);
        let base_minutes = match self.unit {
            CalendarDeltaUnit::Minutes => magnitude,
            CalendarDeltaUnit::Hours => checked_mul_i64(magnitude, MINUTES_PER_HOUR)?,
            CalendarDeltaUnit::Days => checked_mul_i64(magnitude, MINUTES_PER_DAY)?,
            CalendarDeltaUnit::Weeks => {
                let days = checked_mul_i64(magnitude, i64::from(definition.week_len))?;
                checked_mul_i64(days, MINUTES_PER_DAY)?
            }
            CalendarDeltaUnit::Years => {
                let days = checked_mul_i64(magnitude, i64::from(definition.year_len))?;
                checked_mul_i64(days, MINUTES_PER_DAY)?
            }
        };

        Ok(if self.sign >= 0 {
            base_minutes
        } else {
            -base_minutes
        })
    }
}

impl TryFrom<char> for CalendarDeltaUnit {
    type Error = anyhow::Error;

    fn try_from(value: char) -> Result<Self> {
        match value.to_ascii_lowercase() {
            'm' => Ok(CalendarDeltaUnit::Minutes),
            'h' => Ok(CalendarDeltaUnit::Hours),
            'd' => Ok(CalendarDeltaUnit::Days),
            'w' => Ok(CalendarDeltaUnit::Weeks),
            'y' => Ok(CalendarDeltaUnit::Years),
            _ => bail!("unsupported delta unit '{}'. use m, h, d, w, or y", value),
        }
    }
}

pub fn apply_calendar_delta(
    state: &mut CalendarState,
    definition: &CalendarDefinition,
    delta: CalendarDelta,
) -> Result<()> {
    let current = state.total_minutes(definition);
    let delta_minutes = delta.minutes(definition)?;
    let updated = current
        .checked_add(delta_minutes)
        .ok_or_else(|| anyhow!("calendar adjustment is too large"))?;
    state.update_from_total_minutes(definition, updated.max(0))
}

fn split_amount_and_unit(payload: &str) -> Result<(&str, char)> {
    let mut chars = payload.chars();
    let unit_char = chars
        .next_back()
        .ok_or_else(|| anyhow!("delta is missing a unit"))?;
    let amount_len = payload.len() - unit_char.len_utf8();
    if amount_len == 0 {
        bail!("delta is missing an amount");
    }
    let amount = &payload[..amount_len];
    Ok((amount, unit_char))
}

fn checked_mul_i64(left: i64, right: i64) -> Result<i64> {
    left.checked_mul(right)
        .ok_or_else(|| anyhow!("calendar adjustment is too large"))
}

fn days_before_month(definition: &CalendarDefinition, month_index: usize) -> u32 {
    definition
        .months
        .iter()
        .take(month_index)
        .map(|name| definition.month_len.get(name).copied().unwrap_or(0))
        .sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonPhaseKind {
    New,
    WaxingCrescent,
    FirstQuarter,
    WaxingGibbous,
    Full,
    WaningGibbous,
    LastQuarter,
    WaningCrescent,
}

#[derive(Debug, Clone)]
pub struct MoonPhaseInfo {
    pub name: String,
    pub phase: MoonPhaseKind,
    pub age: u32,
    pub cycle_length: u32,
}

pub fn moon_phase_info(calendar: &StoredCalendar) -> Result<Vec<MoonPhaseInfo>> {
    if calendar.definition.moons.is_empty() {
        return Ok(Vec::new());
    }

    let cycles = lunar_cycle_lengths(calendar)?;
    let shifts = lunar_shifts(calendar)?;
    let total_days = total_days_since_epoch(&calendar.state, &calendar.definition);

    let mut infos = Vec::new();
    for moon in &calendar.definition.moons {
        let cycle_length = cycles
            .get(moon)
            .copied()
            .ok_or_else(|| anyhow!("missing cycle length for moon '{}'", moon))?;
        if cycle_length == 0 {
            return Err(anyhow!(
                "cycle length for '{}' must be greater than 0",
                moon
            ));
        }

        let shift = shifts.get(moon).copied().unwrap_or(0);
        let shift_i64 = i64::from(shift);
        let shifted_days = total_days + shift_i64;
        let cycle_len_i64 = i64::from(cycle_length);
        let age = (shifted_days.rem_euclid(cycle_len_i64)) as u32;
        let phase = phase_from_age(age, cycle_length);

        infos.push(MoonPhaseInfo {
            name: moon.clone(),
            phase,
            age,
            cycle_length,
        });
    }

    infos.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    Ok(infos)
}

fn lunar_cycle_lengths(calendar: &StoredCalendar) -> Result<HashMap<String, u32>> {
    match calendar.definition.notes.get("lunar_cyc") {
        Some(value) => {
            let map: HashMap<String, u32> = serde_json::from_value(value.clone())
                .map_err(|err| anyhow!("invalid lunar_cyc data: {}", err))?;
            Ok(map)
        }
        None => Err(anyhow!(
            "calendar includes moons but is missing 'lunar_cyc' data in notes"
        )),
    }
}

fn lunar_shifts(calendar: &StoredCalendar) -> Result<HashMap<String, i32>> {
    match calendar.definition.notes.get("lunar_shf") {
        Some(value) => {
            let map: HashMap<String, i32> = serde_json::from_value(value.clone())
                .map_err(|err| anyhow!("invalid lunar_shf data: {}", err))?;
            Ok(map)
        }
        None => Ok(HashMap::new()),
    }
}

pub fn total_days_since_epoch(state: &CalendarState, definition: &CalendarDefinition) -> i64 {
    let minutes = state.total_minutes(definition);
    minutes / MINUTES_PER_DAY
}

fn phase_from_age(age: u32, cycle_length: u32) -> MoonPhaseKind {
    if cycle_length == 0 {
        return MoonPhaseKind::New;
    }
    let fraction = age as f64 / cycle_length as f64;
    // Round (not floor) so the four principal phases are *centered* on their points:
    // New on age 0, First Quarter on cycle/4, Full on cycle/2, Last Quarter on 3*cycle/4.
    // The crescent/gibbous phases fill the eighths between them. (`% 8` folds the
    // end-of-cycle bucket 8 back onto New.)
    let bucket = (fraction * 8.0).round() as u32 % 8;
    match bucket {
        0 => MoonPhaseKind::New,
        1 => MoonPhaseKind::WaxingCrescent,
        2 => MoonPhaseKind::FirstQuarter,
        3 => MoonPhaseKind::WaxingGibbous,
        4 => MoonPhaseKind::Full,
        5 => MoonPhaseKind::WaningGibbous,
        6 => MoonPhaseKind::LastQuarter,
        _ => MoonPhaseKind::WaningCrescent,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct DonjonCalendarJson {
    year_len: u32,
    #[serde(default)]
    events: u32,
    n_months: u32,
    months: Vec<String>,
    month_len: HashMap<String, u32>,
    week_len: u32,
    weekdays: Vec<String>,
    n_moons: u32,
    moons: Vec<String>,
    #[serde(default)]
    lunar_cyc: HashMap<String, u32>,
    // Signed: a lunar shift is a phase offset that can be negative. Must match the
    // `i32` type read back in `lunar_shifts`, or a negative shift fails to import.
    #[serde(default)]
    lunar_shf: HashMap<String, i32>,
    #[serde(default)]
    year: i64,
    first_day: u32,
    #[serde(default)]
    notes: HashMap<String, serde_json::Value>,
}

pub fn import_donjon_json(json_content: &str) -> Result<StoredCalendar> {
    let donjon: DonjonCalendarJson =
        serde_json::from_str(json_content).context("failed to parse donjon calendar JSON")?;

    let mut notes = donjon.notes;
    if !donjon.lunar_cyc.is_empty() {
        notes.insert(
            "lunar_cyc".to_string(),
            serde_json::to_value(&donjon.lunar_cyc)?,
        );
    }
    if !donjon.lunar_shf.is_empty() {
        notes.insert(
            "lunar_shf".to_string(),
            serde_json::to_value(&donjon.lunar_shf)?,
        );
    }

    let definition = CalendarDefinition {
        year_len: donjon.year_len,
        months: donjon.months,
        month_len: donjon.month_len,
        week_len: donjon.week_len,
        weekdays: donjon.weekdays,
        moons: donjon.moons,
        notes,
        first_day: donjon.first_day,
    };

    definition
        .validate()
        .context("calendar validation failed")?;

    let state = CalendarState::default();

    Ok(StoredCalendar { definition, state })
}

pub fn load_calendar() -> Result<Option<StoredCalendar>> {
    let paths = config_paths(Path::new(""))?;
    if !paths.calendar.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&paths.calendar)
        .with_context(|| format!("failed to read calendar file {}", paths.calendar.display()))?;

    let calendar: StoredCalendar = toml::from_str(&content).with_context(|| {
        format!(
            "failed to parse calendar TOML from {}",
            paths.calendar.display()
        )
    })?;

    calendar.validate().context("loaded calendar is invalid")?;

    Ok(Some(calendar))
}

pub fn save_calendar(calendar: &StoredCalendar) -> Result<PathBuf> {
    calendar
        .validate()
        .context("cannot save invalid calendar")?;

    let paths = config_paths(Path::new(""))?;

    if let Some(parent) = paths.calendar.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create calendar directory {}", parent.display()))?;
    }

    let content =
        toml::to_string_pretty(calendar).context("failed to serialize calendar to TOML")?;

    std::fs::write(&paths.calendar, content)
        .with_context(|| format!("failed to write calendar file {}", paths.calendar.display()))?;

    Ok(paths.calendar)
}

pub fn calendar_toml_path() -> Result<PathBuf> {
    let paths = config_paths(Path::new(""))?;
    Ok(paths.calendar)
}

pub fn weekday_index(state: &CalendarState, def: &CalendarDefinition) -> usize {
    if def.week_len == 0 {
        return 0;
    }
    // Derive from the absolute day count so the weekday advances across year
    // boundaries (when `year_len % week_len != 0`) and stays consistent with the
    // moon-phase math. `total_days_since_epoch` already folds in the year and uses
    // saturating day arithmetic, so this also avoids the previous `day - 1`
    // underflow and the unchecked `def.months[i]` index.
    let total_days = total_days_since_epoch(state, def);
    (i64::from(def.first_day) + total_days).rem_euclid(i64::from(def.week_len)) as usize
}

pub fn ordinal_suffix(day: u32) -> &'static str {
    // Custom calendars allow months longer than 100 days, so derive the suffix
    // from the last two digits (11/12/13 are always "th") rather than enumerating.
    match (day % 100, day % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    }
}

pub fn format_date_conversational(calendar: &StoredCalendar) -> String {
    let month_name = calendar
        .definition
        .months
        .get(calendar.state.month_index)
        .map(|s| s.as_str())
        .unwrap_or("Unknown");

    let day_with_suffix = format!(
        "{}{}",
        calendar.state.day,
        ordinal_suffix(calendar.state.day)
    );

    let weekday = calendar
        .definition
        .weekdays
        .get(weekday_index(&calendar.state, &calendar.definition))
        .map(|s| s.as_str())
        .unwrap_or("Unknown");

    let hour_12 = if calendar.state.hour_24 == 0 {
        12
    } else if calendar.state.hour_24 > 12 {
        calendar.state.hour_24 - 12
    } else {
        calendar.state.hour_24
    };

    let am_pm = if calendar.state.hour_24 < 12 {
        "AM"
    } else {
        "PM"
    };

    let minute_str = format!("{:02}", calendar.state.minute);

    format!(
        "It is the {} of {} in the year {} at {}:{} {} ({})",
        day_with_suffix, month_name, calendar.state.year, hour_12, minute_str, am_pm, weekday
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_donjon_json_to_toml() {
        let json = r#"{
            "year_len": 300,
            "events": 0,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 1,
            "moons": ["Moon"],
            "lunar_cyc": {"Moon": 25},
            "lunar_shf": {"Moon": 0},
            "year": 1330,
            "first_day": 0,
            "notes": {}
        }"#;

        let calendar = import_donjon_json(json).expect("import should succeed");
        assert_eq!(calendar.definition.year_len, 300);
        assert_eq!(calendar.definition.months.len(), 6);
        assert_eq!(calendar.state.year, 0);
        assert_eq!(calendar.state.month_index, 0);
        assert_eq!(calendar.state.day, 1);

        let toml_str = toml::to_string_pretty(&calendar).expect("should serialize to TOML");
        let reparsed: StoredCalendar = toml::from_str(&toml_str).expect("should parse from TOML");

        assert_eq!(reparsed.definition.year_len, 300);
        assert_eq!(reparsed.definition.months, calendar.definition.months);
        assert_eq!(reparsed.definition.moons, vec!["Moon".to_string()]);
    }

    #[test]
    fn import_stores_moons_in_notes() {
        let json = r#"{
            "year_len": 360,
            "n_months": 12,
            "months": ["Month1"],
            "month_len": {"Month1": 360},
            "week_len": 7,
            "weekdays": ["Day1", "Day2", "Day3", "Day4", "Day5", "Day6", "Day7"],
            "n_moons": 1,
            "moons": ["Luna"],
            "lunar_cyc": {"Luna": 28},
            "lunar_shf": {"Luna": 7},
            "first_day": 0,
            "notes": {}
        }"#;

        let calendar = import_donjon_json(json).expect("import should succeed");
        assert_eq!(calendar.definition.moons, vec!["Luna"]);
        assert!(calendar.definition.notes.contains_key("lunar_cyc"));
        assert!(calendar.definition.notes.contains_key("lunar_shf"));
    }

    #[test]
    fn invalid_month_length_detected() {
        let json = r#"{
            "year_len": 300,
            "n_months": 2,
            "months": ["Jan", "Feb"],
            "month_len": {"Jan": 100, "Feb": 100},
            "week_len": 7,
            "weekdays": ["Day1", "Day2", "Day3", "Day4", "Day5", "Day6", "Day7"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;

        let result = import_donjon_json(json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let error_string = err
            .chain()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_string.contains("does not match year_len"),
            "error chain: {}",
            error_string
        );
    }

    #[test]
    fn missing_month_in_month_len_detected() {
        let json = r#"{
            "year_len": 100,
            "n_months": 2,
            "months": ["Jan", "Feb"],
            "month_len": {"Jan": 100},
            "week_len": 7,
            "weekdays": ["Day1", "Day2", "Day3", "Day4", "Day5", "Day6", "Day7"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;

        let result = import_donjon_json(json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let error_string = err
            .chain()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_string.contains("invalid length 0")
                || error_string.contains("does not match year_len"),
            "error chain: {}",
            error_string
        );
    }

    #[test]
    fn calendar_state_default_values() {
        let state = CalendarState::default();
        assert_eq!(state.year, 0);
        assert_eq!(state.month_index, 0);
        assert_eq!(state.day, 1);
        assert_eq!(state.hour_24, 0);
        assert_eq!(state.minute, 0);
    }

    #[test]
    fn calendar_state_reset() {
        let mut state = CalendarState {
            year: 1330,
            month_index: 5,
            day: 28,
            hour_24: 23,
            minute: 59,
        };
        state.reset();
        assert_eq!(state.year, 0);
        assert_eq!(state.month_index, 0);
        assert_eq!(state.day, 1);
        assert_eq!(state.hour_24, 0);
        assert_eq!(state.minute, 0);
    }

    #[test]
    fn ordinal_suffix_st() {
        assert_eq!(ordinal_suffix(1), "st");
        assert_eq!(ordinal_suffix(21), "st");
        assert_eq!(ordinal_suffix(31), "st");
    }

    #[test]
    fn ordinal_suffix_nd() {
        assert_eq!(ordinal_suffix(2), "nd");
        assert_eq!(ordinal_suffix(22), "nd");
    }

    #[test]
    fn ordinal_suffix_rd() {
        assert_eq!(ordinal_suffix(3), "rd");
        assert_eq!(ordinal_suffix(23), "rd");
    }

    #[test]
    fn ordinal_suffix_th() {
        assert_eq!(ordinal_suffix(4), "th");
        assert_eq!(ordinal_suffix(11), "th");
        assert_eq!(ordinal_suffix(12), "th");
        assert_eq!(ordinal_suffix(13), "th");
        assert_eq!(ordinal_suffix(14), "th");
        assert_eq!(ordinal_suffix(24), "th");
    }

    #[test]
    fn weekday_index_first_day_zero() {
        let json = r#"{
            "year_len": 14,
            "n_months": 2,
            "months": ["Jan", "Feb"],
            "month_len": {"Jan": 7, "Feb": 7},
            "week_len": 7,
            "weekdays": ["Day1", "Day2", "Day3", "Day4", "Day5", "Day6", "Day7"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");

        let state = CalendarState {
            year: 0,
            month_index: 0,
            day: 1,
            hour_24: 0,
            minute: 0,
        };
        assert_eq!(weekday_index(&state, &calendar.definition), 0);

        let state = CalendarState {
            year: 0,
            month_index: 0,
            day: 3,
            hour_24: 0,
            minute: 0,
        };
        assert_eq!(weekday_index(&state, &calendar.definition), 2);
    }

    #[test]
    fn weekday_index_first_day_non_zero() {
        let json = r#"{
            "year_len": 14,
            "n_months": 2,
            "months": ["Jan", "Feb"],
            "month_len": {"Jan": 7, "Feb": 7},
            "week_len": 7,
            "weekdays": ["Day1", "Day2", "Day3", "Day4", "Day5", "Day6", "Day7"],
            "n_moons": 0,
            "moons": [],
            "first_day": 3,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");

        let state = CalendarState {
            year: 0,
            month_index: 0,
            day: 1,
            hour_24: 0,
            minute: 0,
        };
        assert_eq!(weekday_index(&state, &calendar.definition), 3);
    }

    #[test]
    fn format_date_midnight() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 1,
            "moons": ["Moon"],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");

        let formatted = format_date_conversational(&calendar);
        assert_eq!(
            formatted,
            "It is the 1st of Emberwane in the year 0 at 12:00 AM (Moonday)"
        );
    }

    #[test]
    fn format_date_afternoon() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 1,
            "moons": ["Moon"],
            "first_day": 3,
            "notes": {}
        }"#;
        let mut calendar = import_donjon_json(json).expect("import should succeed");
        calendar.state.day = 15;
        calendar.state.hour_24 = 14;
        calendar.state.minute = 30;

        let formatted = format_date_conversational(&calendar);
        assert_eq!(
            formatted,
            "It is the 15th of Emberwane in the year 0 at 2:30 PM (Fithday)"
        );
    }

    #[test]
    fn set_year_rejects_negative() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");
        let mut state = calendar.state;

        let result = state.set_year(-1);
        assert!(result.is_err());
        assert_eq!(state.year, 0);
    }

    #[test]
    fn set_year_accepts_zero() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");
        let mut state = calendar.state;

        state.set_year(5).expect("should accept year 5");
        assert_eq!(state.year, 5);
    }

    #[test]
    fn set_day_rejects_zero() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");
        let mut state = calendar.state;

        let result = state.set_day(0, &calendar.definition);
        assert!(result.is_err());
    }

    #[test]
    fn set_day_rejects_too_large() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");
        let mut state = calendar.state;

        let result = state.set_day(51, &calendar.definition);
        assert!(result.is_err());
    }

    #[test]
    fn calendar_delta_parses_units() {
        let delta = CalendarDelta::from_parts('+', "12h").expect("delta should parse");
        assert!(delta.is_positive());
        assert_eq!(delta.magnitude(), 12);
        assert_eq!(delta.unit(), CalendarDeltaUnit::Hours);

        let negative = CalendarDelta::from_parts('-', "3d").expect("delta should parse");
        assert!(!negative.is_positive());
        assert_eq!(negative.unit(), CalendarDeltaUnit::Days);
    }

    #[test]
    fn apply_calendar_delta_adds_hours() {
        let json = r#"{
            "year_len": 300,
            "n_months": 6,
            "months": ["Emberwane", "Stonewake", "Highbloom", "Redreach", "Goldfall", "Deepnight"],
            "month_len": {"Emberwane": 50, "Stonewake": 50, "Highbloom": 50, "Redreach": 50, "Goldfall": 50, "Deepnight": 50},
            "week_len": 7,
            "weekdays": ["Moonday", "Thirday", "Midweekday", "Fithday", "Fastday", "Restday", "Sunday"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let mut calendar = import_donjon_json(json).expect("import should succeed");
        calendar.state.hour_24 = 10;
        calendar.state.minute = 45;

        let delta = CalendarDelta::from_parts('+', "3h").expect("delta should parse");
        apply_calendar_delta(&mut calendar.state, &calendar.definition, delta)
            .expect("delta should apply");

        assert_eq!(calendar.state.hour_24, 13);
        assert_eq!(calendar.state.minute, 45);
    }

    #[test]
    fn apply_calendar_delta_rolls_over_month() {
        let json = r#"{
            "year_len": 100,
            "n_months": 2,
            "months": ["First", "Second"],
            "month_len": {"First": 50, "Second": 50},
            "week_len": 5,
            "weekdays": ["D1", "D2", "D3", "D4", "D5"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let mut calendar = import_donjon_json(json).expect("import should succeed");
        calendar.state.month_index = 0;
        calendar.state.day = 50;
        calendar.state.hour_24 = 23;
        calendar.state.minute = 0;

        let delta = CalendarDelta::from_parts('+', "2d").expect("delta should parse");
        apply_calendar_delta(&mut calendar.state, &calendar.definition, delta)
            .expect("delta should apply");

        assert_eq!(calendar.state.month_index, 1);
        assert_eq!(calendar.state.day, 2);
        assert_eq!(calendar.state.hour_24, 23);
    }

    #[test]
    fn apply_calendar_delta_clamps_negative_values() {
        let json = r#"{
            "year_len": 60,
            "n_months": 2,
            "months": ["First", "Second"],
            "month_len": {"First": 30, "Second": 30},
            "week_len": 6,
            "weekdays": ["D1", "D2", "D3", "D4", "D5", "D6"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let mut calendar = import_donjon_json(json).expect("import should succeed");
        calendar.state.year = 5;
        calendar.state.month_index = 1;
        calendar.state.day = 10;
        calendar.state.hour_24 = 12;
        calendar.state.minute = 30;

        let delta = CalendarDelta::from_parts('-', "400d").expect("delta should parse");
        apply_calendar_delta(&mut calendar.state, &calendar.definition, delta)
            .expect("delta should apply");

        assert_eq!(calendar.state.year, 0);
        assert_eq!(calendar.state.month_index, 0);
        assert_eq!(calendar.state.day, 1);
        assert_eq!(calendar.state.hour_24, 0);
        assert_eq!(calendar.state.minute, 0);
    }

    #[test]
    fn computes_basic_moon_phase() {
        let json = r#"{
            "year_len": 28,
            "events": 0,
            "n_months": 1,
            "months": ["Luna"],
            "month_len": {"Luna": 28},
            "week_len": 7,
            "weekdays": ["D1", "D2", "D3", "D4", "D5", "D6", "D7"],
            "n_moons": 1,
            "moons": ["Selene"],
            "lunar_cyc": {"Selene": 28},
            "lunar_shf": {"Selene": 0},
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");
        let phases = moon_phase_info(&calendar).expect("phase info");
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].phase, MoonPhaseKind::New);
        assert_eq!(phases[0].age, 0);
    }

    #[test]
    fn moon_phase_respects_shift_and_day_progression() {
        let json = r#"{
            "year_len": 60,
            "events": 0,
            "n_months": 2,
            "months": ["First", "Second"],
            "month_len": {"First": 30, "Second": 30},
            "week_len": 5,
            "weekdays": ["D1", "D2", "D3", "D4", "D5"],
            "n_moons": 1,
            "moons": ["Luna"],
            "lunar_cyc": {"Luna": 20},
            "lunar_shf": {"Luna": 3},
            "first_day": 0,
            "notes": {}
        }"#;
        let mut calendar = import_donjon_json(json).expect("import should succeed");
        calendar.state.day = 10;
        let info = moon_phase_info(&calendar).expect("phase info");
        assert_eq!(info[0].cycle_length, 20);
        assert_eq!(info[0].age, (9 + 3) as u32);
    }

    #[test]
    fn weekday_advances_across_year_boundary() {
        // year_len (10) is not a multiple of week_len (7), so New Year's Day must
        // land on a different weekday each year. Regression for weekday_index
        // ignoring state.year.
        let json = r#"{
            "year_len": 10,
            "n_months": 1,
            "months": ["M"],
            "month_len": {"M": 10},
            "week_len": 7,
            "weekdays": ["D1", "D2", "D3", "D4", "D5", "D6", "D7"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");

        let new_years = |year: i32| CalendarState {
            year,
            month_index: 0,
            day: 1,
            hour_24: 0,
            minute: 0,
        };
        let y0 = weekday_index(&new_years(0), &calendar.definition);
        let y1 = weekday_index(&new_years(1), &calendar.definition);
        let y2 = weekday_index(&new_years(2), &calendar.definition);
        assert_eq!(y0, 0);
        assert_eq!(y1, 3); // 10 days later, 10 % 7 == 3
        assert_eq!(y2, 6); // 20 days later, 20 % 7 == 6
        assert_ne!(y0, y1, "weekday must advance across the year boundary");
    }

    #[test]
    fn weekday_index_tolerates_malformed_state() {
        // day == 0 must not panic (was `state.day - 1`).
        let json = r#"{
            "year_len": 14,
            "n_months": 2,
            "months": ["Jan", "Feb"],
            "month_len": {"Jan": 7, "Feb": 7},
            "week_len": 7,
            "weekdays": ["D1", "D2", "D3", "D4", "D5", "D6", "D7"],
            "n_moons": 0,
            "moons": [],
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("import should succeed");
        let state = CalendarState {
            year: 0,
            month_index: 0,
            day: 0,
            hour_24: 0,
            minute: 0,
        };
        // Must not panic; value is unspecified for malformed input but bounded.
        let _ = weekday_index(&state, &calendar.definition);
    }

    #[test]
    fn phase_from_age_centers_principal_phases() {
        let cycle = 28;
        assert_eq!(phase_from_age(0, cycle), MoonPhaseKind::New);
        assert_eq!(
            phase_from_age(cycle / 4, cycle),
            MoonPhaseKind::FirstQuarter
        );
        assert_eq!(phase_from_age(cycle / 2, cycle), MoonPhaseKind::Full);
        assert_eq!(
            phase_from_age(3 * cycle / 4, cycle),
            MoonPhaseKind::LastQuarter
        );
    }

    #[test]
    fn import_accepts_negative_lunar_shift() {
        // donjon shifts can be negative; the import struct must read them as i32.
        let json = r#"{
            "year_len": 28,
            "n_months": 1,
            "months": ["Luna"],
            "month_len": {"Luna": 28},
            "week_len": 7,
            "weekdays": ["D1", "D2", "D3", "D4", "D5", "D6", "D7"],
            "n_moons": 1,
            "moons": ["Selene"],
            "lunar_cyc": {"Selene": 28},
            "lunar_shf": {"Selene": -3},
            "first_day": 0,
            "notes": {}
        }"#;
        let calendar = import_donjon_json(json).expect("negative shift should import");
        let info = moon_phase_info(&calendar).expect("phase info");
        // total_days == 0 at day 1, shift -3 → age = (-3).rem_euclid(28) == 25
        assert_eq!(info[0].age, 25);
    }

    #[test]
    fn ordinal_suffix_large_days() {
        assert_eq!(ordinal_suffix(101), "st");
        assert_eq!(ordinal_suffix(102), "nd");
        assert_eq!(ordinal_suffix(103), "rd");
        assert_eq!(ordinal_suffix(111), "th");
        assert_eq!(ordinal_suffix(112), "th");
        assert_eq!(ordinal_suffix(113), "th");
        assert_eq!(ordinal_suffix(121), "st");
        assert_eq!(ordinal_suffix(122), "nd");
        assert_eq!(ordinal_suffix(123), "rd");
    }
}
