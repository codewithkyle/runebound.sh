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
}
