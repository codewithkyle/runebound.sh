# Monster Manual (Bestiary) Feature — Implementation Plan

> **Status:** 📋 Planned (2026-06-20). A self-contained build plan for the integrated
> monster/stat-block lookup feature, modeled directly on the shipped **Spellbook** feature.
> **Read first:** `docs/spellbook.md` (the template this mirrors) and `docs/architecture.md`
> §8A "Add New Top-Level Command". This plan follows the same
> `TOML store` → `SQLite search index` → `suggestions.rs` typeahead grain as the spellbook.

The spellbook is the reference implementation. Wherever this plan says "mirror the spell X,"
the as-built spell file is the literal template to copy. The map:

| Spellbook (shipped) | Monster Manual (this plan) |
|---|---|
| `runebound-models/src/spells.rs` (`Spell`, `SpellBlock`, `spell_card`) | `runebound-models/src/monsters.rs` (`Monster`, `StatBlock`, `monster_card`) |
| `core/src/spell_import.rs` (`import_spells_from_dir`, `strip_tags`, `slugify`) | `core/src/monster_import.rs` (`import_monsters_from_dir`, **extended** `strip_tags`) |
| `core/src/spell_store.rs` (+ `spells` path in `config.rs`) | `core/src/monster_store.rs` (+ `monsters` path) |
| `core/migrations/0020_spells.sql` + `SpellRow` in `db.rs` | `core/migrations/0021_monsters.sql` + `MonsterRow` in `db.rs` |
| `SpellRepository` in `repositories/mod.rs`; `AppState::spell_repo()` | `MonsterRepository`; `AppState::monster_repo()` |
| `desktop/.../services/spell_library.rs` | `desktop/.../services/bestiary_library.rs` |
| `desktop/.../commands/spell_commands.rs` (`handle_spell`/`handle_spellbook`) | `commands/monster_commands.rs` (`handle_monster`/`handle_bestiary`) |
| `SuggestionHelperText::Spell` + `spell_search_context` in `suggestions.rs` | `SuggestionHelperText::Monster` + `monster_search_context` |
| Bare-name fallback in `router.rs` | Bare-name fallback (after spell fallback) |
| `cleanup` boot task re-projects spell store | same boot task re-projects monster store |

---

## 0. Decisions already locked (do not re-litigate)

| Decision | Choice | Rationale |
|---|---|---|
| **Ship monster data in-repo?** | **No.** Import from a user-supplied path at runtime. | Same as spells: 5etools bestiary text is WotC-copyrighted and this repo has a public remote. Importing from the user's own local copy sidesteps redistribution. No SRD filtering needed. |
| **Edition / dedup** | **2024 / 5.5e canonical**, prefer **`XMM`** (2024 Monster Manual) over older sources, drop `reprintedAs`, all official sources, no homebrew. | Direct analog of the spellbook's `XPHB` preference. `XMM` is to monsters what `XPHB` is to spells. |
| **Scope of `_copy` monsters** | **Skip them in v1** (the 1141 `_copy`-derived entries). Log the skipped count. | `_copy` requires porting 5etools' `_applyCopy`/`_mod` engine (`appendArr`/`replaceArr`/`insertArr`/`removeArr`/`_preserve`/templates) — out of proportion for v1. The skipped set is **overwhelmingly adventure-specific NPC variants**; MPMM (the main extended bestiary) loses only **2** entries to `_copy`. Full resolution is **Phase 2** (§11). |
| **Feature scope** | **Lookup + render only.** | No encounter builder, no "my bestiary" collection, no initiative tracker. The imported set *is* the searchable library. |
| **Command surface** | **`bestiary import [path]`** (import; folder picker if no path) + **`monster <name>`** (lookup) + **bare creature name** via router fallback. | Mirrors the shipped `spellbook import` / `spell <name>` split exactly. |
| **Storage** | **SQLite `monsters` table = search index; per-monster TOML = card payload.** | Same two-layer split as spells (`EntityStore` grain). |
| **Card** | **One rich `EntityCard`** (`entity_card_full`): name = title, "Size Type, Alignment" = subtitle, defensive stats = rows, traits/actions/etc. = `body`. | `EntityCard` already carries `subtitle` + `body` (added for spells). No new output type. |

