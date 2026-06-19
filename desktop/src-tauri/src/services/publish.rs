use std::fmt::Write;

use runebound_models::{
    DungeonFrontmatter, EventFrontmatter, FactionFrontmatter, GodFrontmatter, ItemFrontmatter,
    LocationFrontmatter, NpcFrontmatter,
};

use crate::utils::normalize_unknown_text;

#[cfg(test)]
pub fn render_npc_markdown(frontmatter: &NpcFrontmatter) -> String {
    render_npc_markdown_with_links(frontmatter, &EntityLinker::empty())
}

pub fn render_npc_markdown_with_links(
    frontmatter: &NpcFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Race", &frontmatter.race);
    write_attr_line(&mut out, "Occupation", &frontmatter.occupation);
    write_attr_line(&mut out, "Sex", &frontmatter.sex);
    write_attr_line(&mut out, "Age", &frontmatter.age);
    write_attr_line(&mut out, "Height", &frontmatter.height);
    write_attr_line(&mut out, "Weight", &frontmatter.weight_lbs);
    write_attr_line_linked(&mut out, "Location", &frontmatter.location);
    writeln!(&mut out).ok();

    write_section(&mut out, "Background", &frontmatter.background, linker);
    write_section(&mut out, "Goals", &frontmatter.want_need, linker);
    write_section(&mut out, "Secret", &frontmatter.secret_obstacle, linker);
    write_list_section(&mut out, "Carrying", &frontmatter.carrying);

    out
}

pub fn render_location_markdown(frontmatter: &LocationFrontmatter) -> String {
    render_location_markdown_with_links(frontmatter, &EntityLinker::empty())
}

pub fn render_location_markdown_with_links(
    frontmatter: &LocationFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Kind", &kind_display(frontmatter));
    write_attr_line(&mut out, "Tone", &frontmatter.tone);
    // A guildhall's authority is its faction — an entity reference, so render it as a
    // `[[wikilink]]`. Other kinds carry free-text authority (a lone occupant, a
    // council), which would make a nonsense link target, so they stay plain.
    if frontmatter.kind_type == "guildhall" {
        write_attr_line_linked(&mut out, "Authority", &frontmatter.authority);
    } else {
        write_attr_line(&mut out, "Authority", &frontmatter.authority);
    }
    // The containing location (a guildhall's anchor) is an entity reference, so it is
    // linked; it is empty for other kinds, so the linked writer omits the line.
    write_attr_line_linked(&mut out, "Location", &frontmatter.location);
    write_attr_line(&mut out, "Danger", &frontmatter.danger_level);
    writeln!(&mut out).ok();

    write_section(
        &mut out,
        "Visual Description",
        &frontmatter.visual_description,
        linker,
    );
    write_section(&mut out, "History", &frontmatter.history_background, linker);
    write_list_section(&mut out, "Exports", &frontmatter.exports);
    write_section(
        &mut out,
        "Current Tension",
        &frontmatter.current_tension,
        linker,
    );

    out
}

#[cfg(test)]
pub fn render_faction_markdown(frontmatter: &FactionFrontmatter) -> String {
    render_faction_markdown_with_links(frontmatter, &EntityLinker::empty())
}

pub fn render_faction_markdown_with_links(
    frontmatter: &FactionFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Kind", &frontmatter.kind_type);
    if let Some(custom) = &frontmatter.kind_custom
        && !custom.trim().is_empty()
    {
        write_attr_line(&mut out, "Kind (custom)", custom);
    }
    writeln!(&mut out).ok();
    write_section(&mut out, "Headquarters", &frontmatter.headquarters, linker);
    write_section(
        &mut out,
        "Sphere of Influence",
        &frontmatter.sphere_of_influence,
        linker,
    );
    write_section(&mut out, "Reputation", &frontmatter.reputation, linker);
    write_section(
        &mut out,
        "Public Description",
        &frontmatter.public_description,
        linker,
    );
    write_section(&mut out, "True Agenda", &frontmatter.true_agenda, linker);
    write_section(&mut out, "Methods", &frontmatter.methods, linker);
    write_section(&mut out, "Leadership", &frontmatter.leadership, linker);
    write_list_section(
        &mut out,
        "Resources & Assets",
        &frontmatter.resources_assets,
    );
    write_linked_list_section(&mut out, "Allies", &frontmatter.allies);
    write_linked_list_section(&mut out, "Rivals", &frontmatter.rivals_enemies);
    write_section(
        &mut out,
        "Current Tension",
        &frontmatter.current_tension,
        linker,
    );
    write_list_section(&mut out, "Short-Term Goals", &frontmatter.goals_short_term);
    write_list_section(&mut out, "Long-Term Goals", &frontmatter.goals_long_term);
    write_section(&mut out, "Symbol", &frontmatter.symbol_description, linker);

    out
}

