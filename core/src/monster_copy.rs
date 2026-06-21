//! Resolve 5etools `_copy` monster inheritance (the `_applyCopy`/`_mod` engine).
//!
//! In the source data, ~1100 monsters are not written out in full: they carry a
//! `_copy: {name, source, _mod, _templates, _preserve}` block that says "I am
//! that base monster, with these edits." v1 of the importer skipped them; this
//! module materializes them so the bestiary covers the adventure NPC variants too.
//!
//! It is a faithful port of `js/utils.js` `DataUtil.generic.copyApplier` from the
//! 5etools source, deliberately narrowed to what the real dataset uses (surveyed
//! 2026-06-20): the 21 `_mod` modes that actually appear, `template.json`
//! templating, and `_preserve`. The mechanic 5etools supports but this data never
//! exercises — the `<$...$>` variable resolver (0 occurrences) — is intentionally
//! omitted.
//!
//! The engine runs as a pure `Value` → `Value` pre-pass *before* [`RawMonster`]
//! (`crate::monster_import`) deserialization, because `_mod` edits are defined over
//! the raw JSON shape (path-addressed array splices, text rewrites). That keeps the
//! typed converter downstream completely unaware of copies — a resolved monster is
//! just a normal monster object with `_copy` removed.
//!
//! [`RawMonster`]: crate::monster_import

use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde_json::{Map, Value, json};

/// The props a `_mod` keyed by `"*"` fans out across (5etools `COPY_ENTRY_PROPS`).
const COPY_ENTRY_PROPS: &[&str] = &[
    "action",
    "bonus",
    "reaction",
    "trait",
    "legendary",
    "mythic",
    "variant",
    "spellcasting",
    "actionHeader",
    "bonusHeader",
    "reactionHeader",
    "legendaryHeader",
    "mythicHeader",
];

/// Fields that are NOT auto-inherited from the base unless `_copy._preserve` names
/// them (union of 5etools' generic + monster-specific `_MERGE_REQUIRES_PRESERVE`).
/// The one that matters to what we render is `legendaryGroup`; `reprintedAs` matters
/// because inheriting it would wrongly drop the variant during dedup.
const MERGE_REQUIRES_PRESERVE: &[&str] = &[
    "page",
    "otherSources",
    "referenceSources",
    "srd",
    "srd52",
    "basicRules",
    "basicRules2024",
    "reprintedAs",
    "hasFluff",
    "hasFluffImages",
    "hasToken",
    "tokenCredit",
    "tokenCustom",
    "foundryTokenScale",
    "altArt",
    "_versions",
    "legendaryGroup",
    "environment",
    "soundClip",
    "variant",
    "dragonCastingColor",
    "familiar",
];

/// The outcome of resolving a pool of monster objects: every monster (non-copies
/// unchanged, copies materialized), plus how many `_copy` variants were resolved vs.
/// dropped because their base could not be found.
pub struct CopyResolution {
    pub monsters: Vec<Value>,
    pub resolved_copy: usize,
    pub skipped_copy: usize,
}

/// Resolve every `_copy` in `monsters`, drawing bases from the same pool and
/// templates from `templates` (the `monsterTemplate` array of `template.json`).
///
/// Templates are resolved among themselves first (a few templates `_copy` other
/// templates), then monsters are resolved against the fully-materialized template
/// set. A copy whose base is missing or forms a cycle is dropped and counted.
pub fn resolve_copies(monsters: Vec<Value>, templates: Vec<Value>) -> CopyResolution {
    // Templates can copy each other (e.g. Hill Dwarf copies Mountain Dwarf), so
    // materialize them with the same engine first, against an empty template map.
    let resolved_templates = resolve_pool(templates, &HashMap::new()).values;
    let template_map = index_by_name_source(&resolved_templates);

    let pool = resolve_pool(monsters, &template_map);
    CopyResolution {
        monsters: pool.values,
        resolved_copy: pool.resolved_copy,
        skipped_copy: pool.skipped_copy,
    }
}

// ---------------------------------------------------------------------------
// Pool resolution (memoized, recursive, cycle-safe)
// ---------------------------------------------------------------------------

struct PoolResult {
    values: Vec<Value>,
    resolved_copy: usize,
    skipped_copy: usize,
}

#[derive(Clone)]
enum Cache {
    Pending,
    Visiting,
    Resolved(Value),
    Failed,
}

/// Lowercased `(name, source)` key used to match a `_copy`/`_templates` reference to
/// its base — 5etools' hash for the bestiary page.
fn name_source_key(value: &Value) -> Option<(String, String)> {
    let name = value
        .get("name")
        .and_then(Value::as_str)?
        .to_ascii_lowercase();
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    Some((name, source))
}

fn index_by_name_source(values: &[Value]) -> HashMap<(String, String), Value> {
    let mut map = HashMap::new();
    for value in values {
        if let Some(key) = name_source_key(value) {
            // "Earlier = better": never clobber an existing entry.
            map.entry(key).or_insert_with(|| value.clone());
        }
    }
    map
}

fn resolve_pool(values: Vec<Value>, templates: &HashMap<(String, String), Value>) -> PoolResult {
    // First-wins index from key -> position in `values`.
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for (i, value) in values.iter().enumerate() {
        if let Some(key) = name_source_key(value) {
            index.entry(key).or_insert(i);
        }
    }

    let mut cache: Vec<Cache> = vec![Cache::Pending; values.len()];
    for i in 0..values.len() {
        resolve_idx(i, &values, &index, templates, &mut cache);
    }

    let mut out = Vec::with_capacity(values.len());
    let mut resolved_copy = 0;
    let mut skipped_copy = 0;
    for (i, value) in values.iter().enumerate() {
        let is_copy = value.get("_copy").is_some();
        match &cache[i] {
            Cache::Resolved(resolved) => {
                if is_copy {
                    resolved_copy += 1;
                }
                out.push(resolved.clone());
            }
            _ => {
                // Only copies can fail (a plain monster always resolves to itself).
                if is_copy {
                    skipped_copy += 1;
                }
            }
        }
    }
    PoolResult {
        values: out,
        resolved_copy,
        skipped_copy,
    }
}

