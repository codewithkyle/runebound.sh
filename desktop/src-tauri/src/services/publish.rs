use std::fmt::Write;

use runebound_models::{
    FactionFrontmatter, ItemFrontmatter, LocationFrontmatter, NpcFrontmatter,
};

use crate::utils::normalize_unknown_text;

pub fn render_npc_markdown(frontmatter: &NpcFrontmatter) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Race", &frontmatter.race);
    write_attr_line(&mut out, "Occupation", &frontmatter.occupation);
    write_attr_line(&mut out, "Sex", &frontmatter.sex);
    write_attr_line(&mut out, "Age", &frontmatter.age);
    write_attr_line(&mut out, "Height", &frontmatter.height);
    write_attr_line(&mut out, "Weight", &frontmatter.weight_lbs);
    write_attr_line(&mut out, "Location", &frontmatter.location);
    writeln!(&mut out).ok();

    write_section(&mut out, "Background", &frontmatter.background);
    write_section(&mut out, "Goals", &frontmatter.want_need);
    write_section(&mut out, "Secret", &frontmatter.secret_obstacle);
    write_list_section(&mut out, "Carrying", &frontmatter.carrying);

    out
}

pub fn render_location_markdown(frontmatter: &LocationFrontmatter) -> String {
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
    );
    write_section(
        &mut out,
        "History",
        &frontmatter.history_background,
    );
    write_list_section(&mut out, "Exports", &frontmatter.exports);
    write_section(
        &mut out,
        "Current Tension",
        &frontmatter.current_tension,
    );

    out
}

pub fn render_faction_markdown(frontmatter: &FactionFrontmatter) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Kind", &frontmatter.kind_type);
    if let Some(custom) = &frontmatter.kind_custom {
        if !custom.trim().is_empty() {
            write_attr_line(&mut out, "Kind (custom)", custom);
        }
    }
    writeln!(&mut out).ok();
    write_section(&mut out, "Headquarters", &frontmatter.headquarters);
    write_section(
        &mut out,
        "Sphere of Influence",
        &frontmatter.sphere_of_influence,
    );
    write_section(&mut out, "Reputation", &frontmatter.reputation);
    write_section(&mut out, "Public Description", &frontmatter.public_description);
    write_section(&mut out, "True Agenda", &frontmatter.true_agenda);
    write_section(&mut out, "Methods", &frontmatter.methods);
    write_section(&mut out, "Leadership", &frontmatter.leadership);
    write_text_list_section(&mut out, "Resources & Assets", &frontmatter.resources_assets);
    write_list_section(&mut out, "Allies", &frontmatter.allies);
    write_list_section(&mut out, "Rivals", &frontmatter.rivals_enemies);
    write_section(&mut out, "Current Tension", &frontmatter.current_tension);
    write_list_section(&mut out, "Short-Term Goals", &frontmatter.goals_short_term);
    write_list_section(&mut out, "Long-Term Goals", &frontmatter.goals_long_term);
    write_section(&mut out, "Symbol", &frontmatter.symbol_description);

    out
}

pub fn render_item_markdown(frontmatter: &ItemFrontmatter) -> String {
    let mut out = String::new();
    write_attr_line(&mut out, "Category", &frontmatter.category);
    write_attr_line(&mut out, "Rarity", &frontmatter.rarity);
    write_attr_line(&mut out, "Attunement", &frontmatter.attunement);
    write_attr_line(&mut out, "Value", &frontmatter.value);
    write_attr_line(&mut out, "Location", &frontmatter.location);
    writeln!(&mut out).ok();

    write_section(&mut out, "Appearance", &frontmatter.appearance);
    write_section(&mut out, "Abilities", &frontmatter.abilities);
    write_section(&mut out, "Drawbacks", &frontmatter.drawbacks);
    write_section(&mut out, "History", &frontmatter.history);
    write_list_section(&mut out, "Materials", &frontmatter.materials);

    out
}

fn write_attr_line(out: &mut String, label: &str, value: &str) {
    let normalized = normalize_unknown_text(value);
    if normalized != "Unknown" {
        writeln!(out, "**{label}:** {normalized}").ok();
    }
}

fn write_section(out: &mut String, title: &str, value: &str) {
    let normalized = normalize_unknown_text(value);
    if normalized == "Unknown" {
        return;
    }
    writeln!(out, "## {title}").ok();
    writeln!(out, "{}", normalized).ok();
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
}
