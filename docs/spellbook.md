# Spellbook — Spell Reference Library

> **Status:** Implemented. **Purpose:** the worked example of the reference-library
> pattern (`docs/architecture.md` §5, `docs/feature-development.md` §8). It documents
> the *shipped* spell lookup feature concretely — the data model, the 5etools import,
> the two-layer store, the commands, and the wiring. The monster bestiary
> (`docs/monster-manual.md`) is the twin of this feature.

---

## 1. What it is

A read-only spell reference inside the TUI:

- `spell <name>` renders a **spell card** (casting stats + description + higher-level scaling). Typeahead surfaces matches as you type; a bare spell name (no `spell` prefix) also resolves via the router fallback.
- `spellbook import [path]` builds the library from the user's **own local copy** of [5e.tools](https://5e.tools/) data. No path → native folder picker.

It is **not** an entity (no create/edit/save/reroll) and not a wizard. It is the third feature shape: an external dataset imported in bulk, rendered for lookup. Command-surface details live in `docs/cli.md` §14.

### Design decisions (settled — don't re-litigate)

| Decision | Choice | Why |
|---|---|---|
| Ship spell data in-repo? | **No** — import from a user-supplied path at runtime | 5etools spell text is largely WotC-copyrighted and this repo has a public remote. The user importing their own local copy sidesteps redistribution; no SRD filtering needed. |
| Scope | **Lookup + render only** | No "my spellbook" collection, no add/remove/list. The imported set *is* the library. |
| Edition / dedup | **2024-canonical**, prefer `XPHB` over `PHB`, drop `reprintedAs`, all official sources, no homebrew | `XPHB` (2024 PHB) is the canonical reprint of most 2014 spells. |
| Store | **per-spell TOML card (source of truth) + SQLite `spells` search projection** | Mirrors the entity-store grain; the DB is a rebuildable index, the TOML card is the full payload. |

Measured corpus: a real 5etools checkout yields **~554** spells after 2024-dedup (the `real_dataset_imports_cleanly` smoke test in `core/src/spell_import.rs` expects 520–600).

---

## 2. Data model

`runebound_models::spells::Spell` (`runebound-models/src/spells.rs`) is the converted,
render-ready form — the canonical TOML payload **and** the search-index source. It is
backend-only (`Serialize`/`Deserialize`, **not** `TS`); only the rendered `OutputDoc`
crosses to the frontend.

Fields: `slug` (kebab-case primary key), `name`, `source`, `level` (0 = cantrip),
`school` (expanded, e.g. "Evocation"), `casting_time`, `range`, `components`,
`duration`, `ritual`, `concentration`, `classes` (usually empty for 2024 core),
`description: Vec<SpellBlock>`, `higher_levels: Option<Vec<SpellBlock>>`.

`SpellBlock` is deliberately **flat** (no recursion) so it serializes cleanly to TOML —
a named subsection lowers to a `Heading` followed by its children rather than nesting:

- `Text { spans: Vec<Span> }` — prose; `Span`s carry cross-links
- `Heading { text }` — subsection title (plain text)
- `Bullets { items: Vec<Vec<Span>> }`
- `Table { headers, rows }` — rendered as a fixed-width `code_block` (there is no `OutputBlock::Table`)

### Card rendering

`spell_card(&Spell) -> OutputDoc` (`spells.rs`) builds **one** `entity_card_full`:

- **title** = name; **subtitle** = `level_school_line` — `"{school} Cantrip"` or `"Level {n} {school}"`, with `" (Ritual)"` appended when ritual
- **rows** = `Casting Time:`, `Range:`, `Components:`, `Duration:`
- **body** = the `description` blocks, then `higher_levels` (each set already starts with its own `Heading`), then an emphasized provenance footer (`"Source: {source}"`, plus `" · Classes: …"` when present)

`push_spell_block` maps each `SpellBlock` to an `OutputBlock`; `Text`/`Bullets` lower their `Span`s through `spans_to_inlines`, so a `{@spell}`/`{@creature}` cross-link becomes a clickable `command_ref` (see §6).

---

## 3. Import & conversion

`core/src/spell_import.rs` is a pure converter: 5etools JSON → `Vec<Spell>`. Entry point
`import_spells_from_dir(dir) -> Result<Vec<Spell>>`.

**Source layout.** `locate_spells_dir` accepts the repo root, its `data/spells`, or a
`spells` dir directly — whichever contains `index.json`. `index.json` maps source code →
filename; the importer reads **only** the per-source `spells-*.json` files it names
(`homebrew/` is ignored). Each file is `{ "spell": [ … ] }`.

