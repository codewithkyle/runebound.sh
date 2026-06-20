use runebound_models::utils::{normalize_exports, normalize_unknown_list, parse_list_csv};

pub fn exports_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

pub fn exports_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_exports(items),
        Err(_) => normalize_exports(parse_list_csv(value)),
    }
}

pub fn carrying_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

pub fn carrying_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_unknown_list(items),
        Err(_) => normalize_unknown_list(parse_list_csv(value)),
    }
}

pub fn faction_list_to_db_text(items: &[String]) -> Result<String, String> {
    serde_json::to_string(items).map_err(|err| err.to_string())
}

pub fn faction_list_from_db_text(value: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(items) => normalize_unknown_list(items),
        Err(_) => normalize_unknown_list(parse_list_csv(value)),
    }
}

/// Like [`faction_list_from_db_text`] but for the relational link lists (faction
/// allies/rivals) that are "link or leave blank" (D4): trim + drop empties, and
/// crucially **preserve an empty result** instead of injecting a placeholder
/// "Unknown". This is the read-side mirror of `clean_link_list` on the save path, so
/// a blank-stub list survives a save -> reload round-trip as blank.
pub fn faction_link_list_from_db_text(value: &str) -> Vec<String> {
    let items =
        serde_json::from_str::<Vec<String>>(value).unwrap_or_else(|_| parse_list_csv(value));
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_exports_json() {
        let exports = vec!["spices".to_string(), "silk".to_string()];
        let json = exports_to_db_text(&exports).expect("serialize exports");
        assert_eq!(exports_from_db_text(&json), exports);
    }

    #[test]
    fn exports_fall_back_to_csv() {
        let parsed = exports_from_db_text("amber resin, moon glass");
        assert_eq!(
            parsed,
            vec!["amber resin".to_string(), "moon glass".to_string()]
        );
    }

    #[test]
    fn carrying_defaults_to_unknown_when_empty() {
        let parsed = carrying_from_db_text("[]");
        assert_eq!(parsed, vec!["Unknown".to_string()]);
    }

    #[test]
    fn faction_lists_handle_invalid_json() {
        let parsed = faction_list_from_db_text("allies, patrons");
        assert_eq!(parsed, vec!["allies".to_string(), "patrons".to_string()]);
    }

    #[test]
    fn faction_link_list_preserves_blank_instead_of_unknown() {
        // The coercing reader injects "Unknown" for an empty list; the link reader
        // keeps it empty (blank-stub D4) so allies/rivals survive a round-trip blank.
        assert_eq!(faction_list_from_db_text("[]"), vec!["Unknown".to_string()]);
        assert!(faction_link_list_from_db_text("[]").is_empty());
        // Non-empty links round-trip (and trim/drop empties), no "Unknown" added.
        assert_eq!(
            faction_link_list_from_db_text("[\"House Vey\", \"  \", \"Dust Choir\"]"),
            vec!["House Vey".to_string(), "Dust Choir".to_string()]
        );
    }
}
