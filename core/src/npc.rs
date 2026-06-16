pub use runebound_models::{
    EventFrontmatter, FactionFrontmatter, ItemFrontmatter, LocationFrontmatter, NpcFrontmatter,
    UNKNOWN_LOCATION, make_entity_id, normalize_markdown_file_stem, normalize_unknown_list,
    normalize_unknown_text, now_timestamp, slugify, unique_slug_for_dir,
    unique_slug_for_dir_with_ext,
};

#[cfg(test)]
mod tests {
    use super::normalize_markdown_file_stem;

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
}
