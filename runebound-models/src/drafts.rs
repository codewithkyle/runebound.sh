use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::dungeon_plan::{DungeonContentPlan, PlannedOverlay};

/// Stamp a content plan's overlay + faction tint onto freshly-built beats so they
/// persist with the dungeon. The overlay marks the single beat it layers onto; the
/// dungeon-wide faction tint is mirrored on every beat. Call after `to_beats`.
pub fn apply_plan_meta_to_beats(beats: &mut [DungeonBeat], plan: &DungeonContentPlan) {
    for beat in beats.iter_mut() {
        beat.overlay = None;
        beat.factions = plan.factions;
    }
    if let Some(overlay) = &plan.overlay
        && let Some(beat) = beats.get_mut(overlay.beat_index)
    {
        beat.overlay = Some(overlay.overlay_type.clone());
    }
}

/// Recover the overlay + faction tint a dungeon's beats were stamped with, so a
/// whole-dungeon reroll can rebuild a plan that honors them (inverse of
/// [`apply_plan_meta_to_beats`]).
pub fn plan_meta_from_beats(beats: &[DungeonBeat]) -> (Option<PlannedOverlay>, bool) {
    let overlay = beats.iter().enumerate().find_map(|(index, beat)| {
        beat.overlay.as_ref().map(|overlay_type| PlannedOverlay {
            beat_index: index,
            overlay_type: overlay_type.clone(),
        })
    });
    let factions = beats.iter().any(|beat| beat.factions);
    (overlay, factions)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NpcDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    // `#[serde(default)]` only tolerates pre-slug TOML on *read*; a `String` is
    // always serialized, so a draft crossing to the frontend always carries a
    // slug. The generated TS therefore keeps `slug` required (not optional).
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    /// The location this one stands within (a guildhall's containing place). Empty
    /// when there is no anchor; published as a `[[wikilink]]`.
    #[serde(default)]
    pub location: String,
    /// Transient: true only when the WIZARD built this draft, requesting kind-based
    /// subfoldering of the `.md` vault path. Never persisted, never sent to the
    /// frontend. Consulted ONLY when persisting a brand-new row; existing rows
    /// preserve their on-disk folder regardless (so defaulting to false on load is
    /// harmless). `bool: Default` makes `#[serde(skip)]` read as `false` everywhere.
    #[serde(skip)]
    #[ts(skip)]
    pub wizard_subfoldered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct FactionDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub kind_type: String,
    // Visible face.
    pub public_description: String,
    pub reputation: String,
    pub symbol_description: String,
    // WOAC engine — Want → Obstacle → Action → Consequence (design §5). Absorbs the
    // old `true_agenda`/`current_tension`/`methods`; `consequence` is new.
    pub want: String,
    pub obstacle: String,
    pub action: String,
    pub consequence: String,
    /// Was `leadership`. An NPC link name or free text; wizard-picked or left blank,
    /// never LLM-generated (D3).
    pub leader: String,
    pub sphere_of_influence: String,
    #[serde(default)]
    pub resources_assets: Vec<String>,
    /// Picker-linked or left blank; never LLM-generated (D3/§7).
    #[serde(default)]
    pub allies: Vec<String>,
    /// Picker-linked or left blank; never LLM-generated (D3/§7).
    #[serde(default)]
    pub rivals_enemies: Vec<String>,
    /// Houses Vassal/Lord only — the faction this one is sworn to. Picker or free
    /// text, never LLM.
    #[serde(default)]
    pub liege: Option<String>,
    /// Houses Vassal/Lord only — one of `LOYALTY_TYPES`. Enum-picked or random,
    /// never LLM.
    #[serde(default)]
    pub loyalty_type: Option<String>,
    /// Transient: true only when the WIZARD built this draft, requesting
    /// category-based subfoldering of the `.md` vault path. Mirrors
    /// `LocationDraft.wizard_subfoldered` — never persisted, never sent to the
    /// frontend; consulted only when persisting a brand-new row.
    #[serde(skip)]
    #[ts(skip)]
    pub wizard_subfoldered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct EventDraft {
    pub id: String,
    #[serde(default)]
    pub seed_prompt: Option<String>,
    pub name: String,
    // See `NpcDraft::slug`: lenient on read, always present on the wire.
    #[serde(default)]
    pub slug: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    // The rolled content plan's overlay + faction tint, persisted with the beat so
    // they survive save/load (they ride `beats_json`) and a whole-dungeon reroll can
    // honor them. `overlay` is set only on the single beat it layers onto; `factions`
    // is the dungeon-wide tint, mirrored on every beat.
    #[serde(default)]
    pub overlay: Option<String>, // foreshadowing | history | map, layered on this beat
    #[serde(default)]
    pub factions: bool, // dungeon-wide faction tint
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    pub premise: String,  // the spine / top-line (feature-dungeons.md §6)
    pub topology: String, // one of DUNGEON_TOPOLOGIES, or "none"
    pub tone: String,     // "tragedy" | "comedy"
    pub twist: String,    // "false_victory" | "false_defeat" | "neither"
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
    /// The location this one stands within (a guildhall's containing place). Empty
    /// when there is no anchor; published as a `[[wikilink]]`.
    #[serde(default)]
    pub location: String,
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
    /// Derived from `kind_type` at save (D2); persisted so Obsidian/dataview and the
    /// DB can filter by category.
    pub category: String,
    // Visible face.
    pub public_description: String,
    pub reputation: String,
    pub symbol_description: String,
    // WOAC engine (design §5).
    pub want: String,
    pub obstacle: String,
    pub action: String,
    pub consequence: String,
    pub leader: String,
    pub sphere_of_influence: String,
    #[serde(deserialize_with = "crate::utils::string_or_seq_list")]
    pub resources_assets: Vec<String>,
    #[serde(deserialize_with = "crate::utils::string_or_seq_list")]
    pub allies: Vec<String>,
    #[serde(deserialize_with = "crate::utils::string_or_seq_list")]
    pub rivals_enemies: Vec<String>,
    /// Houses Vassal/Lord only — absent for everything else.
    #[serde(default)]
    pub liege: Option<String>,
    #[serde(default)]
    pub loyalty_type: Option<String>,
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

/// Whether an entity card appends its `save`/`reroll` action footer (and, for
/// dungeons, the per-beat reroll hints). The active-draft editor flow shows it —
/// those commands act on the open draft; the read-only `show`/`preview` flow
/// hides it, since there is no draft for them to act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardFooter {
    Show,
    Hide,
}

fn title_case_sex(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "male" => "Male".to_string(),
        "female" => "Female".to_string(),
        _ => value.to_string(),
    }
}