#[cfg(test)]
pub fn render_item_markdown(frontmatter: &ItemFrontmatter) -> String {
    render_item_markdown_with_links(frontmatter, &EntityLinker::empty())
}

pub fn render_item_markdown_with_links(
    frontmatter: &ItemFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Category", &frontmatter.category);
    write_attr_line(&mut out, "Rarity", &frontmatter.rarity);
    write_attr_line(&mut out, "Attunement", &frontmatter.attunement);
    write_attr_line(&mut out, "Value", &frontmatter.value);
    // The item Location is free-text describing where the item is (the LLM writes
    // a phrase like "buried beneath the old mill"), not a reference to a known
    // location entity — so it's rendered as plain text, not a `[[wikilink]]`.
    write_attr_line(&mut out, "Location", &frontmatter.location);
    writeln!(&mut out).ok();

    write_section(&mut out, "Appearance", &frontmatter.appearance, linker);
    write_section(&mut out, "Abilities", &frontmatter.abilities, linker);
    write_section(&mut out, "Drawbacks", &frontmatter.drawbacks, linker);
    write_section(&mut out, "History", &frontmatter.history, linker);
    write_list_section(&mut out, "Materials", &frontmatter.materials);

    out
}

pub fn render_god_markdown_with_links(
    frontmatter: &GodFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Epithet", &frontmatter.epithet);
    write_attr_line(&mut out, "Rank", &rank_display(frontmatter));
    write_attr_line(&mut out, "Alignment", &frontmatter.alignment);
    writeln!(&mut out).ok();

    write_list_section(&mut out, "Domains", &frontmatter.domains);
    write_section(&mut out, "Symbol", &frontmatter.symbol, linker);
    write_section(&mut out, "Appearance", &frontmatter.appearance, linker);
    write_section(&mut out, "Dogma", &frontmatter.dogma, linker);
    write_section(&mut out, "Realm", &frontmatter.realm, linker);
    write_section(&mut out, "Worshippers", &frontmatter.worshippers, linker);
    write_section(&mut out, "Clergy", &frontmatter.clergy, linker);
    write_linked_list_section(&mut out, "Allies", &frontmatter.allies);
    write_linked_list_section(&mut out, "Rivals", &frontmatter.rivals);

    out
}

#[cfg(test)]
pub fn render_dungeon_markdown(frontmatter: &DungeonFrontmatter) -> String {
    render_dungeon_markdown_with_links(frontmatter, &EntityLinker::empty())
}

/// A dungeon publishes as: a premise intro line, a topology line, then one `##`
/// section per beat (`## 1. Entrance — [combat]`) carrying Idea / Player Goals /
/// Lever / Loot (omitted when absent) / Design. The GM tweaks freely after publish.
pub fn render_dungeon_markdown_with_links(
    frontmatter: &DungeonFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    // Provenance: the Pass-1 micro-story the dungeon was generated from, as a
    // block quote at the very top of the file.
    let story = frontmatter.story.trim();
    if !story.is_empty() {
        for line in story.lines() {
            if line.trim().is_empty() {
                writeln!(&mut out, ">").ok();
            } else {
                writeln!(&mut out, "> {line}").ok();
            }
        }
        writeln!(&mut out).ok();
    }
    let premise = normalize_unknown_text(&frontmatter.premise);
    if premise != "Unknown" {
        writeln!(&mut out, "*{}*", linker.link_prose(&premise)).ok();
        writeln!(&mut out).ok();
    }
    let topology = frontmatter.topology.trim();
    if topology.is_empty() || topology.eq_ignore_ascii_case("none") {
        writeln!(&mut out, "**Topology:** none (lay it out freely)").ok();
    } else {
        writeln!(&mut out, "**Topology:** {topology}").ok();
    }
    write_attr_line(&mut out, "Location", &frontmatter.location);
    write_attr_line(&mut out, "Tone", &frontmatter.tone);
    writeln!(&mut out).ok();

    for (i, beat) in frontmatter.beats.iter().enumerate() {
        let content_type = normalize_unknown_text(&beat.content_type);
        writeln!(&mut out, "## {}. {}", i + 1, beat.function).ok();
        if content_type != "Unknown" {
            writeln!(&mut out, "**Type:** {content_type}").ok();
        }
        let idea = normalize_unknown_text(&beat.idea);
        if idea != "Unknown" {
            writeln!(&mut out, "**Idea:** {}", linker.link_prose(&idea)).ok();
        }
        let player_goals = normalize_unknown_text(&beat.player_goals);
        if player_goals != "Unknown" {
            writeln!(
                &mut out,
                "**Player Goals:** {}",
                linker.link_prose(&player_goals)
            )
            .ok();
        }
        let lever = normalize_unknown_text(&beat.lever);
        if lever != "Unknown" {
            writeln!(&mut out, "**Lever:** {}", linker.link_prose(&lever)).ok();
        }
        if let Some(loot) = &beat.loot {
            let loot = loot.trim();
            if !loot.is_empty() {
                writeln!(&mut out, "**Loot:** {}", linker.link_prose(loot)).ok();
            }
        }
        let design_note = normalize_unknown_text(&beat.design_note);
        if design_note != "Unknown" {
            writeln!(&mut out, "**Design:** {}", linker.link_prose(&design_note)).ok();
        }
        writeln!(&mut out).ok();
    }

    out
}