/// Resolve the monster at `idx`, recursing into its base first. Memoized in `cache`;
/// `Visiting` marks the in-progress set so a `_copy` cycle fails instead of looping.
fn resolve_idx(
    idx: usize,
    values: &[Value],
    index: &HashMap<(String, String), usize>,
    templates: &HashMap<(String, String), Value>,
    cache: &mut Vec<Cache>,
) -> Cache {
    match &cache[idx] {
        Cache::Pending => {}
        other => return other.clone(),
    }

    let value = &values[idx];
    let Some(copy_meta) = value.get("_copy") else {
        // Not a copy: resolves to itself.
        cache[idx] = Cache::Resolved(value.clone());
        return cache[idx].clone();
    };

    // Locate the base by (name, source); a copy may omit source (defaults to its own).
    let base_name = copy_meta
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let base_source = copy_meta
        .get("source")
        .and_then(Value::as_str)
        .or_else(|| value.get("source").and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let base_idx = base_name
        .and_then(|name| index.get(&(name, base_source)).copied())
        .filter(|&base_idx| base_idx != idx);

    let Some(base_idx) = base_idx else {
        cache[idx] = Cache::Failed;
        return Cache::Failed;
    };

    cache[idx] = Cache::Visiting;
    let base = resolve_idx(base_idx, values, index, templates, cache);
    let result = match base {
        Cache::Resolved(base_value) => {
            Cache::Resolved(apply_copy(&base_value, value.clone(), templates))
        }
        _ => Cache::Failed,
    };
    cache[idx] = result.clone();
    result
}

// ---------------------------------------------------------------------------
// The merge itself (port of `copyApplier.getCopy`)
// ---------------------------------------------------------------------------

/// Materialize one copy: fill missing fields from `base`, fold in any templates,
/// then apply the `_mod` edits. `copy_to` is the variant (it owns `_copy`).
fn apply_copy(
    base: &Value,
    mut copy_to: Value,
    templates: &HashMap<(String, String), Value>,
) -> Value {
    let Some(obj) = copy_to.as_object_mut() else {
        return copy_to;
    };

    // Pull `_copy` out so we can read its parts while mutating the rest.
    let copy_meta = obj.remove("_copy").unwrap_or(Value::Null);
    let preserve = copy_meta.get("_preserve");
    let preserve_all = preserve
        .and_then(|p| p.get("*"))
        .is_some_and(|v| !v.is_null());

    // Build the combined mod map: the copy's own `_mod` plus every template's mods.
    let mut mods: Map<String, Value> = normalise_mods(copy_meta.get("_mod"));
    let mut template_roots: Vec<Value> = Vec::new();
    if let Some(refs) = copy_meta.get("_templates").and_then(Value::as_array) {
        for template_ref in refs {
            let Some(key) = name_source_key(template_ref) else {
                continue;
            };
            let Some(template) = templates.get(&key) else {
                continue;
            };
            let Some(apply) = template.get("apply") else {
                continue;
            };
            let template_mods = normalise_mods(apply.get("_mod"));
            for (prop, list) in template_mods {
                merge_mod_list(&mut mods, prop, list);
            }
            if let Some(root) = apply.get("_root") {
                template_roots.push(root.clone());
            }
        }
    }

    // Root props the variant set itself — protected from template `_root` overrides.
    let original_root_props: HashSet<String> = obj.keys().cloned().collect();

    // Base copy: fill keys the variant left absent; an explicit `null` deletes.
    if let Some(base_obj) = base.as_object() {
        for (key, base_value) in base_obj {
            match obj.get(key) {
                Some(Value::Null) => {
                    obj.remove(key);
                }
                Some(_) => {} // variant's own value wins
                None => {
                    let requires_preserve = MERGE_REQUIRES_PRESERVE.contains(&key.as_str());
                    let preserved = preserve_all
                        || preserve
                            .and_then(|p| p.get(key))
                            .is_some_and(|v| !v.is_null());
                    if !requires_preserve || preserved {
                        obj.insert(key.clone(), base_value.clone());
                    }
                }
            }
        }
    }

    // Template `_root` props apply after the base copy, overriding base-inherited
    // values but never the variant's own explicit root props.
    for root in &template_roots {
        if let Some(root_obj) = root.as_object() {
            for (key, root_value) in root_obj {
                if !original_root_props.contains(key) {
                    obj.insert(key.clone(), root_value.clone());
                }
            }
        }
    }

    // Apply mods. Named props first, then `_` (no-prop), then `*` (all entry props).
    let mut keys: Vec<String> = mods.keys().cloned().collect();
    keys.sort_by_key(|key| prop_rank(key));
    for key in keys {
        let Some(mod_infos) = mods.get(&key).and_then(Value::as_array).cloned() else {
            continue;
        };
        apply_mods_for_key(&mut copy_to, &key, &mod_infos);
    }

    copy_to
}

/// `_` and `*` sort after named props (5etools `_PROPS_TAIL` ordering).
fn prop_rank(prop: &str) -> u8 {
    match prop {
        "_" => 1,
        "*" => 2,
        _ => 0,
    }
}

/// Coerce a `_mod` object into `{prop: [modInfo, ...]}` (each value becomes a list).
fn normalise_mods(mods: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(Value::Object(map)) = mods {
        for (prop, value) in map {
            let list = match value {
                Value::Array(items) => items.clone(),
                other => vec![other.clone()],
            };
            out.insert(prop.clone(), Value::Array(list));
        }
    }
    out
}

/// Concatenate a template's mod list onto the copy's list for the same prop.
fn merge_mod_list(mods: &mut Map<String, Value>, prop: String, list: Value) {
    let Value::Array(incoming) = list else { return };
    match mods.get_mut(&prop) {
        Some(Value::Array(existing)) => existing.extend(incoming),
        _ => {
            mods.insert(prop, Value::Array(incoming));
        }
    }
}

/// Dispatch a prop key to its targets: `*` fans across the entry props, `_` runs
/// with no prop path, anything else targets that single prop.
fn apply_mods_for_key(copy_to: &mut Value, key: &str, mod_infos: &[Value]) {
    match key {
        "*" => {
            for prop in COPY_ENTRY_PROPS {
                for mod_info in mod_infos {
                    do_mod(copy_to, Some(prop), mod_info);
                }
            }
        }
        "_" => {
            for mod_info in mod_infos {
                do_mod(copy_to, None, mod_info);
            }
        }
        _ => {
            for mod_info in mod_infos {
                do_mod(copy_to, Some(key), mod_info);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Individual `_mod` modes (port of the `_doMod_*` family)
// ---------------------------------------------------------------------------

fn do_mod(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    // The bare string `"remove"` deletes the targeted prop.
    if let Value::String(mode) = mod_info {
        if mode == "remove"
            && let (Some(prop), Some(obj)) = (prop, copy_to.as_object_mut())
        {
            obj.remove(prop);
        }
        return;
    }

    let Some(mode) = mod_info.get("mode").and_then(Value::as_str) else {
        return;
    };
    match mode {
        "appendStr" => mod_append_str(copy_to, prop, mod_info),
        "replaceTxt" => mod_replace_txt(copy_to, prop, mod_info),
        "prependArr" => mod_prepend_arr(copy_to, prop, mod_info),
        "appendArr" => mod_append_arr(copy_to, prop, mod_info),
        "appendIfNotExistsArr" => mod_append_if_not_exists_arr(copy_to, prop, mod_info),
        "replaceArr" => {
            mod_replace_arr(copy_to, prop, mod_info);
        }
        "replaceOrAppendArr" => {
            let replaced = mod_replace_arr(copy_to, prop, mod_info);
            if !replaced {
                mod_append_arr(copy_to, prop, mod_info);
            }
        }
        "insertArr" => mod_insert_arr(copy_to, prop, mod_info),
        "removeArr" => mod_remove_arr(copy_to, prop, mod_info),
        "setProp" => mod_set_prop(copy_to, prop, mod_info),
        "prefixSuffixStringProp" => mod_prefix_suffix_string_prop(copy_to, prop, mod_info),
        "scalarAddProp" => mod_scalar_prop(copy_to, prop, mod_info, false),
        "scalarMultProp" => mod_scalar_prop(copy_to, prop, mod_info, true),
        "scalarAddHit" => mod_scalar_add_tag(copy_to, prop, mod_info, "hit"),
        "scalarAddDc" => mod_scalar_add_tag(copy_to, prop, mod_info, "dc"),
        "addSenses" => mod_add_senses(copy_to, mod_info),
        "addSkills" => mod_add_skills(copy_to, mod_info),
        "addSpells" => mod_add_spells(copy_to, mod_info),
        "replaceSpells" => mod_replace_spells(copy_to, mod_info),
        "removeSpells" => mod_remove_spells(copy_to, mod_info),
        "maxSize" => mod_max_size(copy_to, mod_info),
        "scalarMultXp" => mod_scalar_mult_xp(copy_to, mod_info),
        // Any mode the real data never uses (e.g. calculateProp, addSaves) is a
        // deliberate no-op rather than a hard failure — the rest of the copy stands.
        _ => {}
    }
}

/// `modInfo.items`, coerced to a list (a lone object is wrapped).
fn mod_items(mod_info: &Value, key: &str) -> Vec<Value> {
    match mod_info.get(key) {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
        None => Vec::new(),
    }
}

fn array_at<'a>(copy_to: &'a mut Value, prop: &str) -> Option<&'a mut Vec<Value>> {
    copy_to.as_object_mut()?.get_mut(prop)?.as_array_mut()
}

fn mod_append_str(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let (Some(prop), Some(str_val)) = (prop, mod_info.get("str").and_then(Value::as_str)) else {
        return;
    };
    let joiner = mod_info.get("joiner").and_then(Value::as_str).unwrap_or("");
    let Some(obj) = copy_to.as_object_mut() else {
        return;
    };
    let next = match obj.get(prop).and_then(Value::as_str) {
        Some(existing) if !existing.is_empty() => format!("{existing}{joiner}{str_val}"),
        _ => str_val.to_string(),
    };
    obj.insert(prop.to_string(), Value::String(next));
}

fn mod_append_arr(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(prop) = prop else { return };
    let items = mod_items(mod_info, "items");
    push_into_array(copy_to, prop, items, false);
}

fn mod_prepend_arr(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(prop) = prop else { return };
    let items = mod_items(mod_info, "items");
    push_into_array(copy_to, prop, items, true);
}

fn push_into_array(copy_to: &mut Value, prop: &str, items: Vec<Value>, prepend: bool) {
    let Some(obj) = copy_to.as_object_mut() else {
        return;
    };
    let entry = obj.entry(prop.to_string()).or_insert_with(|| json!([]));
    if let Some(arr) = entry.as_array_mut() {
        if prepend {
            let mut combined = items;
            combined.append(arr);
            *arr = combined;
        } else {
            arr.extend(items);
        }
    }
}

fn mod_append_if_not_exists_arr(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(prop) = prop else { return };
    let items = mod_items(mod_info, "items");
    let Some(obj) = copy_to.as_object_mut() else {
        return;
    };
    let entry = obj.entry(prop.to_string()).or_insert_with(|| json!([]));
    if let Some(arr) = entry.as_array_mut() {
        for item in items {
            if !arr.iter().any(|existing| existing == &item) {
                arr.push(item);
            }
        }
    }
}

/// `replaceArr`: swap an existing item (matched by name, index, or regex) for the
/// mod's items. Returns whether a replacement happened.
fn mod_replace_arr(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) -> bool {
    let Some(prop) = prop else { return false };
    let items = mod_items(mod_info, "items");
    let Some(replace) = mod_info.get("replace") else {
        return false;
    };
    let Some(arr) = array_at(copy_to, prop) else {
        return false;
    };

    let index = match replace {
        Value::Object(spec) => {
            if let Some(regex) = spec.get("regex").and_then(Value::as_str) {
                let flags = spec.get("flags").and_then(Value::as_str).unwrap_or("");
                build_regex(regex, flags).and_then(|re| {
                    arr.iter().position(|item| match item.get("name") {
                        Some(Value::String(name)) => re.is_match(name),
                        _ => item.as_str().is_some_and(|s| re.is_match(s)),
                    })
                })
            } else {
                spec.get("index")
                    .and_then(Value::as_u64)
                    .map(|i| i as usize)
            }
        }
        Value::String(name) => arr.iter().position(|item| match item.get("name") {
            Some(Value::String(item_name)) => item_name == name,
            _ => item.as_str() == Some(name.as_str()),
        }),
        _ => None,
    };

    match index {
        Some(i) if i < arr.len() => {
            arr.splice(i..=i, items);
            true
        }
        _ => false,
    }
}

fn mod_insert_arr(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(prop) = prop else { return };
    let items = mod_items(mod_info, "items");
    // `index == -1` (or absent) means append at the end.
    let raw_index = mod_info.get("index").and_then(Value::as_i64).unwrap_or(-1);
    let Some(arr) = array_at(copy_to, prop) else {
        return;
    };
    let at = if raw_index < 0 {
        arr.len()
    } else {
        (raw_index as usize).min(arr.len())
    };
    arr.splice(at..at, items);
}

fn mod_remove_arr(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(prop) = prop else { return };
    let Some(arr) = array_at(copy_to, prop) else {
        return;
    };
    if mod_info.get("names").is_some() {
        for name in mod_items(mod_info, "names") {
            let Some(name) = name.as_str() else { continue };
            if let Some(i) = arr
                .iter()
                .position(|item| item.get("name").and_then(Value::as_str) == Some(name))
            {
                arr.remove(i);
            }
        }
    } else if mod_info.get("items").is_some() {
        for item in mod_items(mod_info, "items") {
            if let Some(i) = arr.iter().position(|existing| existing == &item) {
                arr.remove(i);
            }
        }
    }
}

fn mod_set_prop(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(value) = mod_info.get("value") else {
        return;
    };
    let mut path: Vec<String> = mod_info
        .get("prop")
        .and_then(Value::as_str)
        .map(|p| p.split('.').map(str::to_string).collect())
        .unwrap_or_default();
    // A non-wildcard target prop is prepended (5etools `propPathCombined`).
    if let Some(prop) = prop
        && prop != "*"
    {
        path.insert(0, prop.to_string());
    }
    if !path.is_empty() {
        set_path(copy_to, &path, value.clone());
    }
}

fn mod_prefix_suffix_string_prop(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let mut path: Vec<String> = mod_info
        .get("prop")
        .and_then(Value::as_str)
        .map(|p| p.split('.').map(str::to_string).collect())
        .unwrap_or_default();
    if let Some(prop) = prop
        && prop != "*"
    {
        path.insert(0, prop.to_string());
    }
    let prefix = mod_info.get("prefix").and_then(Value::as_str).unwrap_or("");
    let suffix = mod_info.get("suffix").and_then(Value::as_str).unwrap_or("");
    if let Some(Value::String(existing)) = get_path_mut(copy_to, &path) {
        *existing = format!("{prefix}{existing}{suffix}");
    }
}

/// `scalarAddProp` / `scalarMultProp`: add to (or multiply) numeric sub-values of
/// the object at `prop` — `modInfo.prop == "*"` hits every key, else just that one.
fn mod_scalar_prop(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value, mult: bool) {
    let Some(prop) = prop else { return };
    let scalar = mod_info
        .get("scalar")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let floor = mod_info
        .get("floor")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let target_key = mod_info.get("prop").and_then(Value::as_str).unwrap_or("*");
    let Some(target) = copy_to
        .as_object_mut()
        .and_then(|obj| obj.get_mut(prop))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let keys: Vec<String> = if target_key == "*" {
        target.keys().cloned().collect()
    } else {
        vec![target_key.to_string()]
    };
    for key in keys {
        let Some(slot) = target.get_mut(&key) else {
            continue;
        };
        let was_string = slot.is_string();
        let current = match slot {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::String(s) => s.trim_start_matches('+').parse::<f64>().unwrap_or(0.0),
            _ => continue,
        };
        let mut out = if mult {
            current * scalar
        } else {
            current + scalar
        };
        if floor || !mult {
            out = out.floor();
        }
        *slot = numeric_value(out, was_string);
    }
}

/// `scalarAddHit` / `scalarAddDc`: bump every `{@hit N}` / `{@dc N}` in the targeted
/// entries by the scalar (used by half-dragon and similar templates).
fn mod_scalar_add_tag(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value, tag: &str) {
    let Some(prop) = prop else { return };
    let scalar = mod_info.get("scalar").and_then(Value::as_i64).unwrap_or(0);
    let re = match tag {
        "hit" => Regex::new(r"\{@hit ([-+]?\d+)\}").unwrap(),
        _ => Regex::new(r"\{@dc (\d+)(?:\|[^}]+)?\}").unwrap(),
    };
    let tag = tag.to_string();
    let replacer = move |s: &str| -> String {
        re.replace_all(s, |caps: &regex::Captures| {
            let n: i64 = caps[1].parse().unwrap_or(0);
            format!("{{@{} {}}}", tag, n + scalar)
        })
        .into_owned()
    };
    if let Some(field) = copy_to.as_object_mut().and_then(|obj| obj.get_mut(prop)) {
        walk_strings(field, &replacer);
    }
}

fn mod_add_senses(copy_to: &mut Value, mod_info: &Value) {
    let senses = mod_items(mod_info, "senses");
    let Some(obj) = copy_to.as_object_mut() else {
        return;
    };
    let entry = obj.entry("senses".to_string()).or_insert_with(|| json!([]));
    let Some(arr) = entry.as_array_mut() else {
        return;
    };
    for sense in senses {
        let Some(kind) = sense.get("type").and_then(Value::as_str) else {
            continue;
        };
        let range = sense.get("range").and_then(Value::as_i64).unwrap_or(0);
        let kind_re = Regex::new(&format!(r"(?i){}\s+(\d+)", regex::escape(kind))).unwrap();
        let mut found = false;
        for existing in arr.iter_mut() {
            let Some(text) = existing.as_str() else {
                continue;
            };
            if let Some(caps) = kind_re.captures(text) {
                found = true;
                let current: i64 = caps[1].parse().unwrap_or(0);
                if current < range {
                    *existing = Value::String(format!("{} {} ft.", title_case(kind), range));
                }
                break;
            }
        }
        if !found {
            arr.push(Value::String(format!("{} {} ft.", title_case(kind), range)));
        }
    }
}

fn mod_add_skills(copy_to: &mut Value, mod_info: &Value) {
    let Some(skills) = mod_info.get("skills").and_then(Value::as_object) else {
        return;
    };
    let pb = pb_for_cr(copy_to.get("cr").unwrap_or(&Value::Null));
    let skill_pairs: Vec<(String, i64, i64)> = skills
        .iter()
        .filter_map(|(skill, mode)| {
            let mode = mode.as_i64()?;
            let ability = skill_ability(skill)?;
            let score = copy_to.get(ability).and_then(Value::as_i64).unwrap_or(10);
            Some((skill.clone(), mode, ability_modifier(score)))
        })
        .collect();
    let Some(obj) = copy_to.as_object_mut() else {
        return;
    };
    let entry = obj.entry("skill".to_string()).or_insert_with(|| json!({}));
    let Some(skill_obj) = entry.as_object_mut() else {
        return;
    };
    for (skill, mode, ability_mod) in skill_pairs {
        let total = mode * pb + ability_mod;
        let as_text = signed(total);
        let replace = match skill_obj.get(&skill).and_then(Value::as_str) {
            Some(existing) => {
                existing
                    .trim_start_matches('+')
                    .parse::<i64>()
                    .unwrap_or(i64::MIN)
                    < total
            }
            None => true,
        };
        if replace {
            skill_obj.insert(skill, Value::String(as_text));
        }
    }
}

// --- spellcasting mods (operate on `spellcasting[0]`, or a named block) ---

fn spellcasting_mut<'a>(copy_to: &'a mut Value, name: Option<&str>) -> Option<&'a mut Value> {
    let arr = copy_to
        .as_object_mut()?
        .get_mut("spellcasting")?
        .as_array_mut()?;
    match name {
        Some(name) => arr
            .iter_mut()
            .find(|block| block.get("name").and_then(Value::as_str) == Some(name)),
        None => arr.first_mut(),
    }
}

fn mod_add_spells(copy_to: &mut Value, mod_info: &Value) {
    let name = mod_info.get("name").and_then(Value::as_str);
    let Some(block) = spellcasting_mut(copy_to, name) else {
        return;
    };
    let Some(block) = block.as_object_mut() else {
        return;
    };

    // Slot-based spells: merge per level, concatenating spell arrays.
    if let Some(add) = mod_info.get("spells").and_then(Value::as_object) {
        let spells = block
            .entry("spells".to_string())
            .or_insert_with(|| json!({}));
        if let Some(spells) = spells.as_object_mut() {
            for (level, incoming) in add {
                match spells.get_mut(level) {
                    None => {
                        spells.insert(level.clone(), incoming.clone());
                    }
                    Some(existing) => merge_spell_level(existing, incoming),
                }
            }
        }
    }

    // Flat lists.
    for prop in ["constant", "will", "ritual"] {
        if let Some(items) = mod_info.get(prop).and_then(Value::as_array) {
            let entry = block.entry(prop.to_string()).or_insert_with(|| json!([]));
            if let Some(arr) = entry.as_array_mut() {
                arr.extend(items.clone());
            }
        }
    }

    // Bucketed lists (daily/recharge/...): bucket key -> spell array.
    for prop in [
        "recharge",
        "legendary",
        "charges",
        "rest",
        "restLong",
        "daily",
        "weekly",
        "monthly",
        "yearly",
    ] {
        if let Some(buckets) = mod_info.get(prop).and_then(Value::as_object) {
            let entry = block.entry(prop.to_string()).or_insert_with(|| json!({}));
            if let Some(target) = entry.as_object_mut() {
                for (bucket, items) in buckets {
                    let Some(items) = items.as_array() else {
                        continue;
                    };
                    let slot = target.entry(bucket.clone()).or_insert_with(|| json!([]));
                    if let Some(arr) = slot.as_array_mut() {
                        arr.extend(items.clone());
                    }
                }
            }
        }
    }
}

fn merge_spell_level(existing: &mut Value, incoming: &Value) {
    let (Some(existing), Some(incoming)) = (existing.as_object_mut(), incoming.as_object()) else {
        return;
    };
    for (key, value) in incoming {
        match existing.get_mut(key) {
            None => {
                existing.insert(key.clone(), value.clone());
            }
            Some(Value::Array(old)) => {
                if let Some(add) = value.as_array() {
                    old.extend(add.clone());
                    sort_string_array(old);
                }
            }
            Some(slot) => *slot = value.clone(),
        }
    }
}

fn mod_replace_spells(copy_to: &mut Value, mod_info: &Value) {
    let Some(block) = spellcasting_mut(copy_to, None) else {
        return;
    };
    let Some(block) = block.as_object_mut() else {
        return;
    };

    if let Some(by_level) = mod_info.get("spells").and_then(Value::as_object)
        && let Some(spells) = block.get_mut("spells").and_then(Value::as_object_mut)
    {
        for (level, replacements) in by_level {
            if let Some(level_obj) = spells.get_mut(level).and_then(Value::as_object_mut)
                && let Some(list) = level_obj.get_mut("spells").and_then(Value::as_array_mut)
            {
                apply_spell_replacements(list, replacements);
            }
        }
    }

    if let Some(daily) = mod_info.get("daily").and_then(Value::as_object)
        && let Some(target) = block.get_mut("daily").and_then(Value::as_object_mut)
    {
        for (bucket, replacements) in daily {
            if let Some(list) = target.get_mut(bucket).and_then(Value::as_array_mut) {
                apply_spell_replacements(list, replacements);
            }
        }
    }
}

fn apply_spell_replacements(list: &mut Vec<Value>, replacements: &Value) {
    let Some(metas) = replacements.as_array() else {
        return;
    };
    for meta in metas {
        let Some(replace) = meta.get("replace").and_then(Value::as_str) else {
            continue;
        };
        let with = match meta.get("with") {
            Some(Value::Array(items)) => items.clone(),
            Some(other) => vec![other.clone()],
            None => Vec::new(),
        };
        if let Some(i) = list.iter().position(|s| s.as_str() == Some(replace)) {
            list.splice(i..=i, with);
            sort_string_array(list);
        }
    }
}

fn mod_remove_spells(copy_to: &mut Value, mod_info: &Value) {
    let Some(block) = spellcasting_mut(copy_to, None) else {
        return;
    };
    let Some(block) = block.as_object_mut() else {
        return;
    };

    if let Some(by_level) = mod_info.get("spells").and_then(Value::as_object)
        && let Some(spells) = block.get_mut("spells").and_then(Value::as_object_mut)
    {
        for (level, names) in by_level {
            if let Some(list) = spells
                .get_mut(level)
                .and_then(|l| l.get_mut("spells"))
                .and_then(Value::as_array_mut)
            {
                retain_not_in(list, names);
            }
        }
    }

    for prop in ["constant", "will", "ritual"] {
        if let Some(names) = mod_info.get(prop)
            && let Some(list) = block.get_mut(prop).and_then(Value::as_array_mut)
        {
            retain_not_in(list, names);
        }
    }

    for prop in [
        "recharge",
        "legendary",
        "charges",
        "rest",
        "restLong",
        "daily",
        "weekly",
        "monthly",
        "yearly",
    ] {
        if let Some(buckets) = mod_info.get(prop).and_then(Value::as_object)
            && let Some(target) = block.get_mut(prop).and_then(Value::as_object_mut)
        {
            for (bucket, names) in buckets {
                if let Some(list) = target.get_mut(bucket).and_then(Value::as_array_mut) {
                    retain_not_in(list, names);
                }
            }
        }
    }
}

fn retain_not_in(list: &mut Vec<Value>, names: &Value) {
    let Some(names) = names.as_array() else {
        return;
    };
    list.retain(|item| !names.iter().any(|n| n == item));
}

fn mod_max_size(copy_to: &mut Value, mod_info: &Value) {
    const SIZE_ABVS: &[&str] = &["T", "S", "M", "L", "H", "G"];
    let Some(max) = mod_info.get("max").and_then(Value::as_str) else {
        return;
    };
    let Some(max_ix) = SIZE_ABVS.iter().position(|s| *s == max) else {
        return;
    };
    let Some(sizes) = copy_to
        .as_object_mut()
        .and_then(|obj| obj.get_mut("size"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    let mut kept: Vec<Value> = sizes
        .iter()
        .filter(|s| {
            s.as_str()
                .and_then(|s| SIZE_ABVS.iter().position(|x| *x == s))
                .is_some_and(|ix| ix <= max_ix)
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        kept.push(Value::String(max.to_string()));
    }
    *sizes = kept;
}

fn mod_scalar_mult_xp(copy_to: &mut Value, mod_info: &Value) {
    let scalar = mod_info
        .get("scalar")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let floor = mod_info
        .get("floor")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let scale = |xp: f64| -> i64 {
        let out = xp * scalar;
        if floor {
            out.floor() as i64
        } else {
            out as i64
        }
    };
    let Some(cr) = copy_to.as_object_mut().and_then(|obj| obj.get_mut("cr")) else {
        return;
    };
    match cr {
        Value::Object(map) => {
            if let Some(xp) = map.get("xp").and_then(Value::as_f64) {
                map.insert("xp".to_string(), json!(scale(xp)));
            }
        }
        other => {
            if let Some(xp) = cr_string_xp(other) {
                *other = json!({ "cr": other.clone(), "xp": scale(xp as f64) });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `replaceTxt` — tag-aware regex substitution over entry text
// ---------------------------------------------------------------------------

fn mod_replace_txt(copy_to: &mut Value, prop: Option<&str>, mod_info: &Value) {
    let Some(prop) = prop else { return };
    let Some(pattern) = mod_info.get("replace").and_then(Value::as_str) else {
        return;
    };
    let flags = mod_info.get("flags").and_then(Value::as_str).unwrap_or("");
    let Some(re) = build_regex(pattern, flags) else {
        return;
    };
    let with = mod_info.get("with").and_then(Value::as_str).unwrap_or("");
    let tag_insensitive = mod_info
        .get("tagInsensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Which props of each entry to rewrite. `null` in the list means "the entry is
    // itself a bare string" (e.g. a `legendaryHeader` line).
    let (has_null, named): (bool, Vec<String>) =
        match mod_info.get("props").and_then(Value::as_array) {
            Some(props) => {
                let has_null = props.iter().any(Value::is_null);
                let named = props
                    .iter()
                    .filter_map(|p| p.as_str().map(str::to_string))
                    .collect();
                (has_null, named)
            }
            None => (
                true,
                vec![
                    "entries".to_string(),
                    "headerEntries".to_string(),
                    "footerEntries".to_string(),
                ],
            ),
        };

    let replacer = |s: &str| tag_aware_replace(&re, with, tag_insensitive, s);
    let Some(arr) = array_at(copy_to, prop) else {
        return;
    };
    for ent in arr.iter_mut() {
        if has_null && ent.is_string() {
            let replaced = replacer(ent.as_str().unwrap());
            *ent = Value::String(replaced);
        }
        if let Some(obj) = ent.as_object_mut() {
            for field in &named {
                if let Some(value) = obj.get_mut(field) {
                    walk_strings(value, &replacer);
                }
            }
        }
    }
}

/// Apply `re`→`with` to a string, skipping `{@tag ...}` spans unless `tag_insensitive`
/// (mirrors 5etools `_doReplaceStringHandler`, so e.g. renaming "mage" never corrupts
/// `{@spell mage armor}`).
fn tag_aware_replace(re: &Regex, with: &str, tag_insensitive: bool, input: &str) -> String {
    if tag_insensitive {
        return regex_replace(re, with, input);
    }
    let mut out = String::with_capacity(input.len());
    for (is_tag, segment) in split_tags(input) {
        if is_tag {
            out.push_str(segment);
        } else {
            out.push_str(&regex_replace(re, with, segment));
        }
    }
    out
}

/// Split a string into alternating plain / `{@...}` tag segments (tags kept verbatim).
fn split_tags(input: &str) -> Vec<(bool, &str)> {
    let bytes = input.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'@'
            && let Some(rel) = input[i..].find('}')
        {
            if start < i {
                segments.push((false, &input[start..i]));
            }
            let end = i + rel + 1;
            segments.push((true, &input[i..end]));
            i = end;
            start = i;
            continue;
        }
        i += 1;
    }
    if start < input.len() {
        segments.push((false, &input[start..]));
    }
    segments
}

/// `re.replace_all` with JS-style `$1` / `$&` expansion in the replacement (Rust's
/// native `$name` interpolation would misread `$1goblin` as a group named `1goblin`).
fn regex_replace(re: &Regex, with: &str, input: &str) -> String {
    re.replace_all(input, |caps: &regex::Captures| {
        expand_replacement(with, caps)
    })
    .into_owned()
}

fn expand_replacement(template: &str, caps: &regex::Captures) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '$' {
                out.push('$');
                i += 2;
                continue;
            }
            if next == '&' {
                out.push_str(caps.get(0).map_or("", |m| m.as_str()));
                i += 2;
                continue;
            }
            if next.is_ascii_digit() {
                // Greedy up to two digits, falling back to one (JS semantics).
                let mut digits = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() && digits.len() < 2 {
                    digits.push(chars[j]);
                    j += 1;
                }
                if digits.len() == 2 {
                    if let Some(m) = digits.parse::<usize>().ok().and_then(|g| caps.get(g)) {
                        out.push_str(m.as_str());
                        i = j;
                        continue;
                    }
                    // Two-digit group missing: try the first digit, leave the rest literal.
                    if let Some(m) = digits[..1].parse::<usize>().ok().and_then(|g| caps.get(g)) {
                        out.push_str(m.as_str());
                        out.push(digits.as_bytes()[1] as char);
                        i = j;
                        continue;
                    }
                } else if let Some(m) = digits.parse::<usize>().ok().and_then(|g| caps.get(g)) {
                    out.push_str(m.as_str());
                    i = j;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Small shared helpers
// ---------------------------------------------------------------------------

/// Recursively apply `f` to every string node within a JSON value.
fn walk_strings(value: &mut Value, f: &dyn Fn(&str) -> String) {
    match value {
        Value::String(s) => *s = f(s),
        Value::Array(arr) => arr.iter_mut().for_each(|v| walk_strings(v, f)),
        Value::Object(map) => map.iter_mut().for_each(|(_, v)| walk_strings(v, f)),
        _ => {}
    }
}

fn build_regex(pattern: &str, flags: &str) -> Option<Regex> {
    let mut prefix = String::new();
    if flags.contains('i') {
        prefix.push_str("(?i)");
    }
    if flags.contains('m') {
        prefix.push_str("(?m)");
    }
    if flags.contains('s') {
        prefix.push_str("(?s)");
    }
    Regex::new(&format!("{prefix}{pattern}")).ok()
}

/// Navigate `path` (object keys), returning a mutable ref to the leaf if present.
fn get_path_mut<'a>(value: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let mut current = value;
    for key in path {
        current = current.as_object_mut()?.get_mut(key)?;
    }
    Some(current)
}

/// Set `path` (object keys), creating intermediate objects as needed.
fn set_path(value: &mut Value, path: &[String], new_value: Value) {
    let mut current = value;
    for key in &path[..path.len() - 1] {
        if !current.is_object() {
            *current = json!({});
        }
        current = current
            .as_object_mut()
            .unwrap()
            .entry(key.clone())
            .or_insert_with(|| json!({}));
    }
    if !current.is_object() {
        *current = json!({});
    }
    current
        .as_object_mut()
        .unwrap()
        .insert(path[path.len() - 1].clone(), new_value);
}

fn numeric_value(out: f64, as_string: bool) -> Value {
    if as_string {
        let n = out as i64;
        Value::String(if n >= 0 {
            format!("+{n}")
        } else {
            n.to_string()
        })
    } else if out.fract() == 0.0 {
        json!(out as i64)
    } else {
        json!(out)
    }
}

fn sort_string_array(arr: &mut [Value]) {
    arr.sort_by(|a, b| {
        a.as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .cmp(&b.as_str().unwrap_or("").to_ascii_lowercase())
    });
}

fn ability_modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

fn signed(value: i64) -> String {
    if value >= 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Proficiency bonus for a creature's CR (string or `{cr}` object), matching the
/// bands in [`crate::monster_import`].
fn pb_for_cr(cr: &Value) -> i64 {
    let numeric = crate::monster_import::cr_to_sort(cr);
    match numeric {
        n if n < 5.0 => 2,
        n if n < 9.0 => 3,
        n if n < 13.0 => 4,
        n if n < 17.0 => 5,
        n if n < 21.0 => 6,
        n if n < 25.0 => 7,
        n if n < 29.0 => 8,
        _ => 9,
    }
}

/// Standard 5e skill → governing ability abbreviation (lowercase, as the data keys
/// abilities). Used by `addSkills` to compute a save-style bonus.
fn skill_ability(skill: &str) -> Option<&'static str> {
    Some(match skill.to_ascii_lowercase().as_str() {
        "athletics" => "str",
        "acrobatics" | "sleight of hand" | "stealth" => "dex",
        "arcana" | "history" | "investigation" | "nature" | "religion" => "int",
        "animal handling" | "insight" | "medicine" | "perception" | "survival" => "wis",
        "deception" | "intimidation" | "performance" | "persuasion" => "cha",
        _ => return None,
    })
}

/// Base XP for a plain CR string (used only by the rare `scalarMultXp`).
fn cr_string_xp(cr: &Value) -> Option<u32> {
    let cr = cr.as_str()?;
    Some(match cr {
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
        _ => cr.parse::<u32>().ok().map(|n| n * 1000)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(monsters: Vec<Value>, templates: Vec<Value>) -> CopyResolution {
        resolve_copies(monsters, templates)
    }

    fn by_name<'a>(resolution: &'a CopyResolution, name: &str) -> &'a Value {
        resolution
            .monsters
            .iter()
            .find(|m| m.get("name").and_then(Value::as_str) == Some(name))
            .unwrap_or_else(|| panic!("monster {name} missing from resolution"))
    }

    #[test]
    fn pure_override_inherits_base_and_overrides_named_fields() {
        let base = json!({
            "name": "Giant Centipede", "source": "MM",
            "size": ["S"], "type": "beast", "cr": "1/4",
            "ac": [13], "hp": {"average": 4, "formula": "1d6 + 1"},
            "str": 5, "dex": 14, "con": 12, "int": 1, "wis": 7, "cha": 3,
            "action": [{"name": "Bite", "entries": ["The centipede bites."]}]
        });
        let copy = json!({
            "name": "Hellwasp Grub", "source": "TFTYP",
            "_copy": {"name": "Giant Centipede", "source": "MM"}
        });
        let resolution = resolve(vec![base, copy], vec![]);
        assert_eq!(resolution.resolved_copy, 1);
        assert_eq!(resolution.skipped_copy, 0);
        let grub = by_name(&resolution, "Hellwasp Grub");
        // Name/source from the variant; everything else inherited.
        assert_eq!(grub["source"], json!("TFTYP"));
        assert_eq!(grub["cr"], json!("1/4"));
        assert_eq!(grub["action"][0]["name"], json!("Bite"));
        // `_copy` is consumed.
        assert!(grub.get("_copy").is_none());
    }

    #[test]
    fn missing_base_is_skipped_and_counted() {
        let copy = json!({
            "name": "Skeleton Knight", "source": "MM",
            "_copy": {"name": "Skeleton", "source": "MM"}, "cr": "1"
        });
        let resolution = resolve(vec![copy], vec![]);
        assert_eq!(resolution.resolved_copy, 0);
        assert_eq!(resolution.skipped_copy, 1);
        assert!(resolution.monsters.is_empty());
    }

    #[test]
    fn replace_txt_rewrites_text_but_skips_tags() {
        let base = json!({
            "name": "Devil", "source": "MM",
            "action": [{"name": "Claw", "entries": [
                "The devil claws. The devil also casts {@spell devil's sight}."
            ]}]
        });
        let copy = json!({
            "name": "Bitter Breath", "source": "BGDIA",
            "_copy": {
                "name": "Devil", "source": "MM",
                "_mod": {"*": {"mode": "replaceTxt", "replace": "the devil", "with": "Bitter Breath", "flags": "i"}}
            }
        });
        let resolution = resolve(vec![base, copy], vec![]);
        let entry = by_name(&resolution, "Bitter Breath")["action"][0]["entries"][0]
            .as_str()
            .unwrap();
        // Prose rewritten (case-insensitive)...
        assert!(entry.starts_with("Bitter Breath claws. Bitter Breath also casts"));
        // ...but the tag content is untouched.
        assert!(entry.contains("{@spell devil's sight}"));
    }

    #[test]
    fn replace_txt_supports_group_refs() {
        let base = json!({
            "name": "Mage", "source": "MM",
            "trait": [{"name": "About", "entries": ["A mage stands here."]}]
        });
        let copy = json!({
            "name": "Booyahg", "source": "VGM",
            "_copy": {
                "name": "Mage", "source": "MM",
                "_mod": {"trait": {"mode": "replaceTxt", "replace": "(^| )mage", "with": "$1goblin"}}
            }
        });
        let resolution = resolve(vec![base, copy], vec![]);
        let entry = by_name(&resolution, "Booyahg")["trait"][0]["entries"][0]
            .as_str()
            .unwrap();
        assert_eq!(entry, "A goblin stands here.");
    }

    #[test]
    fn array_mods_append_replace_remove_insert() {
        let base = json!({
            "name": "Knight", "source": "MM",
            "trait": [
                {"name": "Bound", "entries": ["Bound."]},
                {"name": "Brave", "entries": ["Brave."]}
            ],
            "action": [{"name": "Multiattack", "entries": ["Two attacks."]}]
        });
        let copy = json!({
            "name": "Knight of the Order", "source": "SKT",
            "_copy": {
                "name": "Knight", "source": "MM",
                "_mod": {
                    "trait": {"mode": "removeArr", "names": "Bound"},
                    "action": [
                        {"mode": "replaceArr", "replace": "Multiattack",
                         "items": {"name": "Multiattack", "entries": ["Three attacks."]}},
                        {"mode": "insertArr", "index": 1,
                         "items": {"name": "Longsword", "entries": ["Slash."]}}
                    ]
                }
            }
        });
        let resolution = resolve(vec![base, copy], vec![]);
        let knight = by_name(&resolution, "Knight of the Order");
        // Bound removed, Brave kept.
        let traits = knight["trait"].as_array().unwrap();
        assert_eq!(traits.len(), 1);
        assert_eq!(traits[0]["name"], json!("Brave"));
        // Multiattack replaced, Longsword inserted at index 1.
        let actions = knight["action"].as_array().unwrap();
        assert_eq!(actions[0]["entries"][0], json!("Three attacks."));
        assert_eq!(actions[1]["name"], json!("Longsword"));
    }

    #[test]
    fn append_if_not_exists_dedups() {
        let base = json!({"name": "Orc", "source": "MM", "languages": ["Common", "Orc"]});
        let copy = json!({
            "name": "Blurg", "source": "WDH",
            "_copy": {
                "name": "Orc", "source": "MM",
                "_mod": {"languages": {"mode": "appendIfNotExistsArr", "items": ["Orc", "Dwarvish"]}}
            }
        });
        let resolution = resolve(vec![base, copy], vec![]);
        let langs = by_name(&resolution, "Blurg")["languages"]
            .as_array()
            .unwrap();
        // "Orc" already present (not duplicated); "Dwarvish" added.
        assert_eq!(
            langs.iter().filter(|l| l.as_str() == Some("Orc")).count(),
            1
        );
        assert!(langs.iter().any(|l| l.as_str() == Some("Dwarvish")));
    }

    #[test]
    fn explicit_null_deletes_inherited_field() {
        let base =
            json!({"name": "Skeleton", "source": "MM", "vulnerable": ["bludgeoning"], "cr": "1/4"});
        let copy = json!({
            "name": "Cat Skeleton", "source": "PaBTSO",
            "_copy": {
                "name": "Skeleton", "source": "MM",
                "_mod": {"_": {"mode": "setProp", "prop": "vulnerable", "value": null}}
            }
        });
        let resolution = resolve(vec![base, copy], vec![]);
        let cat = by_name(&resolution, "Cat Skeleton");
        assert!(cat.get("vulnerable").map(Value::is_null).unwrap_or(true));
    }

    #[test]
    fn recursive_copy_chain_resolves() {
        let a = json!({"name": "A", "source": "X", "cr": "1", "hp": {"average": 10}});
        let b =
            json!({"name": "B", "source": "X", "_copy": {"name": "A", "source": "X"}, "cr": "2"});
        let c = json!({"name": "C", "source": "X", "_copy": {"name": "B", "source": "X"}});
        let resolution = resolve(vec![a, b, c], vec![]);
        assert_eq!(resolution.resolved_copy, 2);
        let resolved_c = by_name(&resolution, "C");
        // C inherits CR via B, and HP via A.
        assert_eq!(resolved_c["cr"], json!("2"));
        assert_eq!(resolved_c["hp"]["average"], json!(10));
    }

    #[test]
    fn cyclic_copy_is_skipped() {
        let x = json!({"name": "X", "source": "S", "_copy": {"name": "Y", "source": "S"}});
        let y = json!({"name": "Y", "source": "S", "_copy": {"name": "X", "source": "S"}});
        let resolution = resolve(vec![x, y], vec![]);
        assert_eq!(resolution.resolved_copy, 0);
        assert_eq!(resolution.skipped_copy, 2);
    }

    #[test]
    fn template_applies_root_and_mods() {
        let base = json!({
            "name": "Archmage", "source": "MM",
            "size": ["M"], "type": "humanoid",
            "str": 10, "dex": 14, "con": 12, "int": 20, "wis": 15, "cha": 16,
            "resist": ["psychic"], "languages": ["Common"], "senses": [], "cr": "12"
        });
        let template = json!({
            "name": "Tiefling", "source": "PHB",
            "apply": {
                "_root": {"type": {"type": "humanoid", "tags": ["tiefling"]}},
                "_mod": {
                    "_": {"mode": "addSenses", "senses": {"type": "darkvision", "range": 60}},
                    "resist": {"mode": "appendIfNotExistsArr", "items": "fire"},
                    "languages": {"mode": "appendIfNotExistsArr", "items": "Infernal"}
                }
            }
        });
        let copy = json!({
            "name": "Sylvira Savikas", "source": "BMT",
            "_copy": {
                "name": "Archmage", "source": "MM",
                "_templates": [{"name": "Tiefling", "source": "PHB"}],
                "_mod": {"_": {"mode": "addSkills", "skills": {"investigation": 2}}}
            }
        });
        let resolution = resolve(vec![base, copy], vec![template]);
        let sylvira = by_name(&resolution, "Sylvira Savikas");
        // Template _root overrode the base-inherited type with the tiefling tag.
        assert_eq!(sylvira["type"]["tags"][0], json!("tiefling"));
        // Template mods: darkvision added, fire resistance + Infernal appended.
        assert!(
            sylvira["senses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s.as_str() == Some("Darkvision 60 ft."))
        );
        assert!(
            sylvira["resist"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r.as_str() == Some("fire"))
        );
        // Copy's own addSkills: investigation = expertise(2)*PB(4 at CR12) + int mod(+5) = 13.
        assert_eq!(sylvira["skill"]["investigation"], json!("+13"));
    }

    #[test]
    fn template_from_template_resolves() {
        // Hill Dwarf copies Mountain Dwarf and rewrites apply._root.type.
        let mountain = json!({
            "name": "Mountain Dwarf", "source": "PHB",
            "apply": {"_root": {"size": ["M"], "type": {"type": "humanoid", "tags": ["dwarf"]}}}
        });
        let hill = json!({
            "name": "Hill Dwarf", "source": "PHB",
            "_copy": {
                "name": "Mountain Dwarf", "source": "PHB",
                "_mod": {"_": {"mode": "setProp", "prop": "apply._root.type",
                               "value": {"type": "humanoid", "tags": [{"tag": "dwarf", "prefix": "Hill"}]}}}
            }
        });
        let base = json!({"name": "Guard", "source": "MM", "size": ["M"], "cr": "1/8"});
        let copy = json!({
            "name": "Hill Dwarf Guard", "source": "HOTDQ",
            "_copy": {"name": "Guard", "source": "MM", "_templates": [{"name": "Hill Dwarf", "source": "PHB"}]}
        });
        let resolution = resolve(vec![base, copy], vec![mountain, hill]);
        let guard = by_name(&resolution, "Hill Dwarf Guard");
        // The Hill Dwarf template (itself a copy) resolved and applied its root type.
        assert_eq!(guard["type"]["tags"][0]["prefix"], json!("Hill"));
    }

    #[test]
    fn add_and_replace_spells() {
        let base = json!({
            "name": "Mage", "source": "MM",
            "spellcasting": [{
                "name": "Spellcasting", "type": "spellcasting",
                "spells": {
                    "5": {"slots": 1, "spells": ["{@spell cone of cold}"]},
                    "7": {"slots": 1, "spells": ["{@spell teleport}"]}
                }
            }]
        });
        let copy = json!({
            "name": "Traxigor", "source": "WDH",
            "_copy": {
                "name": "Mage", "source": "MM",
                "_mod": {"_": [
                    {"mode": "addSpells", "spells": {"5": {"spells": ["{@spell conjure elemental}"]}}},
                    {"mode": "replaceSpells", "spells": {"7": [{"replace": "{@spell teleport}", "with": "{@spell plane shift}"}]}}
                ]}
            }
        });
        let resolution = resolve(vec![base, copy], vec![]);
        let block = &by_name(&resolution, "Traxigor")["spellcasting"][0];
        let l5 = block["spells"]["5"]["spells"].as_array().unwrap();
        assert!(
            l5.iter()
                .any(|s| s.as_str() == Some("{@spell conjure elemental}"))
        );
        assert!(
            l5.iter()
                .any(|s| s.as_str() == Some("{@spell cone of cold}"))
        );
        let l7 = block["spells"]["7"]["spells"].as_array().unwrap();
        assert!(
            l7.iter()
                .any(|s| s.as_str() == Some("{@spell plane shift}"))
        );
        assert!(!l7.iter().any(|s| s.as_str() == Some("{@spell teleport}")));
    }

    #[test]
    fn preserve_controls_legendary_group_inheritance() {
        let base = json!({
            "name": "Dragon", "source": "MM", "cr": "10",
            "legendaryGroup": {"name": "Dragon", "source": "MM"}
        });
        // Without _preserve, legendaryGroup is NOT inherited.
        let plain = json!({
            "name": "Plain Wyrm", "source": "X",
            "_copy": {"name": "Dragon", "source": "MM"}
        });
        // With _preserve, it is.
        let preserved = json!({
            "name": "Kept Wyrm", "source": "X",
            "_copy": {"name": "Dragon", "source": "MM", "_preserve": {"legendaryGroup": true}}
        });
        let resolution = resolve(vec![base, plain, preserved], vec![]);
        assert!(
            by_name(&resolution, "Plain Wyrm")
                .get("legendaryGroup")
                .is_none()
        );
        assert!(
            by_name(&resolution, "Kept Wyrm")
                .get("legendaryGroup")
                .is_some()
        );
    }
}
