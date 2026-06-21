# Monster Manual — Bestiary Reference Library

> **Status:** Implemented. **Purpose:** the worked example of a richer reference library,
> built as the twin of the spellbook (`docs/spellbook.md` — read it first; this doc only
> calls out where monsters differ). It documents the *shipped* monster lookup feature: the
> stat-block model, the 5etools import including `_copy` variant resolution, CR
> sorting/filtering, the two-layer store, the commands, and the wiring. See
> `docs/architecture.md` §5 for the pattern and `docs/feature-development.md` §8 for the
> generic playbook.

---

## 1. What it is

A read-only monster/stat-block reference inside the TUI:

- `monster <name>` renders a **stat-block card**. A bare creature name (no prefix) also resolves via the router fallback.
- `monster --type <kind> --cr <range> [name]` runs a **filtered search** and renders a clickable results list (this is the main thing monsters add over spells).
- `bestiary import [path]` builds the library from the user's **own local copy** of [5e.tools](https://5e.tools/) data. No path → native folder picker.

Like the spellbook it is a reference library, not an entity or wizard. Command-surface details are in `docs/cli.md` §14.

### Design decisions (settled — don't re-litigate)

| Decision | Choice | Why |
|---|---|---|
| Ship monster data in-repo? | **No** — import from a user-supplied path | Same copyright reasoning as spells. |
| Edition / dedup | **2024-canonical**, prefer `XMM` (2024 Monster Manual), drop `reprintedAs` | `XMM` is to monsters what `XPHB` is to spells. |
| `_copy` variants | **Resolved** (the planned v1 "skip" was superseded) | 5etools defines thousands of monsters as a `_copy` of a base plus modifications; resolving them is what makes the corpus complete. See §3. |
| Scope | **Lookup + render + filter only** | No encounter builder, no initiative tracker, no "my bestiary" collection. |
| Store / card | per-monster TOML (source of truth) + SQLite `monsters` projection; one rich `entity_card_full` | Same two-layer split + card shape as spells. |

Measured corpus: a real 5etools checkout yields **~3,200–4,000** monsters with `_copy`
resolved (the `real_dataset_imports_cleanly` smoke test in `core/src/monster_import.rs`
expects 3200–4000 monsters and >900 resolved copies).

---

## 2. Data model

`runebound_models::monsters::Monster` (`runebound-models/src/monsters.rs`) — backend-only,
every defensive stat pre-formatted to a display string during conversion so the card builder
is a straight 1:1 map.

Fields: `slug`, `name`, `source`, `size`, `creature_type` (e.g. "Fey (Goblinoid)"),
`alignment` (may be empty), `ac`, `hp`, `speed`, `abilities: [i16; 6]` (raw STR…CHA scores),
`saves`, `skills`, `damage_resistances`/`_immunities`/`_vulnerabilities`,
`condition_immunities`, `senses`, `languages`, `cr` (verbose, e.g. "1/4 (XP 50; PB +2)"),
`gear`, `sections: Vec<StatSection>`, `lore: Vec<StatBlock>`. Empty stat strings omit their
row.

Supporting types:

- `StatSection { title, intro, abilities }` — a titled group (Traits, Actions, Bonus Actions, Reactions, Legendary Actions, Lair Actions, Regional Effects, …); `intro` is optional lead-in prose.
- `StatAbility { name: Option<String>, body }` — a named ability ("Scimitar", "Nimble Escape"); nameless for bare legendary/lair list items.
- `StatBlock` — `Text`/`Bullets`/`Table` (like `SpellBlock` but **without** `Heading`; subsection titles become a `StatAbility::name` instead). Flat for clean TOML.
- `Span` (`Text`/`Link`) and `spans_to_inlines` are defined here and reused by `spells.rs` — the shared cross-link primitive.

### Card rendering

`monster_card(&Monster) -> OutputDoc` (`monsters.rs`) builds **one** `entity_card_full`:

- **title** = name; **subtitle** = `"{size} {creature_type}, {alignment}"` (alignment omitted when empty)
- **rows** (skipping empties, fixed order): AC, HP, Speed, **Abilities**, Saving Throws, Skills, Resistances, Immunities, Vulnerabilities, Condition Immunities, Gear, Senses, Languages, CR. The Abilities row is `abilities_line` → "STR 8 (-1) · DEX 15 (+2) · …", each score with its modifier (`format_modifier`: `(score-10).div_euclid(2)`, rounds toward −∞, explicit sign).
- **body** = each section as a level-3 `heading` + intro + abilities; a named ability with leading prose renders as a bold `**Name.**` inline followed by its spans; then a `"Lore"` section when fluff exists; then an emphasized `"Source: …"` footer.

Cross-links lower exactly as for spells (`spans_to_inlines` → `command_ref`); see §6 of `docs/spellbook.md` and `docs/render.md` §4.

---

## 3. Import & conversion

`core/src/monster_import.rs` — entry point `import_monsters_from_dir(dir) -> Result<ImportSummary>`,
where `ImportSummary { monsters: Vec<Monster>, resolved_copy: usize, skipped_copy: usize }`.

