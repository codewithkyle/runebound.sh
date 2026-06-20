//! Convert a local 5etools spell dataset into our own [`Spell`] model.
//!
//! This is a **pure converter**: [`import_spells_from_dir`] reads a directory the
//! user points at (their own copy of the 5etools data — nothing copyrighted ships
//! in this repo), parses every official spell file named in `index.json`, dedups to
//! the 2024-canonical set, and lowers each entry into a render-ready [`Spell`].
//!
//! The only fiddly parsing is [`strip_tags`], which lowers 5etools inline markup
//! like `{@damage 8d6}` / `{@condition Prone|XPHB}` to plain display text. It is the
//! single well-tested seam for that; everything else is mechanical field mapping.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use runebound_models::spells::{Spell, SpellBlock};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Raw 5etools schema — only the fields we consume. `rename_all = "camelCase"`
// maps `entriesHigherLevel` / `scalingLevelDice` / `reprintedAs` automatically.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(default)]
    spell: Vec<RawSpell>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSpell {
    name: String,
    source: String,
    #[serde(default)]
    level: u8,
    #[serde(default)]
    school: Option<String>,
    #[serde(default)]
    time: Vec<RawTime>,
    #[serde(default)]
    range: Option<RawRange>,
    #[serde(default)]
    components: Option<RawComponents>,
    #[serde(default)]
    duration: Vec<RawDuration>,
    #[serde(default)]
    meta: Option<RawMeta>,
    #[serde(default)]
    classes: Option<RawClasses>,
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    entries_higher_level: Vec<Entry>,
    /// Parsed defensively in the cantrip-scaling fallback only — shape varies
    /// (object or array across sources), so it stays an untyped value.
    #[serde(default)]
    scaling_level_dice: serde_json::Value,
    /// Presence marks a superseded entry (the canonical reprint lives elsewhere in
    /// the set); we only check whether it exists, never its contents.
    #[serde(default)]
    reprinted_as: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawTime {
    #[serde(default)]
    number: u32,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    condition: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRange {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    distance: Option<RawDistance>,
}

#[derive(Debug, Deserialize)]
struct RawDistance {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    amount: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawComponents {
    #[serde(default)]
    v: bool,
    #[serde(default)]
    s: bool,
    #[serde(default)]
    m: Option<Material>,
}

/// The material component: a bare string, or `{ "text": …, "cost": … }`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Material {
    Text(String),
    Detailed { text: String },
}

#[derive(Debug, Deserialize)]
struct RawDuration {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    duration: Option<RawDurationSpec>,
    #[serde(default)]
    concentration: bool,
    #[serde(default)]
    ends: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawDurationSpec {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    amount: u32,
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    #[serde(default)]
    ritual: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawClasses {
    #[serde(default)]
    from_class_list: Vec<RawClassRef>,
}

#[derive(Debug, Deserialize)]
struct RawClassRef {
    name: String,
}

/// A `5etools` `entries` element: a string or a typed block.
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
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    items: Vec<ListItem>,
    #[serde(default)]
    col_labels: Vec<String>,
    /// Cells are usually strings (possibly with markup); the occasional cell object
    /// is best-effort stringified, never dropped.
    #[serde(default)]
    rows: Vec<Vec<serde_json::Value>>,
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

/// Read `dir` (a 5etools repo root *or* its `data/spells` dir), parse every
/// official spell file, dedup to the 2024-canonical set, and convert each to a
/// [`Spell`]. The result is sorted by name for deterministic import output.
pub fn import_spells_from_dir(dir: &Path) -> Result<Vec<Spell>> {
    let spells_dir = locate_spells_dir(dir)?;
    let index_path = spells_dir.join("index.json");
    let index_raw = std::fs::read_to_string(&index_path)
        .with_context(|| format!("failed to read {}", index_path.display()))?;
    // index.json maps source code -> filename. Order is irrelevant; dedup is by name.
    let index: BTreeMap<String, String> = serde_json::from_str(&index_raw)
        .with_context(|| format!("failed to parse {}", index_path.display()))?;

    let mut raw_spells: Vec<RawSpell> = Vec::new();
    for filename in index.values() {
        let path = spells_dir.join(filename);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read spell file {}", path.display()))?;
        let file: RawFile = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse spell file {}", path.display()))?;
        raw_spells.extend(file.spell);
    }

    let canonical = dedup_to_canonical(raw_spells);
    let mut spells: Vec<Spell> = canonical.iter().map(convert_spell).collect();
    disambiguate_slugs(&mut spells);
    spells.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    Ok(spells)
}

/// Accept the repo root, its `data/spells` dir, or a `spells` dir directly: the
/// data lives wherever `index.json` sits next to `spells-*.json`.
fn locate_spells_dir(dir: &Path) -> Result<PathBuf> {
    let candidates = [
        dir.join("data").join("spells"),
        dir.join("spells"),
        dir.to_path_buf(),
    ];
    for candidate in candidates {
        if candidate.join("index.json").is_file() {
            return Ok(candidate);
        }
    }
    bail!(
        "no 5etools spell data found under {} (looked for data/spells/index.json, \
         spells/index.json, and index.json)",
        dir.display()
    );
}

/// Reduce the full corpus to the 2024-canonical set: drop any entry that has been
/// reprinted (its newer version is in the set), then keep one entry per name,
/// preferring the 2024 PHB (`XPHB`).
fn dedup_to_canonical(raw: Vec<RawSpell>) -> Vec<RawSpell> {
    let mut by_name: BTreeMap<String, RawSpell> = BTreeMap::new();
    for spell in raw {
        if spell.reprinted_as.is_some() {
            continue; // superseded; the canonical reprint appears under its own source
        }
        match by_name.get(&spell.name) {
            // Prefer the first XPHB entry seen; never let a non-XPHB entry replace it.
            Some(existing) if existing.source == "XPHB" => {}
            Some(_) if spell.source == "XPHB" => {
                by_name.insert(spell.name.clone(), spell);
            }
            Some(_) => {}
            None => {
                by_name.insert(spell.name.clone(), spell);
            }
        }
    }
    by_name.into_values().collect()
}

/// On the rare chance two genuinely different spells share a name (so a slug
/// collides), suffix the later ones with `-{source}`. The XPHB-preference dedup
/// already removes the common *reprint* collisions, so this almost never fires.
fn disambiguate_slugs(spells: &mut [Spell]) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for spell in spells.iter_mut() {
        let count = seen.entry(spell.slug.clone()).or_insert(0);
        if *count > 0 {
            spell.slug = format!("{}-{}", spell.slug, slugify(&spell.source));
        }
        *count += 1;
    }
}

// ---------------------------------------------------------------------------
// Per-spell conversion
// ---------------------------------------------------------------------------

fn convert_spell(raw: &RawSpell) -> Spell {
    let concentration = raw.duration.iter().any(|d| d.concentration);
    let higher_levels = if !raw.entries_higher_level.is_empty() {
        Some(lower_entries(&raw.entries_higher_level))
    } else {
        synthesize_cantrip_scaling(&raw.scaling_level_dice)
    };
    let mut classes: Vec<String> = raw
        .classes
        .as_ref()
        .map(|c| c.from_class_list.iter().map(|r| r.name.clone()).collect())
        .unwrap_or_default();
    classes.sort();
    classes.dedup();

    Spell {
        slug: slugify(&raw.name),
        name: raw.name.clone(),
        source: raw.source.clone(),
        level: raw.level,
        school: school_name(raw.school.as_deref()),
        casting_time: format_casting_time(&raw.time),
        range: format_range(raw.range.as_ref()),
        components: format_components(raw.components.as_ref()),
        duration: format_duration(&raw.duration),
        ritual: raw.meta.as_ref().map(|m| m.ritual).unwrap_or(false),
        concentration,
        classes,
        description: lower_entries(&raw.entries),
        higher_levels,
    }
}

fn school_name(letter: Option<&str>) -> String {
    match letter.unwrap_or("").to_ascii_uppercase().as_str() {
        "A" => "Abjuration",
        "C" => "Conjuration",
        "D" => "Divination",
        "E" => "Enchantment",
        "V" => "Evocation",
        "I" => "Illusion",
        "N" => "Necromancy",
        "T" => "Transmutation",
        other if !other.is_empty() => return other.to_string(),
        _ => "Unknown",
    }
    .to_string()
}

fn format_casting_time(time: &[RawTime]) -> String {
    let Some(first) = time.first() else {
        return "—".to_string();
    };
    let unit = match first.unit.as_str() {
        "action" => "Action",
        "bonus" => "Bonus Action",
        "reaction" => "Reaction",
        "minute" => return plural(first.number, "Minute", first.condition.as_deref()),
        "hour" => return plural(first.number, "Hour", first.condition.as_deref()),
        "round" => return plural(first.number, "Round", first.condition.as_deref()),
        other => other,
    };
    let mut out = format!("{} {}", first.number, unit);
    if let Some(condition) = first.condition.as_deref().filter(|c| !c.is_empty()) {
        out.push_str(&format!(", {}", strip_tags(condition)));
    }
    out
}

/// "1 Minute" / "10 Minutes", with an optional trailing condition.
fn plural(number: u32, unit: &str, condition: Option<&str>) -> String {
    let mut out = if number == 1 {
        format!("1 {unit}")
    } else {
        format!("{number} {unit}s")
    };
    if let Some(condition) = condition.filter(|c| !c.is_empty()) {
        out.push_str(&format!(", {}", strip_tags(condition)));
    }
    out
}

fn format_range(range: Option<&RawRange>) -> String {
    let Some(range) = range else {
        return "—".to_string();
    };
    match range.kind.as_str() {
        "point" => match &range.distance {
            Some(distance) => format_point_distance(distance),
            None => "—".to_string(),
        },
        // Self-originating areas: "Self (15-foot Cone)".
        shape @ ("cone" | "line" | "radius" | "sphere" | "emanation" | "cube" | "hemisphere") => {
            match range.distance.as_ref().and_then(|d| d.amount) {
                Some(amount) => format!("Self ({amount}-foot {})", title_case(shape)),
                None => "Self".to_string(),
            }
        }
        "special" => "Special".to_string(),
        other if !other.is_empty() => title_case(other),
        _ => "—".to_string(),
    }
}

fn format_point_distance(distance: &RawDistance) -> String {
    match distance.kind.as_str() {
        "feet" => format!("{} feet", distance.amount.unwrap_or(0)),
        "miles" => {
            let amount = distance.amount.unwrap_or(0);
            if amount == 1 {
                "1 mile".to_string()
            } else {
                format!("{amount} miles")
            }
        }
        "touch" => "Touch".to_string(),
        "self" => "Self".to_string(),
        "sight" => "Sight".to_string(),
        "unlimited" => "Unlimited".to_string(),
        other if !other.is_empty() => title_case(other),
        _ => "—".to_string(),
    }
}

fn format_components(components: Option<&RawComponents>) -> String {
    let Some(components) = components else {
        return "—".to_string();
    };
    let mut parts = Vec::new();
    if components.v {
        parts.push("V".to_string());
    }
    if components.s {
        parts.push("S".to_string());
    }
    if let Some(material) = &components.m {
        let text = match material {
            Material::Text(text) => text.clone(),
            Material::Detailed { text } => text.clone(),
        };
        parts.push(format!("M ({})", strip_tags(&text)));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_duration(duration: &[RawDuration]) -> String {
    let Some(first) = duration.first() else {
        return "—".to_string();
    };
    match first.kind.as_str() {
        "instant" => "Instantaneous".to_string(),
        "permanent" => {
            if first.ends.iter().any(|e| e == "dispel") {
                "Until Dispelled".to_string()
            } else if first.ends.iter().any(|e| e == "trigger") {
                "Until Triggered".to_string()
            } else {
                "Permanent".to_string()
            }
        }
        "special" => "Special".to_string(),
        "timed" => {
            let spec = match &first.duration {
                Some(spec) => spec,
                None => return "—".to_string(),
            };
            let unit = duration_unit(&spec.kind, spec.amount);
            if first.concentration {
                format!("Concentration, up to {} {unit}", spec.amount)
            } else {
                format!("{} {unit}", spec.amount)
            }
        }
        other if !other.is_empty() => title_case(other),
        _ => "—".to_string(),
    }
}

fn duration_unit(unit: &str, amount: u32) -> String {
    // The 5etools unit names ("minute", "hour", …) are already the display words;
    // only pluralization is needed.
    if amount == 1 {
        unit.to_string()
    } else {
        format!("{unit}s")
    }
}

/// Build a one-line scaling summary from `scalingLevelDice` for a cantrip lacking an
/// `entriesHigherLevel` block. Returns `None` when the field is absent/unparseable.
fn synthesize_cantrip_scaling(value: &serde_json::Value) -> Option<Vec<SpellBlock>> {
    // `scalingLevelDice` may be an object or an array of objects; handle the object.
    let scaling = value.get("scaling")?.as_object()?;
    if scaling.is_empty() {
        return None;
    }
    let mut levels: Vec<(u32, String)> = scaling
        .iter()
        .filter_map(|(level, dice)| {
            let level = level.parse::<u32>().ok()?;
            let dice = dice.as_str()?.to_string();
            Some((level, dice))
        })
        .collect();
    levels.sort_by_key(|(level, _)| *level);
    if levels.is_empty() {
        return None;
    }
    let parts: Vec<String> = levels
        .iter()
        .map(|(level, dice)| format!("{dice} (level {level})"))
        .collect();
    Some(vec![
        SpellBlock::Heading {
            text: "Cantrip Upgrade".to_string(),
        },
        SpellBlock::Text {
            text: format!(
                "The damage changes as you gain levels: {}.",
                parts.join(", ")
            ),
        },
    ])
}

// ---------------------------------------------------------------------------
// Entries → SpellBlock lowering
// ---------------------------------------------------------------------------

fn lower_entries(entries: &[Entry]) -> Vec<SpellBlock> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Entry::Text(text) => out.push(SpellBlock::Text {
                text: strip_tags(text),
            }),
            Entry::Block(block) => lower_block(block, &mut out),
        }
    }
    out
}

fn lower_block(block: &EntryBlock, out: &mut Vec<SpellBlock>) {
    match block.kind.as_str() {
        "list" => {
            let items: Vec<String> = block.items.iter().map(lower_list_item).collect();
            if !items.is_empty() {
                out.push(SpellBlock::Bullets { items });
            }
        }
        "table" => {
            let headers: Vec<String> = block.col_labels.iter().map(|h| strip_tags(h)).collect();
            let rows: Vec<Vec<String>> = block
                .rows
                .iter()
                .map(|row| row.iter().map(cell_to_string).collect())
                .collect();
            out.push(SpellBlock::Table { headers, rows });
        }
        // "entries"/"section"/"inset" and any unknown wrapper: surface a named
        // heading (if any), then flatten the children — never drop content.
        _ => {
            if let Some(name) = block.name.as_deref().filter(|n| !n.is_empty()) {
                out.push(SpellBlock::Heading {
                    text: strip_tags(name),
                });
            }
            out.extend(lower_entries(&block.entries));
        }
    }
}

/// Flatten one list item to a single bullet line. A named item (`{type:"item",
/// name, entries}`) renders as "Name. body".
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

fn cell_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => strip_tags(text),
        serde_json::Value::Number(number) => number.to_string(),
        // A cell object (e.g. `{type:"cell", ...}`) — best-effort, never panic.
        other => other
            .get("entry")
            .and_then(|e| e.as_str())
            .map(strip_tags)
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Inline markup
// ---------------------------------------------------------------------------

/// Strip 5etools inline tags `{@tag display|arg|arg}` to their visible text.
///
/// Rules (matching how 5etools renders): the display text is the LAST `|`-segment
/// when an explicit display is given (3+ segments, e.g.
/// `{@variantrule Sphere [Area of Effect]|XPHB|Sphere}` → "Sphere",
/// `{@scaledamage 8d6|3-9|1d6}` → "1d6"), otherwise the FIRST segment
/// (`{@damage 8d6}` → "8d6", `{@condition Prone|XPHB}` → "Prone"). `{@dc 15}` is
/// special-cased to "DC 15".
pub fn strip_tags(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{'
            && i + 1 < chars.len()
            && chars[i + 1] == '@'
            && let Some(close) = (i + 2..chars.len()).position(|j| chars[j] == '}')
        {
            let close = i + 2 + close;
            let inner: String = chars[i + 2..close].iter().collect();
            out.push_str(&render_tag(&inner));
            i = close + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn render_tag(inner: &str) -> String {
    let (tag, rest) = inner.split_once(' ').unwrap_or((inner, ""));
    if rest.is_empty() {
        return String::new();
    }
    let segments: Vec<&str> = rest.split('|').collect();
    if tag.eq_ignore_ascii_case("dc") {
        return format!("DC {}", segments[0].trim());
    }
    let display = if segments.len() >= 3 {
        segments[segments.len() - 1]
    } else {
        segments[0]
    };
    display.trim().to_string()
}

/// Kebab-case a spell name into its slug — the primary key shared by the TOML
/// store and the search DB. Public so lookups can derive a slug from a typed name.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // --- strip_tags ------------------------------------------------------

    #[test]
    fn strip_tags_handles_every_tag_shape() {
        assert_eq!(
            strip_tags("deals {@damage 8d6} Fire damage"),
            "deals 8d6 Fire damage"
        );
        assert_eq!(
            strip_tags("the {@condition Prone|XPHB} condition"),
            "the Prone condition"
        );
        assert_eq!(strip_tags("a {@dc 15} save"), "a DC 15 save");
        assert_eq!(
            strip_tags("a 20-foot {@variantrule Sphere [Area of Effect]|XPHB|Sphere}"),
            "a 20-foot Sphere"
        );
        assert_eq!(
            strip_tags("increases by {@scaledamage 8d6|3-9|1d6} for each slot"),
            "increases by 1d6 for each slot"
        );
        assert_eq!(strip_tags("no tags here"), "no tags here");
    }

    // --- conversion fixtures (real XPHB shapes) --------------------------

    fn import_fixture(files: &[(&str, &str)], index: &str) -> Vec<Spell> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("spell_import_{}_{}", std::process::id(), n));
        let dir = root.join("data").join("spells");
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        std::fs::write(dir.join("index.json"), index).expect("write index");
        for (name, body) in files {
            std::fs::write(dir.join(name), body).expect("write spell file");
        }
        let spells = import_spells_from_dir(&root).expect("import spells");
        let _ = std::fs::remove_dir_all(&root);
        spells
    }

    fn find<'a>(spells: &'a [Spell], name: &str) -> &'a Spell {
        spells.iter().find(|s| s.name == name).unwrap_or_else(|| {
            panic!(
                "spell {name} missing from {:?}",
                spells.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        })
    }

    const XPHB: &str = r#"{ "spell": [
        {
            "name": "Fireball", "source": "XPHB", "level": 3, "school": "V",
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "point", "distance": {"type": "feet", "amount": 150}},
            "components": {"v": true, "s": true, "m": "a ball of bat guano and sulfur"},
            "duration": [{"type": "instant"}],
            "entries": ["Each creature in a 20-foot-radius {@variantrule Sphere [Area of Effect]|XPHB|Sphere} makes a Dexterity saving throw, taking {@damage 8d6} Fire damage."],
            "entriesHigherLevel": [{"type": "entries", "name": "Using a Higher-Level Spell Slot", "entries": ["The damage increases by {@scaledamage 8d6|3-9|1d6} for each spell slot level above 3."]}]
        },
        {
            "name": "Fire Bolt", "source": "XPHB", "level": 0, "school": "V",
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "point", "distance": {"type": "feet", "amount": 120}},
            "components": {"v": true, "s": true},
            "duration": [{"type": "instant"}],
            "entries": ["On a hit, the target takes {@damage 1d10} Fire damage."],
            "scalingLevelDice": {"label": "Fire damage", "scaling": {"1": "1d10", "5": "2d10", "11": "3d10", "17": "4d10"}}
        },
        {
            "name": "Command", "source": "XPHB", "level": 1, "school": "E",
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "point", "distance": {"type": "feet", "amount": 60}},
            "components": {"v": true},
            "duration": [{"type": "instant"}],
            "entries": ["Choose the command:", {"type": "list", "style": "list-hang-notitle", "items": [
                {"type": "item", "name": "Approach", "entries": ["The target moves toward you."]},
                {"type": "item", "name": "Grovel", "entries": ["The target has the {@condition Prone|XPHB} condition."]}
            ]}]
        },
        {
            "name": "Alter Self", "source": "XPHB", "level": 2, "school": "T",
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "point", "distance": {"type": "self"}},
            "components": {"v": true, "s": true},
            "duration": [{"type": "timed", "duration": {"type": "hour", "amount": 1}, "concentration": true}],
            "entries": ["Choose one option.", {"type": "entries", "name": "Aquatic Adaptation", "entries": ["You can breathe underwater."]}, {"type": "entries", "name": "Change Appearance", "entries": ["You alter your appearance."]}]
        },
        {
            "name": "Chromatic Orb", "source": "XPHB", "level": 1, "school": "V",
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "point", "distance": {"type": "feet", "amount": 90}},
            "components": {"v": true, "s": true, "m": {"text": "a diamond worth 50+ GP", "cost": 5000}},
            "duration": [{"type": "instant"}],
            "entries": ["On a hit, the target takes {@damage 3d8} damage."]
        },
        {
            "name": "Burning Hands", "source": "XPHB", "level": 1, "school": "V",
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "cone", "distance": {"type": "feet", "amount": 15}},
            "components": {"v": true, "s": true},
            "duration": [{"type": "instant"}],
            "entries": ["Each creature in a 15-foot {@variantrule Cone [Area of Effect]|XPHB|Cone} makes a Dexterity saving throw."]
        },
        {
            "name": "Detect Magic", "source": "XPHB", "level": 1, "school": "D",
            "meta": {"ritual": true},
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "emanation", "distance": {"type": "feet", "amount": 30}},
            "components": {"v": true, "s": true},
            "duration": [{"type": "timed", "duration": {"type": "minute", "amount": 10}, "concentration": true}],
            "entries": ["You sense the presence of magic within range."]
        }
    ]}"#;

    const PHB: &str = r#"{ "spell": [
        {
            "name": "Fireball", "source": "PHB", "level": 3, "school": "V",
            "reprintedAs": ["Fireball|XPHB"],
            "time": [{"number": 1, "unit": "action"}],
            "range": {"type": "point", "distance": {"type": "feet", "amount": 150}},
            "components": {"v": true, "s": true, "m": "bat guano"},
            "duration": [{"type": "instant"}],
            "entries": ["The 2014 version, superseded."]
        }
    ]}"#;

    fn all() -> Vec<Spell> {
        import_fixture(
            &[("spells-xphb.json", XPHB), ("spells-phb.json", PHB)],
            r#"{"XPHB": "spells-xphb.json", "PHB": "spells-phb.json"}"#,
        )
    }

    #[test]
    fn dedup_prefers_xphb_and_drops_reprints() {
        let spells = all();
        // The PHB Fireball (reprintedAs) is dropped; only the XPHB one survives.
        let fireballs: Vec<_> = spells.iter().filter(|s| s.name == "Fireball").collect();
        assert_eq!(fireballs.len(), 1);
        assert_eq!(fireballs[0].source, "XPHB");
        assert!(fireballs[0].description[0].to_text().contains("8d6"));
    }

    #[test]
    fn converts_core_fields() {
        let spells = all();
        let fireball = find(&spells, "Fireball");
        assert_eq!(fireball.slug, "fireball");
        assert_eq!(fireball.level, 3);
        assert_eq!(fireball.school, "Evocation");
        assert_eq!(fireball.casting_time, "1 Action");
        assert_eq!(fireball.range, "150 feet");
        assert_eq!(
            fireball.components,
            "V, S, M (a ball of bat guano and sulfur)"
        );
        assert_eq!(fireball.duration, "Instantaneous");
        assert!(!fireball.ritual);
    }

    #[test]
    fn cantrip_has_scaling_and_zero_level() {
        let spells = all();
        let bolt = find(&spells, "Fire Bolt");
        assert_eq!(bolt.level, 0);
        let higher = bolt.higher_levels.as_ref().expect("scaling synthesized");
        assert!(matches!(&higher[0], SpellBlock::Heading { text } if text == "Cantrip Upgrade"));
        assert!(higher[1].to_text().contains("4d10"));
    }

    #[test]
    fn list_spell_lowers_named_items_to_bullets() {
        let spells = all();
        let command = find(&spells, "Command");
        let bullets = command
            .description
            .iter()
            .find_map(|b| match b {
                SpellBlock::Bullets { items } => Some(items.clone()),
                _ => None,
            })
            .expect("Command has a bullet list");
        assert!(bullets[0].starts_with("Approach."));
        assert!(bullets[1].contains("Prone")); // tag stripped
    }

    #[test]
    fn subsection_spell_lowers_named_entries_to_headings() {
        let spells = all();
        let alter = find(&spells, "Alter Self");
        let headings: Vec<_> = alter
            .description
            .iter()
            .filter_map(|b| match b {
                SpellBlock::Heading { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(headings, vec!["Aquatic Adaptation", "Change Appearance"]);
        assert!(alter.concentration);
        assert_eq!(alter.range, "Self");
        assert_eq!(alter.duration, "Concentration, up to 1 hour");
    }

    #[test]
    fn material_cost_object_is_stringified() {
        let spells = all();
        let orb = find(&spells, "Chromatic Orb");
        assert_eq!(orb.components, "V, S, M (a diamond worth 50+ GP)");
    }

    #[test]
    fn self_originating_area_range_is_formatted() {
        let spells = all();
        let burning = find(&spells, "Burning Hands");
        assert_eq!(burning.range, "Self (15-foot Cone)");
        let detect = find(&spells, "Detect Magic");
        assert_eq!(detect.range, "Self (30-foot Emanation)");
        assert!(detect.ritual);
        assert_eq!(detect.duration, "Concentration, up to 10 minutes");
    }

    /// Smoke test against a real local 5etools checkout. Ignored by default (the
    /// data isn't in the repo); run with `SPELL_5E_DIR=<path> cargo test -p dnd-core
    /// real_dataset -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_dataset_imports_cleanly() {
        let dir = std::env::var("SPELL_5E_DIR").expect("set SPELL_5E_DIR to a 5etools checkout");
        let spells = import_spells_from_dir(Path::new(&dir)).expect("import real dataset");
        println!("imported {} spells", spells.len());
        assert!(
            (520..=600).contains(&spells.len()),
            "expected ~554 canonical spells, got {}",
            spells.len()
        );
        // No spell should have empty required display fields.
        for spell in &spells {
            assert!(!spell.slug.is_empty(), "{} has empty slug", spell.name);
            assert!(!spell.school.is_empty());
            assert!(!spell.casting_time.is_empty());
            assert!(!spell.range.is_empty());
            assert!(!spell.duration.is_empty());
            assert!(
                !spell.description.is_empty(),
                "{} has empty body",
                spell.name
            );
        }
        // Spot-check a well-known spell survived with sane fields.
        let fireball = find(&spells, "Fireball");
        assert_eq!(fireball.level, 3);
        assert_eq!(fireball.school, "Evocation");
        // No residual 5etools markup leaked into any rendered text.
        for spell in &spells {
            for block in &spell.description {
                let text = block.to_text();
                assert!(
                    !text.contains("{@"),
                    "unstripped tag in {}: {text}",
                    spell.name
                );
            }
        }
        println!(
            "sample: {} | {} | {}",
            fireball.range, fireball.components, fireball.duration
        );
    }

    #[test]
    fn spells_are_sorted_by_name() {
        let spells = all();
        let names: Vec<&str> = spells.iter().map(|s| s.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_ascii_lowercase());
        assert_eq!(names, sorted);
    }
}
