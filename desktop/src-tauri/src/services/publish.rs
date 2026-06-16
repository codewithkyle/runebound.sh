use std::fmt::Write;

use runebound_models::{
    DungeonFrontmatter, EventFrontmatter, FactionFrontmatter, GodFrontmatter, ItemFrontmatter,
    LocationFrontmatter, NpcFrontmatter,
};

use crate::utils::normalize_unknown_text;

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
    write_attr_line(&mut out, "Authority", &frontmatter.authority);
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
    write_section(&mut out, "Current Tension", &frontmatter.current_tension, linker);

    out
}

pub fn render_faction_markdown(frontmatter: &FactionFrontmatter) -> String {
    render_faction_markdown_with_links(frontmatter, &EntityLinker::empty())
}

pub fn render_faction_markdown_with_links(
    frontmatter: &FactionFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Kind", &frontmatter.kind_type);
    if let Some(custom) = &frontmatter.kind_custom {
        if !custom.trim().is_empty() {
            write_attr_line(&mut out, "Kind (custom)", custom);
        }
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
    write_section(&mut out, "Public Description", &frontmatter.public_description, linker);
    write_section(&mut out, "True Agenda", &frontmatter.true_agenda, linker);
    write_section(&mut out, "Methods", &frontmatter.methods, linker);
    write_section(&mut out, "Leadership", &frontmatter.leadership, linker);
    write_text_list_section(&mut out, "Resources & Assets", &frontmatter.resources_assets);
    write_linked_list_section(&mut out, "Allies", &frontmatter.allies);
    write_linked_list_section(&mut out, "Rivals", &frontmatter.rivals_enemies);
    write_section(&mut out, "Current Tension", &frontmatter.current_tension, linker);
    write_list_section(&mut out, "Short-Term Goals", &frontmatter.goals_short_term);
    write_list_section(&mut out, "Long-Term Goals", &frontmatter.goals_long_term);
    write_section(&mut out, "Symbol", &frontmatter.symbol_description, linker);

    out
}

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
    write_attr_line_linked(&mut out, "Location", &frontmatter.location);
    writeln!(&mut out).ok();

    write_section(&mut out, "Appearance", &frontmatter.appearance, linker);
    write_section(&mut out, "Abilities", &frontmatter.abilities, linker);
    write_section(&mut out, "Drawbacks", &frontmatter.drawbacks, linker);
    write_section(&mut out, "History", &frontmatter.history, linker);
    write_list_section(&mut out, "Materials", &frontmatter.materials);

    out
}

pub fn render_god_markdown(frontmatter: &GodFrontmatter) -> String {
    render_god_markdown_with_links(frontmatter, &EntityLinker::empty())
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

pub fn render_dungeon_markdown(frontmatter: &DungeonFrontmatter) -> String {
    render_dungeon_markdown_with_links(frontmatter, &EntityLinker::empty())
}

/// A dungeon publishes as: a premise intro line, a topology line, then one `##`
/// section per beat (`## 1. Entrance — [combat]`) carrying Idea / Lever / Loot
/// (omitted when absent) / Read-Aloud. The GM tweaks freely after publish.
pub fn render_dungeon_markdown_with_links(
    frontmatter: &DungeonFrontmatter,
    linker: &EntityLinker,
) -> String {
    let mut out = String::new();
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
        let read_aloud = normalize_unknown_text(&beat.read_aloud);
        if read_aloud != "Unknown" {
            writeln!(&mut out, "**Read-Aloud:** {}", linker.link_prose(&read_aloud)).ok();
        }
        writeln!(&mut out).ok();
    }

    out
}

/// The narrative prose of a dungeon — every beat's idea/lever/loot/read-aloud
/// joined, for Tier 2 mention extraction.
pub fn dungeon_prose(frontmatter: &DungeonFrontmatter) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for beat in &frontmatter.beats {
        parts.push(beat.idea.as_str());
        parts.push(beat.lever.as_str());
        if let Some(loot) = &beat.loot {
            parts.push(loot.as_str());
        }
        parts.push(beat.read_aloud.as_str());
    }
    join_prose(&parts)
}