**Source layout.** `locate_bestiary_dir` accepts the repo root, `data/bestiary`, or a dir
with `index.json`. Beyond `index.json` + the `bestiary-*.json` files it names, the importer
also reads (gracefully degrading if absent):

- `template.json` — `monsterTemplate` definitions used by `_copy` modifications
- `legendarygroups.json` — Lair Actions / Regional Effects / Mythic Encounters, attached by `(name, source)`
- `fluff-index.json` + `fluff-bestiary-*.json` — lore prose (artwork is intentionally ignored)

**Pipeline**: read raw values → `resolve_copies` (materialize `_copy`, see below) →
deserialize to `RawMonster` (un-parseable values are silently dropped) → attach legendary
groups + fluff → `dedup_to_canonical` (drop `reprintedAs`, prefer `XMM`) → `convert_monster`
→ `disambiguate_slugs` → sort by name.

**Field formatting** turns each raw shape into a display string: size letters →
Tiny…Gargantuan; `{type, tags}` → "Fey (Goblinoid)"; alignment letter codes → words
("U"→Unaligned); AC `{ac, from}` → "16 (natural armor)"; HP → "avg (formula)"; speed with
labeled non-walk modes and "(hover)"; saves/skills in canonical order; senses with "Passive
Perception N"; gear with quantities. **CR** is rendered verbose via `cr_xp_pb` →
"17 (XP 18,000; PB +6)". Sections are built by `build_sections` (spellcasting front-loaded,
then Traits/Actions/Bonus/Reactions/Legendary/Mythic, dropping empties); spellcasting buckets
("At Will:", "N/Day:", "Nth Level (N slots):") render spells as clickable links.

**CR sort key.** `cr_token_to_sort(token) -> Option<f64>` (`pub`) is the single fraction
table ("1/8"→0.125, "1/4"→0.25, "1/2"→0.5, else parse). It is reused everywhere CR is
ordered or filtered (the DB projection and the `--cr` CLI filter), so the scale can't drift.

---

## 4. `_copy` variant resolution

`core/src/monster_copy.rs` is a faithful, narrowed port of 5etools'
`DataUtil.generic.copyApplier`. 5etools defines many monsters as a `_copy` of a base monster
plus modifications; this module materializes them as a pure `Value` → `Value` pre-pass
(`resolve_copies(monsters, templates) -> CopyResolution`) **before** `RawMonster`
deserialization, so the rest of the importer never sees a copy.

How it works:

- A `_copy` names a base by `(name, source)`. The base fills in absent keys; `_preserve`
  controls which base-only props (e.g. `legendaryGroup`, `reprintedAs`) are inherited;
  explicit `null` deletes a key.
- `_mod` operations are applied after the base merge — ~21 modes (`appendArr`, `replaceArr`,
  `insertArr`, `removeArr`, `replaceTxt`, `scalarAddHit`, `scalarAddDc`, `addSpells`,
  `maxSize`, `scalarMultXp`, …); `_templates` reference `template.json` entries that
  contribute their own mods and a `_root`. Unknown modes are deliberate no-ops. (The
  `<$...$>` variable resolver is intentionally omitted — it has zero occurrences in real
  data.)
- A resolved copy increments `resolved_copy`. A copy whose **base can't be found**, or a
  **cyclic** copy chain, fails → it is **dropped and counted in `skipped_copy`**, never
  silently lost. The import success message surfaces both counts.

This module is the single biggest reason the bestiary import is larger and more involved than
the spellbook's; it has ~26 dedicated tests covering each mod mode and the
missing-base/cycle cases.

---

## 5. Storage & search projection

Same two layers as spells:

- **Canonical TOML card** — `~/.config/runebound.sh/monsters/<slug>.toml` via the generic `CardStore<Monster>` (`core/src/card_store.rs`; `store_root = ConfigPaths.monsters`).
- **SQLite `monsters` search table** — `MonsterRow` (`core/src/db.rs`): `id` (= slug), `slug`, `name`, `cr`, **`cr_sort: f64`**, `creature_type`, `size`, `source`, timestamps. (The column is `creature_type`, not `type`, which is reserved.) Generated by `impl_entity_table! { table: "monsters", … }` plus explicit `clear_monsters`/`count_monsters`. Migration `core/migrations/0021_monsters.sql` indexes `name`, `cr_sort`, `creature_type`.

Two query paths:

- `search_monsters_by_name` — the `LIKE`-based name typeahead (same shape as spells).
- `search_monsters_filtered(pool, name?, creature_type?, cr_min, cr_max, limit)` — the hand-written filtered search backing `monster --cr/--type`: `LIKE` on name/type when provided, `cr_sort BETWEEN cr_min AND cr_max`, ordered by `cr_sort` then name.

---

## 6. Library service & boot self-heal

`BestiaryLibraryService` (`desktop/src-tauri/src/services/bestiary_library.rs`) mirrors
`SpellLibraryService` exactly, returning the `ImportSummary` from `import_from_dir`:

- `import_from_dir` → parse (`spawn_blocking`) → replace TOML store → `replace_db` (one transaction: `clear_tx` then `upsert_tx(monster_row(...))`).
- `monster_row` projects the searchable columns; `cr_sort` is computed by the local `cr_sort` helper = `cr_token_to_sort` on the leading token of the verbose `cr` string ("1/4 (XP 50…)" → 0.25).
- `project_store_into_db` is the boot self-heal (no-op when the store is empty or the DB count matches), wired into the `"cleanup"` boot task in `boot.rs` after the spell projection.

---

## 7. Commands & routing

- **Handlers** (`desktop/src-tauri/src/commands/monster_commands.rs`): `handle_monster` (`monster <name>` / `monster help`; a stray `monster import` is redirected to `bestiary import`) and `handle_bestiary` (`bestiary import [path]` / `bestiary help`). `resolve_monster_doc` is the shared lookup (slug fast-path → DB name search → `monster_card`).
- **Filters**: `parse_monster_filters` reads `--cr` / `--type` (`flag_value` handles both `--cr 5` and `--cr=5`); `parse_cr_range` accepts a single rating (`5`, `1/4`), or an inclusive range (`10-17`, `1/4-2`), normalizing reversed bounds. `monster_filtered` renders the matches as a clickable list, each row linking to its own `monster <name>` card, capped at **100** results.
- **Import result**: `bestiary_import`'s success doc reports the count and, when nonzero, how many `_copy` variants were resolved and how many were skipped (base not found).
- **Registration / metadata**: `monster_handler_entry()` / `bestiary_handler_entry()` in `commands/mod.rs`; `CommandSpec`s in `command-specs/src/lib.rs` (`execution: Desktop`; `monster` `requires_subcommand: false` with `--cr`/`--type` options, `bestiary` `requires_subcommand: true` with `import`).
- **Bare-name fallback**: `router.rs` `BARE_NAME_PRECEDENCE = [Entity, Spell, Monster]` — `resolve_monster_doc` is the last arm (entity and spell win on a name collision).
- **Typeahead**: `services/suggestions.rs` monster loop gated by `monster_search_context` (skips `monster help`, the bare root, and any `--` filter query so flags aren't searched as names). `SuggestionHelperText::Monster` labels the rows.

---

## 8. Tests / invariants

- `monsters.rs`: single titled card; subtitle alignment join/omit; abilities row scores+modifiers; `format_modifier` rounds toward −∞; bold ability-name prefix; **TOML round-trip** incl. sections/lore + link spans; lore under a trailing heading.
- `monster_import.rs`: `_copy` resolved through the full pipeline (override wins, appended action); missing-base copy skipped + counted; `XMM`-preference dedup; attack-tag prose; 2024 spellcasting buckets; legendary-group regional effects with a clickable spell link; fluff attach/unwrap; sorted output; plus an `#[ignore]`d `real_dataset_imports_cleanly` (set `MONSTER_5E_DIR`).
- `monster_copy.rs`: ~26 tests — each `_mod` mode, `_preserve`, templates, missing-base/cyclic skip-and-count.
- `db.rs`: monster round-trip + `search_monsters_filtered` by type, CR band, open-high band, and name+type combined.
- `monster_commands.rs`: filter parsing (type+CR range, name fragment composing with filters, fractions/equals-form/swapped bounds, unknown-flag/bad-CR rejection).
- `bestiary_library.rs`: `cr_sort` parses the leading token. `router.rs`: precedence guard.

---

## 9. File map

| Concern | File |
|---|---|
| Model + card builder | `runebound-models/src/monsters.rs` |
| 5etools import + conversion | `core/src/monster_import.rs` |
| `_copy` variant resolution | `core/src/monster_copy.rs` |
| Shared markup parser | `core/src/fivetools_markup.rs` |
| Canonical TOML store (generic) | `core/src/card_store.rs` (`impl Card for Monster`) |
| Store path | `core/src/config.rs` (`ConfigPaths.monsters`) |
| Search row + table + queries | `core/src/db.rs` (`MonsterRow`, `impl_entity_table!`, `search_monsters_filtered`, `clear_monsters`/`count_monsters`), `core/migrations/0021_monsters.sql` |
| Import + projection service | `desktop/src-tauri/src/services/bestiary_library.rs` |
| Commands + filters | `desktop/src-tauri/src/commands/monster_commands.rs` |
| Registration | `desktop/src-tauri/src/commands/mod.rs` |
| Command metadata | `command-specs/src/lib.rs` |
| Bare-name routing | `desktop/src-tauri/src/router.rs` |
| Typeahead | `desktop/src-tauri/src/services/suggestions.rs` |
| Boot self-heal | `desktop/src-tauri/src/boot.rs` |
| Repository accessor | `desktop/src-tauri/src/repositories/mod.rs`, `app_state.rs` (`monster_repo()`) |

---

## 10. Related Docs

- `docs/spellbook.md` — the simpler twin (the template this mirrors)
- `docs/architecture.md` §5 — the reference-library pattern
- `docs/feature-development.md` §8 — Playbook G (add a reference library)
- `docs/cli.md` §14 — user-facing command reference
- `docs/render.md` §4 — card rendering & 5etools markup

---

*Last updated: 2026-06-21*
