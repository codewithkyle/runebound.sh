use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    pub race: String,
    pub occupation: String,
    pub sex: String,
    pub age: String,
    pub height: String,
    pub weight_lbs: String,
    pub background: String,
    pub want_need: String,
    pub secret_obstacle: String,
    #[serde(default)]
    pub carrying: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    #[serde(default)]
    pub exports: Vec<String>,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    #[serde(default)]
    pub allies: Vec<String>,
    #[serde(default)]
    pub rivals_enemies: Vec<String>,
    pub reputation: String,
    pub current_tension: String,
    #[serde(default)]
    pub goals_short_term: Vec<String>,
    #[serde(default)]
    pub goals_long_term: Vec<String>,
    pub symbol_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub category: String,
    pub rarity: String,
    pub attunement: String,
    #[serde(default)]
    pub materials: Vec<String>,
    pub appearance: String,
    pub abilities: String,
    pub drawbacks: String,
    pub history: String,
    pub value: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GodDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub epithet: String,
    pub rank: String,
    #[serde(default)]
    pub rank_custom: Option<String>,
    pub alignment: String,
    #[serde(default)]
    pub domains: Vec<String>,
    pub symbol: String,
    pub appearance: String,
    pub dogma: String,
    pub realm: String,
    pub worshippers: String,
    pub clergy: String,
    #[serde(default)]
    pub allies: Vec<String>,
    #[serde(default)]
    pub rivals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonBeat {
    pub function: String, // fixed skeleton: Entrance|Puzzle|Setback|Climax|Resolution
    pub content_type: String, // one of DUNGEON_CONTENT_TYPES (the 11)
    pub idea: String,     // 1-2 lines: what happens here
    #[serde(default)]
    pub player_goals: String, // what players should learn/do/achieve by completing this beat
    pub lever: String,    // one complication/question/hook
    #[serde(default)]
    pub loot: Option<String>, // conditional — None where the beat doesn't earn it
    #[serde(default)]
    pub design_note: String, // how this beat fits the overall dungeon and story
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>, // premise+context bias, reused by reroll
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    #[serde(default)]
    pub location: String, // the single bounded place all five beats sit inside
    #[serde(default)]
    pub story: String, // the Pass-1 micro-story the dungeon was generated from
    pub premise: String, // the spine / top-line (feature-dungeons.md §6)
    pub topology: String, // one of DUNGEON_TOPOLOGIES, or "none"
    pub tone: String,    // "tragedy" | "comedy"
    pub twist: String,   // "false_victory" | "false_defeat" | "neither"
    #[serde(default)]
    pub beats: Vec<DungeonBeat>, // exactly 5, fixed order
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
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
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub visual_description: String,
    pub history_background: String,
    pub exports: Vec<String>,
    pub tone: String,
    pub authority: String,
    pub danger_level: String,
    pub current_tension: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub kind_type: String,
    #[serde(default)]
    pub kind_custom: Option<String>,
    pub public_description: String,
    pub true_agenda: String,
    pub methods: String,
    pub leadership: String,
    pub headquarters: String,
    pub sphere_of_influence: String,
    pub resources_assets: String,
    pub allies: Vec<String>,
    pub rivals_enemies: Vec<String>,
    pub reputation: String,
    pub current_tension: String,
    pub goals_short_term: Vec<String>,
    pub goals_long_term: Vec<String>,
    pub symbol_description: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub category: String,
    pub rarity: String,
    pub attunement: String,
    pub materials: Vec<String>,
    pub appearance: String,
    pub abilities: String,
    pub drawbacks: String,
    pub history: String,
    pub value: String,
    pub location: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GodFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    pub epithet: String,
    pub rank: String,
    #[serde(default)]
    pub rank_custom: Option<String>,
    pub alignment: String,
    pub domains: Vec<String>,
    pub symbol: String,
    pub appearance: String,
    pub dogma: String,
    pub realm: String,
    pub worshippers: String,
    pub clergy: String,
    pub allies: Vec<String>,
    pub rivals: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String, // "dungeon"
    pub id: String,
    pub slug: String,
    pub name: String,
    pub vault_path: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub story: String,
    pub premise: String,
    pub topology: String,
    pub tone: String,
    pub twist: String,
    pub beats: Vec<DungeonBeat>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

use super::output::{
    OutputDoc, StatusTone, command_ref, doc, entity_card, entity_row, heading, paragraph_text,
    paragraph_with_inlines, status, text_node,
};
use super::utils::{normalize_unknown_list, normalize_unknown_text};

fn title_case_sex(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "male" => "Male".to_string(),
        "female" => "Female".to_string(),
        _ => value.to_string(),
    }
}