/// The narrative prose of a dungeon — every beat's idea/player-goals/lever/loot
/// joined, for Tier 2 mention extraction. The design_note is an out-of-fiction GM
/// aside, so it is excluded.
pub fn dungeon_prose(frontmatter: &DungeonFrontmatter) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for beat in &frontmatter.beats {
        parts.push(beat.idea.as_str());
        parts.push(beat.player_goals.as_str());
        parts.push(beat.lever.as_str());
        if let Some(loot) = &beat.loot {
            parts.push(loot.as_str());
        }
    }
    join_prose(&parts)
}

/// An event publishes as its narrative body run through the prose linker — no
/// attribute lines or section headers, since the whole record is one story.
pub fn render_event_markdown_with_links(
    frontmatter: &EventFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    let body = frontmatter.body.trim();
    if !body.is_empty() {
        writeln!(&mut out, "{}", linker.link_prose(body)).ok();
    }
    out
}

/// The narrative prose of an NPC — the fields rendered as free-text sections.
/// Used to feed Tier 2 mention extraction; mirrors the `write_section` calls in
/// [`render_npc_markdown_with_links`].
pub fn npc_prose(frontmatter: &NpcFrontmatter) -> String {
    join_prose(&[
        &frontmatter.background,
        &frontmatter.want_need,
        &frontmatter.secret_obstacle,
    ])
}

pub fn location_prose(frontmatter: &LocationFrontmatter) -> String {
    join_prose(&[
        &frontmatter.visual_description,
        &frontmatter.history_background,
        &frontmatter.current_tension,
    ])
}

pub fn faction_prose(frontmatter: &FactionFrontmatter) -> String {
    join_prose(&[
        &frontmatter.headquarters,
        &frontmatter.sphere_of_influence,
        &frontmatter.reputation,
        &frontmatter.public_description,
        &frontmatter.true_agenda,
        &frontmatter.methods,
        &frontmatter.leadership,
        &frontmatter.current_tension,
        &frontmatter.symbol_description,
    ])
}

pub fn item_prose(frontmatter: &ItemFrontmatter) -> String {
    join_prose(&[
        &frontmatter.appearance,
        &frontmatter.abilities,
        &frontmatter.drawbacks,
        &frontmatter.history,
    ])
}

pub fn god_prose(frontmatter: &GodFrontmatter) -> String {
    join_prose(&[
        &frontmatter.symbol,
        &frontmatter.appearance,
        &frontmatter.dogma,
        &frontmatter.realm,
        &frontmatter.worshippers,
        &frontmatter.clergy,
    ])
}

pub fn event_prose(frontmatter: &EventFrontmatter) -> String {
    frontmatter.body.trim().to_string()
}

fn join_prose(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|field| field.trim())
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Characters with special meaning inside an Obsidian `[[wikilink]]` target
/// (`|` alias, `#` heading, `^` block, and the brackets themselves). If a value
/// contains any of them we leave it unlinked rather than emit a broken link.
/// Shared with `mention_extraction` so both link layers agree on what's unsafe.
pub(crate) const WIKILINK_UNSAFE_CHARS: &[char] = &['[', ']', '|', '#', '^'];

/// Wrap a single entity name in an Obsidian `[[wikilink]]`. This is what lets a
/// relational reference resolve to — or stub out — another entity's page, even
/// before that page exists.
///
/// Tier 0 only links fields that are entity references by schema design (a
/// location name, an ally/rival group), so the whole value is one link target.
/// Returns the value unchanged when it is empty, already a wikilink, or contains
/// characters unsafe for a link target.
fn wikilink(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
        return trimmed.to_string();
    }
    if trimmed.contains(WIKILINK_UNSAFE_CHARS) {
        return trimmed.to_string();
    }
    format!("[[{trimmed}]]")
}

/// Tier 1 prose linker: wraps mentions of *known* entities found inside
/// narrative text with `[[wikilinks]]`, using the entity's canonical casing.
///
/// Built once per publish from the set of entity names in the index, minus the
/// entity being rendered (so a page never links to itself). Matching is
/// case-insensitive, whole-word, and longest-name-first, and it never links
/// inside an existing `[[...]]` span.
pub struct EntityLinker {
    /// Canonical display names paired with their lowercased form, de-duplicated and
    /// sorted longest-first so that "Crimson Lantern Syndicate" is matched before
    /// "Crimson Lantern". The lowercased form is the needle [`link_prose`] matches
    /// against; computing it once here (instead of re-allocating it per text
    /// position × name in the inner loop) keeps linking linear in the prose length.
    ///
    /// [`link_prose`]: EntityLinker::link_prose
    names: Vec<(String, String)>,
}

