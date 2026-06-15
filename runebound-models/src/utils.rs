use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const UNKNOWN_LOCATION: &str = "Unknown";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LocationKindType {
    Hamlet,
    Town,
    City,
    Dungeon,
    Hideout,
    Ruin,
    Guildhall,
    Landmark,
    Wilderness,
    Other,
}

impl LocationKindType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LocationKindType::Hamlet => "hamlet",
            LocationKindType::Town => "town",
            LocationKindType::City => "city",
            LocationKindType::Dungeon => "dungeon",
            LocationKindType::Hideout => "hideout",
            LocationKindType::Ruin => "ruin",
            LocationKindType::Guildhall => "guildhall",
            LocationKindType::Landmark => "landmark",
            LocationKindType::Wilderness => "wilderness",
            LocationKindType::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FactionKindType {
    Guild,
    Cult,
    MilitaryOrder,
    NobleHouse,
    CriminalSyndicate,
    MercantileLeague,
    ReligiousOrder,
    ArcaneCircle,
    RevolutionaryCell,
    Other,
}

impl FactionKindType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FactionKindType::Guild => "guild",
            FactionKindType::Cult => "cult",
            FactionKindType::MilitaryOrder => "military_order",
            FactionKindType::NobleHouse => "noble_house",
            FactionKindType::CriminalSyndicate => "criminal_syndicate",
            FactionKindType::MercantileLeague => "mercantile_league",
            FactionKindType::ReligiousOrder => "religious_order",
            FactionKindType::ArcaneCircle => "arcane_circle",
            FactionKindType::RevolutionaryCell => "revolutionary_cell",
            FactionKindType::Other => "other",
        }
    }
}

pub const LOCATION_KIND_TYPES: [&str; 10] = [
    "hamlet",
    "town",
    "city",
    "dungeon",
    "hideout",
    "ruin",
    "guildhall",
    "landmark",
    "wilderness",
    "other",
];

pub const LOCATION_DANGER_LEVELS: [&str; 5] = ["Unknown", "safe", "guarded", "risky", "deadly"];

pub const FACTION_KIND_TYPES: [&str; 10] = [
    "guild",
    "cult",
    "military_order",
    "noble_house",
    "criminal_syndicate",
    "mercantile_league",
    "religious_order",
    "arcane_circle",
    "revolutionary_cell",
    "other",
];

pub const ITEM_CATEGORIES: [&str; 8] = [
    "weapon",
    "armor",
    "consumable",
    "wondrous",
    "arcane_focus",
    "tool",
    "trinket",
    "other",
];

pub const ITEM_RARITIES: [&str; 7] = [
    "unknown",
    "common",
    "uncommon",
    "rare",
    "very_rare",
    "legendary",
    "artifact",
];

pub fn now_timestamp() -> String {
    Utc::now().to_rfc3339()
}

pub fn make_entity_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Utc::now().format("%Y%m%d%H%M%S%3f"))
}

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;

    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if (ch.is_ascii_whitespace() || ch == '-' || ch == '_' || ch == '.') && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn unique_slug_for_dir(root: &Path, relative_dir: &str, base_slug: &str) -> String {
    unique_slug_for_dir_with_ext(root, relative_dir, base_slug, "md")
}

pub fn unique_slug_for_dir_with_ext(
    root: &Path,
    relative_dir: &str,
    base_slug: &str,
    extension: &str,
) -> String {
    let mut candidate = base_slug.to_string();
    let mut idx = 2;

    loop {
        let path = root
            .join(relative_dir)
            .join(format!("{candidate}.{extension}"));
        if !path.exists() {
            return candidate;
        }

        candidate = format!("{base_slug}-{idx}");
        idx += 1;
    }
}

pub fn normalize_markdown_file_stem(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;

    for ch in value.trim().chars() {
        if ch.is_control() {
            continue;
        }

        let invalid = matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
        let treated_as_space =
            invalid || ch.is_whitespace() || matches!(ch, '-' | '_' | '\u{2013}' | '\u{2014}');
        if treated_as_space {
            if !out.is_empty() && !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
            continue;
        }

        out.push(ch);
        last_was_space = false;
    }

    let trimmed = out.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        return "Untitled".to_string();
    }

    if should_title_case(trimmed) {
        trimmed
            .split_whitespace()
            .map(title_case_word)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        trimmed.to_string()
    }
}