fn location_kind_display(kind_type: &str, kind_custom: &Option<String>) -> String {
    let kind = normalize_unknown_text(kind_type);
    if kind.to_lowercase() != "other" {
        return kind;
    }
    let custom = kind_custom.as_ref().map(|s| s.as_str()).unwrap_or("");
    let custom_norm = normalize_unknown_text(custom);
    if custom_norm == "Unknown" {
        return "Other".to_string();
    }
    format!("Other ({})", custom_norm)
}

pub fn npc_entity_card(draft: &NpcDraft) -> OutputDoc {
    let rows = vec![
        entity_row("Slug:", normalize_unknown_text(&draft.slug)),
        entity_row("Race:", normalize_unknown_text(&draft.race)),
        entity_row("Occupation:", normalize_unknown_text(&draft.occupation)),
        entity_row("Gender:", title_case_sex(&draft.sex)),
        entity_row("Age:", normalize_unknown_text(&draft.age)),
        entity_row("Height:", normalize_unknown_text(&draft.height)),
        entity_row(
            "Weight:",
            format!("{} lbs", normalize_unknown_text(&draft.weight_lbs)),
        ),
        entity_row("Background:", normalize_unknown_text(&draft.background)),
        entity_row("Want:", normalize_unknown_text(&draft.want_need)),
        entity_row("Secret:", normalize_unknown_text(&draft.secret_obstacle)),
        entity_row(
            "Carrying:",
            normalize_unknown_list(draft.carrying.clone()).join(", "),
        ),
        entity_row("Location:", normalize_unknown_text(&draft.location)),
    ];
    doc()
        .with_block(entity_card(&draft.name, rows))
        .with_block(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this NPC, or "),
            command_ref("reroll", "reroll"),
            text_node(" to generate again."),
        ]))
}

pub fn location_entity_card(draft: &LocationDraft) -> OutputDoc {
    let rows = vec![
        entity_row(
            "Kind:",
            location_kind_display(&draft.kind_type, &draft.kind_custom),
        ),
        entity_row("Visual:", normalize_unknown_text(&draft.visual_description)),
        entity_row(
            "History:",
            normalize_unknown_text(&draft.history_background),
        ),
        entity_row(
            "Exports:",
            normalize_unknown_list(draft.exports.clone()).join(", "),
        ),
        entity_row("Tone:", normalize_unknown_text(&draft.tone)),
        entity_row("Authority:", normalize_unknown_text(&draft.authority)),
        entity_row("Danger:", normalize_unknown_text(&draft.danger_level)),
        entity_row("Tension:", normalize_unknown_text(&draft.current_tension)),
        entity_row("Path:", normalize_unknown_text(&draft.vault_path)),
    ];
    doc()
        .with_block(entity_card(&draft.name, rows))
        .with_block(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this location, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]))
}