/// The category folder each of the 9 faction kinds rolls up into (design §3). The
/// card derives it for display; persistence single-sources the same map via
/// `faction_category_str` in the desktop crate (spec D2). `""` for any out-of-vocab
/// kind, which the card renders as "Unknown".
fn faction_category_display(kind_type: &str) -> &'static str {
    match kind_type {
        "great_house" | "major_vassal" | "minor_vassal" | "individual_lord" => "houses",
        "guild" | "company" | "criminal_syndicate" => "establishments",
        "temple" | "cult" => "religion",
        _ => "",
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

pub fn npc_entity_card(draft: &NpcDraft, footer: CardFooter) -> OutputDoc {
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
    let mut output = doc().with_block(entity_card(&draft.name, rows));
    if footer == CardFooter::Show {
        output.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this NPC, or "),
            command_ref("reroll", "reroll"),
            text_node(" to generate again."),
        ]));
    }
    output
}

pub fn location_entity_card(draft: &LocationDraft, footer: CardFooter) -> OutputDoc {
    let mut rows = vec![
        entity_row(
            "Kind:",
            location_kind_display(&draft.kind_type, &draft.kind_custom),
        ),
        entity_row("Visual:", normalize_unknown_text(&draft.visual_description)),
        entity_row(
            "History:",
            normalize_unknown_text(&draft.history_background),
        ),
    ];
    // Exports is kind-conditional: Site/Hideout suppress it (empty `Vec`), so the
    // row is omitted entirely rather than rendered as "Unknown". Settlements (and
    // the one-shot path) always carry 1-3 items, so the row shows there.
    if !draft.exports.is_empty() {
        rows.push(entity_row(
            "Exports:",
            normalize_unknown_list(draft.exports.clone()).join(", "),
        ));
    }
    rows.push(entity_row("Tone:", normalize_unknown_text(&draft.tone)));
    // Authority is also kind-conditional: the one-shot lane suppresses it (empty
    // `String`), so the row is omitted rather than rendered as "Unknown". The wizard
    // branches always set it (control / owner / occupant), so the row shows there.
    if !draft.authority.trim().is_empty() {
        rows.push(entity_row(
            "Authority:",
            normalize_unknown_text(&draft.authority),
        ));
    }
    // The containing location (a guildhall's anchor) is optional, so the row is
    // omitted when empty rather than rendered as "Unknown".
    if !draft.location.trim().is_empty() {
        rows.push(entity_row(
            "Location:",
            normalize_unknown_text(&draft.location),
        ));
    }
    rows.extend([
        entity_row("Danger:", normalize_unknown_text(&draft.danger_level)),
        entity_row("Tension:", normalize_unknown_text(&draft.current_tension)),
        entity_row("Path:", normalize_unknown_text(&draft.vault_path)),
    ]);
    let mut output = doc().with_block(entity_card(&draft.name, rows));
    if footer == CardFooter::Show {
        output.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this location, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]));
    }
    output
}