**Dedup → canonical.** `dedup_to_canonical` drops any entry whose `reprintedAs` is set
(superseded), then keeps one entry per name **preferring `XPHB`**. `disambiguate_slugs`
suffixes the rare genuine slug collision with `-{slugify(source)}`. Output is sorted by
lowercased name for determinism.

**Field formatting** (each raw shape → a display string):

- `school` single-letter codes (A/C/D/E/V/I/N/T) → full names; unknown → "Unknown"
- `casting_time` → "Action" / "Bonus Action" / "Reaction…" / pluralized minutes/hours/rounds
- `range` → point distances (feet/miles/touch/self/…) or self-origin areas → `"Self ({n}-foot {Shape})"`
- `components` → "V", "S", "M (…)"; the material object's `{text, cost}` form is stringified
- `duration` → "Instantaneous" / "Concentration, up to N units" / "Until Dispelled" / …
- `higher_levels` = the `entriesHigherLevel` content, or a synthesized "Cantrip Upgrade" block derived from `scalingLevelDice` for cantrips

**Entries → `SpellBlock`.** `lower_entries`/`lower_block` map 5etools entry objects:
`"list"` → `Bullets`, `"table"` → `Table`, anything else → a `Heading` (from its `name`)
followed by recursively-lowered children — content is never dropped. All prose runs through
the shared markup parser (next section).

---

## 4. Storage & search projection

Two layers, the standard reference-library shape:

- **Canonical TOML card** — `~/.config/runebound.sh/spells/<slug>.toml`, one file per spell, holding the full `Spell` payload. Managed by the generic `CardStore<Spell>` (`core/src/card_store.rs`; `impl Card for Spell` sets `NOUN`, `slug()`, and `store_root = ConfigPaths.spells`). There is no per-kind store module — `CardStore<T>` is shared with monsters.
- **SQLite `spells` search table** — a rebuildable projection holding only searchable columns. `SpellRow` (`core/src/db.rs`): `id` (= slug), `slug`, `name`, `level`, `school`, `source`, `ritual`, `concentration`, timestamps. The table + CRUD come from one `impl_entity_table! { table: "spells", … }` declaration, which generates `upsert_spell`, `find_*`, `list_spells`, `search_spells_by_name`, etc. Two explicit helpers — `clear_spells` and `count_spells` — round out the read-only surface (no soft-delete/undo: the library is replaced wholesale on import). Migration: `core/migrations/0020_spells.sql` (indexes on `name`, `level`, `school`).

`search_spells_by_name` is a `LIKE`-based substring typeahead (`WHERE lower(name) LIKE
'%…%'`, ordered by name) — no FTS5; the deduped corpus is small.

---

## 5. Library service & boot self-heal

`SpellLibraryService` (`desktop/src-tauri/src/services/spell_library.rs`) orchestrates both
layers. Parse and file IO run inside `tokio::task::spawn_blocking` (blocking work off the
async runtime).

- `import_from_dir(dir, state) -> Result<usize, String>`: parse via `import_spells_from_dir` → **replace** the TOML store (`CardStore::clear()` then `save` each) → `replace_db` → return the count.
- `replace_db` runs in **one transaction**: `clear_tx` then `upsert_tx(spell_row(spell, ts))` per spell. `spell_row` projects the searchable columns; the full card stays in TOML.
- `project_store_into_db(state)` is the **boot self-heal**: it lists the TOML store and re-projects into the DB, so a deleted/rebuilt `app.db` recovers the library with no re-import. It **no-ops** when the store is empty or when `db_count == store_len` (the steady state). Wired into the `"cleanup"` boot task (`desktop/src-tauri/src/boot.rs`).

---

## 6. Cross-links & the shared markup seam

Spell description text keeps 5etools cross-links. The parsing happens **once at import
time**, not at render time:

```
import: {@spell Fireball|XPHB}  --fivetools_markup::render_inline-->  Span::Link { label:"Fireball", command:"spell Fireball" }
        {@damage 8d6}, {@dc 15}, wrapper tags, plain text  -->  Span::Text
store:  the Spans live inside the <slug>.toml card
render: spans_to_inlines maps Span::Link -> InlineNode::CommandRef -> clickable button (renderer.tsx)
```

`core/src/fivetools_markup.rs` is the single shared parser — `render_inline` (text +
clickable links), `strip_tags` (plain text), `slugify` (the primary key). **Both** the
spell and monster importers parse through it, so the two can't drift. Only `{@spell}` and
`{@creature}` map to commands (`spell <name>` / `monster <name>`); every other tag
collapses to its display text.