**Expected import size:** **~2575 canonical monsters** (skip `_copy`, drop `reprintedAs`, dedup
preferring `XMM`). Measured against the real dataset 2026-06-20.

---

## 1. What we're building

A read-only monster/stat-block reference inside the TUI:

- `monster <fragment>` — typing `monster gob` surfaces typeahead **Goblin Warrior**; Tab
  completes to `monster Goblin Warrior`; executing renders a **stat-block card**.
- A bare creature name (`Goblin Warrior`, no prefix) also renders the card via the router
  fallback (entities win, then spells, then monsters — see §8).
- `bestiary import [path]` — points at a **local copy of the 5etools data** (user supplies the
  path; nothing copyrighted ships in this repo), reads every official monster, converts it to our
  own model, and stores it locally for search + rendering. No path → native folder picker.

---

## 2. Verified facts about the 5etools bestiary data

Source layout (under the path the user imports, e.g. `<5etools>/data/bestiary/`):

- **`index.json`** maps **source code → filename**, e.g. `{"XMM":"bestiary-xmm.json","MM":"bestiary-mm.json", …}`.
  **~84 source files.** Read these and only these. `homebrew/` is a sibling dir we ignore.
- Each `bestiary-*.json` is `{ "monster": [ {…}, {…} ] }`.
- **`legendarygroups.json`** = `{ "legendaryGroup": [ {name, source, lairActions?, regionalEffects?, mythicEncounter?} ] }`
  (187 groups). Referenced by a monster's `legendaryGroup: {name, source}`. **Load this once and
  resolve references** (simple map lookup; no `_mod`).
- **`fluff-bestiary-*.json`** hold lore prose + artwork only — **ignore them** for v1 (like
  `fluff-spells-*` for spells). The stat block lives entirely in `bestiary-*.json`.
- **`template.json`** (monster templates) and the `_copy` mechanic are **Phase 2** — ignored in v1.

**Counts (measured 2026-06-20):** 4528 monster objects across 106 sources. `XMM` (2024 MM) = 503;
`MM` (2014) = 450; `MPMM` = 261; `VGM` 143, `MTF` 140, … `_copy`-derived = **1141** (skipped);
`reprintedAs` = 787 (dropped). **Deduped, skip-`_copy`, XMM-preferred corpus = ~2575.**

**`XMM` is fully self-contained: it uses zero `_copy`.** This is why skip-`_copy` keeps the entire
2024 core intact and only sheds adventure NPC variants.

### 2a. Monster object schema (fields we consume)

```jsonc
{
  "name": "Goblin Warrior",
  "source": "XMM", "page": 142,
  "_copy": { … },                     // PRESENT → SKIP this monster in v1
  "reprintedAs": ["Goblin|XMM"],      // PRESENT → DROP (superseded)
  "size": ["S"],                      // array of letters: T S M L H G
  "type": { "type": "fey", "tags": ["goblinoid"] },   // OR a bare string "humanoid"
  "alignment": ["C", "N"],            // letters; may include "any", prefix, or be absent
  "ac": [15],                         // OR [{ "ac": 16, "from": ["natural armor"] }] OR mixed
  "hp": { "average": 10, "formula": "3d6" },          // OR { "special": "58" }
  "speed": { "walk": 30, "fly": 60, "climb": 30, "hover": true },  // values are numbers OR {number,condition}
  "str": 8, "dex": 15, "con": 10, "int": 10, "wis": 8, "cha": 8,   // raw scores
  "save":  { "dex": "+4", "con": "+6" },              // optional
  "skill": { "stealth": "+6", "perception": "+5" },   // optional
  "resist":  ["cold", {"resist":["bludgeoning"],"note":"from nonmagical","cond":true}],
  "immune":  ["necrotic", "poison"],  "vulnerable": [...],         // damage; same shape as resist
  "conditionImmune": ["charmed", "frightened"],
  "senses": ["Darkvision 60 ft."],   "passive": 9,   // passive Perception
  "languages": ["Common", "Goblin"],
  "cr": "1/4",                        // OR { "cr": "21", "xpLair": 41000 } OR {"cr":"3","lair":"4"}
  "gear": ["leather armor|xphb", "scimitar|xphb"],    // optional; strip the |source suffix
  "trait":     [{ "name": "...", "entries": [ … ] }], // passive traits
  "action":    [{ "name": "Scimitar", "entries": [ "{@atkr m} {@hit 4}…" ] }],
  "bonus":     [{ "name": "Nimble Escape", "entries": [ … ] }],   // bonus actions
  "reaction":  [{ … }],
  "legendary": [{ "name": "...", "entries": [ … ] }], "legendaryHeader": ["…"],
  "mythic":    [{ … }], "mythicHeader": ["…"],
  "legendaryGroup": { "name": "Lich", "source": "XMM" },          // → legendarygroups.json
  "spellcasting": [{ … }]            // structured; see §2c
}
```