pub fn faction_entity_card(draft: &FactionDraft, footer: CardFooter) -> OutputDoc {
    let category = faction_category_display(&draft.kind_type);
    let category_display = if category.is_empty() {
        "Unknown".to_string()
    } else {
        category.to_string()
    };

    let mut rows = vec![
        entity_row("Name:", normalize_unknown_text(&draft.name)),
        entity_row("Slug:", normalize_unknown_text(&draft.slug)),
        entity_row("Kind:", normalize_unknown_text(&draft.kind_type)),
        entity_row("Category:", category_display),
        entity_row(
            "Public Face:",
            normalize_unknown_text(&draft.public_description),
        ),
        entity_row("Reputation:", normalize_unknown_text(&draft.reputation)),
        entity_row("Symbol:", normalize_unknown_text(&draft.symbol_description)),
        entity_row("Want:", normalize_unknown_text(&draft.want)),
        entity_row("Obstacle:", normalize_unknown_text(&draft.obstacle)),
        entity_row("Action:", normalize_unknown_text(&draft.action)),
        entity_row("Consequence:", normalize_unknown_text(&draft.consequence)),
        entity_row("Leader:", normalize_unknown_text(&draft.leader)),
        entity_row(
            "Sphere of Influence:",
            normalize_unknown_text(&draft.sphere_of_influence),
        ),
        entity_row(
            "Resources:",
            normalize_unknown_list(draft.resources_assets.clone()).join(", "),
        ),
        entity_row(
            "Allies:",
            normalize_unknown_list(draft.allies.clone()).join(", "),
        ),
        entity_row(
            "Rivals:",
            normalize_unknown_list(draft.rivals_enemies.clone()).join(", "),
        ),
    ];
    // Liege + Loyalty are houses Vassal/Lord only — render only when set.
    if let Some(liege) = draft
        .liege
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        rows.push(entity_row("Liege:", liege.to_string()));
    }
    if let Some(loyalty) = draft
        .loyalty_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        rows.push(entity_row("Loyalty:", loyalty.to_string()));
    }
    rows.push(entity_row("Path:", normalize_unknown_text(&draft.vault_path)));

    let mut output = doc().with_block(entity_card(&draft.name, rows));
    if footer == CardFooter::Show {
        output.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this faction, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]));
    }
    output
}

pub fn god_entity_card(draft: &GodDraft, footer: CardFooter) -> OutputDoc {
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
    let mut output = doc().with_block(entity_card(&draft.name, rows));
    if footer == CardFooter::Show {
        output.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this god, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]));
    }
    output
}

pub fn item_entity_card(draft: &ItemDraft, footer: CardFooter) -> OutputDoc {
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
    let mut output = doc().with_block(entity_card(&draft.name, rows));
    if footer == CardFooter::Show {
        output.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this item, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate it."),
        ]));
    }
    output
}

pub fn event_entity_card(draft: &EventDraft, footer: CardFooter) -> OutputDoc {
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
    if footer == CardFooter::Show {
        output.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(" to persist this event, or "),
            command_ref("reroll", "reroll"),
            text_node(" to regenerate the narrative."),
        ]));
    }
    output
}