impl EntityLinker {
    pub fn new<I>(candidate_names: I, self_name: &str) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let self_lower = self_name.trim().to_ascii_lowercase();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut names: Vec<(String, String)> = candidate_names
            .into_iter()
            .filter_map(|name| {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                let lower = name.to_ascii_lowercase();
                // Never link a page to itself.
                if lower == self_lower {
                    return None;
                }
                // A name with link-unsafe characters can't form a clean target.
                if name.contains(WIKILINK_UNSAFE_CHARS) {
                    return None;
                }
                // De-duplicate case-insensitively, keeping the first casing seen.
                if !seen.insert(lower.clone()) {
                    return None;
                }
                Some((name, lower))
            })
            .collect();
        names.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        Self { names }
    }

    /// An linker that links nothing — used where no cross-entity index is
    /// available (e.g. auto-creating a stub location) and by Tier 0 tests.
    pub fn empty() -> Self {
        Self { names: Vec::new() }
    }

    /// Wrap every whole-word mention of a known entity in `text` with a
    /// `[[wikilink]]`. Spans already inside `[[...]]` are copied through
    /// untouched so we never double-link.
    fn link_prose(&self, text: &str) -> String {
        if self.names.is_empty() {
            return text.to_string();
        }

        // `to_ascii_lowercase` is byte-length preserving, so byte offsets into
        // `lowered` line up with `text` and we can slice the original to recover
        // canonical casing.
        let lowered = text.to_ascii_lowercase();
        let mut result = String::with_capacity(text.len());
        let mut i = 0;

        while i < text.len() {
            // Copy through an existing wikilink span verbatim.
            if text[i..].starts_with("[[")
                && let Some(rel_end) = text[i..].find("]]")
            {
                let end = i + rel_end + 2;
                result.push_str(&text[i..end]);
                i = end;
                continue;
            }

            if boundary_before(text, i) {
                let mut matched: Option<(&str, usize)> = None;
                for (name, needle) in &self.names {
                    if lowered[i..].starts_with(needle.as_str()) {
                        let end = i + needle.len();
                        if boundary_after(text, end) {
                            matched = Some((name.as_str(), end));
                            break;
                        }
                    }
                }
                if let Some((name, end)) = matched {
                    result.push_str("[[");
                    result.push_str(name);
                    result.push_str("]]");
                    i = end;
                    continue;
                }
            }

            // Copy a single char, respecting UTF-8 boundaries.
            let ch = text[i..].chars().next().expect("char at boundary");
            let ch_len = ch.len_utf8();
            result.push_str(&text[i..i + ch_len]);
            i += ch_len;
        }

        result
    }
}

/// A left word boundary exists at byte `i` when the preceding char is absent or
/// not alphanumeric (so we match whole words, not substrings inside a word).
pub(crate) fn boundary_before(text: &str, i: usize) -> bool {
    text[..i]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric())
}

/// A right word boundary exists at byte `end` when the following char is absent
/// or not alphanumeric. This keeps possessives working: matching "Waterdeep" in
/// "Waterdeep's" ends before the apostrophe, yielding `[[Waterdeep]]'s`.
pub(crate) fn boundary_after(text: &str, end: usize) -> bool {
    text[end..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric())
}

/// Whole-word containment: true iff `needle` occurs in `haystack` bounded by a
/// word boundary on both sides. Both inputs must already be lowercased
/// (`to_ascii_lowercase` is byte-preserving, so byte offsets line up — the same
/// invariant [`EntityLinker::link_prose`] relies on). This is the grounding check
/// the mention extractor shares with the prose linker, so "Vex" is not treated as
/// present just because the text contains "Vexley".
pub(crate) fn contains_word_boundary(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.match_indices(needle).any(|(idx, _)| {
        boundary_before(haystack, idx) && boundary_after(haystack, idx + needle.len())
    })
}

fn write_attr_line(out: &mut String, label: &str, value: &str) {
    let normalized = normalize_unknown_text(value);
    if normalized != "Unknown" {
        writeln!(out, "**{label}:** {normalized}").ok();
    }
}

/// Like [`write_attr_line`], but the value is an entity reference and is rendered
/// as a `[[wikilink]]` (e.g. an NPC's `Location`).
fn write_attr_line_linked(out: &mut String, label: &str, value: &str) {
    let normalized = normalize_unknown_text(value);
    if normalized != "Unknown" {
        writeln!(out, "**{label}:** {}", wikilink(&normalized)).ok();
    }
}

fn write_section(out: &mut String, title: &str, value: &str, linker: &EntityLinker) {
    let normalized = normalize_unknown_text(value);
    if normalized == "Unknown" {
        return;
    }
    writeln!(out, "## {title}").ok();
    writeln!(out, "{}", linker.link_prose(&normalized)).ok();
    writeln!(out).ok();
}

