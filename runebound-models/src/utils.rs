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

pub const GOD_RANKS: [&str; 6] = [
    "greater",
    "intermediate",
    "lesser",
    "demigod",
    "dead",
    "other",
];

pub const GOD_ALIGNMENTS: [&str; 9] = [
    "LG", "NG", "CG", "LN", "TN", "CN", "LE", "NE", "CE",
];

pub const DUNGEON_FUNCTIONS: [&str; 5] =
    ["Entrance", "Puzzle", "Setback", "Climax", "Resolution"];

pub const DUNGEON_CONTENT_TYPES: [&str; 12] = [
    "combat",
    "cache",
    "sidekick",
    "offshoot",
    "foreshadowing",
    "history",
    "oddity",
    "forge",
    "factions",
    "map",
    "puzzle",
    "ability_check",
];

pub const DUNGEON_TONES: [&str; 2] = ["tragedy", "comedy"];

pub const DUNGEON_TWISTS: [&str; 3] = ["false_victory", "false_defeat", "neither"];

// "none" is a first-class choice = no topology imposed (feature-dungeons.md §6, step E).
// Order 1..=9 mirrors the topology illustration (topology.png) left-to-right,
// top row then bottom row, so the menu numbering matches the picture.
pub const DUNGEON_TOPOLOGIES: [&str; 10] = [
    "none",
    "The Railroad",
    "The Fauchard Fork",
    "The Paw",
    "Foglio's Snail",
    "The Evil Mule",
    "The V for Vendetta",
    "The Arrow",
    "The Cross",
    "The Moose",
];

pub fn normalize_dungeon_content_type(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if DUNGEON_CONTENT_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "content_type must be one of: {}",
            DUNGEON_CONTENT_TYPES.join(", ")
        ))
    }
}

pub fn normalize_dungeon_tone(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if DUNGEON_TONES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!("tone must be one of: {}", DUNGEON_TONES.join(", ")))
    }
}

pub fn normalize_dungeon_twist(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if DUNGEON_TWISTS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "twist must be one of: {}",
            DUNGEON_TWISTS.join(", ")
        ))
    }
}

/// Resolve a topology to its canonical name. Accepts the exact name
/// (case-insensitive) or `none`; returns an error otherwise.
pub fn normalize_dungeon_topology(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Ok("none".to_string());
    }
    for canonical in DUNGEON_TOPOLOGIES {
        if canonical.eq_ignore_ascii_case(trimmed) {
            return Ok(canonical.to_string());
        }
    }
    Err(format!(
        "topology must be one of: {}",
        DUNGEON_TOPOLOGIES.join(", ")
    ))
}

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

/// Strip leaked `@reference` directory paths (e.g. `@locations/`, `@events/`)
/// from text, keeping only the referenced file name.
///
/// `@some/path/to/Name` is an **input-only** convention: a user types it in a
/// generation prompt to point the LLM at a vault document. The model sometimes
/// echoes that syntax back into generated prose, leaving junk like
/// `@locations/Elyria` in stored fields. We drop the `@` and the directory path
/// (through the final slash) and keep the referenced name (`@events/Harvest
/// Moon` → `Harvest Moon`) — which the publish linker can then turn into a
/// `[[Harvest Moon]]` wikilink. Any directory works, so new Obsidian folders
/// created on the fly need no configuration here.
///
/// Only an `@` at a word boundary followed by a slash-bearing path token is
/// touched, so ordinary text, bare `@handles`, and emails (`gm@example.com`)
/// are left alone.
pub fn strip_reference_syntax(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;

    while i < text.len() {
        if bytes[i] == b'@'
            && reference_start_boundary(text, i)
            && let Some(consumed) = reference_path_prefix_len(&text[i..])
        {
            i += consumed;
            continue;
        }

        let ch = text[i..].chars().next().expect("char at boundary");
        let len = ch.len_utf8();
        result.push_str(&text[i..i + len]);
        i += len;
    }

    result
}

