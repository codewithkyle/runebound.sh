# Cleanup: Spellbook & Bestiary Consolidation

> **Status:** 📋 Planned (2026-06-20). A self-contained work plan for a follow-up agent.
> **Origin:** Post-merge review of the Spellbook (`docs/spellbook.md`) and Monster
> Manual (`docs/monster-manual.md`) features. The features shipped correct, idiomatic,
> and well-tested (204 green in `dnd-core` + `runebound-models`). This doc captures the
> **consolidation/polish follow-ups** the review surfaced — none are bugs in production
> today, but the DRY items (Part A) are active drift vectors and should land before more
> reference-library features (items, feats) are built on top of the copy-pasted scaffolding.

## How to use this doc

- Tasks are grouped **A (DRY — do first)**, **B (tests)**, **C (robustness polish)**,
  **D (consistency & docs)**. Within a part they're independent — do them in any order,
  one commit each.
- Each task states: **why**, the exact **anchors** (file:line as of 2026-06-20 — re-grep
  to confirm, line numbers drift), **steps**, and **acceptance**.
- **Do not** stage/commit on the user's behalf — they review locally. Leave the work
  staged or as separate commits per the repo convention.
- This is a read-only-feature codebase area: spells/monsters are imported reference data,
  not entities. Do **not** try to fold them into `EntityKind`/`EntityDomain` — that
  architecture is for AI-generated, user-editable drafts and does not fit here.

## Verification commands (run after each task)

```bash
# core + shared models (fast; the import/model logic lives here)
cargo test -p dnd-core -p runebound-models

# desktop crate is a SEPARATE workspace — `cargo --workspace` at root SKIPS it.
# Run it explicitly (see memory: "Desktop separate workspace").
cargo test --manifest-path desktop/src-tauri/Cargo.toml
cargo test --manifest-path desktop/src-tauri/Cargo.toml suggestions   # for Part B

# full build last
make build
```

The two `#[ignore]` real-dataset tests (`spell_import.rs:1261`, `monster_import.rs:1965`)
are gated on env vars pointing at a local 5etools copy; run them if you have the data, but
they are not required to pass for these tasks.

---

## Part A — DRY consolidations (do first; these are live drift vectors)

### A1. Collapse the two TOML stores into one generic `CardStore<T>`

**Why.** `core/src/spell_store.rs` (172 lines) and `core/src/monster_store.rs` (201 lines)
are ~95% identical — `new`/`with_root`/`root`/`ensure_dir`/`path_for`/`save`/`load`/`list`/`clear`
and even the test names (`save_load_round_trips`, `missing_*_is_none`,
`clear_then_list_is_empty`) differ only by the noun and the payload type. This is exactly
the shape the crate already generalized: `core/src/entity_store.rs:63,83` has generic
`save_entity<T: Serialize>` / `load_entity<T: DeserializeOwned>` helpers behind thin typed
wrappers. The spell/monster stores ignored that precedent.

**Anchors.**
- `core/src/spell_store.rs` — full file (delete or shrink to a facade).
- `core/src/monster_store.rs` — full file (delete or shrink to a facade).
- `core/src/entity_store.rs:22-90` — the precedent (generic `<T>` helpers + `ENTITY_DIRS`).
- `core/src/config.rs` — `ConfigPaths { spells, monsters, … }` (the roots).
- `core/src/lib.rs` — module declarations (`pub mod spell_store; pub mod monster_store;`).

**Recommended design** (`core/src/card_store.rs`, a new file):

```rust
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::{ConfigPaths, config_paths};

/// A record stored as one `<root>/<slug>.toml` file in a reference library.
/// `core` owns the trait, so it may impl it for the `runebound-models` payload
/// types (the trait is local — orphan rule is satisfied).
pub trait Card: Serialize + DeserializeOwned + Sized {
    /// Human noun for error context ("spell", "monster").
    const NOUN: &'static str;
    /// Stable kebab-case primary key.
    fn slug(&self) -> &str;
    /// Where this card kind's files live under the config dir.
    fn store_root(paths: &ConfigPaths) -> PathBuf;
}

/// Canonical TOML store for an imported reference library. The SQLite search table
/// is a rebuildable projection of this store (see the `*LibraryService`s).
pub struct CardStore<T: Card> {
    root: PathBuf,
    _marker: PhantomData<T>,
}

impl<T: Card> CardStore<T> {
    pub fn new() -> Result<Self> {
        let store = Self::with_root(T::store_root(&config_paths()?));
        store.ensure_dir()?;
        Ok(store)
    }
    pub fn with_root(root: PathBuf) -> Self { Self { root, _marker: PhantomData } }
    pub fn root(&self) -> &Path { &self.root }
    // ensure_dir / path_for / save / load / list / clear: lift verbatim from
    // spell_store.rs, swapping the literal "spell" for `T::NOUN` and the slug
    // source for `card.slug()`.
}

impl Card for runebound_models::spells::Spell {
    const NOUN: &'static str = "spell";
    fn slug(&self) -> &str { &self.slug }
    fn store_root(paths: &ConfigPaths) -> PathBuf { paths.spells.clone() }
}

impl Card for runebound_models::monsters::Monster {
    const NOUN: &'static str = "monster";
    fn slug(&self) -> &str { &self.slug }
    fn store_root(paths: &ConfigPaths) -> PathBuf { paths.monsters.clone() }
}
```

