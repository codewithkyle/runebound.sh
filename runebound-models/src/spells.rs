//! Spell reference model + card renderer.
//!
//! `Spell`/`SpellBlock` are the converted, render-ready form of a 5etools spell:
//! the canonical TOML card payload *and* the search-index source. They are
//! **backend-only** — mirroring the entity `*Frontmatter` types in `drafts.rs`,
//! they never cross to the frontend (only the rendered [`OutputDoc`] does), so they
//! derive `Serialize`/`Deserialize` but not `TS`.
//!
//! The conversion from raw 5etools JSON lives in `dnd_core::spell_import`; this
//! module owns only the shipped shape + how it renders to a card.

use serde::{Deserialize, Serialize};

use crate::monsters::{Span, spans_to_inlines};
use crate::output::{
    OutputBlock, OutputDoc, code_block, doc, emphasis, entity_card_full, entity_row, heading, list,
    paragraph_with_inlines, render_table,
};

/// A converted, render-ready spell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spell {
    /// Kebab-case name, the primary key in both the TOML store and the search DB.
    pub slug: String,
    pub name: String,
    /// 5etools source code ("XPHB", "TCE", …), kept for the provenance footer.
    pub source: String,
    /// 0 = cantrip, 1-9 otherwise.
    pub level: u8,
    /// Expanded school name, e.g. "Evocation".
    pub school: String,
    /// "1 Action", "1 Bonus Action", "1 Reaction, which you take when…".
    pub casting_time: String,
    /// "150 feet", "Self (15-foot Cone)", "Touch".
    pub range: String,
    /// "V, S, M (a ball of bat guano and sulfur)" or "—".
    pub components: String,
    /// "Instantaneous", "Concentration, up to 1 minute", "8 hours", "Until Dispelled".
    pub duration: String,
    pub ritual: bool,
    pub concentration: bool,
    /// Class associations when the source carries them (the 2024 core data does not,
    /// so this is usually empty — the card omits the line then).
    #[serde(default)]
    pub classes: Vec<String>,
    /// The spell body, with `{@tag}` markup already stripped.
    pub description: Vec<SpellBlock>,
    /// "Using a Higher-Level Spell Slot" / "Cantrip Upgrade" content. Each block set
    /// already begins with its own [`SpellBlock::Heading`], so the card renders it
    /// verbatim rather than injecting a fixed heading (cantrips vs leveled spells
    /// title this section differently).
    #[serde(default)]
    pub higher_levels: Option<Vec<SpellBlock>>,
}

