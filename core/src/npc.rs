use std::path::Path;

use chrono::Utc;
use serde::Serialize;

pub const UNKNOWN_LOCATION: &str = "Unknown";

#[derive(Debug, Clone, Serialize)]
pub struct NpcFrontmatter {
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    pub carrying: Vec<String>,
    pub location: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocationFrontmatter {
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
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
    let mut candidate = base_slug.to_string();
    let mut idx = 2;

    loop {
        let path = root.join(relative_dir).join(format!("{candidate}.md"));
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
        if invalid || ch.is_whitespace() {
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
        "Untitled".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn render_npc_markdown(frontmatter: &NpcFrontmatter) -> Result<String, toml::ser::Error> {
    #[derive(Serialize)]
    struct Frontmatter<'a> {
        #[serde(rename = "type")]
        doc_type: &'a str,
        id: &'a str,
        slug: &'a str,
        name: &'a str,
        race: &'a str,
        occupation: &'a str,
        sex: &'a str,
        age: &'a str,
        height: &'a str,
        weight_lbs: &'a str,
        background: &'a str,
        want_need: &'a str,
        secret_obstacle: &'a str,
        carrying: &'a [String],
        location: &'a str,
        created_at: &'a str,
        updated_at: &'a str,
    }

    let fm = Frontmatter {
        doc_type: &frontmatter.doc_type,
        id: &frontmatter.id,
        slug: &frontmatter.slug,
        name: &frontmatter.name,
        race: &frontmatter.race,
        occupation: &frontmatter.occupation,
        sex: &frontmatter.sex,
        age: &frontmatter.age,
        height: &frontmatter.height,
        weight_lbs: &frontmatter.weight_lbs,
        background: &frontmatter.background,
        want_need: &frontmatter.want_need,
        secret_obstacle: &frontmatter.secret_obstacle,
        carrying: &frontmatter.carrying,
        location: &frontmatter.location,
        created_at: &frontmatter.created_at,
        updated_at: &frontmatter.updated_at,
    };

    let mut out = String::new();
    out.push_str("```runebound\n");
    out.push_str(&toml::to_string_pretty(&fm)?);
    out.push_str("```\n");
    Ok(out)
}

pub fn render_location_markdown(
    frontmatter: &LocationFrontmatter,
) -> Result<String, toml::ser::Error> {
    #[derive(Serialize)]
    struct Frontmatter<'a> {
        #[serde(rename = "type")]
        doc_type: &'a str,
        id: &'a str,
        slug: &'a str,
        name: &'a str,
        created_at: &'a str,
        updated_at: &'a str,
    }

    let fm = Frontmatter {
        doc_type: &frontmatter.doc_type,
        id: &frontmatter.id,
        slug: &frontmatter.slug,
        name: &frontmatter.name,
        created_at: &frontmatter.created_at,
        updated_at: &frontmatter.updated_at,
    };

    let mut out = String::new();
    out.push_str("```runebound\n");
    out.push_str(&toml::to_string_pretty(&fm)?);
    out.push_str("```\n");
    Ok(out)
}

pub fn merge_runebound_block(existing: &str, runebound_block: &str) -> String {
    let Some(block_start) = existing.find("```runebound") else {
        if existing.trim().is_empty() {
            return runebound_block.to_string();
        }
        return format!("{}\n{}", runebound_block, existing);
    };

    let search_from = block_start + "```runebound".len();
    let Some(relative_end) = existing[search_from..].find("\n```") else {
        if existing.trim().is_empty() {
            return runebound_block.to_string();
        }
        return format!("{}\n{}", runebound_block, existing);
    };

    let mut block_end = search_from + relative_end + "\n```".len();
    if existing[block_end..].starts_with("\r\n") {
        block_end += 2;
    } else if existing[block_end..].starts_with('\n') {
        block_end += 1;
    }
    let mut merged = String::with_capacity(existing.len() + runebound_block.len());
    merged.push_str(&existing[..block_start]);
    merged.push_str(runebound_block);
    merged.push_str(&existing[block_end..]);
    merged
}

#[cfg(test)]
mod tests {
    use super::merge_runebound_block;

    #[test]
    fn merge_replaces_existing_runebound_block() {
        let existing =
            "# Notes\n\n```runebound\ntype = \"npc\"\nname = \"Old\"\n```\n\nPlayer notes here.\n";
        let replacement = "```runebound\ntype = \"npc\"\nname = \"New\"\n```\n";

        let merged = merge_runebound_block(existing, replacement);

        assert!(merged.contains("name = \"New\""));
        assert!(!merged.contains("name = \"Old\""));
        assert!(merged.contains("Player notes here."));
    }

    #[test]
    fn merge_prepends_block_when_missing() {
        let existing = "# Story\nThis should remain.\n";
        let replacement = "```runebound\ntype = \"location\"\nname = \"Neverwinter\"\n```\n";

        let merged = merge_runebound_block(existing, replacement);

        assert!(merged.starts_with(replacement));
        assert!(merged.contains("# Story"));
    }

    #[test]
    fn merge_does_not_accumulate_blank_lines_after_repeated_saves() {
        let replacement = "```runebound\ntype = \"npc\"\nname = \"A\"\n```\n";
        let original = "```runebound\ntype = \"npc\"\nname = \"A\"\n```\n\nNotes\n";

        let once = merge_runebound_block(original, replacement);
        let twice = merge_runebound_block(&once, replacement);

        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_markdown_file_stem_keeps_readable_name() {
        assert_eq!(
            super::normalize_markdown_file_stem("  Lady Aria of Neverwinter  "),
            "Lady Aria of Neverwinter"
        );
    }

    #[test]
    fn normalize_markdown_file_stem_replaces_invalid_chars() {
        assert_eq!(
            super::normalize_markdown_file_stem("Drizzt/Do'Urden: Ranger?"),
            "Drizzt Do'Urden Ranger"
        );
    }
}