**Steps.**
1. Add `core/src/card_store.rs` as above; lift the fs bodies from `spell_store.rs` once,
   parameterized on `T::NOUN` / `card.slug()`.
2. Declare `pub mod card_store;` in `core/src/lib.rs`; delete the `spell_store`/`monster_store`
   module declarations.
3. Delete `core/src/spell_store.rs` and `core/src/monster_store.rs`. Move their three tests
   into `card_store.rs` once each (generic over `Spell`; one extra `Monster` round-trip test
   is enough to prove the trait wiring).
4. Update the call sites (there are only a handful — grep `SpellStore`/`MonsterStore`):
   - `desktop/src-tauri/src/commands/spell_commands.rs:11,59,61` →
     `use dnd_core::card_store::CardStore;` … `CardStore::<Spell>::new()` … `store.load(&slug)`.
   - `desktop/src-tauri/src/commands/monster_commands.rs:11,262,264` → same shape.
   - `desktop/src-tauri/src/services/spell_library.rs:13,35,55` and
     `bestiary_library.rs:13,40,59` → `CardStore::<Spell>::new()` / `.save(card)` / `.list()` /
     `.clear()`.
   - The `Spell`/`Monster` types are already imported in those files.

**Acceptance.** `spell_store.rs`/`monster_store.rs` are gone; one generic store remains;
all `cargo test -p dnd-core` + desktop tests pass; `make build` green. Net ~150 lines removed.

> If you prefer to minimize call-site churn, you *may* keep `pub type SpellStore =
> CardStore<Spell>;` aliases — but then also rename the call-site methods (`load_spell` →
> `load`, etc.) since the generic drops the noun. The trait-based generic above is the
> target; aliases are a convenience, not the goal.

### A2. Make CR→sort a single function (currently duplicated 4×)

**Why.** The `1/8→0.125, 1/4→0.25, 1/2→0.5, else parse` mapping exists in four places. A new
fraction (or a future `"1/16"`) means four lockstep edits with nothing enforcing them — the
textbook drift trap.

**Anchors (the four copies).**
- ✅ **Canonical, keep:** `core/src/monster_import.rs:1209` `pub fn cr_to_sort(value: &Value) -> f64`
  — operates on the *raw JSON* CR (`String` / `Number` / `{cr, …}` object).
- ❌ `desktop/src-tauri/src/services/bestiary_library.rs:113` `fn cr_sort(display_cr: &str) -> f64`
  — re-parses from the *formatted display string* ("1/4 (XP 50…)").
- ❌ `desktop/src-tauri/src/commands/monster_commands.rs:175` `fn parse_cr_token(token: &str) -> Option<f64>`
  — the fraction arm duplicates the table.
- A fragment also lives in `core/src/monster_copy.rs` (CR handling) — leave it if it's not the
  same public mapping, but note it in your commit message.

**Steps.**
1. Add a small string-input helper next to the canonical one in `monster_import.rs`, e.g.
   `pub fn cr_token_to_sort(token: &str) -> Option<f64>` holding the single fraction table, and
   have `cr_to_sort` call it for its string arm. (Two public entry points — one for raw JSON,
   one for a display/CLI token — sharing one table.)
2. `bestiary_library.rs:113`: delete `cr_sort`; in `monster_row` (`:102`) call
   `dnd_core::monster_import::cr_token_to_sort(monster.cr.split_whitespace().next().unwrap_or("")).unwrap_or(0.0)`.
   Keep the existing `cr_sort_parses_the_leading_token` test by repointing it at the shared fn,
   or move it.