pub fn faction_entity_card(draft: &FactionDraft) -> OutputDoc {
    let kind_custom_display = draft
        .kind_custom
        .as_deref()
        .map(|value| {
            let normalized = normalize_unknown_text(value);
            if normalized == "Unknown" {
                "(none)".to_string()
            } else {
                normalized
            }
        })
        .unwrap_or_else(|| "(none)".to_string());

    let rows = vec![
        entity_row("Name:", normalize_unknown_text(&draft.name)),
        entity_row("Slug:", normalize_unknown_text(&draft.slug)),
        entity_row("Kind:", normalize_unknown_text(&draft.kind_type)),
        entity_row("Custom Kind:", kind_custom_display),
        entity_row(
            "Public Face:",
            normalize_unknown_text(&draft.public_description),
        ),
        entity_row("True Agenda:", normalize_unknown_text(&draft.true_agenda)),
        entity_row("Methods:", normalize_unknown_text(&draft.methods)),
        entity_row("Leadership:", normalize_unknown_text(&draft.leadership)),
        entity_row("Headquarters:", normalize_unknown_text(&draft.headquarters)),
        entity_row(
            "Sphere of Influence:",
            normalize_unknown_text(&draft.sphere_of_influence),
        ),
        entity_row(
            "Resources:",
            normalize_unknown_text(&draft.resources_assets),
        ),
        entity_row(
            "Allies:",
            normalize_unknown_list(draft.allies.clone()).join(", "),
        ),
        entity_row(
            "Rivals:",
            normalize_unknown_list(draft.rivals_enemies.clone()).join(", "),
        ),
        entity_row("Reputation:", normalize_unknown_text(&draft.reputation)),
        entity_row(
            "Current Tension:",
            normalize_unknown_text(&draft.current_tension),
        ),
        entity_row(
            "Short-Term Goals:",
            normalize_unknown_list(draft.goals_short_term.clone()).join(", "),
        ),
        entity_row(
            "Long-Term Goals:",
            normalize_unknown_list(draft.goals_long_term.clone()).join(", "),
        ),
        entity_row("Symbol:", normalize_unknown_text(&draft.symbol_description)),
        entity_row("Path:", normalize_unknown_text(&draft.vault_path)),
    ];
    doc()
        .with_block(entity_card(&draft.name, rows))
        .with_block(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this faction, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]))
}

pub fn god_entity_card(draft: &GodDraft) -> OutputDoc {
    let rank_custom_display = draft
        .rank_custom
        .as_deref()
        .map(|value| {
            let normalized = normalize_unknown_text(value);
            if normalized == "Unknown" {
                "(none)".to_string()
            } else {
                normalized
            }
        })
        .unwrap_or_else(|| "(none)".to_string());

    let rows = vec![
        entity_row("Name:", normalize_unknown_text(&draft.name)),
        entity_row("Slug:", normalize_unknown_text(&draft.slug)),
        entity_row("Epithet:", normalize_unknown_text(&draft.epithet)),
        entity_row("Rank:", normalize_unknown_text(&draft.rank)),
        entity_row("Custom Rank:", rank_custom_display),
        entity_row("Alignment:", normalize_unknown_text(&draft.alignment)),
        entity_row(
            "Domains:",
            normalize_unknown_list(draft.domains.clone()).join(", "),
        ),
        entity_row("Symbol:", normalize_unknown_text(&draft.symbol)),
        entity_row("Appearance:", normalize_unknown_text(&draft.appearance)),
        entity_row("Dogma:", normalize_unknown_text(&draft.dogma)),
        entity_row("Realm:", normalize_unknown_text(&draft.realm)),
        entity_row("Worshippers:", normalize_unknown_text(&draft.worshippers)),
        entity_row("Clergy:", normalize_unknown_text(&draft.clergy)),
        entity_row(
            "Allies:",
            normalize_unknown_list(draft.allies.clone()).join(", "),
        ),
        entity_row(
            "Rivals:",
            normalize_unknown_list(draft.rivals.clone()).join(", "),
        ),
        entity_row("Path:", normalize_unknown_text(&draft.vault_path)),
    ];
    doc()
        .with_block(entity_card(&draft.name, rows))
        .with_block(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this god, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]))
}

