//! Convert a local 5etools bestiary dataset into our own [`Monster`] model.
//!
//! This mirrors [`crate::spell_import`]: [`import_monsters_from_dir`] reads a
//! directory the user points at (their own copy of the 5etools data — nothing
//! copyrighted ships in this repo), parses every official monster file named in
//! `index.json`, dedups to the 2024-canonical set, and lowers each entry into a
//! render-ready [`Monster`] with every defensive stat pre-formatted to a display
//! string.
//!
//! Two things this has that the spell importer does not:
//! - **`_copy` monsters are skipped** (v1). Resolving them needs 5etools'
//!   `_applyCopy`/`_mod` engine; the skipped set is overwhelmingly adventure NPC
//!   variants and the count is reported so coverage is never silently capped.
//! - **Legendary groups** (`legendarygroups.json`) are resolved by `(name, source)`
//!   to append a monster's Lair Actions / Regional Effects sections.
//!
//! Inline `{@tag}` markup is lowered by [`crate::spell_import::strip_tags`], which
//! is the single shared seam for that and already understands the monster
//! attack/save/recharge tags.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use runebound_models::monsters::{Monster, StatAbility, StatBlock, StatSection};
use serde::Deserialize;
use serde_json::Value;

use crate::spell_import::{slugify, strip_tags};

/// The outcome of an import: the canonical monsters plus the count of `_copy`
/// variants that were skipped (surfaced to the user so coverage is explicit).
#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub monsters: Vec<Monster>,
    pub skipped_copy: usize,
}

// ---------------------------------------------------------------------------
// Raw 5etools schema — only the fields we consume. `rename_all = "camelCase"`
// maps `reprintedAs` / `legendaryGroup` / `headerEntries` automatically; the
// reserved-word and underscore-prefixed keys are renamed explicitly.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(default)]
    monster: Vec<RawMonster>,
}

/// Deserialize a field that may be explicitly `null` in the source as its default.
/// 5etools writes `"spellcasting": null`, `"senses": null`, … and a plain `Vec`
/// rejects an explicit null even with `#[serde(default)]`; this reads it as empty.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMonster {
    name: String,
    source: String,
    /// Presence marks a `_copy`-derived variant (a base monster + `_mod` edits).
    /// We only check whether it exists; resolving it is Phase 2.
    #[serde(rename = "_copy", default)]
    copy: Option<Value>,
    /// Presence marks a superseded entry (its canonical reprint is elsewhere).
    #[serde(default)]
    reprinted_as: Option<Value>,
    #[serde(default, deserialize_with = "null_default")]
    size: Vec<String>,
    #[serde(rename = "type", default)]
    creature_type: Value,
    #[serde(default)]
    alignment: Value,
    #[serde(default, deserialize_with = "null_default")]
    ac: Vec<Value>,
    #[serde(default)]
    hp: Value,
    #[serde(default)]
    speed: Value,
    // Usually integers, but PB-scaling sidekicks store e.g. "10 + (PB × 2)".
    #[serde(rename = "str", default)]
    strength: Value,
    #[serde(rename = "dex", default)]
    dexterity: Value,
    #[serde(rename = "con", default)]
    constitution: Value,
    #[serde(rename = "int", default)]
    intelligence: Value,
    #[serde(rename = "wis", default)]
    wisdom: Value,
    #[serde(rename = "cha", default)]
    charisma: Value,
    #[serde(default)]
    save: Value,
    #[serde(default)]
    skill: Value,
    #[serde(default, deserialize_with = "null_default")]
    resist: Vec<Value>,
    #[serde(default, deserialize_with = "null_default")]
    immune: Vec<Value>,
    #[serde(default, deserialize_with = "null_default")]
    vulnerable: Vec<Value>,
    #[serde(default, deserialize_with = "null_default")]
    condition_immune: Vec<Value>,
    #[serde(default, deserialize_with = "null_default")]
    senses: Vec<Value>,
    // Usually an integer; PB-scaling sidekicks store e.g. "10 + (PB × 2)".
    #[serde(default)]
    passive: Value,
    #[serde(default, deserialize_with = "null_default")]
    languages: Vec<Value>,
    #[serde(default)]
    cr: Value,
    #[serde(default, deserialize_with = "null_default")]
    gear: Vec<Value>,
    #[serde(rename = "trait", default, deserialize_with = "null_default")]
    traits: Vec<RawNamedEntry>,
    #[serde(default, deserialize_with = "null_default")]
    action: Vec<RawNamedEntry>,
    #[serde(default, deserialize_with = "null_default")]
    bonus: Vec<RawNamedEntry>,
    #[serde(default, deserialize_with = "null_default")]
    reaction: Vec<RawNamedEntry>,
    #[serde(default, deserialize_with = "null_default")]
    legendary: Vec<RawNamedEntry>,
    #[serde(default, deserialize_with = "null_default")]
    legendary_header: Vec<String>,
    #[serde(default, deserialize_with = "null_default")]
    mythic: Vec<RawNamedEntry>,
    #[serde(default, deserialize_with = "null_default")]
    mythic_header: Vec<String>,
    #[serde(default)]
    legendary_group: Option<RawGroupRef>,
    #[serde(default, deserialize_with = "null_default")]
    spellcasting: Vec<RawSpellcasting>,
}