fn should_title_case(value: &str) -> bool {
    let mut has_alpha = false;
    let mut has_lower = false;
    let mut has_upper = false;

    for ch in value.chars() {
        if !ch.is_alphabetic() {
            continue;
        }
        has_alpha = true;
        if ch.is_lowercase() {
            has_lower = true;
        } else if ch.is_uppercase() {
            has_upper = true;
        }
    }

    has_alpha && (!has_lower || !has_upper)
}

fn title_case_word(word: &str) -> String {
    let mut result = String::with_capacity(word.len());
    let mut first_alpha_found = false;

    for ch in word.chars() {
        if ch.is_alphabetic() {
            if !first_alpha_found {
                result.push(ch.to_ascii_uppercase());
                first_alpha_found = true;
            } else {
                result.push(ch.to_ascii_lowercase());
            }
        } else {
            result.push(ch);
        }
    }

    result
}

pub fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "Unknown".to_string();
    }

    let sanitized = trimmed
        .trim_matches(|ch: char| matches!(ch, ',' | ';'))
        .trim();

    if sanitized.is_empty() {
        "Unknown".to_string()
    } else {
        sanitized.to_string()
    }
}

pub fn normalize_unknown_list(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();

    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

pub fn normalize_sex(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "male" || normalized == "female" {
        Ok(normalized)
    } else {
        Err("sex must be one of: male, female".to_string())
    }
}

pub fn normalize_location_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if LOCATION_KIND_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "kind_type must be one of: {}",
            LOCATION_KIND_TYPES.join(", ")
        ))
    }
}

pub fn normalize_location_danger_level(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let normalized = if trimmed.eq_ignore_ascii_case("unknown") {
        "Unknown".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    };
    if LOCATION_DANGER_LEVELS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "danger_level must be one of: {}",
            LOCATION_DANGER_LEVELS.join(", ")
        ))
    }
}

pub fn normalize_faction_kind_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if FACTION_KIND_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "kind_type must be one of: {}",
            FACTION_KIND_TYPES.join(", ")
        ))
    }
}

pub fn normalize_item_category(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if ITEM_CATEGORIES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "category must be one of: {}",
            ITEM_CATEGORIES.join(", ")
        ))
    }
}

pub fn normalize_item_rarity(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if ITEM_RARITIES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "rarity must be one of: {}",
            ITEM_RARITIES.join(", ")
        ))
    }
}

pub fn parse_list_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn normalize_exports(values: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if cleaned.is_empty() {
        vec!["Unknown".to_string()]
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_markdown_file_stem_keeps_readable_name() {
        assert_eq!(
            normalize_markdown_file_stem("  Lady Aria of Neverwinter  "),
            "Lady Aria of Neverwinter"
        );
    }

    #[test]
    fn normalize_markdown_file_stem_replaces_invalid_chars() {
        assert_eq!(
            normalize_markdown_file_stem("Drizzt/Do'Urden: Ranger?"),
            "Drizzt Do'Urden Ranger"
        );
    }

    #[test]
    fn normalize_markdown_file_stem_title_cases_kebab_and_snake_inputs() {
        assert_eq!(
            normalize_markdown_file_stem("lady-aria-of-neverwinter"),
            "Lady Aria Of Neverwinter"
        );
        assert_eq!(
            normalize_markdown_file_stem("__ashen_guard__"),
            "Ashen Guard"
        );
    }

    #[test]
    fn normalize_unknown_text_empty() {
        assert_eq!(normalize_unknown_text(""), "Unknown");
        assert_eq!(normalize_unknown_text("   "), "Unknown");
    }

    #[test]
    fn normalize_unknown_text_preserves() {
        assert_eq!(normalize_unknown_text("something"), "something");
    }

    #[test]
    fn normalize_unknown_text_trims_edge_commas() {
        assert_eq!(normalize_unknown_text(",133"), "133");
        assert_eq!(normalize_unknown_text("243,"), "243");
        assert_eq!(normalize_unknown_text(",523,"), "523");
    }
}
