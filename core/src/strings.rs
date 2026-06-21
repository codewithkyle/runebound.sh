//! Small shared string helpers for the reference-data importers.
//!
//! These were copy-pasted across `spell_import`, `monster_import`, and
//! `monster_copy`; they live here once so the importers share one definition.
//!
//! Note: slug generation is **not** here. There are two intentionally-different
//! slugifiers — [`crate::fivetools_markup::slugify`] (reference cards: every
//! non-alphanumeric becomes a dash) and [`runebound_models::utils::slugify`]
//! (vault notes: only whitespace/`-`/`_`/`.` separate, with an `"untitled"`
//! fallback). They produce different slugs for punctuated names, so merging them
//! would shift stored keys — keep them separate.

/// Uppercase the first character of `word`, leaving the rest unchanged. Used to
/// title-case a single word (and, via [`capitalize`], a whole label).
pub fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Uppercase the first character of `text` — the label/phrase-level alias of
/// [`title_case`]. Same behavior; the separate name reads clearer at call sites
/// that capitalize a whole phrase rather than a single word.
pub fn capitalize(text: &str) -> String {
    title_case(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_case_uppercases_only_the_first_char() {
        assert_eq!(title_case("dragon"), "Dragon");
        assert_eq!(title_case(""), "");
        // The remainder is left exactly as-is.
        assert_eq!(title_case("dDoS"), "DDoS");
    }

    #[test]
    fn capitalize_matches_title_case() {
        assert_eq!(capitalize("fiend"), title_case("fiend"));
    }
}
