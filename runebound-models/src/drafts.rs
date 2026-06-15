use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
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
    pub value_gp: String,
    pub current_owner: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcFrontmatter {
    #[serde(rename = "type")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemFrontmatter {
    #[serde(rename = "type")]
    pub doc_type: String,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub category: String,
    pub rarity: String,
    pub attunement: String,
    pub materials: Vec<String>,
    pub appearance: String,
    pub abilities: String,
    pub drawbacks: String,
    pub history: String,
    pub value_gp: String,
    pub current_owner: String,
    pub location: String,
    pub created_at: String,
    pub updated_at: String,
}

use super::output::{
    OutputDoc, command_ref, doc, entity_card, entity_row, paragraph_with_inlines, text_node,
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
        entity_row("Value:", normalize_unknown_text(&draft.value_gp)),
        entity_row("Owner:", normalize_unknown_text(&draft.current_owner)),
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
