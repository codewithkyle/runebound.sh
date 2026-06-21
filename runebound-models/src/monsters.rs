//! Monster / stat-block reference model + card renderer.
//!
//! `Monster`/`StatSection`/`StatAbility`/`StatBlock` are the converted,
//! render-ready form of a 5etools monster: the canonical TOML card payload *and*
//! the search-index source. Like [`crate::spells::Spell`] they are **backend-only**
//! — they never cross to the frontend (only the rendered [`OutputDoc`] does), so
//! they derive `Serialize`/`Deserialize` but not `TS`.
//!
//! The conversion from raw 5etools JSON lives in `dnd_core::monster_import`; this
//! module owns only the shipped shape + how it renders to a card. Every defensive
//! stat is pre-formatted to a display string during conversion, so the card builder
//! is a straight 1:1 map (mirroring `spell_card`).

use serde::{Deserialize, Serialize};

use crate::output::{
    InlineNode, OutputBlock, OutputDoc, code_block, command_ref, doc, emphasis, entity_card_full,
    entity_row, heading, list, paragraph_with_inlines, strong, text_node,
};

/// A converted, render-ready monster stat block. All `{@tag}` markup already
/// stripped; all defensive stats pre-formatted to display strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monster {
    /// Kebab-case name, the primary key in both the TOML store and the search DB.
    pub slug: String,
    pub name: String,
    /// 5etools source code ("XMM", "MPMM", …), kept for the provenance footer.
    pub source: String,
    pub size: String,          // "Small"
    pub creature_type: String, // "Fey (Goblinoid)"
    pub alignment: String,     // "Chaotic Neutral" (may be empty)
    pub ac: String,            // "15 (natural armor)"
    pub hp: String,            // "10 (3d6)"
    pub speed: String,         // "30 ft., Fly 60 ft."
    /// STR DEX CON INT WIS CHA, raw scores; the card renders score + modifier.
    pub abilities: [i16; 6],
    pub saves: String,                  // "Dex +4, Con +6"  (empty → omit row)
    pub skills: String,                 // "Perception +5, Stealth +6"
    pub damage_resistances: String,     // empty → omit
    pub damage_immunities: String,      // empty → omit
    pub damage_vulnerabilities: String, // empty → omit
    pub condition_immunities: String,   // empty → omit
    pub senses: String,                 // includes "Passive Perception N"
    pub languages: String,              // "Common, Goblin" / "—"
    pub cr: String,                     // "1/4 (XP 50; PB +2)"
    pub gear: String,                   // "scimitar, shield"  (empty → omit)
    /// Trait / action / bonus-action / reaction / legendary / lair / regional
    /// sections, in render order. Empty sections are dropped during conversion.
    pub sections: Vec<StatSection>,
    /// Lore prose from `fluff-bestiary-*.json`, lowered like the stat-block
    /// sections and rendered under a trailing "Lore" heading. Empty when the
    /// monster has no fluff. Artwork is intentionally omitted — the frontend
    /// resolves only built-in asset keys, not external 5etools image files.
    #[serde(default)]
    pub lore: Vec<StatBlock>,
}

/// One titled group of stat-block abilities ("Traits", "Actions", "Legendary
/// Actions", "Lair Actions", "Regional Effects", …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatSection {
    pub title: String,
    /// Optional lead-in prose (legendary/mythic header, lair-action preamble,
    /// regional-effect intro), tags stripped.
    #[serde(default)]
    pub intro: Vec<StatBlock>,
    #[serde(default)]
    pub abilities: Vec<StatAbility>,
}

/// A single named ability ("Scimitar", "Nimble Escape", "Spellcasting").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatAbility {
    /// Some abilities (a bare legendary/lair list) have no name.
    #[serde(default)]
    pub name: Option<String>,
    pub body: Vec<StatBlock>,
}

/// A run of stat-block prose: plain text, or a clickable cross-link into the
/// spellbook / bestiary. Produced during import — `{@spell …}` / `{@creature …}`
/// markup lowers to a [`Span::Link`]; everything else lowers to [`Span::Text`].
/// [`monster_card`] maps each `Link` to an [`crate::output::InlineNode::CommandRef`]
/// and each `Text` to a plain inline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Span {
    /// Plain, non-interactive text.
    Text { text: String },
    /// A clickable cross-link: `label` is the visible text, `command` is the
    /// command run on click (e.g. `"spell Fireball"`, `"monster Goblin"`).
    Link { label: String, command: String },
}