> **Note:** `fivetools_markup::slugify` is **not** `runebound_models::utils::slugify` — it
> turns every non-alphanumeric run into a single dash ("Tasha's" → `tasha-s`). It must stay
> stable, or every stored `<slug>.toml` card orphans.

The rendering contract (the full `OutputDoc` → JSX path) is documented in `docs/render.md` §4.

---

## 7. Commands & routing

- **Handlers** (`desktop/src-tauri/src/commands/spell_commands.rs`): `handle_spell` (`spell <name>` / `spell help`) and `handle_spellbook` (`spellbook import [path]` / `spellbook help`). `resolve_spell_doc(state, query)` is the shared lookup — slug fast-path (`CardStore::load(slugify(query))`), then a DB name search, then `spell_card`. When nothing is imported, `spell_not_found` returns a clickable `spellbook import` prompt.
- **Registration**: `spell_handler_entry()` / `spellbook_handler_entry()` are registered in `desktop/src-tauri/src/commands/mod.rs`; the `CommandSpec`s live in `command-specs/src/lib.rs` (`execution: Desktop`; `spell` `requires_subcommand: false`, `spellbook` `requires_subcommand: true` with an `import` subcommand).
- **Bare-name fallback**: a name with no command root is resolved in `router.rs` via `BARE_NAME_PRECEDENCE = [Entity, Spell, Monster]` — `resolve_spell_doc` is the Spell arm. First hit wins (a saved entity beats a spell); the order is pinned by a test.
- **Typeahead**: `services/suggestions.rs` mirrors the router with a spell loop gated by `spell_search_context` (explicit `spell <fragment>` prefixes the completion; a bare fragment with no known root does not). `SuggestionHelperText::Spell` labels the rows.

---

## 8. Tests / invariants

- `spells.rs`: card is a single titled `entity_card`; subtitle wording (cantrip/leveled/ritual); higher-level blocks render inside the body; **TOML round-trip** across all `SpellBlock` variants; `{@spell}` link → `command_ref`.
- `spell_import.rs`: `XPHB`-preference dedup drops reprints; core-field conversion; cantrip scaling synthesis; list/subsection/table lowering; material-object stringification; self-origin area ranges; sorted output; plus an `#[ignore]`d `real_dataset_imports_cleanly` smoke test (set `SPELL_5E_DIR`).
- `card_store.rs`: `CardStore<Spell>` save/load round-trip, missing-is-none, clear-then-empty.
- `router.rs`: `BARE_NAME_PRECEDENCE` order guard.
- `suggestions.rs`: spell typeahead gating (explicit root prefixes; bare fragment falls back but not for command roots; excludes `spell help`/bare root).

---

## 9. File map

| Concern | File |
|---|---|
| Model + card builder | `runebound-models/src/spells.rs` |
| 5etools import + conversion | `core/src/spell_import.rs` |
| Shared markup parser | `core/src/fivetools_markup.rs` |
| Canonical TOML store (generic) | `core/src/card_store.rs` (`impl Card for Spell`) |
| Store path | `core/src/config.rs` (`ConfigPaths.spells`) |
| Search row + table + queries | `core/src/db.rs` (`SpellRow`, `impl_entity_table!`, `clear_spells`/`count_spells`), `core/migrations/0020_spells.sql` |
| Import + projection service | `desktop/src-tauri/src/services/spell_library.rs` |
| Commands | `desktop/src-tauri/src/commands/spell_commands.rs` |
| Registration | `desktop/src-tauri/src/commands/mod.rs` |
| Command metadata | `command-specs/src/lib.rs` |
| Bare-name routing | `desktop/src-tauri/src/router.rs` |
| Typeahead | `desktop/src-tauri/src/services/suggestions.rs` |
| Boot self-heal | `desktop/src-tauri/src/boot.rs` |
| Repository accessor | `desktop/src-tauri/src/repositories/mod.rs`, `app_state.rs` (`spell_repo()`) |

---

## 10. Related Docs

- `docs/monster-manual.md` — the twin feature (monsters), with `--cr`/`--type` filters and `_copy` resolution
- `docs/architecture.md` §5 — the reference-library pattern
- `docs/feature-development.md` §8 — Playbook G (add a reference library)
- `docs/cli.md` §14 — user-facing command reference
- `docs/render.md` §4 — card rendering & 5etools markup

---

*Last updated: 2026-06-21*