3. `monster_commands.rs:175`: delete `parse_cr_token`; route `parse_cr_range` through
   `cr_token_to_sort`. The existing `parse_cr_range_*` / `cr_accepts_fractions_*` tests must
   stay green unchanged.

**Acceptance.** Exactly one fraction table in `core`; the service and command both call it;
all monster tests pass. Grep `=> 0.125` / `=> 0.25` returns one production site (plus
`monster_copy.rs` if its fragment is genuinely different math).

### A3. Move `render_table` (and the table `to_text` arm) into `output.rs`

**Why.** `render_table` is **byte-for-byte identical** in `runebound-models/src/spells.rs:168`
and `runebound-models/src/monsters.rs:310` (the monster copy's own comment says "the monster
twin of `spells::render_table`"). Both also compute column width with `.len()` (byte length —
a latent bug for multi-byte cells). One shared impl fixes the width math once.

**Anchors.**
- `runebound-models/src/spells.rs:168-207` `fn render_table` (+ the `Table` arm of
  `SpellBlock::to_text`, `:82-88`).
- `runebound-models/src/monsters.rs:310-349` `fn render_table` (+ `StatBlock::to_text`, `:141-147`).
- `runebound-models/src/output.rs` — destination (both model files already
  `use crate::output::{…}`).

**Steps.**
1. Add `pub fn render_table(headers: &[String], rows: &[Vec<String>]) -> String` to `output.rs`
   (lift one copy verbatim). Optional, while you're here: change the width measure from
   `str::len()` to a display-width count (`chars().count()` is the cheap fix; a Unicode-width
   crate is overkill for stat tables). Do it once, here.
2. Delete both private `render_table`s; call `crate::output::render_table(...)` from
   `push_spell_block` (`spells.rs:162`) and `push_stat_block` (`monsters.rs:290`).
3. The identical `to_text` table-flattening arms can stay (they're 6 lines and tightly coupled
   to each enum) — or extract a tiny `pub fn join_table_text(headers, rows) -> String` if you
   want zero duplication. Either is acceptable; note your choice.

**Acceptance.** One `render_table` in `output.rs`; `table_block_renders_aligned_columns`
(`spells.rs`) and the monster round-trip/table tests pass.

### A4. Lift `title_case` / `capitalize` into one shared helper

**Why.** Low severity, pure noise: `title_case` is copied in `spell_import.rs:909`,
`monster_import.rs:1461`, `monster_copy.rs:1325`; `capitalize` in `monster_import.rs:1469` and
`monster_commands.rs:184`. **Also verify** a possible `slugify` duplication while here:
`runebound-models/src/utils.rs:245` has `pub fn slugify` *and* `core/src/spell_import.rs:894`
exports its own `pub fn slugify` (the one everything imports). Confirm whether they're identical;
if so, collapse to one.

**Steps.**
1. Pick one home. `core` string utils are import-side; `runebound-models/src/utils.rs` already
   hosts `slugify`. Put `title_case` / `capitalize` next to `slugify` in whichever crate the
   most callers sit in (likely a small `core` util module, since most callers are in `core`'s
   importers). Don't create a new crate.
2. Replace the copies with calls. Keep each function's exact current behavior (they're tested
   indirectly via the importer fixtures — re-run `cargo test -p dnd-core`).
3. For `slugify`: if `utils::slugify` == `spell_import::slugify`, re-export one and delete the
   other; if they differ subtly, leave a comment explaining why both exist (don't silently
   merge two different normalizers — that would shift slugs and break stored TOML lookups).

**Acceptance.** One `title_case`, one `capitalize`; slugify duplication resolved or documented;
no slug values change (critical — a slug change orphans every stored `<slug>.toml`).

---

## Part B — Test coverage gaps

### B1. Suggestion-path tests (required by the playbook)

**Why.** `docs/feature-development.md` Playbook E step 3 and `docs/command-contexts.md` §6
mandate "every new suggestion path gets a test." The spell/monster typeahead has **zero**
coverage — every other path in that test module is tested.

**Anchors.**
- `desktop/src-tauri/src/services/suggestions.rs:663` `fn spell_search_context`
- `desktop/src-tauri/src/services/suggestions.rs:686` `fn monster_search_context`
- The typeahead loops at `:157-201` (map to `CommandSuggestion` with
  `SuggestionHelperText::Spell` / `::Monster`).
- Test module: `desktop/src-tauri/src/services/suggestions.rs:993`.