#[derive(Debug, Deserialize)]
struct RawNamedEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct RawGroupRef {
    name: String,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSpellcasting {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    header_entries: Vec<Entry>,
    /// Spells castable at will (2024 `will` / 2014 cantrip-style at-will lists).
    #[serde(default, deserialize_with = "null_default")]
    will: Vec<Value>,
    /// 2024 `daily`: bucket key → spells, e.g. `{"1e": [...], "2e": [...]}` where
    /// the digit is the per-day count and a trailing `e` means "each".
    #[serde(default, deserialize_with = "null_default")]
    daily: BTreeMap<String, Vec<Value>>,
    /// 2014 spell-slot table: level key ("0" = cantrips) → slots + spells.
    #[serde(default, deserialize_with = "null_default")]
    spells: BTreeMap<String, RawSpellLevel>,
    /// "action" / "bonus" / "reaction" / "legendary" — which section this casts in.
    #[serde(default)]
    display_as: Option<String>,
    /// Buckets to omit from the rendered list (the header already names them).
    #[serde(default, deserialize_with = "null_default")]
    hidden: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawSpellLevel {
    #[serde(default)]
    slots: Option<u32>,
    #[serde(default, deserialize_with = "null_default")]
    spells: Vec<Value>,
}

/// Legendary group: a monster's shared lair actions / regional effects, keyed by
/// `(name, source)` from `legendarygroups.json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLegendaryGroup {
    name: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    lair_actions: Option<Vec<Entry>>,
    #[serde(default)]
    regional_effects: Option<Vec<Entry>>,
    #[serde(default)]
    mythic_encounter: Option<Vec<Entry>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGroupFile {
    #[serde(default)]
    legendary_group: Vec<RawLegendaryGroup>,
}

/// A `5etools` `entries` element: a string or a typed block (shared shape with the
/// spell importer, kept local so each importer owns its own raw schema).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Entry {
    Text(String),
    Block(EntryBlock),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryBlock {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    entries: Vec<Entry>,
    #[serde(default, deserialize_with = "null_default")]
    items: Vec<ListItem>,
    #[serde(default, deserialize_with = "null_default")]
    col_labels: Vec<String>,
    #[serde(default, deserialize_with = "null_default")]
    rows: Vec<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ListItem {
    Text(String),
    Item(EntryBlock),
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Read `dir` (a 5etools repo root *or* its `data/bestiary` dir), parse every
/// official monster file, dedup to the 2024-canonical set (skipping `_copy`
/// variants), and convert each to a [`Monster`]. The result is sorted by name.
pub fn import_monsters_from_dir(dir: &Path) -> Result<ImportSummary> {
    let bestiary_dir = locate_bestiary_dir(dir)?;
    let index_path = bestiary_dir.join("index.json");
    let index_raw = std::fs::read_to_string(&index_path)
        .with_context(|| format!("failed to read {}", index_path.display()))?;
    // index.json maps source code -> filename. Order is irrelevant; dedup is by name.
    let index: BTreeMap<String, String> = serde_json::from_str(&index_raw)
        .with_context(|| format!("failed to parse {}", index_path.display()))?;

    let mut raw_monsters: Vec<RawMonster> = Vec::new();
    for filename in index.values() {
        let path = bestiary_dir.join(filename);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read bestiary file {}", path.display()))?;
        let file: RawFile = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse bestiary file {}", path.display()))?;
        raw_monsters.extend(file.monster);
    }

    let legendary_groups = load_legendary_groups(&bestiary_dir)?;
    let (canonical, skipped_copy) = dedup_to_canonical(raw_monsters);
    let mut monsters: Vec<Monster> = canonical
        .iter()
        .map(|raw| convert_monster(raw, &legendary_groups))
        .collect();
    disambiguate_slugs(&mut monsters);
    monsters.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    Ok(ImportSummary {
        monsters,
        skipped_copy,
    })
}

/// Accept the repo root, its `data/bestiary` dir, or a `bestiary` dir directly:
/// the data lives wherever `index.json` sits next to `bestiary-*.json`.
fn locate_bestiary_dir(dir: &Path) -> Result<PathBuf> {
    let candidates = [
        dir.join("data").join("bestiary"),
        dir.join("bestiary"),
        dir.to_path_buf(),
    ];
    for candidate in candidates {
        if candidate.join("index.json").is_file() {
            return Ok(candidate);
        }
    }
    bail!(
        "no 5etools bestiary data found under {} (looked for data/bestiary/index.json, \
         bestiary/index.json, and index.json)",
        dir.display()
    );
}

/// Load `legendarygroups.json` from the bestiary dir, if present. A missing file is
/// fine (returns an empty list — the monster's lair/regional sections are simply
/// omitted).
fn load_legendary_groups(bestiary_dir: &Path) -> Result<Vec<RawLegendaryGroup>> {
    let path = bestiary_dir.join("legendarygroups.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let file: RawGroupFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(file.legendary_group)
}

/// Reduce the full corpus to the 2024-canonical set: skip `_copy` variants
/// (counting them), drop reprinted entries, then keep one entry per name,
/// preferring the 2024 Monster Manual (`XMM`).
fn dedup_to_canonical(raw: Vec<RawMonster>) -> (Vec<RawMonster>, usize) {
    let mut skipped_copy = 0usize;
    let mut by_name: BTreeMap<String, RawMonster> = BTreeMap::new();
    for monster in raw {
        if monster.copy.is_some() {
            skipped_copy += 1; // a `_copy`-derived variant; resolving it is Phase 2
            continue;
        }
        if monster.reprinted_as.is_some() {
            continue; // superseded; the canonical reprint appears under its own source
        }
        match by_name.get(&monster.name) {
            // Prefer the first XMM entry seen; never let a non-XMM entry replace it.
            Some(existing) if existing.source == "XMM" => {}
            Some(_) if monster.source == "XMM" => {
                by_name.insert(monster.name.clone(), monster);
            }
            Some(_) => {}
            None => {
                by_name.insert(monster.name.clone(), monster);
            }
        }
    }
    (by_name.into_values().collect(), skipped_copy)
}

/// On a slug collision (named creatures recur across adventures far more than
/// spells do), suffix the later ones with `-{source}`.
fn disambiguate_slugs(monsters: &mut [Monster]) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for monster in monsters.iter_mut() {
        let count = seen.entry(monster.slug.clone()).or_insert(0);
        if *count > 0 {
            monster.slug = format!("{}-{}", monster.slug, slugify(&monster.source));
        }
        *count += 1;
    }
}

// ---------------------------------------------------------------------------
// Per-monster conversion
// ---------------------------------------------------------------------------

fn convert_monster(raw: &RawMonster, legendary_groups: &[RawLegendaryGroup]) -> Monster {
    let abilities: [i16; 6] = [
        ability_score(&raw.strength),
        ability_score(&raw.dexterity),
        ability_score(&raw.constitution),
        ability_score(&raw.intelligence),
        ability_score(&raw.wisdom),
        ability_score(&raw.charisma),
    ];

    Monster {
        slug: slugify(&raw.name),
        name: raw.name.clone(),
        source: raw.source.clone(),
        size: format_size(&raw.size),
        creature_type: format_type(&raw.creature_type),
        alignment: format_alignment(&raw.alignment),
        ac: format_ac(&raw.ac),
        hp: format_hp(&raw.hp),
        speed: format_speed(&raw.speed),
        abilities,
        saves: format_saves(&raw.save),
        skills: format_skills(&raw.skill),
        damage_resistances: format_damage(&raw.resist, "resist"),
        damage_immunities: format_damage(&raw.immune, "immune"),
        damage_vulnerabilities: format_damage(&raw.vulnerable, "vulnerable"),
        condition_immunities: format_conditions(&raw.condition_immune),
        senses: format_senses(&raw.senses, &raw.passive),
        languages: format_languages(&raw.languages),
        cr: format_cr(&raw.cr),
        gear: format_gear(&raw.gear),
        sections: build_sections(raw, legendary_groups),
    }
}

/// An ability score, defaulting to 10. Usually a number; a PB-scaling string
/// ("10 + (PB × 2)") contributes its leading integer.
fn ability_score(value: &Value) -> i16 {
    match value {
        Value::Number(number) => number.as_i64().unwrap_or(10) as i16,
        Value::String(text) => text
            .split_whitespace()
            .next()
            .and_then(|token| token.parse().ok())
            .unwrap_or(10),
        _ => 10,
    }
}

/// Assemble the trait/action/.../legendary-group sections in render order,
/// inserting each spellcasting block at the front of its target section.
fn build_sections(raw: &RawMonster, legendary_groups: &[RawLegendaryGroup]) -> Vec<StatSection> {
    let mut sections: Vec<StatSection> = Vec::new();

    // Group spellcasting blocks by the section they display in (front-loaded there).
    let mut spellcasting: HashMap<&'static str, Vec<StatAbility>> = HashMap::new();
    for block in &raw.spellcasting {
        let target = spellcasting_section(block.display_as.as_deref());
        spellcasting
            .entry(target)
            .or_default()
            .push(lower_spellcasting(block));
    }

    add_section(
        &mut sections,
        &mut spellcasting,
        "Traits",
        Vec::new(),
        &raw.traits,
    );
    add_section(
        &mut sections,
        &mut spellcasting,
        "Actions",
        Vec::new(),
        &raw.action,
    );
    add_section(
        &mut sections,
        &mut spellcasting,
        "Bonus Actions",
        Vec::new(),
        &raw.bonus,
    );
    add_section(
        &mut sections,
        &mut spellcasting,
        "Reactions",
        Vec::new(),
        &raw.reaction,
    );
    add_section(
        &mut sections,
        &mut spellcasting,
        "Legendary Actions",
        lower_string_list(&raw.legendary_header),
        &raw.legendary,
    );
    add_section(
        &mut sections,
        &mut spellcasting,
        "Mythic Actions",
        lower_string_list(&raw.mythic_header),
        &raw.mythic,
    );

    // Legendary group → Lair Actions / Regional Effects / Mythic Encounter.
    if let Some(group_ref) = &raw.legendary_group
        && let Some(group) = resolve_group(legendary_groups, group_ref)
    {
        push_group_section(&mut sections, "Lair Actions", group.lair_actions.as_deref());
        push_group_section(
            &mut sections,
            "Regional Effects",
            group.regional_effects.as_deref(),
        );
        push_group_section(
            &mut sections,
            "Mythic Encounter",
            group.mythic_encounter.as_deref(),
        );
    }

    sections
}

/// Push one section, prepending any spellcasting blocks that display in it. A
/// section with neither intro prose nor abilities is dropped.
fn add_section(
    sections: &mut Vec<StatSection>,
    spellcasting: &mut HashMap<&'static str, Vec<StatAbility>>,
    title: &'static str,
    intro: Vec<StatBlock>,
    entries: &[RawNamedEntry],
) {
    let mut abilities = spellcasting.remove(title).unwrap_or_default();
    abilities.extend(named_entries(entries));
    if !intro.is_empty() || !abilities.is_empty() {
        sections.push(StatSection {
            title: title.to_string(),
            intro,
            abilities,
        });
    }
}

/// Append a legendary-group section (lair/regional/mythic) when present and
/// non-empty. These are prose + lists, so they live entirely in the section intro.
fn push_group_section(sections: &mut Vec<StatSection>, title: &str, entries: Option<&[Entry]>) {
    let Some(entries) = entries else { return };
    let intro = lower_entries(entries);
    if !intro.is_empty() {
        sections.push(StatSection {
            title: title.to_string(),
            intro,
            abilities: Vec::new(),
        });
    }
}

fn named_entries(entries: &[RawNamedEntry]) -> Vec<StatAbility> {
    entries
        .iter()
        .map(|entry| StatAbility {
            name: entry
                .name
                .as_deref()
                .filter(|name| !name.is_empty())
                .map(strip_tags),
            body: lower_entries(&entry.entries),
        })
        .collect()
}

fn spellcasting_section(display_as: Option<&str>) -> &'static str {
    match display_as {
        Some("action") => "Actions",
        Some("bonus") => "Bonus Actions",
        Some("reaction") => "Reactions",
        Some("legendary") => "Legendary Actions",
        Some("mythic") => "Mythic Actions",
        _ => "Traits",
    }
}

/// Lower one spellcasting block to a [`StatAbility`]: its header prose, then a
/// bullet-ish line per spell bucket ("At Will: …", "1/Day Each: …", "Cantrips
/// (at will): …", "1st Level (4 slots): …").
fn lower_spellcasting(block: &RawSpellcasting) -> StatAbility {
    let mut body: Vec<StatBlock> = lower_entries(&block.header_entries);

    let hidden = |bucket: &str| block.hidden.iter().any(|h| h == bucket);

    if !block.will.is_empty() && !hidden("will") {
        body.push(StatBlock::Text {
            text: format!("At Will: {}", join_spells(&block.will)),
        });
    }

    if !hidden("daily") {
        let mut keys: Vec<&String> = block.daily.keys().collect();
        keys.sort_by_key(|key| std::cmp::Reverse(daily_count(key))); // highest per-day first
        for key in keys {
            body.push(StatBlock::Text {
                text: format!("{}: {}", daily_label(key), join_spells(&block.daily[key])),
            });
        }
    }

    if !hidden("spells") {
        let mut keys: Vec<&String> = block.spells.keys().collect();
        keys.sort_by_key(|key| key.parse::<u32>().unwrap_or(0));
        for key in keys {
            let level = &block.spells[key];
            body.push(StatBlock::Text {
                text: format!(
                    "{}: {}",
                    slot_label(key, level.slots),
                    join_spells(&level.spells)
                ),
            });
        }
    }

    StatAbility {
        name: Some(
            block
                .name
                .clone()
                .unwrap_or_else(|| "Spellcasting".to_string()),
        ),
        body,
    }
}

/// Render a list of spell values (`{@spell X}` strings) to "a, b, c".
fn join_spells(values: &[Value]) -> String {
    values
        .iter()
        .filter_map(|value| value.as_str())
        .map(strip_tags)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The numeric per-day count from a `daily` key: "2e"/"2" → 2.
fn daily_count(key: &str) -> u32 {
    key.trim_end_matches('e').parse().unwrap_or(0)
}

/// "1e" → "1/Day Each", "2" → "2/Day".
fn daily_label(key: &str) -> String {
    let count = daily_count(key);
    if key.ends_with('e') {
        format!("{count}/Day Each")
    } else {
        format!("{count}/Day")
    }
}

/// "0" → "Cantrips (at will)", "3" + 3 slots → "3rd Level (3 slots)".
fn slot_label(key: &str, slots: Option<u32>) -> String {
    if key == "0" {
        return "Cantrips (at will)".to_string();
    }
    let level = key.parse::<u32>().unwrap_or(0);
    let ordinal = ordinal(level);
    match slots {
        Some(1) => format!("{ordinal} Level (1 slot)"),
        Some(n) => format!("{ordinal} Level ({n} slots)"),
        None => format!("{ordinal} Level"),
    }
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn resolve_group<'a>(
    groups: &'a [RawLegendaryGroup],
    group_ref: &RawGroupRef,
) -> Option<&'a RawLegendaryGroup> {
    let name = group_ref.name.to_ascii_lowercase();
    let source = group_ref.source.as_deref().map(str::to_ascii_lowercase);
    // Exact (name, source) match, then fall back to name-only (reprint source drift).
    groups
        .iter()
        .find(|group| {
            group.name.to_ascii_lowercase() == name
                && group.source.as_deref().map(str::to_ascii_lowercase) == source
        })
        .or_else(|| {
            groups
                .iter()
                .find(|group| group.name.to_ascii_lowercase() == name)
        })
}

// ---------------------------------------------------------------------------
// Stat-field formatters (each produces a display string; see the plan §2b)
// ---------------------------------------------------------------------------

fn format_size(sizes: &[String]) -> String {
    sizes
        .iter()
        .map(|letter| size_name(letter))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn size_name(letter: &str) -> String {
    match letter.to_ascii_uppercase().as_str() {
        "T" => "Tiny",
        "S" => "Small",
        "M" => "Medium",
        "L" => "Large",
        "H" => "Huge",
        "G" => "Gargantuan",
        other => return title_case_words(other),
    }
    .to_string()
}

/// `{type:"fey", tags:["goblinoid"]}` → "Fey (Goblinoid)"; a bare `"humanoid"` →
/// "Humanoid".
fn format_type(value: &Value) -> String {
    match value {
        Value::String(text) => title_case_words(text),
        Value::Object(map) => {
            let base = match map.get("type") {
                Some(Value::String(text)) => title_case_words(text),
                // `type` can itself be a `{choose:[...]}` — best effort, never panic.
                Some(other) => other
                    .get("choose")
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(title_case_words)
                    .unwrap_or_default(),
                None => String::new(),
            };
            let tags: Vec<String> = map
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| arr.iter().filter_map(tag_text).collect())
                .unwrap_or_default();
            if tags.is_empty() {
                base
            } else {
                format!("{base} ({})", tags.join(", "))
            }
        }
        _ => String::new(),
    }
}

/// A type tag is usually a bare string; occasionally `{tag, prefix}`.
fn tag_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(title_case_words(text)),
        Value::Object(map) => map
            .get("tag")
            .and_then(|t| t.as_str())
            .map(title_case_words),
        _ => None,
    }
}

fn format_alignment(value: &Value) -> String {
    match value {
        Value::Array(arr) if arr.iter().all(Value::is_string) => {
            let letters: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            alignment_from_letters(&letters)
        }
        // Array of `{alignment:[...], chance?}` objects: each is its own alignment.
        Value::Array(arr) => arr
            .iter()
            .filter_map(|entry| {
                entry.get("alignment").and_then(|a| a.as_array()).map(|a| {
                    let letters: Vec<String> = a
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    alignment_from_letters(&letters)
                })
            })
            .collect::<Vec<_>>()
            .join(" or "),
        Value::String(text) => alignment_from_letters(std::slice::from_ref(text)),
        _ => String::new(),
    }
}

fn alignment_from_letters(letters: &[String]) -> String {
    let upper: Vec<String> = letters.iter().map(|l| l.to_ascii_uppercase()).collect();
    // Special single-letter codes + all-neutral.
    match upper.as_slice() {
        [one] if one == "U" => return "Unaligned".to_string(),
        [one] if one == "A" => return "Any Alignment".to_string(),
        _ if upper.iter().all(|l| l == "N") => return "Neutral".to_string(),
        _ => {}
    }
    let words: Vec<&str> = upper.iter().filter_map(|l| alignment_word(l)).collect();
    if words.is_empty() {
        letters.join(" ")
    } else {
        words.join(" ")
    }
}

fn alignment_word(letter: &str) -> Option<&'static str> {
    match letter {
        "L" => Some("Lawful"),
        "C" => Some("Chaotic"),
        "G" => Some("Good"),
        "E" => Some("Evil"),
        "N" => Some("Neutral"),
        "U" => Some("Unaligned"),
        "A" => Some("Any Alignment"),
        _ => None,
    }
}