/// One render-ready body element. Deliberately **flat** (no recursive nesting): a
/// 5etools named subsection lowers to a [`SpellBlock::Heading`] followed by its
/// children rather than a nested block, so the whole `Spell` serializes cleanly to
/// TOML (an array of simple tables) and the card builder is a straight 1:1 map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpellBlock {
    /// A paragraph of prose, as a run of text + cross-link [`Span`]s (so `{@spell}`
    /// / `{@creature}` references render clickable, matching monster stat blocks).
    Text { spans: Vec<Span> },
    /// A subsection title (a named option such as "Aquatic Adaptation"). Plain text;
    /// titles never carry cross-links.
    Heading { text: String },
    /// A bulleted list; each item is one already-flattened line of spans.
    Bullets { items: Vec<Vec<Span>> },
    /// A small table. Rendered as a fixed-width code block (there is no
    /// `OutputBlock::Table`); spell tables are tiny, so this reads fine. Cells stay
    /// plain text (spell tables rarely cross-link).
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl SpellBlock {
    /// The plain-text content of this block — used by plain-text fallbacks and tests.
    /// Links degrade to their visible label.
    pub fn to_text(&self) -> String {
        match self {
            SpellBlock::Text { spans } => spans.iter().map(Span::text).collect(),
            SpellBlock::Heading { text } => text.clone(),
            SpellBlock::Bullets { items } => items
                .iter()
                .map(|item| item.iter().map(Span::text).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n"),
            SpellBlock::Table { headers, rows } => {
                let mut lines = vec![headers.join(" | ")];
                for row in rows {
                    lines.push(row.join(" | "));
                }
                lines.join("\n")
            }
        }
    }
}

/// Render a spell as a single entity card: the name as the card title, the
/// level/school line as its subtitle, the casting stats as label/value rows, and
/// the description + higher-level scaling + provenance footer as the card body.
///
/// Everything lives *inside* one card so the whole spell reads as one unit rather
/// than a heading followed by loose prose blocks.
pub fn spell_card(spell: &Spell) -> OutputDoc {
    let mut body: Vec<OutputBlock> = Vec::new();
    for block in &spell.description {
        push_spell_block(&mut body, block);
    }
    if let Some(higher) = &spell.higher_levels
        && !higher.is_empty()
    {
        for block in higher {
            push_spell_block(&mut body, block);
        }
    }
    body.push(paragraph_with_inlines(vec![emphasis(provenance_line(
        spell,
    ))]));

    doc().with_block(entity_card_full(
        spell.name.clone(),
        Some(level_school_line(spell)),
        vec![
            entity_row("Casting Time:", spell.casting_time.clone()),
            entity_row("Range:", spell.range.clone()),
            entity_row("Components:", spell.components.clone()),
            entity_row("Duration:", spell.duration.clone()),
        ],
        body,
    ))
}

/// The stat-block title line: "Evocation Cantrip" / "Level 3 Evocation", with a
/// "(Ritual)" suffix when the spell can be cast ritually.
fn level_school_line(spell: &Spell) -> String {
    let mut line = if spell.level == 0 {
        format!("{} Cantrip", spell.school)
    } else {
        format!("Level {} {}", spell.level, spell.school)
    };
    if spell.ritual {
        line.push_str(" (Ritual)");
    }
    line
}

/// The footer: source code, plus class list when present.
fn provenance_line(spell: &Spell) -> String {
    let mut line = format!("Source: {}", spell.source);
    if !spell.classes.is_empty() {
        line.push_str(&format!(" · Classes: {}", spell.classes.join(", ")));
    }
    line
}

fn push_spell_block(out: &mut Vec<OutputBlock>, block: &SpellBlock) {
    match block {
        SpellBlock::Text { spans } => out.push(paragraph_with_inlines(spans_to_inlines(spans))),
        SpellBlock::Heading { text } => out.push(heading(3, text.clone())),
        SpellBlock::Bullets { items } => out.push(list(
            items.iter().map(|item| spans_to_inlines(item)).collect(),
        )),
        SpellBlock::Table { headers, rows } => {
            out.push(code_block(None::<String>, render_table(headers, rows)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBlock;

    fn sample(level: u8, ritual: bool) -> Spell {
        Spell {
            slug: "fireball".to_string(),
            name: "Fireball".to_string(),
            source: "XPHB".to_string(),
            level,
            school: "Evocation".to_string(),
            casting_time: "1 Action".to_string(),
            range: "150 feet".to_string(),
            components: "V, S, M (a ball of bat guano and sulfur)".to_string(),
            duration: "Instantaneous".to_string(),
            ritual,
            concentration: false,
            classes: Vec::new(),
            description: vec![SpellBlock::Text {
                spans: vec![Span::Text {
                    text: "A bright streak flashes from you.".to_string(),
                }],
            }],
            higher_levels: Some(vec![
                SpellBlock::Heading {
                    text: "Using a Higher-Level Spell Slot".to_string(),
                },
                SpellBlock::Text {
                    spans: vec![Span::Text {
                        text: "The damage increases by 1d6 for each slot above 3.".to_string(),
                    }],
                },
            ]),
        }
    }

    /// The spell card is a single entity card; pull out its title, subtitle, and
    /// body for assertions.
    fn card(doc: &OutputDoc) -> (&str, Option<&str>, &[OutputBlock]) {
        doc.blocks
            .iter()
            .find_map(|block| match block {
                OutputBlock::EntityCard {
                    title,
                    subtitle,
                    body,
                    ..
                } => Some((title.as_str(), subtitle.as_deref(), body.as_slice())),
                _ => None,
            })
            .expect("spell card should render an entity card")
    }

    #[test]
    fn leveled_card_subtitles_with_level_and_school() {
        let doc = spell_card(&sample(3, false));
        let (title, subtitle, _) = card(&doc);
        assert_eq!(title, "Fireball");
        assert_eq!(subtitle, Some("Level 3 Evocation"));
    }

    #[test]
    fn cantrip_card_subtitles_with_cantrip_wording() {
        let doc = spell_card(&sample(0, false));
        assert_eq!(card(&doc).1, Some("Evocation Cantrip"));
    }

    #[test]
    fn ritual_flag_annotates_the_subtitle() {
        let doc = spell_card(&sample(1, true));
        assert_eq!(card(&doc).1, Some("Level 1 Evocation (Ritual)"));
    }

    #[test]
    fn card_is_a_single_entity_card_titled_by_name() {
        let doc = spell_card(&sample(3, false));
        // One block: the whole spell lives inside one card (name as title).
        assert_eq!(doc.blocks.len(), 1);
        assert!(matches!(
            doc.blocks.first(),
            Some(OutputBlock::EntityCard { title, .. }) if title == "Fireball"
        ));
    }

    #[test]
    fn higher_level_blocks_render_inside_the_card_body() {
        let doc = spell_card(&sample(3, false));
        // The synthesized higher-level heading appears within the card body.
        let (_, _, body) = card(&doc);
        assert!(body.iter().any(|block| matches!(
            block,
            OutputBlock::Heading { level: 3, text } if text == "Using a Higher-Level Spell Slot"
        )));
    }

    #[test]
    fn provenance_footer_lists_source() {
        let doc = spell_card(&sample(3, false));
        let plain = doc.to_plain_text();
        assert!(plain.contains("Source: XPHB"), "got:\n{plain}");
    }

    #[test]
    fn spell_round_trips_through_toml() {
        // The card payload is stored as TOML; a nested block body must survive a
        // serialize → parse round-trip (the storage contract).
        let original = Spell {
            description: vec![
                SpellBlock::Text {
                    spans: vec![
                        Span::Text {
                            text: "Choose one option, such as ".to_string(),
                        },
                        Span::Link {
                            label: "Fireball".to_string(),
                            command: "spell Fireball".to_string(),
                        },
                        Span::Text {
                            text: ".".to_string(),
                        },
                    ],
                },
                SpellBlock::Heading {
                    text: "Aquatic Adaptation".to_string(),
                },
                SpellBlock::Bullets {
                    items: vec![vec![Span::Text {
                        text: "You can breathe underwater.".to_string(),
                    }]],
                },
                SpellBlock::Table {
                    headers: vec!["d6".to_string(), "Effect".to_string()],
                    rows: vec![
                        vec!["1".to_string(), "Acid".to_string()],
                        vec!["2".to_string(), "Cold".to_string()],
                    ],
                },
            ],
            ..sample(2, false)
        };
        let encoded = toml::to_string_pretty(&original).expect("serialize spell to toml");
        let decoded: Spell = toml::from_str(&encoded).expect("parse spell from toml");
        assert_eq!(decoded, original, "toml round-trip changed the spell");
    }

    #[test]
    fn link_spans_render_as_clickable_command_refs() {
        use crate::output::InlineNode;
        // A `{@spell}` cross-link inside a spell description should reach the card as
        // a clickable command_ref (parity with monster stat blocks — D1).
        let doc = spell_card(&Spell {
            description: vec![SpellBlock::Text {
                spans: vec![
                    Span::Text {
                        text: "Works like ".to_string(),
                    },
                    Span::Link {
                        label: "Fireball".to_string(),
                        command: "spell Fireball".to_string(),
                    },
                    Span::Text {
                        text: ", but colder.".to_string(),
                    },
                ],
            }],
            ..sample(3, false)
        });
        let (_, _, body) = card(&doc);
        let has_link = body.iter().any(|block| match block {
            OutputBlock::Paragraph { inlines } => inlines.iter().any(|node| {
                matches!(node, InlineNode::CommandRef { label, command }
                    if label == "Fireball" && command == "spell Fireball")
            }),
            _ => false,
        });
        assert!(has_link, "spell cross-link should render as a command_ref");
    }

    #[test]
    fn table_block_renders_aligned_columns() {
        let doc = spell_card(&Spell {
            description: vec![SpellBlock::Table {
                headers: vec!["d6".to_string(), "Damage".to_string()],
                rows: vec![vec!["1".to_string(), "Acid".to_string()]],
            }],
            ..sample(1, false)
        });
        let (_, _, body) = card(&doc);
        let has_code = body
            .iter()
            .any(|block| matches!(block, OutputBlock::Code { .. }));
        assert!(has_code, "table should render as a code block in the body");
    }
}
