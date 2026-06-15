use std::path::MAIN_SEPARATOR;

use crate::services::ai_generation::{FactionSeed, LocationSeed};

pub use runebound_models::utils::{
    normalize_exports, normalize_faction_kind_type, normalize_location_danger_level,
    normalize_location_kind_type, normalize_item_category, normalize_item_rarity, normalize_sex,
    normalize_unknown_list, normalize_unknown_text, parse_list_csv,
};

pub fn parse_carrying_csv(value: &str) -> Vec<String> {
    let items: Vec<String> = value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    normalize_unknown_list(items)
}

pub fn normalize_optional_prompt(prompt: Option<String>) -> Option<String> {
    prompt.map(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed.to_string()
        }
    })
}

pub fn normalize_relative_path_for_storage(path: &str) -> String {
    path
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn path_for_display(path: &str) -> String {
    if MAIN_SEPARATOR == '\\' {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

pub fn sentence_count(value: &str) -> usize {
    value
        .split_terminator(['.', '!', '?'])
        .filter(|part| !part.trim().is_empty())
        .count()
}

pub fn word_count(value: &str) -> usize {
    value.split_whitespace().count()
}

/// Rough token estimate for local-model context budgeting (~4 chars per token
/// for English; ceil-divided so it errs slightly high rather than low).
pub fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}

/// Prepend an optional non-blocking notice (e.g. a context-capacity warning) to a
/// response body, separated by a blank line. Returns `body` unchanged when `None`.
pub fn prepend_notice(notice: Option<String>, body: String) -> String {
    match notice {
        Some(notice) => format!("{notice}\n\n{body}"),
        None => body,
    }
}

pub fn validate_sentence_range(value: &str, min: usize, max: usize, field: &str) -> Result<(), String> {
    let count = sentence_count(value);
    if count < min || count > max {
        return Err(format!("{field} must be {min}-{max} sentences; got {count}"));
    }
    Ok(())
}

pub fn normalize_location_seed(mut seed: LocationSeed) -> Result<LocationSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_location_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed.kind_custom.as_ref().is_none_or(|value| value.trim().is_empty()) {
            return Err("kind_custom is required when kind_type is other".to_string());
        }
    } else {
        seed.kind_custom = None;
    }
    seed.visual_description = normalize_unknown_text(&seed.visual_description);
    seed.history_background = normalize_unknown_text(&seed.history_background);
    seed.exports = normalize_exports(seed.exports);
    seed.tone = normalize_unknown_text(&seed.tone);
    seed.authority = normalize_unknown_text(&seed.authority);
    seed.danger_level = normalize_location_danger_level(&seed.danger_level)?;
    seed.current_tension = normalize_unknown_text(&seed.current_tension);
    Ok(seed)
}

pub fn validate_location_details(seed: &LocationSeed) -> Result<(), String> {
    if seed.name.trim().is_empty() {
        return Err("location name cannot be empty".to_string());
    }
    if seed.visual_description != "Unknown" {
        validate_sentence_range(&seed.visual_description, 1, 3, "visual_description")?;
    }
    if seed.history_background != "Unknown" {
        validate_sentence_range(&seed.history_background, 2, 5, "history_background")?;
    }
    if seed.current_tension != "Unknown" {
        validate_sentence_range(&seed.current_tension, 1, 2, "current_tension")?;
    }
    if seed.exports.is_empty() || seed.exports.len() > 3 {
        return Err("exports must have 1-3 items".to_string());
    }
    if !(seed.exports.len() == 1 && seed.exports[0] == "Unknown") {
        let empty_item = seed.exports.iter().any(|item| item.trim().is_empty());
        if empty_item {
            return Err("exports cannot contain empty items".to_string());
        }
    }
    let tone_words = word_count(&seed.tone);
    if seed.tone != "Unknown" && !(2..=5).contains(&tone_words) {
        return Err(format!("tone must be 2-5 words; got {tone_words}"));
    }
    Ok(())
}

pub fn normalize_faction_seed(mut seed: FactionSeed) -> Result<FactionSeed, String> {
    seed.name = seed.name.trim().to_string();
    seed.kind_type = normalize_faction_kind_type(&seed.kind_type)?;
    seed.kind_custom = seed.kind_custom.map(|value| value.trim().to_string());
    if seed.kind_type == "other" {
        if seed.kind_custom.as_ref().is_none_or(|value| value.trim().is_empty()) {
            return Err("kind_custom is required when kind_type is other".to_string());
        }
    } else {
        seed.kind_custom = None;
    }
    seed.public_description = normalize_unknown_text(&seed.public_description);
    seed.true_agenda = normalize_unknown_text(&seed.true_agenda);
    seed.methods = normalize_unknown_text(&seed.methods);
    seed.leadership = normalize_unknown_text(&seed.leadership);
    seed.headquarters = normalize_unknown_text(&seed.headquarters);
    seed.sphere_of_influence = normalize_unknown_text(&seed.sphere_of_influence);
    seed.resources_assets = normalize_unknown_text(&seed.resources_assets);
    seed.allies = normalize_unknown_list(seed.allies);
    seed.rivals_enemies = normalize_unknown_list(seed.rivals_enemies);
    seed.reputation = normalize_unknown_text(&seed.reputation);
    seed.current_tension = normalize_unknown_text(&seed.current_tension);
    seed.goals_short_term = normalize_unknown_list(seed.goals_short_term);
    seed.goals_long_term = normalize_unknown_list(seed.goals_long_term);
    seed.symbol_description = normalize_unknown_text(&seed.symbol_description);
    Ok(seed)
}