pub fn item_entity_card(draft: &ItemDraft) -> OutputDoc {
    let rows = vec![
        entity_row("Slug:", normalize_unknown_text(&draft.slug)),
        entity_row("Category:", normalize_unknown_text(&draft.category)),
        entity_row("Rarity:", normalize_unknown_text(&draft.rarity)),
        entity_row("Attunement:", normalize_unknown_text(&draft.attunement)),
        entity_row(
            "Materials:",
            normalize_unknown_list(draft.materials.clone()).join(", "),
        ),
        entity_row("Appearance:", normalize_unknown_text(&draft.appearance)),
        entity_row("Abilities:", normalize_unknown_text(&draft.abilities)),
        entity_row("Drawbacks:", normalize_unknown_text(&draft.drawbacks)),
        entity_row("History:", normalize_unknown_text(&draft.history)),
        entity_row("Value:", normalize_unknown_text(&draft.value)),
        entity_row("Location:", normalize_unknown_text(&draft.location)),
        entity_row("Path:", normalize_unknown_text(&draft.vault_path)),
    ];
    doc()
        .with_block(entity_card(&draft.name, rows))
        .with_block(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this item, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]))
}

pub fn event_entity_card(draft: &EventDraft) -> OutputDoc {
    let rows = vec![entity_row("Slug:", normalize_unknown_text(&draft.slug))];
    let mut output = doc().with_block(entity_card(&draft.name, rows));
    // The body is narrative prose, not an attribute. Render each paragraph
    // (split on blank lines) as its own block so the story reads naturally.
    for para in draft.body.split("\n\n") {
        let trimmed = para.trim();
        if !trimmed.is_empty() {
            output.push(paragraph_text(trimmed.to_string()));
        }
    }
    output.push(paragraph_with_inlines(vec![
        text_node("Use "),
        command_ref("save", "save"),
        text_node(" to persist this event, or "),
        command_ref("reroll", "reroll"),
        text_node(" to regenerate the narrative."),
    ]));
    output
}

pub fn dungeon_entity_card(draft: &DungeonDraft) -> OutputDoc {
    let mut out = doc();
    // 1. spine / premise top-line (feature-dungeons.md §6)
    out.push(heading(
        2,
        format!(
            "{} — {}",
            normalize_unknown_text(&draft.name),
            normalize_unknown_text(&draft.premise)
        ),
    ));
    // 2. location line — the single bounded place all five beats sit inside
    let location = normalize_unknown_text(&draft.location);
    if location != "Unknown" {
        out.push(status(StatusTone::Info, format!("Location: {location}")));
    }
    // 3. topology line
    let topo = if draft.topology.is_empty() || draft.topology == "none" {
        "Topology: none (lay it out freely)".to_string()
    } else {
        format!("Topology: {}", draft.topology)
    };
    out.push(status(StatusTone::Info, topo));
    // 3. five beat cards, each followed by its own reroll Paragraph
    for (i, beat) in draft.beats.iter().enumerate() {
        let mut rows = vec![
            entity_row("Type:", normalize_unknown_text(&beat.content_type)),
            entity_row("Idea:", normalize_unknown_text(&beat.idea)),
            entity_row("Player Goals:", normalize_unknown_text(&beat.player_goals)),
            entity_row("Lever:", normalize_unknown_text(&beat.lever)),
        ];
        if let Some(loot) = &beat.loot {
            if !loot.trim().is_empty() {
                rows.push(entity_row("Loot:", loot.clone()));
            }
        }
        rows.push(entity_row("Design:", normalize_unknown_text(&beat.design_note)));
        out.push(entity_card(
            format!("{}. {}", i + 1, beat.function),
            rows,
        ));
        let key = beat.function.to_lowercase();
        out.push(paragraph_with_inlines(vec![
            text_node("Reroll this beat: "),
            command_ref(
                format!("reroll {key}"),
                format!("dungeon reroll {key}"),
            ),
        ]));
    }
    // 4. footer actions
    out.push(paragraph_with_inlines(vec![
        text_node("Use "),
        command_ref("save", "save"),
        text_node(", "),
        command_ref("reroll", "reroll"),
        text_node(" for a whole new dungeon, or "),
        command_ref("cancel", "cancel"),
        text_node(" to discard."),
    ]));
    out
}
