# Spellbook Feature — Implementation Plan

> **Status:** ✅ Implemented (2026-06-20). The sections below are the original build plan;
> see **§0 As-built notes** for where the shipped code deviates from it.
> **Purpose:** A self-contained build plan for the integrated spell lookup feature. Read
> `docs/architecture.md` first — this plan follows its §8A "Add New Top-Level Command"
> playbook and the `EntityStore` → `VaultSyncService` → `suggestions.rs` grain.

---

## 0. As-built notes (what shipped, and deviations from the plan)

The feature is implemented across these files:

- **Model + card:** `runebound-models/src/spells.rs` — `Spell`, `SpellBlock`, `spell_card`.
- **Converter:** `core/src/spell_import.rs` — `import_spells_from_dir`, `strip_tags`, `slugify`.
- **TOML store:** `core/src/spell_store.rs` (+ `spells` path in `core/src/config.rs`).
- **Search index:** migration `core/migrations/0020_spells.sql`, `SpellRow` + `impl_entity_table!`
  + `clear_spells`/`count_spells` in `core/src/db.rs`.
- **Repository:** `SpellRepository`/`ProdSpellRepository` in `desktop/.../repositories/mod.rs`;
  `AppState::spell_repo()`.
- **Import orchestration:** `desktop/.../services/spell_library.rs`.
- **Commands:** `desktop/.../commands/spell_commands.rs` (`handle_spell`, `handle_spellbook`,
  `resolve_spell_doc`); registered in `commands/mod.rs`; manifest specs in `command-specs/src/lib.rs`.
- **Bare-name lookup:** fallback in `desktop/.../router.rs` (after entity resolution).
- **Typeahead:** `SuggestionHelperText::Spell` + `spell_search_context` in `services/suggestions.rs`;
  regenerated `desktop/src/generated/manifest.ts`.
- **Boot self-heal:** `SpellLibraryService::project_store_into_db` called from the `cleanup`
  boot task in `desktop/.../boot.rs`.

Deviations from the plan below, all deliberate:

1. **Command surface.** Two roots, not a single `spell` root: **`spellbook import [path]`** (import;
   opens a native folder picker when no path is given) and **`spell <name>`** (lookup). A **bare
   spell name** (`Fireball`, no prefix) also renders the card via the router fallback — entities win
   on a name collision (resolved first).
2. **`SpellBlock` is flat, not recursive.** Named subsections lower to a `Heading` block followed by
   their flattened children (rather than a nested `Subsection { body }`). This keeps the whole
   `Spell` serializing cleanly to TOML and makes the card builder a 1:1 map.
3. **`Spell`/`SpellBlock` are backend-only** (no `TS` derive) — only the rendered `OutputDoc` crosses
   to the frontend, mirroring the `*Frontmatter` types. No `models.ts` change was needed.
4. **Dedup rule + count.** Drop any entry with a `reprintedAs` field, then prefer `XPHB` by name →
   **554** canonical spells (verified against the real dataset), with zero slug/name collisions.
5. **No class lists.** The 2024 core data carries no `classes.fromClassList`, so the card's class
   line is usually omitted. The field is still parsed defensively.
6. **Boot re-projection.** The `cleanup` boot task re-projects the TOML store into the `spells`
   table (no-op when already in sync) so a deleted/rebuilt `app.db` self-heals — matching the
   entity-store grain.