pub fn validate_faction_details(seed: &FactionSeed) -> Result<(), String> {
    if seed.name.trim().is_empty() {
        return Err("faction name cannot be empty".to_string());
    }
    if seed.public_description != "Unknown" {
        validate_sentence_range(&seed.public_description, 1, 3, "public_description")?;
    }
    if seed.true_agenda != "Unknown" {
        validate_sentence_range(&seed.true_agenda, 1, 3, "true_agenda")?;
    }
    if seed.current_tension != "Unknown" {
        validate_sentence_range(&seed.current_tension, 1, 2, "current_tension")?;
    }
    if seed.symbol_description != "Unknown" {
        validate_sentence_range(&seed.symbol_description, 1, 1, "symbol_description")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        estimate_tokens, normalize_faction_seed, normalize_location_seed,
        normalize_relative_path_for_storage, path_for_display, prepend_notice,
        validate_faction_details, validate_location_details,
    };
    use crate::services::ai_generation::{FactionSeed, LocationSeed};

    #[test]
    fn estimate_tokens_is_roughly_chars_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        // 8 chars -> 2 tokens; ceil division rounds partials up.
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn prepend_notice_adds_blank_line_when_present() {
        assert_eq!(prepend_notice(None, "body".to_string()), "body");
        assert_eq!(
            prepend_notice(Some("warn".to_string()), "body".to_string()),
            "warn\n\nbody"
        );
    }

    #[test]
    fn normalizes_storage_paths_to_forward_slashes() {
        assert_eq!(
            normalize_relative_path_for_storage(r"npcs\\grave cleric.md"),
            "npcs/grave cleric.md"
        );
    }

    #[test]
    fn displays_paths_with_host_separator() {
        let displayed = path_for_display("locations/frostholm.md");
        if std::path::MAIN_SEPARATOR == '\\' {
            assert_eq!(displayed, r"locations\\frostholm.md");
        } else {
            assert_eq!(displayed, "locations/frostholm.md");
        }
    }

    #[test]
    fn location_seed_requires_custom_kind_for_other() {
        let seed = LocationSeed {
            name: "Gloomreach".to_string(),
            kind_type: "other".to_string(),
            kind_custom: None,
            visual_description: "Moss-slick walls drip in torchlight.".to_string(),
            history_background: "Built by exiles. Later seized by smugglers.".to_string(),
            exports: vec!["amber resin".to_string()],
            tone: "wet tense".to_string(),
            authority: "Smuggler council".to_string(),
            danger_level: "risky".to_string(),
            current_tension: "A rival gang stalks the tunnels.".to_string(),
        };

        let err = normalize_location_seed(seed).expect_err("expected missing kind_custom error");
        assert!(err.contains("kind_custom"));
    }

    #[test]
    fn location_seed_validation_accepts_unknown_backcompat_values() {
        let seed = LocationSeed {
            name: "Unknown Hold".to_string(),
            kind_type: "other".to_string(),
            kind_custom: Some("Unknown".to_string()),
            visual_description: "Unknown".to_string(),
            history_background: "Unknown".to_string(),
            exports: vec!["Unknown".to_string()],
            tone: "Unknown".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
        };

        let normalized = normalize_location_seed(seed).expect("normalize succeeds");
        validate_location_details(&normalized)
            .expect("expected Unknown defaults to pass validation");
    }

    #[test]
    fn faction_seed_validation_enforces_sentence_ranges() {
        let seed = FactionSeed {
            name: "Amber Syndicate".to_string(),
            kind_type: "guild".to_string(),
            kind_custom: None,
            public_description: "Unknown".to_string(),
            true_agenda: "Unknown".to_string(),
            methods: "Unknown".to_string(),
            leadership: "Unknown".to_string(),
            headquarters: "Unknown".to_string(),
            sphere_of_influence: "Unknown".to_string(),
            resources_assets: "Unknown".to_string(),
            allies: vec!["Unknown".to_string()],
            rivals_enemies: vec!["Unknown".to_string()],
            reputation: "Unknown".to_string(),
            current_tension: "Unknown".to_string(),
            goals_short_term: vec!["Unknown".to_string()],
            goals_long_term: vec!["Unknown".to_string()],
            symbol_description: "Unknown".to_string(),
        };

        let normalized = normalize_faction_seed(seed).expect("normalize succeeds");
        validate_faction_details(&normalized).expect("validation succeeds");
    }
}
