use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
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

        let total_month_days: u32 = self.months.iter()
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
            bail!("first_day ({}) must be less than week_len ({})", self.first_day, self.week_len);
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
        let month_len = self.definition.month_len.get(month_name).copied().unwrap_or(0);
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
}

#[derive(Debug, Clone, Deserialize)]
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
    #[serde(default)]
    lunar_shf: HashMap<String, u32>,
    #[serde(default)]
    year: i64,
    first_day: u32,
    #[serde(default)]
    notes: HashMap<String, serde_json::Value>,
}

pub fn import_donjon_json(json_content: &str) -> Result<StoredCalendar> {
    let donjon: DonjonCalendarJson = serde_json::from_str(json_content)
        .context("failed to parse donjon calendar JSON")?;

    let mut notes = donjon.notes;
    if !donjon.lunar_cyc.is_empty() {
        notes.insert("lunar_cyc".to_string(), serde_json::to_value(&donjon.lunar_cyc)?);
    }
    if !donjon.lunar_shf.is_empty() {
        notes.insert("lunar_shf".to_string(), serde_json::to_value(&donjon.lunar_shf)?);
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

    definition.validate().context("calendar validation failed")?;

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

    let calendar: StoredCalendar = toml::from_str(&content)
        .with_context(|| format!("failed to parse calendar TOML from {}", paths.calendar.display()))?;

    calendar.validate().context("loaded calendar is invalid")?;

    Ok(Some(calendar))
}

pub fn save_calendar(calendar: &StoredCalendar) -> Result<PathBuf> {
    calendar.validate().context("cannot save invalid calendar")?;

    let paths = config_paths(Path::new(""))?;

    if let Some(parent) = paths.calendar.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create calendar directory {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(calendar)
        .context("failed to serialize calendar to TOML")?;

    std::fs::write(&paths.calendar, content)
        .with_context(|| format!("failed to write calendar file {}", paths.calendar.display()))?;

    Ok(paths.calendar)
}

pub fn calendar_toml_path() -> Result<PathBuf> {
    let paths = config_paths(Path::new(""))?;
    Ok(paths.calendar)
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
        let error_string = err.chain().map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
        assert!(error_string.contains("does not match year_len"), "error chain: {}", error_string);
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
        let error_string = err.chain().map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
        assert!(error_string.contains("invalid length 0") || error_string.contains("does not match year_len"),
            "error chain: {}", error_string);
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
}