fn format_ac(ac: &[Value]) -> String {
    ac.iter()
        .filter_map(format_ac_entry)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_ac_entry(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        Value::Object(map) => {
            // Summon-style AC is a free-form expression: `{special: "11 + …"}`.
            let Some(base) = map.get("ac").and_then(Value::as_i64).map(|n| n.to_string()) else {
                return map.get("special").and_then(Value::as_str).map(strip_tags);
            };
            if let Some(from) = map.get("from").and_then(|f| f.as_array())
                && !from.is_empty()
            {
                let sources: Vec<String> = from
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(strip_tags)
                    .collect();
                return Some(format!("{base} ({})", sources.join(", ")));
            }
            if let Some(condition) = map.get("condition").and_then(|c| c.as_str()) {
                return Some(format!("{base} {}", strip_tags(condition)));
            }
            Some(base)
        }
        _ => None,
    }
}

fn format_hp(value: &Value) -> String {
    if let Some(special) = value.get("special") {
        return match special {
            Value::String(text) => strip_tags(text),
            Value::Number(number) => number.to_string(),
            _ => String::new(),
        };
    }
    let average = value.get("average").and_then(Value::as_i64);
    let formula = value
        .get("formula")
        .and_then(Value::as_str)
        .filter(|f| !f.is_empty());
    match (average, formula) {
        (Some(avg), Some(formula)) => format!("{avg} ({formula})"),
        (Some(avg), None) => avg.to_string(),
        _ => String::new(),
    }
}