/// An `@` can begin a reference only at the start of the text or after
/// whitespace / an opening bracket or quote — never mid-word (e.g. an email).
fn reference_start_boundary(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_none_or(|c| c.is_whitespace() || matches!(c, '(' | '[' | '{' | '"' | '\''))
}

/// Given `s` starting with `@`, return the byte length of the `@<path>/` prefix
/// to drop — everything from the `@` through the final slash of the reference
/// token. Returns `None` when the `@` token has no slash (a bare `@handle`,
/// not a path reference), so it is left untouched.
fn reference_path_prefix_len(s: &str) -> Option<usize> {
    // The reference token runs from `@` to the first whitespace; the file name
    // may contain spaces, but those fall *after* the final directory slash.
    let token_end = s.find(char::is_whitespace).unwrap_or(s.len());
    let last_slash = s[..token_end].rfind('/')?;
    Some(last_slash + 1)
}

pub fn normalize_unknown_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "Unknown".to_string();
    }

    let stripped = strip_reference_syntax(trimmed);
    let sanitized = stripped
        .trim()
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
        .map(|value| strip_reference_syntax(value.trim()).trim().to_string())
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

pub fn normalize_god_rank(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if GOD_RANKS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!("rank must be one of: {}", GOD_RANKS.join(", ")))
    }
}

pub fn normalize_god_alignment(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_uppercase();
    if GOD_ALIGNMENTS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "alignment must be one of: {}",
            GOD_ALIGNMENTS.join(", ")
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

    #[test]
    fn strip_reference_syntax_removes_dir_prefix_keeping_name() {
        assert_eq!(
            strip_reference_syntax("@locations/Elyria Guard Station"),
            "Elyria Guard Station"
        );
        assert_eq!(strip_reference_syntax("@npcs/Lirael Drake"), "Lirael Drake");
    }

    #[test]
    fn strip_reference_syntax_works_for_any_directory_not_just_known_ones() {
        // Folders created on the fly in Obsidian need no configuration.
        assert_eq!(
            strip_reference_syntax("@events/Harvest Moon Festival"),
            "Harvest Moon Festival"
        );
        assert_eq!(strip_reference_syntax("@quests/The Lost Crown"), "The Lost Crown");
    }

    #[test]
    fn strip_reference_syntax_collapses_nested_paths_to_the_file_name() {
        assert_eq!(
            strip_reference_syntax("@events/festivals/Harvest Moon"),
            "Harvest Moon"
        );
        assert_eq!(strip_reference_syntax("@some/path/to/Klarg"), "Klarg");
    }

    #[test]
    fn strip_reference_syntax_handles_mentions_mid_sentence() {
        assert_eq!(
            strip_reference_syntax("The festival at @events/Harvest Moon draws crowds."),
            "The festival at Harvest Moon draws crowds."
        );
    }

    #[test]
    fn strip_reference_syntax_leaves_emails_and_bare_tokens_alone() {
        // `@` mid-word (email) is not a reference boundary.
        assert_eq!(strip_reference_syntax("reach gm@example.com"), "reach gm@example.com");
        // A bare `@token` with no path is not a directory reference.
        assert_eq!(strip_reference_syntax("warn @everyone now"), "warn @everyone now");
    }

    #[test]
    fn normalize_unknown_text_strips_leaked_reference_prefix() {
        // The reported bug: a generated field carrying the input `@reference`
        // syntax is cleaned to the bare name (which the linker then wikilinks).
        assert_eq!(normalize_unknown_text("@locations/Elyria"), "Elyria");
        assert_eq!(normalize_unknown_text("@events/Harvest Moon"), "Harvest Moon");
    }

    #[test]
    fn normalize_unknown_list_strips_leaked_reference_prefixes() {
        assert_eq!(
            normalize_unknown_list(vec![
                "@npcs/Liam Vesper".to_string(),
                "smoked eel".to_string(),
            ]),
            vec!["Liam Vesper".to_string(), "smoked eel".to_string()]
        );
    }
}