**Steps.** Add tests covering, for each of spell and monster:
1. Explicit root (`spell fire` / `monster gob`) returns name suggestions with the right
   `completion` form and helper text.
2. The bare-name fallback path (no command prefix) surfaces matches, but a known command root
   does **not** trigger it.
3. `spell help` / `monster help` and the bare root are excluded (not treated as a search
   fragment).
4. For monster: a `--cr` / `--type` filter query is handled without crashing the suggestion
   builder (it shouldn't emit name typeahead for a filter query).

Use the existing test harness in that module (it already builds suggestions against a manifest
+ state). Follow the shape of the nearest existing entity-search suggestion test.

**Acceptance.** `cargo test --manifest-path desktop/src-tauri/Cargo.toml suggestions` shows the
new tests; all green.

### B2. Pin the bare-name resolution precedence

**Why.** Entity → spell → monster precedence lives only in `router.rs` runtime ordering and the
parallel order of the suggestion loops; nothing pins it. A future reorder would silently change
which card a name collision resolves to.

**Steps.** Add one test (router-level or a focused unit) asserting that when a name exists as
both a spell and a monster, the spell wins; and document the precedence in a one-line comment at
the router fallback site if not already explicit. Keep it small — this is a guard, not a feature.

**Acceptance.** A failing test if someone swaps the spell/monster fallback order.

---

## Part C — `monster_copy.rs` robustness polish

> Context: `core/src/monster_copy.rs` is a faithful, well-contained port of 5etools'
> `_copy`/`_applyCopy` engine. The review found **no input-triggerable panic** — its
> "degrade, don't crash" design is sound. These are hardening + faithfulness items, not bugs.

### C1. Malformed-input tests (the headline property is untested)

**Why.** "No panic on evolving 3rd-party data" is this module's whole reason to exist, yet no
test feeds it garbage. Add ~3-4 cases:
- a `_copy` that is a non-object / a `_mod` whose value is a string instead of an array;
- an `items`/`names` that is a scalar where an array is expected;
- an `insertArr` with an out-of-range or negative index;
- a mod targeting a prop whose existing value is the wrong type (e.g. `appendArr` onto a string).

Each should resolve to a graceful no-op or skip, **not** panic. Anchor the dispatch at
`monster_copy.rs:414` (`do_mod`).

### C2. Cover the untested `_mod` modes

**Why.** ~10 modes have zero coverage, several with the trickiest arithmetic/string logic:
`scalarMultXp` (`:1029`), `maxSize` (`:999`), `removeSpells` (`:939`), multi-segment `setProp`
(`set_path`, `:621`/`:1276`), `prefixSuffixStringProp` (`:626`), `scalarAddHit`/`scalarAddDc`
(`:693`), and the two-digit `$NN` group fallback in `expand_replacement` (`:1196-1221`). Add a
focused happy-path test per mode.

### C3. Faithfulness + idiom nits (optional, low priority)

- `monster_copy.rs:1043` `scalarMultXp` truncates (`as i64`) where 5etools rounds — match the
  source (round-half-up) for the rare non-floor case.
- `monster_copy.rs:727` — the one regex built from input is `.unwrap()`; use `.ok()` +
  early-return (it's `regex::escape`d so it can't actually fail, but this removes the only
  input-derived `unwrap` in the file).
- `LazyLock`-hoist the static regexes (`:697-698`, and the per-call rebuild in `mod_add_senses`)
  — negligible perf, more idiomatic.

**Acceptance for C.** New tests green; if you do C3, behavior unchanged on the real dataset
(run the `#[ignore]` import test if you have the data).

---

## Part D — Consistency & docs

### D1. Resolve the spell cross-link asymmetry

**Why.** Monster card bodies carry `Vec<Span>` and lower `{@spell}` / `{@creature}` markup to
clickable `command_ref`s; spell card bodies are plain `String` and don't. So `{@spell fireball}`
is clickable **inside a monster stat block** but renders as plain text **inside a spell card**.
It's a known deferral (`docs/spellbook.md §11`), but the `Span` machinery now exists, so the fix
is cheap and makes the two consistent (and lets spell→spell references click).

**Decision required (ask the user if unsure):** either
- **(a)** lift `SpellBlock::Text`/`Bullets` from `String` to `Vec<Span>` (mirror `StatBlock`),
  switch spell lowering from `strip_tags` to `render_inline`, and update `spell_card`'s
  `push_spell_block` to map spans → inlines (copy `monsters.rs:297` `spans_to_inlines`); **or**
- **(b)** explicitly decide spell cards stay link-free and update `docs/spellbook.md §11` to say
  so deliberately (close the "deferred" item as "won't do").

If you do (a): `Spell` is stored as TOML, so this changes the on-disk card shape — bump nothing
(no migration for TOML), but note that a re-import is needed to repopulate spell stores with
spans (old `<slug>.toml` files parse fine into the new shape only if `Span` is `#[serde(...)]`
compatible — verify the round-trip test, mirroring `monsters.rs:522`).

**Anchors.** `runebound-models/src/spells.rs:61` (`SpellBlock`), `:151` (`push_spell_block`);
`core/src/spell_import.rs` lowering (uses `strip_tags`; `render_inline` already exists at `:683`).

### D2. Register the "imported reference library" pattern in `architecture.md`

**Why.** These features introduce a genuine **third** first-class pattern beside *entities* and
*wizards*: an **imported read-only reference library** (canonical TOML store → SQLite search
projection → router bare-name fallback → a read-only repo with no `upsert`/`delete` surface).
`docs/architecture.md` documents entities and wizards extensively but never mentions this one —
its own closing rule is "update in the same PR as the architecture change," and that was missed.
Without it, the next library feature (items, feats, magic items) has no documented grain to follow
except by reading the spell/monster code.

**Steps.** Add a short section to `docs/architecture.md` (a `§4` sibling, e.g. "Reference
Library Architecture") describing:
- the two-layer store (TOML card payload + SQLite search index, store is source of truth);
- the boot re-projection self-heal (`*LibraryService::project_store_into_db`);
- the read-only repository shape (search/find/upsert_tx/clear — no editable-entity methods);
- the router bare-name fallback and its **entity → spell → monster** precedence;
- a one-line "when to reach for this" vs entity/wizard (read-only imported data that is never
  user-edited or AI-generated).
Cross-reference `docs/spellbook.md` and `docs/monster-manual.md` as the worked examples. After
A1–A4 land, mention `CardStore<T>` as the shared store primitive. Bump the "Last updated" date.

### D3. (Optional) Rename the shared markup seam

**Why.** `monster_import.rs:30` does `use crate::spell_import::{render_inline, slugify, strip_tags};`
— functionally fine (same crate) but reads oddly ("monsters import from spells"). The shared
5etools markup parser is the single best-factored piece of the feature; a neutral home would say
so.

**Steps (only if low-risk).** Move `strip_tags`/`render_inline`/`render_tag`/`matching_brace`/
`tag_link`/`is_wrapper_tag`/`slugify` from `spell_import.rs:612-894` into a new
`core/src/fivetools_markup.rs` (or `markup.rs`); re-point both importers' `use`s. This is a pure
move — keep every function body and test identical, run the full `dnd-core` suite. Skip if it
risks churn during the same window as A1; it has no correctness payoff, only clarity.

**Acceptance for D.** Chosen D1 path implemented or documented; `architecture.md` has the new
section; any move in D3 leaves all tests green.

---

## Suggested order

1. **A1, A2, A3, A4** (DRY — each its own commit; A1 is the biggest win).
2. **B1, B2** (lock in the new/parallel surfaces with tests).
3. **C1, C2** (robustness tests), then **C3** if time permits.
4. **D2** (cheap, high-value docs), **D1** (needs a product decision — ask the user), **D3** (optional).

## Explicitly out of scope (leave as-is)

- The **importer scaffolding** (`dedup_to_canonical`, `locate_*_dir`, the `Raw*` schemas, the
  per-field formatters) — divergent by design; the monster side carries `_copy`/fluff/legendary
  logic with no spell analog. Generalizing would re-monolithize.
- The **command handlers' overall shape** — the shared skeleton is small/stable and the monster
  filter UI is genuine, tested divergence.
- The **card builders** (`spell_card`/`monster_card`, `push_*_block`) beyond A3 — the
  `Span`-vs-`String` payload split is a real type difference (and D1 may erase it anyway).
- The **DB/repository layer** — already DRY via `impl_entity_table!` (one declaration each at
  `core/src/db.rs:429`/`:478`); the review found no issues there.
- The **transaction / boot self-heal model** — correct as built. The count-equality short-circuit
  in `project_store_into_db` is an accepted trade-off (single-writer store); leave it.

---

*Last updated: 2026-06-20*
*Derived from the post-merge review of `docs/spellbook.md` + `docs/monster-manual.md`.*
</content>
</invoke>