fn format_speed(value: &Value) -> String {
    let Some(map) = value.as_object() else {
        return String::new();
    };
    let hover = map
        .get("hover")
        .or_else(|| map.get("canHover"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut parts = Vec::new();
    for mode in ["walk", "burrow", "climb", "fly", "swim"] {
        let Some(raw) = map.get(mode) else { continue };
        let (number, condition) = speed_value(raw);
        let Some(number) = number else { continue };
        let label = if mode == "walk" {
            String::new()
        } else {
            format!("{} ", title_case(mode))
        };
        let mut part = format!("{label}{number} ft.");
        let condition =
            condition.or_else(|| (mode == "fly" && hover).then(|| "(hover)".to_string()));
        if let Some(condition) = condition {
            part.push_str(&format!(" {condition}"));
        }
        parts.push(part);
    }
    parts.join(", ")
}

/// A speed value is a bare number, or `{number, condition}` (the condition usually
/// already carries its own parens, e.g. "(hover)").
fn speed_value(value: &Value) -> (Option<i64>, Option<String>) {
    match value {
        Value::Number(number) => (number.as_i64(), None),
        Value::Object(map) => {
            let number = map.get("number").and_then(Value::as_i64);
            let condition = map
                .get("condition")
                .and_then(Value::as_str)
                .map(strip_tags)
                .filter(|c| !c.is_empty());
            (number, condition)
        }
        _ => (None, None),
    }
}

fn format_saves(value: &Value) -> String {
    let Some(map) = value.as_object() else {
        return String::new();
    };
    [
        ("str", "Str"),
        ("dex", "Dex"),
        ("con", "Con"),
        ("int", "Int"),
        ("wis", "Wis"),
        ("cha", "Cha"),
    ]
    .iter()
    .filter_map(|(key, label)| {
        map.get(*key)
            .and_then(Value::as_str)
            .map(|value| format!("{label} {}", value.trim()))
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn format_skills(value: &Value) -> String {
    let Some(map) = value.as_object() else {
        return String::new();
    };
    let mut entries: Vec<(String, String)> = map
        .iter()
        .filter_map(|(key, value)| {
            value
                .as_str()
                .map(|v| (title_case_words(key), v.trim().to_string()))
        })
        .collect();
    entries.sort();
    entries
        .iter()
        .map(|(skill, bonus)| format!("{skill} {bonus}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Damage resist/immune/vulnerable: bare strings comma-join into one group;
/// each `{<inner_key>:[...], note}` object is its own group, groups joined by "; ".
fn format_damage(values: &[Value], inner_key: &str) -> String {
    let mut simple: Vec<String> = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    for value in values {
        match value {
            Value::String(text) => simple.push(strip_tags(text)),
            Value::Object(map) => {
                let items: Vec<String> = map
                    .get(inner_key)
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(strip_tags)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut group = items.join(", ");
                if let Some(note) = map.get("note").and_then(Value::as_str) {
                    if !group.is_empty() {
                        group.push(' ');
                    }
                    group.push_str(&strip_tags(note));
                }
                if !group.is_empty() {
                    groups.push(group);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if !simple.is_empty() {
        out.push(simple.join(", "));
    }
    out.extend(groups);
    out.join("; ")
}

fn format_conditions(values: &[Value]) -> String {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(|text| capitalize(&strip_tags(text)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_senses(senses: &[Value], passive: &Value) -> String {
    let mut parts: Vec<String> = senses
        .iter()
        .filter_map(Value::as_str)
        .map(strip_tags)
        .collect();
    let passive = match passive {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(strip_tags(text)),
        _ => None,
    };
    if let Some(passive) = passive {
        parts.push(format!("Passive Perception {passive}"));
    }
    parts.join(", ")
}

fn format_languages(languages: &[Value]) -> String {
    let parts: Vec<String> = languages
        .iter()
        .filter_map(Value::as_str)
        .map(strip_tags)
        .collect();
    if parts.is_empty() {
        return "—".to_string();
    }
    parts.join(", ")
}

fn format_cr(value: &Value) -> String {
    let base = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Object(map) => map
            .get("cr")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    };
    if base.is_empty() {
        return String::new();
    }
    match cr_xp_pb(&base) {
        Some((xp, pb)) => format!("{base} (XP {}; PB +{pb})", group_thousands(xp)),
        None => base,
    }
}

/// Numeric CR for sort ordering ("1/4" → 0.25). Public so the search-index
/// projection can store it.
pub fn cr_to_sort(value: &Value) -> f64 {
    let base = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => return number.as_f64().unwrap_or(0.0),
        Value::Object(map) => map
            .get("cr")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    };
    match base.as_str() {
        "1/8" => 0.125,
        "1/4" => 0.25,
        "1/2" => 0.5,
        other => other.parse().unwrap_or(0.0),
    }
}

/// The XP + proficiency bonus for a challenge rating. Misses (an odd CR string)
/// fall back to showing the CR alone.
fn cr_xp_pb(cr: &str) -> Option<(u32, i8)> {
    let xp = match cr {
        "0" => 10,
        "1/8" => 25,
        "1/4" => 50,
        "1/2" => 100,
        "1" => 200,
        "2" => 450,
        "3" => 700,
        "4" => 1100,
        "5" => 1800,
        "6" => 2300,
        "7" => 2900,
        "8" => 3900,
        "9" => 5000,
        "10" => 5900,
        "11" => 7200,
        "12" => 8400,
        "13" => 10000,
        "14" => 11500,
        "15" => 13000,
        "16" => 15000,
        "17" => 18000,
        "18" => 20000,
        "19" => 22000,
        "20" => 25000,
        "21" => 33000,
        "22" => 41000,
        "23" => 50000,
        "24" => 62000,
        "25" => 75000,
        "26" => 90000,
        "27" => 105000,
        "28" => 120000,
        "29" => 135000,
        "30" => 155000,
        _ => return None,
    };
    // Proficiency bonus scales with CR in bands.
    let numeric = cr_to_sort(&Value::String(cr.to_string()));
    let pb = match numeric {
        n if n < 5.0 => 2,
        n if n < 9.0 => 3,
        n if n < 13.0 => 4,
        n if n < 17.0 => 5,
        n if n < 21.0 => 6,
        n if n < 25.0 => 7,
        n if n < 29.0 => 8,
        _ => 9,
    };
    Some((xp, pb))
}

/// Gear is a list of item references: a bare `"scimitar|xphb"` string (strip the
/// `|source` suffix) or a `{item, quantity}` object → "javelin (×5)".
fn format_gear(gear: &[Value]) -> String {
    gear.iter()
        .filter_map(format_gear_entry)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_gear_entry(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(strip_source_suffix(text)),
        Value::Object(map) => {
            let name = strip_source_suffix(map.get("item")?.as_str()?);
            match map.get("quantity").and_then(Value::as_u64) {
                Some(quantity) if quantity > 1 => Some(format!("{name} (×{quantity})")),
                _ => Some(name),
            }
        }
        _ => None,
    }
}

fn strip_source_suffix(item: &str) -> String {
    item.split('|').next().unwrap_or(item).trim().to_string()
}

// ---------------------------------------------------------------------------
// Entries → StatBlock lowering (the monster twin of spell_import's lowering;
// StatBlock has no Heading, so a named subsection becomes a leading text line).
// ---------------------------------------------------------------------------

fn lower_entries(entries: &[Entry]) -> Vec<StatBlock> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Entry::Text(text) => {
                let text = strip_tags(text);
                if !text.trim().is_empty() {
                    out.push(StatBlock::Text { text });
                }
            }
            Entry::Block(block) => lower_block(block, &mut out),
        }
    }
    out
}

fn lower_block(block: &EntryBlock, out: &mut Vec<StatBlock>) {
    match block.kind.as_str() {
        "list" => {
            let items: Vec<String> = block
                .items
                .iter()
                .map(lower_list_item)
                .filter(|item| !item.is_empty())
                .collect();
            if !items.is_empty() {
                out.push(StatBlock::Bullets { items });
            }
        }
        "table" => {
            let headers: Vec<String> = block.col_labels.iter().map(|h| strip_tags(h)).collect();
            let rows: Vec<Vec<String>> = block
                .rows
                .iter()
                .map(|row| row.iter().map(cell_to_string).collect())
                .collect();
            out.push(StatBlock::Table { headers, rows });
        }
        // Any wrapper ("entries"/"inset"/…): surface a named lead-in line (StatBlock
        // has no heading), then flatten the children — never drop content.
        _ => {
            if let Some(name) = block.name.as_deref().filter(|n| !n.is_empty()) {
                out.push(StatBlock::Text {
                    text: format!("{}.", strip_tags(name)),
                });
            }
            out.extend(lower_entries(&block.entries));
        }
    }
}

fn lower_list_item(item: &ListItem) -> String {
    match item {
        ListItem::Text(text) => strip_tags(text),
        ListItem::Item(block) => {
            let body = block
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::Text(text) => Some(strip_tags(text)),
                    Entry::Block(_) => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            match block.name.as_deref().filter(|n| !n.is_empty()) {
                Some(name) => {
                    let name = strip_tags(name);
                    if body.is_empty() {
                        name
                    } else {
                        format!("{name}. {body}")
                    }
                }
                None => body,
            }
        }
    }
}

fn cell_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => strip_tags(text),
        Value::Number(number) => number.to_string(),
        other => other
            .get("entry")
            .and_then(|e| e.as_str())
            .map(strip_tags)
            .unwrap_or_default(),
    }
}

fn lower_string_list(lines: &[String]) -> Vec<StatBlock> {
    lines
        .iter()
        .map(|line| strip_tags(line))
        .filter(|line| !line.trim().is_empty())
        .map(|text| StatBlock::Text { text })
        .collect()
}

// ---------------------------------------------------------------------------
// Small string helpers
// ---------------------------------------------------------------------------

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn capitalize(text: &str) -> String {
    title_case(text)
}

fn title_case_words(text: &str) -> String {
    text.split_whitespace()
        .map(title_case)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Group an integer with thousands separators: 41000 → "41,000".
fn group_thousands(value: u32) -> String {
    let digits = value.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (len - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Representative monster JSON in the real 5etools shapes observed in XMM/MM.
    // Inline (like the spell importer's fixtures) so the tests are self-contained
    // and never depend on the `temp/` checkout being present.

    const XMM: &str = r#"{ "monster": [
        {
            "name": "Goblin Warrior", "source": "XMM", "page": 142,
            "size": ["S"], "type": {"type": "fey", "tags": ["goblinoid"]},
            "alignment": ["C", "N"], "ac": [15], "hp": {"average": 10, "formula": "3d6"},
            "speed": {"walk": 30},
            "str": 8, "dex": 15, "con": 10, "int": 10, "wis": 8, "cha": 8,
            "skill": {"stealth": "+6"}, "senses": ["Darkvision 60 ft."], "passive": 9,
            "languages": ["Common", "Goblin"], "cr": "1/4",
            "gear": ["leather armor|xphb", "scimitar|xphb"],
            "action": [
                {"name": "Scimitar", "entries": ["{@atkr m} {@hit 4}, reach 5 ft. {@h}5 ({@damage 1d6 + 2}) Slashing damage."]}
            ],
            "bonus": [
                {"name": "Nimble Escape", "entries": ["The goblin takes the {@action Disengage|XPHB} action."]}
            ]
        },
        {
            "name": "Adult Red Dragon", "source": "XMM", "page": 96,
            "size": ["H"], "type": {"type": "dragon", "tags": ["chromatic"]},
            "alignment": ["C", "E"], "ac": [19], "hp": {"average": 256, "formula": "19d12 + 133"},
            "speed": {"walk": 40, "climb": 40, "fly": 80},
            "str": 27, "dex": 10, "con": 25, "int": 16, "wis": 13, "cha": 23,
            "save": {"dex": "+6", "wis": "+7"}, "cr": {"cr": "17", "xpLair": 20000},
            "action": [
                {"name": "Fire Breath {@recharge 5}", "entries": ["{@actSave dex} {@dc 21}, each creature in a 60-foot {@variantrule Cone [Area of Effect]|XPHB|Cone}. {@actSaveFail} 59 ({@damage 17d6}) Fire damage. {@actSaveSuccess} Half damage."]}
            ]
        },
        {
            "name": "Lich", "source": "XMM", "page": 213,
            "size": ["M"], "type": {"type": "undead"}, "alignment": ["N", "E"],
            "ac": [20], "hp": {"average": 315, "formula": "42d8 + 126"}, "speed": {"walk": 30},
            "str": 11, "dex": 16, "con": 16, "int": 21, "wis": 14, "cha": 16,
            "save": {"con": "+10", "int": "+12", "wis": "+9"},
            "skill": {"arcana": "+19", "perception": "+9"},
            "resist": ["cold", "lightning"], "immune": ["necrotic", "poison"],
            "conditionImmune": ["charmed", "frightened", "paralyzed"],
            "senses": ["Truesight 120 ft."], "passive": 19, "languages": ["Common", "Abyssal"],
            "cr": {"cr": "21", "xpLair": 41000},
            "spellcasting": [
                {"name": "Spellcasting", "type": "spellcasting",
                 "headerEntries": ["The lich casts one of the following spells (save {@dc 20}):"],
                 "will": ["{@spell Detect Magic|XPHB}", "{@spell Fireball|XPHB}"],
                 "daily": {"1e": ["{@spell Power Word Kill|XPHB}"], "2e": ["{@spell Animate Dead|XPHB}"]},
                 "ability": "int", "displayAs": "action"}
            ],
            "trait": [{"name": "Spirit Jar", "entries": ["If destroyed, the lich reforms."]}],
            "legendary": [{"name": "Deathly Teleport", "entries": ["The lich teleports up to 60 feet."]}],
            "legendaryGroup": {"name": "Lich", "source": "XMM"}
        }
    ]}"#;

    const MM: &str = r#"{ "monster": [
        {
            "name": "Goblin Warrior", "source": "MM", "page": 166,
            "reprintedAs": ["Goblin Warrior|XMM"],
            "size": ["S"], "type": "humanoid", "alignment": ["N", "E"],
            "ac": [15], "hp": {"average": 7}, "speed": {"walk": 30},
            "str": 8, "dex": 14, "con": 10, "int": 10, "wis": 8, "cha": 8, "cr": "1/4"
        },
        {
            "name": "Mage", "source": "MM", "page": 347,
            "size": ["M"], "type": "humanoid", "alignment": ["A"],
            "ac": [12, {"ac": 15, "condition": "with {@spell mage armor}", "braces": true}],
            "hp": {"average": 40, "formula": "9d8"}, "speed": {"walk": 30},
            "str": 9, "dex": 14, "con": 11, "int": 17, "wis": 12, "cha": 11, "cr": "6",
            "spellcasting": [
                {"name": "Spellcasting", "type": "spellcasting",
                 "headerEntries": ["The mage is a 9th-level spellcaster (save {@dc 14}):"],
                 "spells": {"0": {"spells": ["{@spell fire bolt}", "{@spell light}"]},
                            "1": {"slots": 4, "spells": ["{@spell mage armor}", "{@spell shield}"]},
                            "3": {"slots": 3, "spells": ["{@spell fireball}", "{@spell fly}"]}},
                 "ability": "int"}
            ]
        },
        {
            "name": "Skeleton Knight", "source": "MM", "page": 200,
            "_copy": {"name": "Skeleton", "source": "MM"},
            "size": ["M"], "type": "undead", "cr": "1"
        }
    ]}"#;

    const GROUPS: &str = r#"{ "legendaryGroup": [
        {
            "name": "Lich", "source": "XMM",
            "regionalEffects": [
                "The region is warped by the lich's presence:",
                {"type": "list", "style": "list-hang-notitle", "items": [
                    {"type": "item", "name": "All-Seeing", "entries": ["The lich can cast {@spell Clairvoyance|XPHB}."]}
                ]}
            ]
        }
    ]}"#;

    fn import_fixture(files: &[(&str, &str)], index: &str, groups: Option<&str>) -> ImportSummary {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("monster_import_{}_{}", std::process::id(), n));
        let dir = root.join("data").join("bestiary");
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        std::fs::write(dir.join("index.json"), index).expect("write index");
        for (name, body) in files {
            std::fs::write(dir.join(name), body).expect("write bestiary file");
        }
        if let Some(groups) = groups {
            std::fs::write(dir.join("legendarygroups.json"), groups).expect("write groups");
        }
        let summary = import_monsters_from_dir(&root).expect("import monsters");
        let _ = std::fs::remove_dir_all(&root);
        summary
    }

    fn all() -> ImportSummary {
        import_fixture(
            &[("bestiary-xmm.json", XMM), ("bestiary-mm.json", MM)],
            r#"{"XMM": "bestiary-xmm.json", "MM": "bestiary-mm.json"}"#,
            Some(GROUPS),
        )
    }

    fn find<'a>(monsters: &'a [Monster], name: &str) -> &'a Monster {
        monsters.iter().find(|m| m.name == name).unwrap_or_else(|| {
            panic!(
                "monster {name} missing from {:?}",
                monsters.iter().map(|m| &m.name).collect::<Vec<_>>()
            )
        })
    }

    #[test]
    fn skips_copy_and_counts_them() {
        let summary = all();
        assert_eq!(summary.skipped_copy, 1, "Skeleton Knight has _copy");
        assert!(
            !summary.monsters.iter().any(|m| m.name == "Skeleton Knight"),
            "a _copy monster must not be imported in v1"
        );
    }

    #[test]
    fn dedup_prefers_xmm_and_drops_reprints() {
        let summary = all();
        let goblins: Vec<_> = summary
            .monsters
            .iter()
            .filter(|m| m.name == "Goblin Warrior")
            .collect();
        assert_eq!(goblins.len(), 1);
        assert_eq!(goblins[0].source, "XMM");
    }

    #[test]
    fn converts_goblin_core_fields() {
        let summary = all();
        let goblin = find(&summary.monsters, "Goblin Warrior");
        assert_eq!(goblin.slug, "goblin-warrior");
        assert_eq!(goblin.size, "Small");
        assert_eq!(goblin.creature_type, "Fey (Goblinoid)");
        assert_eq!(goblin.alignment, "Chaotic Neutral");
        assert_eq!(goblin.ac, "15");
        assert_eq!(goblin.hp, "10 (3d6)");
        assert_eq!(goblin.speed, "30 ft.");
        assert_eq!(goblin.abilities, [8, 15, 10, 10, 8, 8]);
        assert_eq!(goblin.skills, "Stealth +6");
        assert_eq!(goblin.senses, "Darkvision 60 ft., Passive Perception 9");
        assert_eq!(goblin.languages, "Common, Goblin");
        assert_eq!(goblin.cr, "1/4 (XP 50; PB +2)");
        assert_eq!(goblin.gear, "leather armor, scimitar");
    }

    #[test]
    fn goblin_attack_tags_lower_to_prose() {
        let summary = all();
        let goblin = find(&summary.monsters, "Goblin Warrior");
        let actions = goblin
            .sections
            .iter()
            .find(|s| s.title == "Actions")
            .expect("Actions section");
        let scimitar = &actions.abilities[0];
        assert_eq!(scimitar.name.as_deref(), Some("Scimitar"));
        assert_eq!(
            scimitar.body[0].to_text(),
            "Melee Attack Roll: +4, reach 5 ft. Hit: 5 (1d6 + 2) Slashing damage."
        );
        // Bonus action landed in its own section.
        assert!(goblin.sections.iter().any(|s| s.title == "Bonus Actions"));
    }

    #[test]
    fn dragon_recharge_in_name_and_save_in_body() {
        let summary = all();
        let dragon = find(&summary.monsters, "Adult Red Dragon");
        assert_eq!(dragon.size, "Huge");
        assert_eq!(dragon.creature_type, "Dragon (Chromatic)");
        assert_eq!(dragon.speed, "40 ft., Climb 40 ft., Fly 80 ft.");
        assert_eq!(dragon.saves, "Dex +6, Wis +7");
        assert_eq!(dragon.cr, "17 (XP 18,000; PB +6)");
        let breath = &dragon
            .sections
            .iter()
            .find(|s| s.title == "Actions")
            .unwrap()
            .abilities[0];
        assert_eq!(breath.name.as_deref(), Some("Fire Breath (Recharge 5-6)"));
        assert_eq!(
            breath.body[0].to_text(),
            "Dexterity Saving Throw: DC 21, each creature in a 60-foot Cone. \
             Failure: 59 (17d6) Fire damage. Success: Half damage."
        );
    }

    #[test]
    fn lich_2024_spellcasting_groups_buckets() {
        let summary = all();
        let lich = find(&summary.monsters, "Lich");
        let actions = lich
            .sections
            .iter()
            .find(|s| s.title == "Actions")
            .expect("Actions section (displayAs action)");
        let spellcasting = &actions.abilities[0];
        assert_eq!(spellcasting.name.as_deref(), Some("Spellcasting"));
        let text: Vec<String> = spellcasting.body.iter().map(StatBlock::to_text).collect();
        assert!(text.iter().any(|t| t.contains("save DC 20")));
        assert!(text.iter().any(|t| t == "At Will: Detect Magic, Fireball"));
        // 2/Day Each sorts before 1/Day Each.
        let two = text
            .iter()
            .position(|t| t.starts_with("2/Day Each"))
            .unwrap();
        let one = text
            .iter()
            .position(|t| t.starts_with("1/Day Each"))
            .unwrap();
        assert!(two < one);
    }

    #[test]
    fn lich_resolves_legendary_group_regional_effects() {
        let summary = all();
        let lich = find(&summary.monsters, "Lich");
        let regional = lich
            .sections
            .iter()
            .find(|s| s.title == "Regional Effects")
            .expect("Regional Effects from legendary group");
        // Intro prose + the bulleted named effect (tag stripped).
        assert!(regional.intro.iter().any(|b| matches!(
            b,
            StatBlock::Bullets { items } if items[0].starts_with("All-Seeing.") && items[0].contains("Clairvoyance")
        )));
        // And the monster keeps its own Legendary Actions + Traits.
        assert!(lich.sections.iter().any(|s| s.title == "Legendary Actions"));
        assert!(lich.sections.iter().any(|s| s.title == "Traits"));
    }

    #[test]
    fn mage_2014_slots_and_complex_ac() {
        let summary = all();
        let mage = find(&summary.monsters, "Mage");
        assert_eq!(mage.alignment, "Any Alignment");
        assert_eq!(mage.ac, "12, 15 with mage armor");
        let spellcasting = &mage
            .sections
            .iter()
            .find(|s| s.title == "Traits")
            .unwrap()
            .abilities[0];
        let text: Vec<String> = spellcasting.body.iter().map(StatBlock::to_text).collect();
        assert!(
            text.iter()
                .any(|t| t == "Cantrips (at will): fire bolt, light")
        );
        assert!(
            text.iter()
                .any(|t| t == "1st Level (4 slots): mage armor, shield")
        );
        assert!(
            text.iter()
                .any(|t| t == "3rd Level (3 slots): fireball, fly")
        );
    }

    #[test]
    fn monsters_are_sorted_by_name() {
        let summary = all();
        let names: Vec<&str> = summary.monsters.iter().map(|m| m.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_ascii_lowercase());
        assert_eq!(names, sorted);
    }

    #[test]
    fn formatters_handle_edge_shapes() {
        assert_eq!(format_hp(&serde_json::json!({"special": "58"})), "58");
        assert_eq!(format_alignment(&serde_json::json!(["U"])), "Unaligned");
        assert_eq!(format_alignment(&serde_json::json!(["N"])), "Neutral");
        assert_eq!(
            format_alignment(&serde_json::json!(["L", "G"])),
            "Lawful Good"
        );
        assert_eq!(
            format_speed(
                &serde_json::json!({"walk": 30, "fly": {"number": 30, "condition": "(hover)"}, "canHover": true})
            ),
            "30 ft., Fly 30 ft. (hover)"
        );
        assert_eq!(
            format_ac(
                serde_json::json!([{"ac": 16, "from": ["natural armor"]}])
                    .as_array()
                    .unwrap()
            ),
            "16 (natural armor)"
        );
        assert_eq!(format_languages(&[]), "—");
        assert_eq!(group_thousands(41000), "41,000");
    }

    /// Smoke test against a real local 5etools checkout. Ignored by default (the
    /// data isn't in the repo); run with `MONSTER_5E_DIR=<path> cargo test -p
    /// dnd-core real_dataset -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_dataset_imports_cleanly() {
        let dir =
            std::env::var("MONSTER_5E_DIR").expect("set MONSTER_5E_DIR to a 5etools checkout");
        let summary = import_monsters_from_dir(Path::new(&dir)).expect("import real dataset");
        let monsters = &summary.monsters;
        println!(
            "imported {} monsters, skipped {} _copy variants",
            monsters.len(),
            summary.skipped_copy
        );
        assert!(
            (2400..=2700).contains(&monsters.len()),
            "expected ~2575 canonical monsters, got {}",
            monsters.len()
        );
        assert!(
            summary.skipped_copy > 500,
            "expected many skipped _copy variants"
        );

        // Invariants: every monster has a name + a unique slug, and no residual
        // {@ markup leaks into any rendered text. (AC/HP can legitimately be empty
        // — some adventure NPC stat blocks omit them; the card just drops the row.)
        let mut slugs = std::collections::HashSet::new();
        let mut missing_ac = 0usize;
        for monster in monsters {
            assert!(!monster.name.is_empty(), "a monster has an empty name");
            assert!(!monster.slug.is_empty(), "{} has empty slug", monster.name);
            assert!(
                slugs.insert(monster.slug.clone()),
                "duplicate slug {}",
                monster.slug
            );
            if monster.ac.is_empty() {
                missing_ac += 1;
            }
            assert!(
                !monster.ac.contains("{@"),
                "unstripped tag in {} AC",
                monster.name
            );
            for section in &monster.sections {
                for block in &section.intro {
                    assert!(
                        !block.to_text().contains("{@"),
                        "unstripped tag in {}",
                        monster.name
                    );
                }
                for ability in &section.abilities {
                    for block in &ability.body {
                        assert!(
                            !block.to_text().contains("{@"),
                            "unstripped tag in {}: {}",
                            monster.name,
                            block.to_text()
                        );
                    }
                }
            }
        }
        println!("{missing_ac} monsters have no AC (incomplete adventure stat blocks)");
        let goblin = find(monsters, "Goblin Warrior");
        println!("goblin: {} | {} | {}", goblin.ac, goblin.cr, goblin.speed);
    }
}