fn write_list_section(out: &mut String, title: &str, values: &[String]) {
    let items: Vec<String> = values
        .iter()
        .map(|v| normalize_unknown_text(v))
        .filter(|v| v != "Unknown")
        .collect();
    if items.is_empty() {
        return;
    }
    writeln!(out, "## {title}").ok();
    for item in items {
        writeln!(out, "- {}", item).ok();
    }
    writeln!(out).ok();
}

/// Like [`write_list_section`], but each list item is an entity reference and is
/// rendered as a `[[wikilink]]` (e.g. a faction's allies / rivals).
fn write_linked_list_section(out: &mut String, title: &str, values: &[String]) {
    let items: Vec<String> = values
        .iter()
        .map(|v| normalize_unknown_text(v))
        .filter(|v| v != "Unknown")
        .map(|v| wikilink(&v))
        .collect();
    if items.is_empty() {
        return;
    }
    writeln!(out, "## {title}").ok();
    for item in items {
        writeln!(out, "- {}", item).ok();
    }
    writeln!(out).ok();
}

fn kind_display(frontmatter: &LocationFrontmatter) -> String {
    let kind = normalize_unknown_text(&frontmatter.kind_type);
    if !kind.eq_ignore_ascii_case("other") {
        return kind;
    }
    match frontmatter
        .kind_custom
        .as_ref()
        .map(|value| normalize_unknown_text(value))
    {
        Some(custom) if custom != "Unknown" => format!("Other ({custom})"),
        _ => "Other".to_string(),
    }
}