7. **One card, not a block sequence** (revised from §6's plan). The whole spell renders as a
   single `OutputBlock::EntityCard`: the **name** is the card title, the level/school line is a new
   **`subtitle`** (so there is no separate heading bar above the card), and the description +
   higher-level scaling + source footer render in a new **`body: Vec<OutputBlock>`** *inside* the
   card. This required adding `subtitle` and `body` to `EntityCard` (both `#[serde(default)]`;
   `entity_card()` stays the bare stat-card helper, `entity_card_full()` builds the rich one), and
   teaching the frontend renderer + `to_plain_text` to walk them. `spellbook help` is one line plus
   usage; a successful import reports only the count.

---

## 1. What we're building

A read-only spell reference inside the TUI:

- `spell <fragment>` — typing `spell fire` surfaces a typeahead suggestion **Fireball**;
  Tab completes to `spell Fireball`; executing renders a **spell card** (stat block +
  description + higher-level scaling).
- `spell import <path>` — points at a **local copy of the 5etools data** (the user supplies
  the path; nothing copyrighted ships in this repo), reads every official spell, converts it
  into our own model, and stores it locally for search + rendering.

### Decisions already locked (do not re-litigate)

| Decision | Choice | Rationale |
|---|---|---|
| **Ship spell data in-repo?** | **No.** Import from a user-supplied path at runtime. | 5etools spell text is largely WotC-copyrighted; this repo has a public remote. Importing from the user's own local copy sidesteps redistribution entirely. No SRD filtering needed — it's the user's data on their machine. |
| **Feature scope** | **Lookup + render only.** | No "my spellbook" collection, no add/remove/list, no per-user curation. The imported set *is* the searchable library. |
| **Invocation** | `spell <fragment>` (lookup), `spell import <path>` (import). | User-chosen. Single `spell` root, `import` subcommand. |
| **Search store** | **SQLite `spells` table = search index; per-spell TOML = card payload.** | Mirrors the existing `EntityStore` (canonical TOML) → DB-projection → `suggestions.rs` typeahead path. The user explicitly wanted TOML-for-card + SQLite-for-search; it also matches the codebase grain. |
| **Edition** | **2024 / 5.5e canonical**, deduped (prefer `XPHB` over `PHB`), all official sources, no homebrew. | The user's "all extended official content, 2024 edition." |

---

## 2. Verified facts about the 5etools spell data

Source layout (under the path the user imports, e.g. `<5etools>/data/spells/`):

- `index.json` maps **source code → filename**, e.g. `{"PHB":"spells-phb.json","XPHB":"spells-xphb.json", …}`.
  17 source files total. **Read these and only these** — `homebrew/` is a sibling dir we ignore.
- Each `spells-*.json` is `{ "spell": [ {…}, {…} ] }`.

**Counts (measured):** 936 spell objects, **557 unique names**. `XPHB` (2024 PHB) = 391;
`PHB` (2014) = 361; supplements (`XGE` 95, `TCE` 21, `FRHoF` 19, `EGW` 15, …) make up the rest.
Only **2** PHB-2014 names are absent from XPHB; **166** unique names live only outside XPHB.
Deduped 2024-canonical corpus = **~557 spells**.

**No `edition` field exists.** Edition is inferred from `source` (`XPHB` = 2024 core,
`PHB` = 2014) and the flags `srd52` / `basicRules2024` (2024) vs `srd` / `basicRules` (2014).
We don't need these flags for import-from-path (no licensing filter), but the **dedup** does
need `reprintedAs`.

### Spell object schema (fields we consume)

```jsonc
{
  "name": "Fireball",
  "source": "XPHB",
  "level": 3,                       // 0 = cantrip … 9
  "school": "V",                    // single letter, see map below
  "time":  [{ "number": 1, "unit": "action", "condition": "…?" }],
  "range": { "type": "point", "distance": { "type": "feet", "amount": 150 } },
  "components": { "v": true, "s": true,
                  "m": "a tiny ball of bat guano and sulfur" },   // m may be string OR
                  // { "text": "a diamond worth 50+ gp", "cost": 5000, "consume": false }
  "duration": [{ "type": "instant" }],                            // or {"type":"timed",
                  // "duration":{"type":"minute","amount":1},"concentration":true,"upTo":true}
  "meta": { "ritual": true },                                      // optional
  "classes": { "fromClassList": [{ "name": "Wizard", "source": "XPHB" }], … },  // optional
  "entries": [ "string with {@tags}", { "type":"list", … }, { "type":"entries", … } ],
  "entriesHigherLevel": [{ "type":"entries", "name":"Using a Higher-Level Spell Slot",
                           "entries": ["…"] }],
  "scalingLevelDice": { "label":"fire damage", "scaling": {"1":"1d10","5":"2d10",…} }, // cantrips
  "reprintedAs": ["Fireball|XPHB"],   // present on the OLD entry that was superseded
  "miscTags": ["…"], "areaTags": ["…"], "savingThrow": ["dexterity"], "damageInflict": ["fire"]
}
```

**School letter → name:** `A`=Abjuration, `C`=Conjuration, `D`=Divination, `E`=Enchantment,
`V`=Evocation, `I`=Illusion, `N`=Necromancy, `T`=Transmutation.

**Fluff files** (`fluff-spells-*.json`) hold **artwork only**, not text — **ignore them**.
`entries` + `entriesHigherLevel` contain the full card body.

### The `entries` rich-text structure

`entries` is `(string | object)[]`. Objects we must handle:

- `{ "type": "list", "items": [ "str" | { "type":"item", "name":"…", "entries":[…] } ] }`
- `{ "type": "entries", "name": "Subsection", "entries": [ … ] }` (named option, e.g. *Alter Self*)
- `{ "type": "table", "colLabels": [...], "rows": [[...]] }`

Strings carry inline markup tags `{@tag display|args}`. Common tags and how to render them
(**v1: render the human-visible text, drop the link target**):

| Tag | Example | v1 render |
|---|---|---|
| `{@damage X}` / `{@dice X}` / `{@scaledamage …}` | `{@damage 8d6}` | `8d6` |
| `{@condition X}` / `{@status X}` | `{@condition prone}` | `prone` |
| `{@spell X}` / `{@creature X}` / `{@item X}` | `{@spell fireball}` | `fireball` |
| `{@variantrule X\|SRC\|disp}` | `{@variantrule Cover\|XPHB}` | `Cover` (use the display alias if present) |
| `{@dc N}` | `{@dc 15}` | `DC 15` |
| anything else `{@x a\|b\|c}` | — | first `\|`-segment after the tag name |

**Tag-stripping rule:** `{@tagname DISPLAY|arg|arg}` → `DISPLAY` (the substring up to the first
`|`), except `{@dc N}` → `DC N`. A single regex pass `\{@\w+ ([^|}]+)(?:\|[^}]*)?\}` → `$1`
covers the vast majority; special-case `{@dc}`. Keep this in one well-tested function
(`strip_tags`) — it is the only fiddly parsing in the feature.

> **Phase 2 (optional, not v1):** convert `{@spell fireball}` into a clickable
> `InlineNode::CommandRef { label: "fireball", command: "spell fireball" }` so spell cards
> cross-link. The data model below already supports inline nodes, so this is additive.

---

## 3. Data model (`runebound-models`)

`core` depends on `runebound-models`, so the converter (in `core`) can produce these types and
the desktop card renderer can consume them. **Model first** (architecture §7): add the model,
derive `TS`, regenerate `models.ts`.

New file `runebound-models/src/spells.rs`, exported from `lib.rs` (`pub mod spells; pub use spells::*;`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Spell {
    pub slug: String,          // kebab name + source disambiguation, e.g. "fireball"
    pub name: String,
    pub source: String,        // "XPHB", "TCE", … (kept for provenance/footer)
    pub level: u8,             // 0 = cantrip
    pub school: String,        // expanded, e.g. "Evocation"
    pub casting_time: String,  // "1 Action", "1 Bonus Action, when …"
    pub range: String,         // "150 feet", "Self (15-foot cone)", "Touch"
    pub components: String,    // "V, S, M (a tiny ball of bat guano …)"
    pub duration: String,      // "Instantaneous", "Concentration, up to 1 minute"
    pub ritual: bool,
    pub concentration: bool,
    pub classes: Vec<String>,  // ["Sorcerer","Wizard"] (may be empty)
    pub description: Vec<SpellBlock>,        // the spell body
    pub higher_levels: Option<Vec<SpellBlock>>, // "Using a Higher-Level Spell Slot" / cantrip scaling
}

/// A pre-flattened, render-ready body element. The converter lowers 5etools `entries`
/// into this; the card builder lowers this into `OutputBlock`s. Tags already stripped.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpellBlock {
    Text { text: String },
    Subsection { title: String, body: Vec<SpellBlock> },   // named option ("Aquatic Adaptation")
    Bullets { items: Vec<String> },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
}
```

`SpellBlock` exists so the persisted card data stays decoupled from the `OutputBlock`
rendering contract (architecture §9: no storing rendering types). Anything in `entries` we
don't recognize collapses to `SpellBlock::Text` via `strip_tags` — never drop content.

### Card rendering (`spell_card`)

> **As-built differs — see §0 #7.** The shipped card is a *single* `EntityCard` (name as title,
> level/school as `subtitle`, body inside `body`), not the loose block sequence sketched below. The
> sketch is kept for the lowering logic (`push_spell_block`, `level_school_line`), which is unchanged.

Add `pub fn spell_card(spell: &Spell) -> OutputDoc` next to `npc_entity_card` in
`runebound-models/src/drafts.rs` (or in `spells.rs`). **A spell card is a sequence of blocks**,
because `OutputBlock::EntityCard` rows are label/value only:

```rust
pub fn spell_card(spell: &Spell) -> OutputDoc {
    let mut out = doc();
    out.push(heading(2, spell.name.clone()));
    // Stat block as a label/value EntityCard:
    out.push(entity_card(&level_school_line(spell), vec![           // e.g. "Level 3 Evocation"
        entity_row("Casting Time:", &spell.casting_time),
        entity_row("Range:",        &spell.range),
        entity_row("Components:",    &spell.components),
        entity_row("Duration:",      &spell.duration),
    ]));
    // Body: lower each SpellBlock to OutputBlock(s)
    for block in &spell.description { push_spell_block(&mut out, block); }
    if let Some(hl) = &spell.higher_levels {
        out.push(heading(3, "Using a Higher-Level Spell Slot".into()));
        for block in hl { push_spell_block(&mut out, block); }
    }
    out
}
```

`push_spell_block`: `Text`→`paragraph_text`; `Bullets`→`list(items as single-text inlines)`;
`Subsection`→`heading(3, title)` + recurse; `Table`→render as a `Code` block (fixed-width) or a
markdown-ish paragraph (no `OutputBlock::Table` exists — keep tables simple, most spell tables
are tiny). `level_school_line` handles cantrip wording ("Evocation Cantrip" vs "Level 3 Evocation"),
appends "(Ritual)" when `spell.ritual`.

After adding the model: regenerate TS via `UPDATE_MODELS=1 cargo test -p runebound-models`
(per architecture §8C step 2) and consume from the frontend's generated `models.ts`.

---

## 4. Storage architecture

Two layers, mirroring `EntityStore` (canonical TOML) → DB projection:

### 4a. Spell store (TOML, card payload) — mirror `EntityStore`

New `core/src/spell_store.rs`, modeled on `core/src/entity_store.rs`:

- Root: a new `spells/` dir under the app data/config dir (extend `config_paths()` in
  `core/src/config.rs` with a `spells: PathBuf`, or reuse the data dir — match how
  `EntityStore` resolves `paths.entities`).
- `save_spell(&Spell) -> path` writes `spells/<slug>.toml` (`toml::to_string`, serde already derived).
- `load_spell(slug) -> Option<Spell>` reads + parses it.
- `clear()` / `list_slugs()` for re-import and projection.

### 4b. Spell search index (SQLite)

Migration `core/migrations/0020_spells.sql`:

```sql
CREATE TABLE IF NOT EXISTS spells (
    id          TEXT PRIMARY KEY,         -- = slug
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    level       INTEGER NOT NULL,
    school      TEXT NOT NULL,
    source      TEXT NOT NULL,
    ritual      INTEGER NOT NULL DEFAULT 0,
    concentration INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_spells_name   ON spells(name);
CREATE INDEX IF NOT EXISTS idx_spells_level  ON spells(level);
CREATE INDEX IF NOT EXISTS idx_spells_school ON spells(school);
```

> Migrations are immutable and sequential (architecture §10) — `0020` is the next number after
> `0019_factions_woac.sql`. The DB holds **search columns only**; the full card lives in the
> TOML store and is fetched by `slug` at render time. (Simpler alternative if the TOML layer
> proves not worth it: add a `payload TEXT` column holding `toml`/`json` of the `Spell` and drop
> `spell_store.rs`. The user preferred the TOML split, so default to 4a.)

In `core/src/db.rs`, add a `SpellRow` struct + one `impl_entity_table!` block (it generates
`upsert_spell`, `find_spell_by_slug`, `search_spells_by_name`, `list_spells`,
`delete_spell_by_id`, …) — copy the `locations` block at `core/src/db.rs:229`. `search_spells_by_name`
gives us the `LIKE`-based typeahead for free (557 rows — no FTS5 needed).

Repository (architecture §6 — handlers never touch `core::db` directly): add a `SpellRepository`
trait + `ProdSpellRepository` in `desktop/src-tauri/src/repositories/mod.rs` exposing
`search_by_name(query, limit)`, `find_by_slug(slug)`, `upsert_tx`, `clear`, and expose it from
`AppState` (`state.spell_repo()`).

---

## 5. The import pipeline — `spell import <path>`

Two parts: a **pure converter** (in `core`, unit-testable, no IO beyond reading the given dir)
and a thin **orchestration** in the desktop command handler/service.

### 5a. Converter — `core/src/spell_import.rs`

```rust
// External 5etools schema — local deserialize structs (only the fields in §2):
#[derive(Deserialize)] struct RawFile { spell: Vec<RawSpell> }
#[derive(Deserialize)] struct RawSpell { name, source, level, school, time, range,
                                          components, duration, meta, classes,
                                          entries, entriesHigherLevel, scalingLevelDice,
                                          reprintedAs, … }

/// Read <dir> (a 5etools repo root OR its data/spells dir), parse all official spell files,
/// dedup to the 2024-canonical set, convert each to `runebound_models::Spell`.
pub fn import_spells_from_dir(dir: &Path) -> Result<Vec<Spell>>;
```

Steps:

1. **Locate the data.** Accept either the repo root or the spells dir: look for
   `dir/data/spells/index.json`, else `dir/spells/index.json`, else `dir/index.json`.
   Error clearly if none found ("no 5etools spell data at <path>").
2. **Load all sources** named in `index.json` (skip nothing — they're all official).
3. **Dedup → 2024 canonical.** Group by spell `name`. For each name:
   - If an `XPHB` entry exists, take it (the 2024 version wins).
   - Else take the single most authoritative entry. Skip any entry that has
     `reprintedAs` pointing at a source we already kept (it's the superseded 2014 copy).
   - Result ≈ 557 spells.
4. **Convert each** `RawSpell` → `Spell`:
   - `school` letter → full name; `level` as-is.
   - `casting_time` from `time[]` (`"{number} {Unit}"`, append `, {condition}` if present).
   - `range` from `range` (`point`→`"{amount} {unit}"`; `self`+area→`"Self ({size}-foot {shape})"`;
     `touch`/`special`/`sight`/`unlimited` → their words).
   - `components` → `"V, S, M (…)"`; material is the string, or the object's `text`.
   - `duration` → `"Instantaneous"` / `"Concentration, up to 1 minute"` / `"8 hours"` / `"Until Dispelled"`.
     Set `concentration` from `duration[].concentration`; `ritual` from `meta.ritual`.
   - `classes` from `classes.fromClassList[].name` (dedup, sort).
   - `description` = `entries` lowered to `Vec<SpellBlock>` (see §2 mapping), tags stripped.
   - `higher_levels` = `entriesHigherLevel` lowered the same way; if absent but
     `scalingLevelDice` present (cantrips), synthesize a one-line `Text` block from the scaling map.
   - `slug` = kebab-case `name` (`"Fireball"` → `"fireball"`); on collision across sources,
     suffix `-{source}` (rare; only when two different spells share a name).

Unit tests live beside it (convert Fire Bolt, Fireball, Command (list), Alter Self (subsections),
a material-cost spell, a concentration spell). Test against `temp/5etools-src/data/spells/`
**before that dir is deleted** — copy 2–3 representative spell JSON snippets into a test fixture
so the test survives.

### 5b. Orchestration — `desktop/src-tauri/src/commands/spell_commands.rs` + a service

`spell import <path>` handler:

1. Parse the path argument from `invocation.raw_input` (everything after `spell import `).
   Empty path → usage message. (Optional nicety: if no path given, fire the existing native
   folder picker — `WizardHost::perform_native` / `NativeAction` already exists for setup — but
   a path arg is the v1 contract.)
2. Call `core::spell_import::import_spells_from_dir(path)`.
3. In one DB transaction (architecture §10 — `Database::begin()`): `clear` the `spells` table +
   spell store, then `upsert` every spell row and `save_spell` every TOML. (Full replace keeps
   re-import idempotent and simple.)
4. Return a `Status`/summary `OutputDoc`: `"Imported 557 spells from <path>."` Emit a
   `command_ref` hint: try `spell fireball`. (A progress `Spinner`/`Status` is available if it
   feels slow; 557 spells parse in well under a second.)

Put the file-walk + repo writes in a small `services/spell_import.rs` (orchestration) if you
want to keep the handler thin (architecture §6); the pure conversion stays in `core`.

---

## 6. The lookup command — `spell <fragment>`

`spell <name>` handler (same `spell_commands.rs`):

1. `spell help` → usage card. `spell import …` → routed to the import path above.
2. Otherwise treat the remainder as a spell name/fragment. Resolve via
   `state.spell_repo().find_by_slug(slug(query))`, falling back to
   `search_by_name(query, 1)` (so `spell fireball` works even without exact slug).
3. Miss → `"No spell found for '<query>'. Did you run spell import?"` (status/error tone).
   If the `spells` table is empty, specifically prompt to `spell import <path>`.
4. Hit → `spell_repo` gives the row; load the full `Spell` from the **spell store** by slug;
   `spell_card(&spell)` → return `ok_response_with_doc(card.to_plain_text(), Some(card), None)`.

---

## 7. Typeahead integration (`suggestions.rs`)

The seam is `SuggestionService::build_suggestions` in
`desktop/src-tauri/src/services/suggestions.rs`. Today it special-cases `entity_search_root`
(load/show/…) → `search_entities` → `CommandSuggestion { label, completion, helper_text }`.
Add a parallel branch:

1. New `SuggestionHelperText::Spell` variant (enum at `suggestions.rs:199`).
2. When the parsed root is `spell` and the remainder is **not** the `import` subcommand,
   call `state.spell_repo().search_by_name(fragment, 6)` and map each hit to:
   ```rust
   CommandSuggestion {
       label: spell.name.clone(),                       // "Fireball"
       completion: format!("spell {}", spell.name),     // tab inserts this
       helper_text: Some(SuggestionHelperText::Spell),  // e.g. "Lvl 3 Evocation"
   }
   ```
   So `spell fire` → suggestion **Fireball**, Tab → `spell Fireball`, Enter → card. This reuses
   the exact mechanism that already powers entity typeahead.
3. Optionally make the helper text show level/school (extend `SuggestionHelperText` rendering, or
   put it in `label`). Keep `import` as a normal subcommand suggestion (manifest-driven).

---

## 8. Command manifest + registration (architecture §8A)

1. **Manifest** (`command-specs/src/lib.rs`, `command_manifest()`): add a `CommandSpec` for
   `spell` — `requires_subcommand: false` (it takes a free-form name **and** has subcommands),
   `subcommands: [import, help]`, `examples: ["spell fireball", "spell import <path>"]`,
   `execution: CommandExecution::Desktop`, `show_in_autocomplete: true`.
2. **Availability** (`command_availability()` at `command-specs/src/lib.rs:189`): add
   `"spell" => CommandAvailability::Default`. (Without this it's invisible in editor contexts —
   default is correct here.)
3. **Handler** (`desktop/src-tauri/src/commands/spell_commands.rs`): `handle_spell(invocation)`
   dispatching `import` / `help` / lookup as above. Model on `moon_commands.rs`.
4. **Register** (`desktop/src-tauri/src/commands/mod.rs`): add `pub mod spell_commands;`,
   a `spell_handler_entry()` (copy `moon_handler_entry()`), and
   `registry.register(spell_handler_entry())` in `build_desktop_handler_registry()`.
5. No `router.rs` change (architecture §2 — normal top-level command).

---

## 9. Build order / milestones

Each milestone compiles + tests green before the next.

1. **Model** — `runebound-models/src/spells.rs` (`Spell`, `SpellBlock`), `spell_card`,
   regenerate `models.ts`. (`cargo test -p runebound-models`.)
2. **Converter** — `core/src/spell_import.rs` (`import_spells_from_dir` + `strip_tags`) with unit
   tests against fixture snippets copied from `temp/5etools-src` *before deletion*.
3. **Storage** — migration `0020_spells.sql`, `SpellRow` + `impl_entity_table!` in `db.rs`,
   `spell_store.rs`, `SpellRepository` in `repositories/mod.rs`, `state.spell_repo()`.
4. **Commands** — manifest + availability + `spell_commands.rs` (`import` then lookup) +
   registration.
5. **Typeahead** — `SuggestionHelperText::Spell` + the `spell` branch in `suggestions.rs`.
   (`cargo test suggestions`.)
6. **Verify** — `make build`, then walk: `spell import <path to temp/5etools-src>` →
   `spell fire` (typeahead shows Fireball) → Tab → Enter (card renders) → a cantrip
   (Fire Bolt, scaling), a subsection spell (Alter Self), a list spell (Command),
   a material-cost spell (Chromatic Orb). Test the empty-DB path (`spell fireball` before import).

---

## 10. Edge cases & notes

- **Re-import** is a full replace (clear + reload), so it's safe to run repeatedly and to point at
  an updated 5etools copy.
- **Name collisions across sources** (same name, genuinely different spell) are rare; the
  `-{source}` slug suffix handles them. The XPHB-preference dedup removes the *reprint* kind of
  collision.
- **Tables** in `entries` are uncommon and small; rendering as a fixed-width `Code` block is
  acceptable for v1 rather than building an `OutputBlock::Table`.
- **`temp/5etools-src` will be deleted** by the user. Copy any needed test fixtures into the repo
  (`core/tests/fixtures/spells/…`) during milestone 2 — do not depend on `temp/` at test time.
- **Licensing:** because data is imported from the user's own local copy and never committed,
  this repo ships no copyrighted spell text. Do not add a `spells.toml` of real spell data to git.
  Keep any local import output under the app data dir (already outside the repo) — and if a dev
  ever dumps converted TOML into the tree, `.gitignore` it.

## 11. Deferred / phase 2 (explicitly out of scope for v1)

- Clickable `{@spell …}` cross-links (convert to `InlineNode::CommandRef`).
- Filters/search by school, level, class (`spell search <school>`), or a spell list view.
- A persistent personal "spellbook" collection (add/remove/list) — the user chose lookup-only;
  this would layer on as a new table + commands without touching the import/render core.
- Spell artwork from the `fluff-spells-*.json` files.