pub fn render_event_markdown(frontmatter: &EventFrontmatter) -> String {
    render_event_markdown_with_links(frontmatter, &EntityLinker::empty())
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
const WIKILINK_UNSAFE: &[char] = &['[', ']', '|', '#', '^'];

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
    if trimmed.contains(WIKILINK_UNSAFE) {
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
    /// Canonical display names, de-duplicated and sorted longest-first so that
    /// "Crimson Lantern Syndicate" is matched before "Crimson Lantern".
    names: Vec<String>,
}

impl EntityLinker {
    pub fn new<I>(candidate_names: I, self_name: &str) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let self_lower = self_name.trim().to_ascii_lowercase();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut names: Vec<String> = candidate_names
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            // Never link a page to itself.
            .filter(|name| name.to_ascii_lowercase() != self_lower)
            // A name with link-unsafe characters can't form a clean target.
            .filter(|name| !name.contains(WIKILINK_UNSAFE))
            // De-duplicate case-insensitively, keeping the first casing seen.
            .filter(|name| seen.insert(name.to_ascii_lowercase()))
            .collect();
        names.sort_by(|a, b| b.len().cmp(&a.len()));
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
            if text[i..].starts_with("[[") {
                if let Some(rel_end) = text[i..].find("]]") {
                    let end = i + rel_end + 2;
                    result.push_str(&text[i..end]);
                    i = end;
                    continue;
                }
            }

            if boundary_before(text, i) {
                let mut matched: Option<(&str, usize)> = None;
                for name in &self.names {
                    let needle = name.to_ascii_lowercase();
                    if lowered[i..].starts_with(&needle) {
                        let end = i + needle.len();
                        if boundary_after(text, end) {
                            matched = Some((name, end));
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
fn boundary_before(text: &str, i: usize) -> bool {
    text[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric())
}

/// A right word boundary exists at byte `end` when the following char is absent
/// or not alphanumeric. This keeps possessives working: matching "Waterdeep" in
/// "Waterdeep's" ends before the apostrophe, yielding `[[Waterdeep]]'s`.
fn boundary_after(text: &str, end: usize) -> bool {
    text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric())
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

fn write_text_list_section(out: &mut String, title: &str, value: &str) {
    let normalized = normalize_unknown_text(value);
    if normalized == "Unknown" {
        return;
    }

    let mut items = parse_text_list_items(&normalized);

    if items.is_empty() {
        items.push(normalized);
    }

    writeln!(out, "## {title}").ok();
    for item in items {
        writeln!(out, "- {}", item).ok();
    }
    writeln!(out).ok();
}

fn parse_text_list_items(value: &str) -> Vec<String> {
    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(value) {
        let cleaned: Vec<String> = parsed
            .into_iter()
            .map(|item| normalize_unknown_text(&item))
            .filter(|item| item != "Unknown")
            .collect();
        if !cleaned.is_empty() {
            return cleaned;
        }
    }

    value
        .split(|ch| matches!(ch, '\n' | ';' | ','))
        .map(|chunk| chunk.trim())
        .map(|chunk| chunk.trim_start_matches(|c| matches!(c, '-' | '*' | '•' | '[' | ']')))
        .map(|chunk| chunk.trim_matches(|c| c == '[' || c == ']'))
        .map(|chunk| normalize_unknown_text(chunk))
        .filter(|chunk| chunk != "Unknown")
        .collect()
}

fn kind_display(frontmatter: &LocationFrontmatter) -> String {
    let kind = normalize_unknown_text(&frontmatter.kind_type);
    if kind.to_ascii_lowercase() != "other" {
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
    if rank.to_ascii_lowercase() != "other" {
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
            resources_assets: "Hidden vaults;Arcane scouts".to_string(),
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

    #[test]
    fn faction_resources_handle_json_array_string() {
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
            resources_assets: "[\"Hidden vaults\", \"Arcane scouts\"]".to_string(),
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
        assert_eq!(wikilink("Waterdeep | Sword Coast"), "Waterdeep | Sword Coast");
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
    fn item_location_is_rendered_as_a_wikilink() {
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
            location: "Smolderkeep".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            updated_at: "2026-06-15T00:00:00Z".to_string(),
            published_at: None,
        };
        let markdown = render_item_markdown(&frontmatter);
        assert!(
            markdown.contains("**Location:** [[Smolderkeep]]"),
            "expected linked item location, got:\n{markdown}"
        );
        // Materials are substances, not entities — not linked.
        assert!(markdown.contains("- stormglass"));
        assert!(!markdown.contains("[[stormglass]]"));
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
            resources_assets: "Hidden vaults".to_string(),
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
            premise: "A drowned forge that still burns.".to_string(),
            topology: "The Moose".to_string(),
            tone: "tragedy".to_string(),
            twist: "false_victory".to_string(),
            beats: vec![
                DungeonBeat {
                    function: "Entrance".to_string(),
                    content_type: "puzzle".to_string(),
                    idea: "A sealed sluice gate bars the way.".to_string(),
                    lever: "What keeps the water out?".to_string(),
                    loot: None,
                    read_aloud: "A rusted iron gate, chest-high water beyond.".to_string(),
                },
                DungeonBeat {
                    function: "Resolution".to_string(),
                    content_type: "cache".to_string(),
                    idea: "The reignited forge yields its prize.".to_string(),
                    lever: "Who else wants what was forged here?".to_string(),
                    loot: Some("A still-warm blade".to_string()),
                    read_aloud: "A cold anvil the size of a cart.".to_string(),
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
        assert_eq!(
            linker.link_prose("ashes and Ash"),
            "ashes and [[Ash]]"
        );
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
        assert_eq!(linker.link_prose("Nothing to link here."), "Nothing to link here.");
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