fn rank_display(frontmatter: &GodFrontmatter) -> String {
    let rank = normalize_unknown_text(&frontmatter.rank);
    if !rank.eq_ignore_ascii_case("other") {
        return rank;
    }
    match frontmatter
        .rank_custom
        .as_ref()
        .map(|value| normalize_unknown_text(value))
    {
        Some(custom) if custom != "Unknown" => format!("Other ({custom})"),
        _ => "Other".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npc_renderer_includes_attributes() {
        let frontmatter = NpcFrontmatter {
            doc_type: "npc".to_string(),
            id: "npc_1".to_string(),
            slug: "lirael".to_string(),
            name: "Lirael Drake".to_string(),
            vault_path: "npcs/Lirael Drake.md".to_string(),
            race: "Elf".to_string(),
            occupation: "Archivist".to_string(),
            sex: "female".to_string(),
            age: "133".to_string(),
            height: "5'9\"".to_string(),
            weight_lbs: "140".to_string(),
            background: "Raised in the argent library.".to_string(),
            want_need: "Safeguard forbidden scrolls.".to_string(),
            secret_obstacle: "Cursed with prophetic dreams.".to_string(),
            carrying: vec!["Silver quill".to_string()],
            location: "Silversong".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T12:00:00Z".to_string(),
            published_at: None,
        };

        let markdown = render_npc_markdown(&frontmatter);
        assert!(markdown.contains("**Race:** Elf"));
        assert!(markdown.contains("## Background"));
        assert!(markdown.contains("- Silver quill"));
    }

    fn location_frontmatter(
        kind_type: &str,
        authority: &str,
        location: &str,
    ) -> LocationFrontmatter {
        LocationFrontmatter {
            doc_type: "location".to_string(),
            id: "loc_1".to_string(),
            slug: "the-hall".to_string(),
            name: "The Hall".to_string(),
            vault_path: "locations/The Hall.md".to_string(),
            kind_type: kind_type.to_string(),
            kind_custom: None,
            visual_description: "Marble columns.".to_string(),
            history_background: "Founded long ago.".to_string(),
            exports: Vec::new(),
            tone: "stern".to_string(),
            authority: authority.to_string(),
            danger_level: "safe".to_string(),
            current_tension: "An audit looms.".to_string(),
            location: location.to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T12:00:00Z".to_string(),
            published_at: None,
        }
    }

    #[test]
    fn guildhall_links_authority_and_location() {
        // A guildhall's authority is its faction and its anchor is a location, so both
        // publish as `[[wikilinks]]`.
        let markdown = render_location_markdown(&location_frontmatter(
            "guildhall",
            "Crimson Lanterns",
            "Silverhall",
        ));
        assert!(markdown.contains("**Authority:** [[Crimson Lanterns]]"));
        assert!(markdown.contains("**Location:** [[Silverhall]]"));
    }

    #[test]
    fn non_guildhall_authority_plain_and_empty_location_omitted() {
        // A site's authority is free prose (not an entity), so it stays unlinked; an
        // empty anchor drops the Location line entirely.
        let markdown = render_location_markdown(&location_frontmatter("ruin", "a marsh hag", ""));
        assert!(markdown.contains("**Authority:** a marsh hag"));
        assert!(!markdown.contains("[[a marsh hag]]"));
        assert!(!markdown.contains("**Location:**"));
    }

    #[test]
    fn faction_resources_render_as_list() {
        let frontmatter = FactionFrontmatter {
            doc_type: "faction".to_string(),
            id: "fac_1".to_string(),
            slug: "ashen-circle".to_string(),
            name: "Ashen Circle".to_string(),
            vault_path: "factions/Ashen Circle.md".to_string(),
            kind_type: "guild".to_string(),
            kind_custom: None,
            public_description: "A secretive guild.".to_string(),
            true_agenda: "Protect forbidden lore.".to_string(),
            methods: "Shadow operations.".to_string(),
            leadership: "Triumvirate".to_string(),
            headquarters: "Smolderkeep".to_string(),
            sphere_of_influence: "Borderlands".to_string(),
            resources_assets: vec!["Hidden vaults".to_string(), "Arcane scouts".to_string()],
            allies: vec![],
            rivals_enemies: vec![],
            reputation: "Feared".to_string(),
            current_tension: "Hunters closing in.".to_string(),
            goals_short_term: vec![],
            goals_long_term: vec![],
            symbol_description: "A burned coin.".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
            published_at: None,
        };

        let markdown = render_faction_markdown(&frontmatter);
        assert!(markdown.contains("## Resources & Assets"));
        assert!(markdown.contains("- Hidden vaults"));
        assert!(markdown.contains("- Arcane scouts"));
    }

    // ----------------------------------------------------------------------
    // Tier 0 wikilinks: relational fields that are entity references by schema
    // design are rendered as Obsidian `[[wikilinks]]` so they resolve to (or
    // stub out) the referenced entity's page.
    // ----------------------------------------------------------------------

    #[test]
    fn wikilink_wraps_a_plain_name() {
        assert_eq!(wikilink("Waterdeep"), "[[Waterdeep]]");
    }

    #[test]
    fn wikilink_trims_surrounding_whitespace() {
        assert_eq!(wikilink("  Neverwinter Harbor  "), "[[Neverwinter Harbor]]");
    }

    #[test]
    fn wikilink_is_idempotent_for_already_linked_values() {
        assert_eq!(wikilink("[[Waterdeep]]"), "[[Waterdeep]]");
    }

    #[test]
    fn wikilink_leaves_empty_values_empty() {
        assert_eq!(wikilink("   "), "");
    }

    #[test]
    fn wikilink_skips_values_with_link_unsafe_characters() {
        // A `|`, `#`, `^`, or stray bracket would produce a broken link target,
        // so the value is left as-is rather than corrupted.
        assert_eq!(
            wikilink("Waterdeep | Sword Coast"),
            "Waterdeep | Sword Coast"
        );
        assert_eq!(wikilink("Vault #3"), "Vault #3");
        assert_eq!(wikilink("[redacted]"), "[redacted]");
    }

    fn sample_npc_frontmatter() -> NpcFrontmatter {
        NpcFrontmatter {
            doc_type: "npc".to_string(),
            id: "npc_1".to_string(),
            slug: "lirael".to_string(),
            name: "Lirael Drake".to_string(),
            vault_path: "npcs/Lirael Drake.md".to_string(),
            race: "Elf".to_string(),
            occupation: "Archivist".to_string(),
            sex: "female".to_string(),
            age: "133".to_string(),
            height: "5'9\"".to_string(),
            weight_lbs: "140".to_string(),
            background: "Raised in the argent library.".to_string(),
            want_need: "Safeguard forbidden scrolls.".to_string(),
            secret_obstacle: "Cursed with prophetic dreams.".to_string(),
            carrying: vec!["Silver quill".to_string()],
            location: "Silversong".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T12:00:00Z".to_string(),
            published_at: None,
        }
    }

    #[test]
    fn npc_location_is_rendered_as_a_wikilink() {
        let markdown = render_npc_markdown(&sample_npc_frontmatter());
        assert!(
            markdown.contains("**Location:** [[Silversong]]"),
            "expected linked location, got:\n{markdown}"
        );
    }

    #[test]
    fn npc_descriptive_sections_are_not_wikilinked() {
        // Regression guard: Tier 0 must NOT auto-link narrative prose — only the
        // relational fields. Background/Goals/Secret stay plain text.
        let markdown = render_npc_markdown(&sample_npc_frontmatter());
        assert!(markdown.contains("Raised in the argent library."));
        assert!(
            !markdown.contains("[[Raised"),
            "background prose should not be wikilinked:\n{markdown}"
        );
        // The carrying list is generic items, not entity references — left plain.
        assert!(markdown.contains("- Silver quill"));
        assert!(!markdown.contains("[[Silver quill]]"));
    }

    #[test]
    fn npc_unknown_location_is_omitted_entirely() {
        let mut frontmatter = sample_npc_frontmatter();
        frontmatter.location = String::new();
        let markdown = render_npc_markdown(&frontmatter);
        assert!(!markdown.contains("**Location:**"));
        assert!(!markdown.contains("[["));
    }

    #[test]
    fn item_location_is_rendered_as_plain_text() {
        // The item Location is a free-text description of where the item rests, not
        // a reference to a known location entity, so it must not be wikilinked.
        let frontmatter = ItemFrontmatter {
            doc_type: "item".to_string(),
            id: "item_1".to_string(),
            slug: "everember-blade".to_string(),
            name: "Everember Blade".to_string(),
            vault_path: "items/Everember Blade.md".to_string(),
            category: "weapon".to_string(),
            rarity: "legendary".to_string(),
            attunement: "Required".to_string(),
            materials: vec!["stormglass".to_string()],
            appearance: "A blade woven from stormglass.".to_string(),
            abilities: "Channels stormlight.".to_string(),
            drawbacks: "Hums in the rain.".to_string(),
            history: "Forged in the old wars.".to_string(),
            value: "1000gp".to_string(),
            location: "Buried in the vaults beneath Smolderkeep.".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
            published_at: None,
        };
        let markdown = render_item_markdown(&frontmatter);
        assert!(
            markdown.contains("**Location:** Buried in the vaults beneath Smolderkeep."),
            "expected plain-text item location, got:\n{markdown}"
        );
        // Neither the location description nor the materials are entity references.
        assert!(markdown.contains("- stormglass"));
        assert!(!markdown.contains("[["));
    }

    #[test]
    fn faction_allies_and_rivals_are_rendered_as_wikilinks() {
        let frontmatter = FactionFrontmatter {
            doc_type: "faction".to_string(),
            id: "fac_1".to_string(),
            slug: "ashen-circle".to_string(),
            name: "Ashen Circle".to_string(),
            vault_path: "factions/Ashen Circle.md".to_string(),
            kind_type: "guild".to_string(),
            kind_custom: None,
            public_description: "A secretive guild.".to_string(),
            true_agenda: "Protect forbidden lore.".to_string(),
            methods: "Shadow operations.".to_string(),
            leadership: "Triumvirate".to_string(),
            headquarters: "Smolderkeep".to_string(),
            sphere_of_influence: "Borderlands".to_string(),
            resources_assets: vec!["Hidden vaults".to_string()],
            allies: vec!["Crimson Lantern Syndicate".to_string()],
            rivals_enemies: vec!["Harbor Watch".to_string()],
            reputation: "Feared".to_string(),
            current_tension: "Hunters closing in.".to_string(),
            goals_short_term: vec![],
            goals_long_term: vec![],
            symbol_description: "A burned coin.".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
            published_at: None,
        };
        let markdown = render_faction_markdown(&frontmatter);
        assert!(
            markdown.contains("- [[Crimson Lantern Syndicate]]"),
            "expected linked ally, got:\n{markdown}"
        );
        assert!(
            markdown.contains("- [[Harbor Watch]]"),
            "expected linked rival, got:\n{markdown}"
        );
        // Short-term goals are descriptive sentences elsewhere — confirm we did
        // not start linking non-relational list sections.
        assert!(!markdown.contains("[[Hidden vaults]]"));
    }

    #[test]
    fn dungeon_markdown_renders_premise_topology_and_beats() {
        use runebound_models::DungeonBeat;
        let frontmatter = DungeonFrontmatter {
            doc_type: "dungeon".to_string(),
            id: "dungeon_1".to_string(),
            slug: "the-sunken-forge".to_string(),
            name: "The Sunken Forge".to_string(),
            vault_path: "dungeons/The Sunken Forge.md".to_string(),
            location: "A drowned bell-foundry beneath the tide line.".to_string(),
            story: "The tide took the foundry, but the forge never went out.".to_string(),
            premise: "A drowned forge that still burns.".to_string(),
            topology: "The Moose".to_string(),
            tone: "tragedy".to_string(),
            twist: "false_victory".to_string(),
            beats: vec![
                DungeonBeat {
                    function: "Entrance".to_string(),
                    content_type: "puzzle".to_string(),
                    idea: "A sealed sluice gate bars the way.".to_string(),
                    player_goals: "Find what opens the gate and get inside.".to_string(),
                    lever: "What keeps the water out?".to_string(),
                    loot: None,
                    design_note: "Establishes the flooded threat and gates entry.".to_string(),
                    overlay: None,
                    factions: false,
                },
                DungeonBeat {
                    function: "Resolution".to_string(),
                    content_type: "cache".to_string(),
                    idea: "The reignited forge yields its prize.".to_string(),
                    player_goals: "Claim the forged reward and decide who gets it.".to_string(),
                    lever: "Who else wants what was forged here?".to_string(),
                    loot: Some("A still-warm blade".to_string()),
                    design_note: "Pays off the forge setup and hooks a rival faction.".to_string(),
                    overlay: None,
                    factions: false,
                },
            ],
            created_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:00Z".to_string(),
            published_at: None,
        };
        let markdown = render_dungeon_markdown(&frontmatter);
        assert!(markdown.contains("*A drowned forge that still burns.*"));
        assert!(markdown.contains("**Topology:** The Moose"));
        assert!(markdown.contains("## 1. Entrance"));
        assert!(markdown.contains("## 2. Resolution"));
        assert!(markdown.contains("**Type:** puzzle"));
        assert!(markdown.contains("**Type:** cache"));
        // Type is a detail line, not bracketed in the heading (Obsidian-safe).
        assert!(!markdown.contains("— [puzzle]"));
        assert!(markdown.contains("**Loot:** A still-warm blade"));
        // The Entrance has no loot line — conditional loot omitted, not blank.
        let entrance = markdown.split("## 2.").next().unwrap_or("");
        assert!(!entrance.contains("**Loot:**"));
    }

    // ----------------------------------------------------------------------
    // Tier 1: deterministic prose linking of *known* entity mentions.
    // ----------------------------------------------------------------------

    fn make_linker(names: &[&str], self_name: &str) -> EntityLinker {
        EntityLinker::new(names.iter().map(|n| n.to_string()), self_name)
    }

    #[test]
    fn links_a_known_entity_mentioned_in_prose() {
        let linker = make_linker(&["Waterdeep"], "Lirael Drake");
        assert_eq!(
            linker.link_prose("She fled to Waterdeep at dawn."),
            "She fled to [[Waterdeep]] at dawn."
        );
    }

    #[test]
    fn prose_link_matches_case_insensitively_but_uses_canonical_casing() {
        let linker = make_linker(&["Waterdeep"], "Lirael Drake");
        assert_eq!(
            linker.link_prose("rumors from waterdeep"),
            "rumors from [[Waterdeep]]"
        );
    }

    #[test]
    fn prose_link_respects_word_boundaries() {
        // "Ash" must not match inside "ashes"; only the standalone word links.
        let linker = make_linker(&["Ash"], "Branwen");
        assert_eq!(linker.link_prose("ashes and Ash"), "ashes and [[Ash]]");
    }

    #[test]
    fn prose_link_prefers_the_longest_matching_name() {
        let linker = make_linker(&["Crimson Lantern", "Crimson Lantern Syndicate"], "x");
        assert_eq!(
            linker.link_prose("The Crimson Lantern Syndicate rules the docks."),
            "The [[Crimson Lantern Syndicate]] rules the docks."
        );
    }

    #[test]
    fn prose_link_keeps_possessives_outside_the_link() {
        let linker = make_linker(&["Waterdeep"], "x");
        assert_eq!(
            linker.link_prose("Waterdeep's docks burned."),
            "[[Waterdeep]]'s docks burned."
        );
    }

    #[test]
    fn linker_never_links_a_page_to_itself() {
        let linker = make_linker(&["Lirael Drake", "Waterdeep"], "Lirael Drake");
        let linked = linker.link_prose("Lirael Drake walked to Waterdeep.");
        assert!(!linked.contains("[[Lirael Drake]]"));
        assert!(linked.contains("[[Waterdeep]]"));
    }

    #[test]
    fn prose_link_does_not_double_link_existing_wikilinks() {
        let linker = make_linker(&["Waterdeep"], "x");
        assert_eq!(
            linker.link_prose("Visit [[Waterdeep]] today."),
            "Visit [[Waterdeep]] today."
        );
    }

    #[test]
    fn empty_linker_leaves_prose_untouched() {
        let linker = EntityLinker::empty();
        assert_eq!(
            linker.link_prose("Nothing to link here."),
            "Nothing to link here."
        );
    }

    #[test]
    fn links_a_full_multi_word_name_mentioned_mid_sentence() {
        // Regression for the reported case: a previously-published NPC with a
        // three-word name, referenced inside another NPC's generated background
        // (set off by commas). The sister being rendered has a different name,
        // so the self-exclusion doesn't interfere.
        let linker = make_linker(&["Liam Vesper Thistlewaite"], "Mara Thistlewaite");
        let background =
            "Her younger brother, Liam Vesper Thistlewaite, was dragged into banditry.";
        assert_eq!(
            linker.link_prose(background),
            "Her younger brother, [[Liam Vesper Thistlewaite]], was dragged into banditry."
        );
    }

    #[test]
    fn render_with_links_links_known_mentions_in_narrative_sections() {
        let mut frontmatter = sample_npc_frontmatter();
        frontmatter.background = "Trained as a scribe in Silversong before the war.".to_string();
        let linker = make_linker(&["Silversong"], &frontmatter.name);

        let markdown = render_npc_markdown_with_links(&frontmatter, &linker);
        // Prose mention is linked...
        assert!(
            markdown.contains("Trained as a scribe in [[Silversong]] before the war."),
            "expected linked prose mention, got:\n{markdown}"
        );
        // ...and the Tier 0 relational Location link still works alongside it.
        assert!(markdown.contains("**Location:** [[Silversong]]"));
    }
}