pub fn dungeon_entity_card(draft: &DungeonDraft, footer: CardFooter) -> OutputDoc {
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
    // 3b. the rolled overlay, recovered from the beats.
    let (overlay, _) = plan_meta_from_beats(&draft.beats);
    if let Some(overlay) = overlay {
        out.push(status(
            StatusTone::Info,
            format!(
                "Overlay: {} (on the {})",
                overlay.overlay_type, draft.beats[overlay.beat_index].function
            ),
        ));
    }
    // 4. five beat cards, each followed by its own reroll Paragraph
    for (i, beat) in draft.beats.iter().enumerate() {
        let mut rows = vec![
            entity_row("Type:", normalize_unknown_text(&beat.content_type)),
            entity_row("Idea:", normalize_unknown_text(&beat.idea)),
            entity_row("Player Goals:", normalize_unknown_text(&beat.player_goals)),
            entity_row("Lever:", normalize_unknown_text(&beat.lever)),
        ];
        if let Some(loot) = &beat.loot
            && !loot.trim().is_empty()
        {
            rows.push(entity_row("Loot:", loot.clone()));
        }
        rows.push(entity_row(
            "Design:",
            normalize_unknown_text(&beat.design_note),
        ));
        out.push(entity_card(format!("{}. {}", i + 1, beat.function), rows));
        if footer == CardFooter::Show {
            let key = beat.function.to_lowercase();
            out.push(paragraph_with_inlines(vec![
                text_node("Reroll this beat: "),
                command_ref(format!("reroll {key}"), format!("dungeon reroll {key}")),
            ]));
        }
    }
    // 4. footer actions
    if footer == CardFooter::Show {
        out.push(paragraph_with_inlines(vec![
            text_node("Use "),
            command_ref("save", "save"),
            text_node(", "),
            command_ref("reroll", "reroll"),
            text_node(" for a whole new dungeon, or "),
            command_ref("cancel", "cancel"),
            text_node(" to discard."),
        ]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBlock;

    fn location_draft(exports: Vec<String>) -> LocationDraft {
        LocationDraft {
            id: "loc_1".to_string(),
            seed_prompt: None,
            name: "Greenhollow".to_string(),
            slug: "greenhollow".to_string(),
            vault_path: String::new(),
            kind_type: "ruin".to_string(),
            kind_custom: None,
            visual_description: "A misty fen.".to_string(),
            history_background: "Old. Older still.".to_string(),
            exports,
            tone: "quiet and damp".to_string(),
            authority: "Unknown".to_string(),
            danger_level: "deadly".to_string(),
            current_tension: "Something stirs.".to_string(),
            location: String::new(),
            wizard_subfoldered: false,
        }
    }

    fn card_labels(draft: &LocationDraft) -> Vec<String> {
        location_entity_card(draft, CardFooter::Hide)
            .blocks
            .iter()
            .flat_map(|block| match block {
                OutputBlock::EntityCard { rows, .. } => {
                    rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn location_card_omits_exports_row_when_empty() {
        // Site/Hideout suppress exports (empty Vec): the row is dropped, not shown
        // as "Unknown".
        let labels = card_labels(&location_draft(Vec::new()));
        assert!(
            !labels.iter().any(|label| label == "Exports:"),
            "empty exports should omit the row, got {labels:?}"
        );
        // The neighboring rows still render.
        assert!(labels.iter().any(|label| label == "History:"));
        assert!(labels.iter().any(|label| label == "Tone:"));
    }

    #[test]
    fn location_card_keeps_exports_row_when_present() {
        let labels = card_labels(&location_draft(vec!["reed".to_string()]));
        assert!(
            labels.iter().any(|label| label == "Exports:"),
            "non-empty exports should render the row, got {labels:?}"
        );
    }

    #[test]
    fn location_card_omits_authority_row_when_empty() {
        // The one-shot lane suppresses authority (empty String): the row is dropped,
        // not shown as "Unknown".
        let mut draft = location_draft(vec!["reed".to_string()]);
        draft.authority = String::new();
        let labels = card_labels(&draft);
        assert!(
            !labels.iter().any(|label| label == "Authority:"),
            "empty authority should omit the row, got {labels:?}"
        );
        // Neighboring rows still render.
        assert!(labels.iter().any(|label| label == "Tone:"));
        assert!(labels.iter().any(|label| label == "Danger:"));
    }

    #[test]
    fn location_card_keeps_authority_row_when_present() {
        // location_draft seeds a non-empty authority.
        let labels = card_labels(&location_draft(Vec::new()));
        assert!(
            labels.iter().any(|label| label == "Authority:"),
            "non-empty authority should render the row, got {labels:?}"
        );
    }
}