**Size letter → name:** `T`=Tiny, `S`=Small, `M`=Medium, `L`=Large, `H`=Huge, `G`=Gargantuan.
**Alignment letter → word:** `L`/`N`/`C` (lawful/neutral/chaotic) × `G`/`N`/`E` (good/neutral/evil),
plus `U`=Unaligned, `A`=Any. Render the common pairs ("Chaotic Neutral", "Unaligned", "Any
Alignment", "Neutral Good"); a single `["N"]` is "Neutral". Defensive: unknown → join the letters.

### 2b. Stat-field formatters (all produce display strings)

Each is a small pure function in `monster_import.rs`, unit-tested. Mirrors the spell formatters
(`format_range`, `format_duration`, …):

| Field | Input shape | Output example |
|---|---|---|
| `size` | `["S"]` (or two for "S or M") | `Small` |
| `creature_type` | `{type:"fey",tags:["goblinoid"]}` / `"humanoid"` | `Fey (Goblinoid)` |
| `alignment` | `["C","N"]` | `Chaotic Neutral` |
| `ac` | `[15]` / `[{ac:16,from:["natural armor"]}]` | `15` / `16 (natural armor)` |
| `hp` | `{average:10,formula:"3d6"}` / `{special:"58"}` | `10 (3d6)` / `58` |
| `speed` | `{walk:30,fly:60,hover:true}` | `30 ft., Fly 60 ft. (hover)` |
| `abilities` | six i16 scores | rendered in the card as `STR 8 (-1) · DEX 15 (+2) · …` |
| `saves` / `skills` | `{dex:"+4"}` / `{stealth:"+6"}` | `Dex +4, Con +6` / `Perception +5, Stealth +6` |
| `resist`/`immune`/`vulnerable` | array of strings + `{resist,note,cond}` objects | `cold, lightning; bludgeoning, piercing, slashing from nonmagical attacks` |
| `conditionImmune` | `["charmed","frightened"]` | `Charmed, Frightened` |
| `senses` | `["Darkvision 60 ft."]` + `passive:9` | `Darkvision 60 ft., Passive Perception 9` |
| `languages` | `["Common","Goblin"]` (may be empty) | `Common, Goblin` / `—` |
| `cr` | `"1/4"` / `{cr:"21",xpLair:41000}` | `1/4 (XP 50; PB +2)` |
| `gear` | `["scimitar|xphb"]` | `scimitar, shield` (strip `|source`) |

- **Ability modifier:** `mod = (score - 10).div_euclid(2)`, displayed `+N` / `-N` (`(score-10)` is
  the 2024 layout's own modifier; show `score (mod)`).
- **CR → XP / PB:** embed a small static table (`CR_TABLE: &[(&str, u32, i8)]` — only ~34 rows, e.g.
  `("1/4", 50, 2)`, `("21", 33000, 7)`). XP/PB are nice-to-have; if the lookup misses, show CR alone.
  When `cr` is an object, the inner `cr` is the value and `lair`/`xpLair` annotate it.

### 2c. Spellcasting (two formats)

`spellcasting` is `[{name, type:"spellcasting", headerEntries, ability, displayAs?, ...}]`.
Two shapes coexist:

- **2024 (`XMM`):** `will: [...]`, `daily: { "1e":[...], "2e":[...] }` — "casts X spell, ... 1/day each".
- **2014 (`MM`):** `spells: { "0": {spells:[…]}, "1": {slots:3, spells:[…]} }` — spell-slot table.

**v1 lowering:** render `headerEntries` (a paragraph, tags stripped), then list the spells grouped
by their bucket as bullet lines: `"At Will: detect magic, mage hand"`, `"1/Day Each: animate
dead"` (2024) or `"Cantrips (at will): light, sacred flame"`, `"1st level (3 slots): bless"`
(2014). Each `{@spell X}` strips to its display name (§4). Treat spellcasting as one more entry in
the **Traits** (or **Actions** if `displayAs:"action"`) section — give it the block's `name`
("Spellcasting") as the ability heading.

---

## 3. Inline markup — extended `strip_tags`

The spell `strip_tags` (`core/src/spell_import.rs:611`) already handles the generic
`{@tag display|args}` → display rule and `{@dc N}` → `DC N`. Monster text uses a **larger
vocabulary** (measured top tags: `@spell` 10k, `@damage` 9k, `@hit` 5.5k, `@condition` 5.3k,
`@h` 5.2k, `@dc` 4.7k, `@atk` 4.3k, `@atkr`, `@recharge`, `@actSave*`, …). Most fall through the
generic rule correctly; the **attack/save/recharge tags carry no display text** and need explicit
rendering rules. Extend `render_tag` with this table (these tag names never appear in spell text,
so extending the **shared** `strip_tags` is safe — verify by re-running the spell tests):

| Tag | v1 render |
|---|---|
| `{@atk mw}` / `{@atk rw}` / `{@atk mw,rw}` | `Melee Weapon Attack:` / `Ranged Weapon Attack:` / `Melee or Ranged Weapon Attack:` |
| `{@atkr m}` / `{@atkr r}` / `{@atkr m,r}` (2024) | `Melee Attack Roll:` / `Ranged Attack Roll:` / `Melee or Ranged Attack Roll:` |
| `{@hit 4}` | `+4` (prepend `+`; negatives keep their sign) |
| `{@h}` | `Hit: ` |
| `{@dc 15}` | `DC 15` *(already handled)* |
| `{@recharge}` / `{@recharge 5}` | `(Recharge 5-6)` |
| `{@recharge 6}` | `(Recharge 6)` |
| `{@actSave con}` | `Constitution Saving Throw:` (expand the 3-letter ability) |
| `{@actSaveFail}` | `Failure:` |
| `{@actSaveSuccess}` | `Success:` |
| `{@actSaveSuccessOrFail}` | `Failure or Success:` |
| `{@actTrigger}` | `Trigger:` |
| `{@actResponse}` | `Response:` |
| `{@hitYourSpellAttack}` | `your spell attack modifier` |
| `{@spell X}` / `{@creature X}` / `{@item X}` / `{@condition X}` / `{@damage X}` / `{@dice X}` | display text (generic rule — first/last segment) |

Keep this in the one well-tested `strip_tags`/`render_tag` seam. Add monster-shape cases to the
existing `strip_tags_handles_every_tag_shape` test (or a sibling test) so the spell cases stay green.

> **Phase 2 (optional):** convert `{@spell fireball}` / `{@creature goblin}` into clickable
> `InlineNode::CommandRef { command: "spell fireball" / "monster goblin" }` so stat blocks
> cross-link into the spellbook and bestiary. The model already supports inline nodes.

---

## 4. Data model (`runebound-models`)

New file `runebound-models/src/monsters.rs`, exported from `lib.rs`
(`pub mod monsters; pub use monsters::*;`). **Backend-only** (derive `Serialize`/`Deserialize`,
**not** `TS`) — mirrors `Spell`: only the rendered `OutputDoc` crosses to the frontend, so no
`models.ts` change is needed.

```rust
/// A converted, render-ready monster stat block. All `{@tag}` markup already stripped;
/// all defensive stats pre-formatted to display strings (see §2b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monster {
    pub slug: String,          // kebab name; primary key in TOML store + search DB
    pub name: String,
    pub source: String,        // "XMM", "MPMM", … (provenance footer)
    pub size: String,          // "Small"
    pub creature_type: String, // "Fey (Goblinoid)"
    pub alignment: String,     // "Chaotic Neutral"
    pub ac: String,            // "15 (natural armor)"
    pub hp: String,            // "10 (3d6)"
    pub speed: String,         // "30 ft., Fly 60 ft."
    pub abilities: [i16; 6],   // STR DEX CON INT WIS CHA (raw); card renders score + modifier
    pub saves: String,         // "Dex +4, Con +6"  (empty → omit row)
    pub skills: String,
    pub damage_resistances: String,
    pub damage_immunities: String,
    pub damage_vulnerabilities: String,
    pub condition_immunities: String,
    pub senses: String,        // includes "Passive Perception N"
    pub languages: String,
    pub cr: String,            // "1/4 (XP 50; PB +2)"
    pub gear: String,          // "scimitar, shield"  (empty → omit)
    /// Trait / action / bonus-action / reaction / legendary / lair / regional sections,
    /// in render order. Empty sections are dropped during conversion.
    pub sections: Vec<StatSection>,
}

/// One titled group of stat-block abilities ("Traits", "Actions", "Legendary Actions",
/// "Lair Actions", "Regional Effects", …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatSection {
    pub title: String,
    /// Optional lead-in prose (legendary/mythic header, lair-action preamble), tags stripped.
    #[serde(default)]
    pub intro: Vec<StatBlock>,
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

/// Render-ready body element — the monster analog of `SpellBlock`, deliberately flat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatBlock {
    Text { text: String },
    Bullets { items: Vec<String> },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
}
```

> `StatBlock` is `SpellBlock` minus `Heading` (subsection titles become `StatAbility.name`
> instead). The `lower_entries`/`strip_tags`/`render_table` logic is **identical** to
> `spell_import.rs` — factor the shared pieces or copy them; tables stay fixed-width `Code` blocks.

### 4a. Card rendering — `monster_card(&Monster) -> OutputDoc`

One `entity_card_full` (exactly like `spell_card`):

- **title** = `monster.name`
- **subtitle** = `"{size} {creature_type}, {alignment}"` → `"Small Fey (Goblinoid), Chaotic Neutral"`
- **rows** (`entity_row`, omit empty ones): `AC`, `HP`, `Speed`, **Abilities** (one row:
  `STR 8 (-1) · DEX 15 (+2) · CON 10 (+0) · INT 10 (+0) · WIS 8 (-1) · CHA 8 (-1)`), then optional
  `Saving Throws`, `Skills`, `Resistances`, `Immunities`, `Vulnerabilities`, `Condition Immunities`,
  `Gear`, `Senses`, `Languages`, `CR`.
- **body** = for each `StatSection`: a `heading(3, title)`, its `intro` blocks, then each
  `StatAbility` as `paragraph_with_inlines([strong("Name."), text(" ")] ++ first-line)` followed by
  the rest of its body blocks (`push` via a shared `push_stat_block`, same shape as
  `push_spell_block`). Footer: `paragraph_with_inlines([emphasis("Source: XMM")])`.

Add `monster_card` + `push_stat_block` next to `spell_card`. After adding the model, no
`models.ts` regen is needed (backend-only) — but run `cargo test -p runebound-models`.

---

## 5. Storage architecture (mirror the spell layers)

### 5a. Monster store (TOML, card payload) — mirror `core/src/spell_store.rs`

- Extend `config_paths()` in `core/src/config.rs` with a `monsters: PathBuf` (sibling of `spells`).
- `save_monster(&Monster) -> path` writes `monsters/<slug>.toml`; `load_monster(slug)`,
  `clear()`, `list_slugs()` for re-import + projection. (`Monster` serializes cleanly to TOML —
  all fields are scalars, strings, or `Vec` of simple tables. Round-trip test it, like the spell.)

### 5b. Monster search index (SQLite) — migration `core/migrations/0021_monsters.sql`

> `0021` is the next immutable, sequential number after `0020_spells.sql` (architecture §10).

```sql
CREATE TABLE IF NOT EXISTS monsters (
    id          TEXT PRIMARY KEY,        -- = slug
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    cr          TEXT NOT NULL DEFAULT '',
    cr_sort     REAL NOT NULL DEFAULT 0,  -- numeric CR for ordering ("1/4" -> 0.25)
    type        TEXT NOT NULL DEFAULT '',
    size        TEXT NOT NULL DEFAULT '',
    source      TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_monsters_name ON monsters(name);
CREATE INDEX IF NOT EXISTS idx_monsters_cr   ON monsters(cr_sort);
CREATE INDEX IF NOT EXISTS idx_monsters_type ON monsters(type);
```

The DB holds **search columns only**; the full card lives in the TOML store, fetched by `slug` at
render time (identical to spells). In `core/src/db.rs`, add a `MonsterRow` struct + one
`impl_entity_table!` block (copy the `SpellRow` block) — generates `upsert_monster`,
`find_monster_by_slug`, `search_monsters_by_name`, `list_monsters`, `delete_monster_by_id`, plus
`clear_monsters`/`count_monsters`. 2575 rows → `LIKE` search is plenty (no FTS5).

Repository (architecture §6): add `MonsterRepository` + `ProdMonsterRepository` in
`desktop/src-tauri/src/repositories/mod.rs` exposing `search_by_name(query, limit)`,
`find_by_slug(slug)`, `upsert_tx`, `clear`, `count`; expose `state.monster_repo()`.

---

## 6. The import pipeline — `bestiary import [path]`

### 6a. Converter — `core/src/monster_import.rs`

```rust
pub fn import_monsters_from_dir(dir: &Path) -> Result<ImportSummary>;
// ImportSummary { monsters: Vec<Monster>, skipped_copy: usize }
```

Steps (mirror `import_spells_from_dir`):

1. **Locate the data.** Look for `dir/data/bestiary/index.json`, else `dir/bestiary/index.json`,
   else `dir/index.json`. Error clearly if none found.
2. **Load `legendarygroups.json`** from the same dir (if present) → `Map<(name, source), RawLegendaryGroup>`.
3. **Load all sources** named in `index.json` (`{ "monster": [...] }` each).
4. **Filter + dedup → canonical:**
   - Skip any monster with **`_copy`** (count them → `skipped_copy`).
   - Drop any monster with **`reprintedAs`**.
   - Group by `name`; prefer the **`XMM`** entry, else keep the first seen.
   - Result ≈ 2575.
5. **Convert each** `RawMonster` → `Monster` via the §2b formatters + §4 lowering:
   - Defensive stats → display strings; abilities → `[i16;6]`.
   - `trait`→ "Traits", `action`→ "Actions", `bonus`→ "Bonus Actions", `reaction`→ "Reactions",
     `legendary`→ "Legendary Actions" (with `legendaryHeader` as `intro`), `mythic`→ "Mythic Actions".
   - **Spellcasting** (§2c) → a `StatAbility` inside Traits (or Actions if `displayAs:"action"`).
   - **Legendary group:** if `legendaryGroup` resolves, append "Lair Actions" (from `lairActions`)
     and "Regional Effects" (from `regionalEffects`) sections. Match by `(name, source)`; fall back
     to match-by-name if the exact pair misses (reprint sources can drift).
   - `slug` = `slugify(name)`; on cross-source collision, suffix `-{source}` (`disambiguate_slugs`,
     copied from spells — named creatures across adventures collide more than spells do, so this
     matters here).
6. Sort by name; return.

**Tests** beside it (copy 3–4 representative monster JSON snippets into
`core/tests/fixtures/bestiary/` **before `temp/5etools-src` is deleted**): a vanilla melee creature
(Goblin Warrior — attack-tag stripping, type+tags, CR/XP/PB), a 2024 spellcaster + legendary +
legendary-group creature (Lich — `will`/`daily` spellcasting, lair/regional resolution), a
recharge-breath dragon (`{@recharge}`, `{@actSave}`/`{@actSaveFail}`/`{@actSaveSuccess}`), and a
`_copy` entry (assert it is **skipped**, `skipped_copy` increments). Add an `#[ignore]`
`real_dataset_imports_cleanly` test gated on `MONSTER_5E_DIR=<path>` asserting `~2400..=2700`
monsters, no residual `{@` markup, and no empty required fields — exactly like the spell version.

### 6b. Orchestration — `commands/monster_commands.rs` + `services/bestiary_library.rs`

`bestiary import [path]` handler (mirror `handle_spellbook`):

1. Parse the path from `invocation.raw_input` (everything after `bestiary import`). **No path →
   fire the native folder picker** (reuse the spellbook's `WizardHost::perform_native` /
   `NativeAction` path — the spellbook already wired this).
2. Call `core::monster_import::import_monsters_from_dir(path)`.
3. In **one DB transaction** (architecture §10): `clear` the `monsters` table + monster store,
   then `upsert_tx` every row and `save_monster` every TOML. Full replace = idempotent re-import.
4. Return a summary `OutputDoc`: `"Imported 2575 monsters from <path>."` and, when `skipped_copy >
   0`, a second line: `"Skipped 1141 variant monsters (derived stat blocks)."` — **never silently
   drop them.** Hint: try `monster goblin`.

Put the file-walk + repo writes in `services/bestiary_library.rs` (orchestration); the pure
conversion stays in `core`. Add `BestiaryLibraryService::project_store_into_db` and call it from
the `cleanup` boot task in `desktop/.../boot.rs` (self-heals a deleted/rebuilt `app.db`, mirroring
the spell re-projection).

---

## 7. The lookup command — `monster <fragment>`

`handle_monster` (same file; mirror `handle_spell`):

1. `monster help` → usage card. `monster import …` is **not** valid here (import lives under
   `bestiary`); a lone `monster import` can hint at `bestiary import`.
2. Otherwise treat the remainder as a name/fragment: `monster_repo().find_by_slug(slugify(query))`,
   falling back to `search_by_name(query, 1)`.
3. Miss → `"No monster found for '<query>'. Did you run bestiary import?"`. If the table is empty,
   specifically prompt `bestiary import <path>`.
4. Hit → load the full `Monster` from the **monster store** by slug; `monster_card(&monster)` →
   `ok_response_with_doc(card.to_plain_text(), Some(card), None)`.

---

## 8. Typeahead + bare-name fallback

**Typeahead** (`desktop/src-tauri/src/services/suggestions.rs`, mirror the spell branch at
`suggestions.rs:157`):

1. Add `SuggestionHelperText::Monster` to the enum (`suggestions.rs:222`); render it as the CR +
   type, e.g. `"CR 1/4 Fey"` (the search row has `cr` + `type`).
2. Add `monster_search_context(trimmed, &manifest)` (copy `spell_search_context` at
   `suggestions.rs:639`): when the root is `monster`, call `monster_repo().search_by_name(fragment,
   6)` and map each hit to `CommandSuggestion { label: name, completion: "monster {name}",
   helper_text: Some(Monster) }`.
3. Regenerate `desktop/src/generated/manifest.ts`.

**Bare-name fallback** (`desktop/src-tauri/src/router.rs`): after the existing entity resolution
**and the spell fallback**, add a monster fallback so `Goblin Warrior` (no prefix) renders the
card. **Precedence: entity → spell → monster** (first hit wins; document this — a name shared by an
entity and a monster resolves to the entity).

---

## 9. Command manifest + registration (architecture §8A)

Mirror the spellbook's two-root registration:

1. **Manifest** (`command-specs/src/lib.rs`, `command_manifest()`): add `CommandSpec` for
   **`monster`** (`requires_subcommand: false`, free-form name + `help` subcommand,
   `examples: ["monster goblin", "monster Adult Red Dragon"]`, `execution: Desktop`,
   `show_in_autocomplete: true`) and **`bestiary`** (`subcommands: [import, help]`,
   `examples: ["bestiary import <path>"]`).
2. **Availability** (`command_availability()`): add `"monster" => Default` and
   `"bestiary" => Default`.
3. **Handlers** (`commands/monster_commands.rs`): `handle_monster` + `handle_bestiary`. Model on
   `spell_commands.rs`.
4. **Register** (`commands/mod.rs`): `pub mod monster_commands;`, a `monster_handler_entry()` and
   `bestiary_handler_entry()` (copy the spell/spellbook entries), registered in
   `build_desktop_handler_registry()`.
5. The bare-name fallback is the only `router.rs` change (§8).

---

## 10. Build order / milestones

Each milestone compiles + tests green before the next (mirrors the spellbook order):

1. **Model** — `runebound-models/src/monsters.rs` (`Monster`, `StatSection`, `StatAbility`,
   `StatBlock`, `monster_card`). `cargo test -p runebound-models`.
2. **Converter** — `core/src/monster_import.rs` (`import_monsters_from_dir` + extended
   `strip_tags`/`render_tag` + the §2b formatters) with fixtures copied from `temp/5etools-src`
   *before deletion*. Re-run the spell tests to confirm the shared `strip_tags` still passes.
3. **Storage** — `0021_monsters.sql`, `MonsterRow` + `impl_entity_table!` in `db.rs`,
   `monster_store.rs`, `MonsterRepository`, `state.monster_repo()`.
4. **Commands** — manifest + availability + `monster_commands.rs` (`bestiary import` then `monster`
   lookup) + registration + boot re-projection.
5. **Typeahead + fallback** — `SuggestionHelperText::Monster` + `monster_search_context`, regen
   `manifest.ts`, router bare-name fallback. `cargo test suggestions`.
6. **Verify** — `make build`, then: `bestiary import <path to temp/5etools-src>` (reports ~2575 +
   skipped count) → `monster gob` (typeahead shows Goblin Warrior) → Tab → Enter (card renders) →
   spot-check a legendary spellcaster (Lich: spellcasting + lair actions + regional effects), a
   recharge-breath dragon (Adult Red Dragon), and the bare-name path (`Goblin Warrior`). Test the
   empty-DB path (`monster goblin` before import).

---

## 11. Edge cases & notes

- **Re-import** is a full replace (clear + reload) — safe to repeat and to point at an updated copy.
- **Name collisions across sources** are **more common** than for spells (adventures reuse NPC
  names). The `-{source}` slug suffix handles them; the XMM-preference dedup removes the reprint
  kind. Verify zero slug collisions on the real dataset in the `#[ignore]` test.
- **Tables** in entries are uncommon and small → fixed-width `Code` block (same as spells).
- **`temp/5etools-src` will be deleted.** Copy fixtures into `core/tests/fixtures/bestiary/` during
  milestone 2 — do not depend on `temp/` at test time.
- **Licensing:** data is imported from the user's own local copy and never committed. Ship no
  monster TOML in git; keep import output under the app data dir (outside the repo); `.gitignore`
  any stray converted output.

## 12. Deferred / Phase 2 (explicitly out of scope for v1)

- **`_copy` resolution** — ✅ **Shipped 2026-06-20** (`core/src/monster_copy.rs`). A faithful port
  of 5etools' `copyApplier` (`js/utils.js`), scoped to the 21 `_mod` modes the real data actually
  uses plus `template.json` templating and `_preserve`. Runs as a `Value` → `Value` pre-pass before
  `RawMonster` deserialization. The full dataset now resolves **all 1141** `_copy` variants (0 left
  unresolved), growing the canonical corpus from ~2575 to **~3668**. The `<$…$>` variable resolver
  is omitted (0 occurrences in the data). Import reports the resolved/skipped counts (§6b).
- **Clickable cross-links** — `{@spell …}`/`{@creature …}`/`{@item …}` → `InlineNode::CommandRef`
  so stat blocks link into the spellbook and bestiary (§3).
- **Fluff** — lore prose + artwork from `fluff-bestiary-*.json`.
- **Filters / encounter tooling** — search by CR/type/environment, a monster list view, an
  encounter or initiative builder. The imported set is lookup-only by design.