impl Span {
    /// The visible text of this span (its label), ignoring any link target — the
    /// source for plain-text fallbacks and tests.
    pub fn text(&self) -> &str {
        match self {
            Span::Text { text } | Span::Link { label: text, .. } => text,
        }
    }
}

/// Flatten a span run to its plain text (links degrade to their visible label).
fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(Span::text).collect()
}

/// Render-ready body element — the monster analog of [`crate::spells::SpellBlock`],
/// deliberately flat (no recursive nesting) so the whole `Monster` serializes
/// cleanly to TOML. It is `SpellBlock` minus `Heading` (subsection titles become a
/// [`StatAbility::name`] instead). Prose and bullets carry [`Span`]s so cross-links
/// survive; table cells stay plain text (stat-block tables rarely cross-link).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatBlock {
    /// A paragraph of prose, as a run of text + cross-link spans.
    Text { spans: Vec<Span> },
    /// A bulleted list; each item is one already-flattened line of spans.
    Bullets { items: Vec<Vec<Span>> },
    /// A small table, rendered as a fixed-width code block (there is no
    /// `OutputBlock::Table`); stat-block tables are tiny, so this reads fine.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl StatBlock {
    /// The plain-text content of this block — used by plain-text fallbacks and tests.
    pub fn to_text(&self) -> String {
        match self {
            StatBlock::Text { spans } => spans_text(spans),
            StatBlock::Bullets { items } => items
                .iter()
                .map(|item| spans_text(item))
                .collect::<Vec<_>>()
                .join("\n"),
            StatBlock::Table { headers, rows } => {
                let mut lines = vec![headers.join(" | ")];
                for row in rows {
                    lines.push(row.join(" | "));
                }
                lines.join("\n")
            }
        }
    }
}

/// The six ability scores in stat-block order.
const ABILITY_LABELS: [&str; 6] = ["STR", "DEX", "CON", "INT", "WIS", "CHA"];

/// Render a monster as a single entity card: the name as the card title, the
/// "Size Type, Alignment" line as its subtitle, the defensive stats as label/value
/// rows, and the trait/action/etc. sections + provenance footer as the card body.
///
/// Everything lives *inside* one card (exactly like [`crate::spells::spell_card`])
/// so the whole stat block reads as one unit.
pub fn monster_card(monster: &Monster) -> OutputDoc {
    let mut body: Vec<OutputBlock> = Vec::new();
    for section in &monster.sections {
        body.push(heading(3, section.title.clone()));
        for block in &section.intro {
            push_stat_block(&mut body, block);
        }
        for ability in &section.abilities {
            push_stat_ability(&mut body, ability);
        }
    }
    // Fluff lore, when imported, renders as a final titled section above the footer.
    if !monster.lore.is_empty() {
        body.push(heading(3, "Lore"));
        for block in &monster.lore {
            push_stat_block(&mut body, block);
        }
    }
    body.push(paragraph_with_inlines(vec![emphasis(format!(
        "Source: {}",
        monster.source
    ))]));

    doc().with_block(entity_card_full(
        monster.name.clone(),
        Some(subtitle_line(monster)),
        stat_rows(monster),
        body,
    ))
}

/// "Small Fey (Goblinoid), Chaotic Neutral" — alignment is omitted when absent.
fn subtitle_line(monster: &Monster) -> String {
    let lead = [monster.size.as_str(), monster.creature_type.as_str()]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    if monster.alignment.is_empty() {
        lead
    } else if lead.is_empty() {
        monster.alignment.clone()
    } else {
        format!("{lead}, {}", monster.alignment)
    }
}

/// The defensive label/value rows, skipping any that are empty.
fn stat_rows(monster: &Monster) -> Vec<crate::output::EntityCardRow> {
    let mut rows = Vec::new();
    let mut push = |label: &str, value: &str| {
        if !value.is_empty() {
            rows.push(entity_row(label, value.to_string()));
        }
    };
    push("AC", &monster.ac);
    push("HP", &monster.hp);
    push("Speed", &monster.speed);
    push("Abilities", &abilities_line(&monster.abilities));
    push("Saving Throws", &monster.saves);
    push("Skills", &monster.skills);
    push("Resistances", &monster.damage_resistances);
    push("Immunities", &monster.damage_immunities);
    push("Vulnerabilities", &monster.damage_vulnerabilities);
    push("Condition Immunities", &monster.condition_immunities);
    push("Gear", &monster.gear);
    push("Senses", &monster.senses);
    push("Languages", &monster.languages);
    push("CR", &monster.cr);
    rows
}

/// "STR 8 (-1) · DEX 15 (+2) · CON 10 (+0) · …" — each score with its modifier.
fn abilities_line(abilities: &[i16; 6]) -> String {
    ABILITY_LABELS
        .iter()
        .zip(abilities.iter())
        .map(|(label, &score)| format!("{label} {score} ({})", format_modifier(score)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The 5e ability modifier `(score - 10) / 2` (round toward negative infinity),
/// formatted with an explicit sign: `+2`, `+0`, `-1`.
fn format_modifier(score: i16) -> String {
    let modifier = (score - 10).div_euclid(2);
    if modifier >= 0 {
        format!("+{modifier}")
    } else {
        modifier.to_string()
    }
}

/// Render one named ability: the name as a bold inline prefix on its first line,
/// then the rest of its body. A nameless ability renders its body verbatim.
fn push_stat_ability(out: &mut Vec<OutputBlock>, ability: &StatAbility) {
    let mut blocks = ability.body.iter();
    match (&ability.name, blocks.next()) {
        // Name + a leading prose line → "**Name.** body" as one paragraph, the
        // body spans (links and all) following the bold name inline.
        (Some(name), Some(StatBlock::Text { spans })) => {
            let mut inlines = vec![strong(format!("{name}.")), text_node(" ")];
            inlines.extend(spans_to_inlines(spans));
            out.push(paragraph_with_inlines(inlines));
        }
        // Name but the first block is a list/table → the name on its own line.
        (Some(name), Some(first)) => {
            out.push(paragraph_with_inlines(vec![strong(format!("{name}."))]));
            push_stat_block(out, first);
        }
        (Some(name), None) => {
            out.push(paragraph_with_inlines(vec![strong(format!("{name}."))]));
        }
        (None, Some(first)) => push_stat_block(out, first),
        (None, None) => {}
    }
    for block in blocks {
        push_stat_block(out, block);
    }
}

fn push_stat_block(out: &mut Vec<OutputBlock>, block: &StatBlock) {
    match block {
        StatBlock::Text { spans } => out.push(paragraph_with_inlines(spans_to_inlines(spans))),
        StatBlock::Bullets { items } => out.push(list(
            items.iter().map(|item| spans_to_inlines(item)).collect(),
        )),
        StatBlock::Table { headers, rows } => {
            out.push(code_block(None::<String>, render_table(headers, rows)))
        }
    }
}

/// Map stat-block spans to output inlines: a [`Span::Link`] becomes a clickable
/// `command_ref`, a [`Span::Text`] a plain text node.
fn spans_to_inlines(spans: &[Span]) -> Vec<InlineNode> {
    spans
        .iter()
        .map(|span| match span {
            Span::Text { text } => text_node(text.clone()),
            Span::Link { label, command } => command_ref(label.clone(), command.clone()),
        })
        .collect()
}

/// Render a tiny table as aligned fixed-width text for a `Code` block (the monster
/// twin of `spells::render_table`; stat-block tables are small enough that this
/// reads cleanly).
fn render_table(headers: &[String], rows: &[Vec<String>]) -> String {
    fn cell(row: &[String], i: usize) -> &str {
        row.get(i).map(String::as_str).unwrap_or("")
    }
    let col_count = headers
        .len()
        .max(rows.iter().map(|row| row.len()).max().unwrap_or(0));
    let mut widths = vec![0usize; col_count];
    for (i, width) in widths.iter_mut().enumerate() {
        *width = headers.get(i).map(|h| h.len()).unwrap_or(0);
        for row in rows {
            *width = (*width).max(cell(row, i).len());
        }
    }
    let fmt_row = |row: &[String]| {
        (0..col_count)
            .map(|i| format!("{:<width$}", cell(row, i), width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let mut lines = Vec::new();
    if !headers.is_empty() {
        lines.push(fmt_row(headers));
        lines.push(
            widths
                .iter()
                .map(|w| "-".repeat(*w))
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_string(),
        );
    }
    for row in rows {
        lines.push(fmt_row(row));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBlock;

    fn goblin() -> Monster {
        Monster {
            slug: "goblin-warrior".to_string(),
            name: "Goblin Warrior".to_string(),
            source: "XMM".to_string(),
            size: "Small".to_string(),
            creature_type: "Fey (Goblinoid)".to_string(),
            alignment: "Chaotic Neutral".to_string(),
            ac: "15".to_string(),
            hp: "10 (3d6)".to_string(),
            speed: "30 ft.".to_string(),
            abilities: [8, 15, 10, 10, 8, 8],
            saves: String::new(),
            skills: "Stealth +6".to_string(),
            damage_resistances: String::new(),
            damage_immunities: String::new(),
            damage_vulnerabilities: String::new(),
            condition_immunities: String::new(),
            senses: "Darkvision 60 ft., Passive Perception 9".to_string(),
            languages: "Common, Goblin".to_string(),
            cr: "1/4 (XP 50; PB +2)".to_string(),
            gear: "leather armor, scimitar, shield, shortbow".to_string(),
            sections: vec![StatSection {
                title: "Actions".to_string(),
                intro: Vec::new(),
                abilities: vec![StatAbility {
                    name: Some("Scimitar".to_string()),
                    body: vec![StatBlock::Text {
                        spans: vec![Span::Text {
                            text:
                                "Melee Attack Roll: +4, reach 5 ft. Hit: 5 (1d6 + 2) Slashing damage."
                                    .to_string(),
                        }],
                    }],
                }],
            }],
            lore: Vec::new(),
        }
    }

    fn card(
        doc: &OutputDoc,
    ) -> (
        &str,
        Option<&str>,
        &[crate::output::EntityCardRow],
        &[OutputBlock],
    ) {
        doc.blocks
            .iter()
            .find_map(|block| match block {
                OutputBlock::EntityCard {
                    title,
                    subtitle,
                    rows,
                    body,
                } => Some((
                    title.as_str(),
                    subtitle.as_deref(),
                    rows.as_slice(),
                    body.as_slice(),
                )),
                _ => None,
            })
            .expect("monster card should render an entity card")
    }

    #[test]
    fn card_is_a_single_entity_card_titled_by_name() {
        let doc = monster_card(&goblin());
        assert_eq!(doc.blocks.len(), 1);
        let (title, _, _, _) = card(&doc);
        assert_eq!(title, "Goblin Warrior");
    }

    #[test]
    fn subtitle_joins_size_type_and_alignment() {
        let doc = monster_card(&goblin());
        assert_eq!(card(&doc).1, Some("Small Fey (Goblinoid), Chaotic Neutral"));
    }

    #[test]
    fn subtitle_omits_absent_alignment() {
        let mut m = goblin();
        m.alignment = String::new();
        let doc = monster_card(&m);
        assert_eq!(card(&doc).1, Some("Small Fey (Goblinoid)"));
    }

    #[test]
    fn abilities_row_shows_scores_with_modifiers() {
        let doc = monster_card(&goblin());
        let (_, _, rows, _) = card(&doc);
        let abilities = rows
            .iter()
            .find(|r| r.label == "Abilities")
            .expect("abilities row present");
        assert_eq!(
            abilities.value,
            "STR 8 (-1) · DEX 15 (+2) · CON 10 (+0) · INT 10 (+0) · WIS 8 (-1) · CHA 8 (-1)"
        );
    }

    #[test]
    fn empty_stat_rows_are_omitted() {
        let doc = monster_card(&goblin());
        let (_, _, rows, _) = card(&doc);
        // Goblin has no saves / resistances, so those rows must not appear.
        assert!(rows.iter().all(|r| r.label != "Saving Throws"));
        assert!(rows.iter().all(|r| r.label != "Resistances"));
        // But it does carry Skills, Gear, Senses, Languages, CR.
        for label in [
            "AC",
            "HP",
            "Speed",
            "Abilities",
            "Skills",
            "Gear",
            "Senses",
            "Languages",
            "CR",
        ] {
            assert!(rows.iter().any(|r| r.label == label), "missing {label} row");
        }
    }

    #[test]
    fn ability_renders_name_as_bold_prefix() {
        use crate::output::InlineNode;
        let doc = monster_card(&goblin());
        let (_, _, _, body) = card(&doc);
        let has_bold_name = body.iter().any(|block| match block {
            OutputBlock::Paragraph { inlines } => inlines
                .iter()
                .any(|node| matches!(node, InlineNode::Strong { text } if text == "Scimitar.")),
            _ => false,
        });
        assert!(has_bold_name, "action name should render as a bold prefix");
    }

    #[test]
    fn section_titles_become_headings() {
        let doc = monster_card(&goblin());
        let (_, _, _, body) = card(&doc);
        assert!(body.iter().any(|block| matches!(
            block,
            OutputBlock::Heading { level: 3, text } if text == "Actions"
        )));
    }

    #[test]
    fn provenance_footer_lists_source() {
        let plain = monster_card(&goblin()).to_plain_text();
        assert!(plain.contains("Source: XMM"), "got:\n{plain}");
    }

    #[test]
    fn modifier_rounds_toward_negative_infinity() {
        assert_eq!(format_modifier(1), "-5"); // (1-10)/2 = -4.5 -> -5
        assert_eq!(format_modifier(8), "-1");
        assert_eq!(format_modifier(10), "+0");
        assert_eq!(format_modifier(11), "+0");
        assert_eq!(format_modifier(20), "+5");
    }

    #[test]
    fn monster_round_trips_through_toml() {
        // The card payload is stored as TOML; a monster with sections (arrays of
        // tables), bullets, and a table must survive a serialize → parse round-trip.
        let mut original = goblin();
        original.sections.push(StatSection {
            title: "Regional Effects".to_string(),
            intro: vec![StatBlock::Text {
                spans: vec![Span::Text {
                    text: "The region is warped:".to_string(),
                }],
            }],
            abilities: vec![StatAbility {
                name: None,
                body: vec![
                    // A bullet carrying a cross-link span must survive the round-trip.
                    StatBlock::Bullets {
                        items: vec![vec![
                            Span::Text {
                                text: "All-Seeing. The lich casts ".to_string(),
                            },
                            Span::Link {
                                label: "Clairvoyance".to_string(),
                                command: "spell Clairvoyance".to_string(),
                            },
                            Span::Text {
                                text: ".".to_string(),
                            },
                        ]],
                    },
                    StatBlock::Table {
                        headers: vec!["d6".to_string(), "Effect".to_string()],
                        rows: vec![vec!["1".to_string(), "Fog".to_string()]],
                    },
                ],
            }],
        });
        // Lore (fluff) is also part of the stored card payload.
        original.lore = vec![StatBlock::Text {
            spans: vec![Span::Text {
                text: "Goblins are small, black-hearted humanoids.".to_string(),
            }],
        }];
        let encoded = toml::to_string_pretty(&original).expect("serialize monster to toml");
        let decoded: Monster = toml::from_str(&encoded).expect("parse monster from toml");
        assert_eq!(decoded, original, "toml round-trip changed the monster");
    }

    #[test]
    fn link_spans_render_as_clickable_command_refs() {
        use crate::output::InlineNode;
        let mut goblin = goblin();
        // A creature cross-link inside an action body should reach the card as a
        // clickable command_ref targeting the bestiary lookup.
        goblin.sections[0].abilities[0].body = vec![StatBlock::Text {
            spans: vec![
                Span::Text {
                    text: "Summons a ".to_string(),
                },
                Span::Link {
                    label: "Goblin Boss".to_string(),
                    command: "monster Goblin Boss".to_string(),
                },
                Span::Text {
                    text: ".".to_string(),
                },
            ],
        }];
        let doc = monster_card(&goblin);
        let (_, _, _, body) = card(&doc);
        let has_link = body.iter().any(|block| match block {
            OutputBlock::Paragraph { inlines } => inlines.iter().any(|node| {
                matches!(node, InlineNode::CommandRef { label, command }
                    if label == "Goblin Boss" && command == "monster Goblin Boss")
            }),
            _ => false,
        });
        assert!(
            has_link,
            "creature cross-link should render as a command_ref"
        );
    }

    #[test]
    fn lore_renders_under_a_trailing_heading() {
        let mut goblin = goblin();
        goblin.lore = vec![StatBlock::Text {
            spans: vec![Span::Text {
                text: "Goblins infest the wild places of the world.".to_string(),
            }],
        }];
        let plain = monster_card(&goblin).to_plain_text();
        assert!(plain.contains("### Lore"), "lore heading missing:\n{plain}");
        assert!(
            plain.contains("Goblins infest the wild places"),
            "lore prose missing:\n{plain}"
        );
    }
}
