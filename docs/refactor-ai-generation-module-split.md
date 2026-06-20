# Refactor Plan — split `services/ai_generation.rs` into a per-kind module (v0.7.0)

> **Status: IMPLEMENTED** on branch `chore/ai-cleanup`. The 3,465-line file is now a
> 10-file directory (mod.rs is 29 lines); 229 tests pass and `make lint` is clean; the
> 11 consumer files are untouched. Two refinements from the plan below: (1) the six
> `*_GEN_SAMPLING` consts stayed in `engine.rs` alongside `SeedSampling` (the doc
> comment compares them as a set) rather than moving to kind modules; (2) `opt_clause`
> turned out to be shared by both faction and location prompt builders, so it lives in
> `engine.rs`, not `location.rs`. (3) `EventSeed`, `ItemSeed`, and
> `PromptReferenceContext` were `pub` but referenced by no consumer, so their glob
> re-exports were pruned to satisfy `-D warnings` — they remain reachable in-crate via
> their submodule. (4) `faction.rs` landed at ~970 lines (not the estimated ~700);
> still single-concern, but the natural next split is `faction_woac.rs` (knowledge
> tables) + `faction_wizard.rs` (prompt builders) if it keeps growing.

> **For the implementing agent.** This is a self-contained work order; you do not
> need prior conversation context. The goal is a **pure structural refactor** of one
> 3,465-line file into a module directory — **no behavior change**. Proof of success
> is mechanical: every existing test passes *unchanged*, and `make lint` is clean
> (0 clippy warnings) before and after. Do the phases in order; the tree compiles
> green after **every** phase. Line numbers are accurate as of branch
> `chore/ai-cleanup` — regenerate them with the locator command in §3 if the tree
> has moved.

## Orientation (read first)

- **Two cargo workspaces.** The root workspace (`dnd-core`, `command-handler`,
  `command-specs`, `runebound-models`, `wizard`) **excludes** the Tauri crate
  (`Cargo.toml`: `exclude = ["desktop/src-tauri"]`). All of this work is inside
  `desktop/src-tauri`, which is its own workspace. `cargo build --workspace` at the
  root does **not** compile it. Build/test/lint it explicitly:
  - `cargo check  --manifest-path desktop/src-tauri/Cargo.toml`
  - `cargo test   --manifest-path desktop/src-tauri/Cargo.toml`
  - `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `cargo fmt    --check --manifest-path desktop/src-tauri/Cargo.toml`
  - Or run the project's gate for both workspaces at once: `make lint`.
- **No behavior change.** This is a move-and-re-export refactor. Do **not** rewrite
  prompt strings, sampling values, control flow, or test assertions. If you find a
  bug or a cleanup you want to make, note it for the "Future work" list — do not do
  it in this pass (it would pollute an otherwise mechanical, trivially-reviewable diff).
- **Do NOT commit.** The repo owner reviews locally; do not `git commit` and do not
  stage unless explicitly asked. Hand off green.

---

## 1. Context / why

`desktop/src-tauri/src/services/ai_generation.rs` has grown to **3,465 lines / 157 KB**
— roughly 3× the next-largest service. It is a "god file" that interleaves eight
unrelated concerns:

| # | Concern | What it is |
|---|---------|-----------|
| 1 | **Generation engine** | kind-agnostic retry loop, payload assembly, token-budget math, sampling struct |
| 2 | **Service surface** | `impl AiGenerationService` — 10 `generate_*` methods, ~1,160 lines |
| 3 | **Faction domain knowledge** | WOAC lord/control/mandate/reach tables + fact lookups |
| 4 | **Prompt builders** | faction + location wizard system prompts, JSON schemas, user prompts |
| 5 | **Seed DTOs** | `NpcSeed`, `LocationSeed`, `FactionSeed`, `GodSeed`, `ItemSeed`, `EventSeed`, `Dungeon*` |
| 6 | **Recency / dedup** | per-kind "describe recently-generated seeds so the model avoids repeats" helpers |
| 7 | **Vault `@reference` grounding** | read vault, find @-mentioned entities, emit authoritative metadata block |
| 8 | **Tests** | ~540 lines of unit tests at the bottom |

**Why a per-kind split (not an arbitrary one).** The codebase already organizes
entity logic by kind: `entities/domains/{npc,location,faction,god,item,event,dungeon}_domain.rs`.
Splitting `ai_generation` the same way mirrors an established convention and gives each
entity kind a single **vertical slice** — its DTO, sampling, prompt builders, recency
helpers, generation method(s), and tests all in one file. Shared, kind-agnostic
machinery (the engine, the vault-reference subsystem) becomes its own module.

**Why this is low-risk.** Structural analysis confirms the seams are clean:
- The engine is **fully generic** — `run_seed_attempts<T>` / `build_seed_payload`
  reference no concrete seed type.
- There is **no cross-kind coupling** (e.g., the dungeon helpers reference no
  faction/location helpers, and vice-versa).
- The reference subsystem has a **single async entry point** (`build_reference_context`)
  that all generators call the same way.

---

## 2. Target structure

Convert the single file into a directory. The submodule files are **private** (`mod foo;`);
the parent `mod.rs` re-exports the public surface, so the flat path
`crate::services::ai_generation::X` keeps resolving for all 11 consumers (see §4).

```
desktop/src-tauri/src/services/ai_generation/
├── mod.rs        # module decls + AiGenerationService struct + the re-export wall (§4)
├── engine.rs     # concern 1: SeedGeneration<T>, SeedSampling, capacity_notice,
│                 #   build_seed_payload, SeedStep<T>, run_seed_attempts<T>,
│                 #   reference_system_suffix, parse_recent_seeds<T>, OUTPUT_RESERVE_TOKENS,
│                 #   SYSTEM_BOILERPLATE_TOKENS  (+ engine tests)
├── reference.rs  # concern 7: PromptReferenceContext, build_reference_context (entry),
│                 #   build_prompt_reference_context, canonical_metadata_map,
│                 #   extract_runebound_toml, reference_payload_from_markdown (+ its tests)
├── npc.rs        # NpcSeed, NPC_GEN_SAMPLING, npc recency helpers, generate_npc_seed (+ tests)
├── location.rs   # LocationSeed, LocationBranch + path helpers, LocationWizardInputs,
│                 #   location prompt builders/schema, LOCATION_GEN_SAMPLING,
│                 #   location recency helpers, generate_location_seed[_for_wizard] (+ tests)
├── faction.rs    # FactionSeed, FactionCategory + path helpers, WOAC tables + fact fns,
│                 #   FactionWizardInputs, faction prompt builders/schema,
│                 #   FACTION_GEN_SAMPLING, faction recency helpers,
│                 #   generate_faction_seed[_for_wizard] (+ tests)
├── god.rs        # GodSeed, GOD_GEN_SAMPLING, god recency helpers, generate_god_seed
├── item.rs       # ItemSeed, ITEM_GEN_SAMPLING, generate_item_seed
├── event.rs      # EventSeed, EVENT_GEN_SAMPLING, event recency helpers, generate_event_seed
└── dungeon.rs    # Dungeon{BeatSeed,Seed,Story,Structured,StructuredBeat}, all dungeon
                  #   prompt-block helpers, generate_dungeon_story, structure_dungeon_story
```

Each `generate_*` method moves into its kind module as its **own** `impl AiGenerationService`
block. Inherent impls may live in any module of the defining crate, so:

```rust
// in faction.rs
use super::AiGenerationService;
impl AiGenerationService {
    pub async fn generate_faction_seed(/* … */) { /* unchanged body */ }
    pub async fn generate_faction_seed_for_wizard(/* … */) { /* unchanged body */ }
}
```

**One judgment call (resolved):** `god`/`item`/`event` are small (~100–150 lines each).
Keep them as separate files anyway — it's consistent with the per-kind convention and
the `entities/domains/` layout. Do **not** lump them into a `misc.rs`.

---

## 3. Symbol locator (run this first)

Regenerate the authoritative item→line map (use it to drive your `sed`/cut extraction
and to confirm nothing shifted):

```sh
grep -nE '^(pub |pub\(crate\) )?(async )?(fn|struct|enum|trait|impl|mod|const|static|type) |^    (pub |pub\(crate\) )?(async )?fn ' \
  desktop/src-tauri/src/services/ai_generation.rs
```

For reference, the §6 tables list every item with its line number as of `chore/ai-cleanup`.

---

## 4. The linchpin — `mod.rs` re-export wall (zero-touch for consumers)

11 modules import from `ai_generation`. **Do not touch any of them.** Instead make
`mod.rs` re-export the exact public + `pub(crate)` surface they use. The submodules
stay private; the flat path is preserved by these re-exports.

```rust
// desktop/src-tauri/src/services/ai_generation/mod.rs

mod engine;
mod reference;
mod npc;
mod location;
mod faction;
mod god;
mod item;
mod event;
mod dungeon;

/// Stateless namespace for all seed/story generators. Inherent `impl` blocks for
/// this type are split across the per-kind submodules above.
pub struct AiGenerationService;

// ---- public surface (glob re-exports each submodule's `pub` items) ----
pub use engine::*;       // SeedGeneration
pub use reference::*;    // PromptReferenceContext
pub use npc::*;          // NpcSeed
pub use location::*;     // LocationSeed, LocationBranch, LocationWizardInputs, location_* fns
pub use faction::*;      // FactionSeed, FactionCategory, FactionWizardInputs, faction_* fns, WOAC consts
pub use god::*;          // GodSeed
pub use item::*;         // ItemSeed
pub use event::*;        // EventSeed
pub use dungeon::*;      // DungeonSeed, DungeonBeatSeed, DungeonStory

// ---- pub(crate) cross-service surface (glob doesn't re-export pub(crate); list them) ----
pub(crate) use engine::parse_recent_seeds;
pub(crate) use reference::build_reference_context;
pub(crate) use faction::build_faction_wizard_user_prompt;
pub(crate) use location::build_wizard_user_prompt;
pub(crate) use dungeon::anchor_mechanic;
pub(crate) use npc::{
    describe_recent_npc_occupation_anchors, occupation_anchor, recent_occupation_anchor_set,
};
```

**This list is exhaustive** — it is derived from the actual imports in the consumer
files. After the refactor, verify it by checking these import sites still compile
untouched:

| Consumer | Imports from `ai_generation` |
|---|---|
| `commands/create_commands.rs` | `AiGenerationService, SeedGeneration` |
| `commands/system_commands.rs` | `AiGenerationService, SeedGeneration` |
| `utils.rs` (× incl. tests) | `FactionSeed, GodSeed, LocationSeed` |
| `services/entity_persistence.rs` | `LocationSeed`, `location_dir_for_kind`, `faction_dir_for_kind`, `faction_category_str` |
| `services/entity_reroll.rs` | `NpcSeed, anchor_mechanic, build_reference_context, describe_recent_npc_occupation_anchors, occupation_anchor, parse_recent_seeds, recent_occupation_anchor_set` |
| `entities/domains/event_domain.rs` | `AiGenerationService, SeedGeneration` |
| `wizards/dungeon.rs` | `AiGenerationService, DungeonStory, SeedGeneration` |
| `wizards/faction.rs` | `AiGenerationService, CONTROL_TYPES, FactionSeed, FactionWizardInputs, HOUSE_BRANDS, LORD_TYPES, MANDATES, REACH, SeedGeneration, build_faction_wizard_user_prompt` |
| `wizards/location.rs` | `AiGenerationService, LocationSeed, LocationWizardInputs, SeedGeneration, build_wizard_user_prompt, location_subfolder` |

> Note `faction_category_str` and `location_dir_for_kind` etc. are referenced by
> *fully-qualified path* in `entity_persistence.rs` (e.g.
> `crate::services::ai_generation::faction_dir_for_kind(...)`), not a `use`. The glob
> re-exports cover these too — just confirm they still resolve.

---

## 5. Visibility rule (apply mechanically)

Splitting one module into many turns previously-free intra-file calls into
cross-module calls. Resolve visibility with one rule, minimizing surface:

> **Default every moved item to its current visibility. Then, for each compiler
> error `E0603` (private item) / `E0433` (unresolved), widen the *minimum* amount:**
> - If the only callers are **other `ai_generation` submodules** → `pub(super)`
>   (visible within `ai_generation` and its descendants, not beyond).
> - If a **different service/module** needs it (the §4 table) → keep `pub`/`pub(crate)`
>   and ensure `mod.rs` re-exports it.
> - Otherwise → leave it **private**.

Pre-computed result for the engine items the per-kind methods call (these become
`pub(super)` in `engine.rs`):

| Item | Why it crosses | New vis |
|---|---|---|
| `SeedSampling` (struct **and** fields) | per-kind `*_GEN_SAMPLING` consts construct it | `pub(super)` |
| `run_seed_attempts` | called by npc/location/faction/god/item/event methods | `pub(super)` |
| `capacity_notice` | called by **all** generate methods incl. both dungeon ones | `pub(super)` |
| `reference_system_suffix` | called by npc/location/faction/god/item/event methods | `pub(super)` |
| `SeedStep<T>` | per-kind closures passed to `run_seed_attempts` construct it | `pub(super)` (verify; private if not) |
| `OUTPUT_RESERVE_TOKENS`, `SYSTEM_BOILERPLATE_TOKENS` | token-budget math in methods | `pub(super)` if referenced in methods, else private — let the compiler decide |
| `build_seed_payload` | called **only** by `run_seed_attempts` (engine-internal) | **stays private** |
| `parse_recent_seeds<T>` | already `pub(crate)`; `entity_reroll` uses it | keep `pub(crate)` + re-export |

Per-kind `*_GEN_SAMPLING` consts and per-kind recency helpers move into the same file
as the method that calls them, so they **stay private** (except the three NPC helpers
in the §4 table that `entity_reroll` imports — those keep `pub(crate)`).

---

## 6. Per-module move map

Move each item with its doc-comment. Extract by name (`grep -n`) — names are unique.
Line numbers are as of `chore/ai-cleanup`.

### engine.rs
- `OUTPUT_RESERVE_TOKENS` (30), `SYSTEM_BOILERPLATE_TOKENS` (32)
- `SeedGeneration<T>` (37) — **pub**, re-exported
- `capacity_notice` (44), `SeedSampling` (89), `reference_system_suffix` (129),
  `build_seed_payload` (140), `SeedStep<T>` (169), `run_seed_attempts<T>` (184)
- `parse_recent_seeds<T>` (2614) — **pub(crate)**, re-exported
- **Tests:** `seed_payload_wraps_messages_schema_and_sampling_with_num_ctx`,
  `capacity_notice_none_when_comfortably_under_budget`,
  `capacity_notice_fires_when_prompt_crowds_output_reserve`

### reference.rs
- `build_reference_context` (59) — **pub(crate)** async entry, re-exported
- `PromptReferenceContext` (2607) — **pub**, re-exported
- `build_prompt_reference_context` (2766), `canonical_metadata_map` (2832),
  `extract_runebound_toml` (2893), `reference_payload_from_markdown` (2910)
- **Tests:** `reference_payload_prefers_runebound_block`, `reference_payload_falls_back_to_full_file`
- *Imports to bring along:* `VaultReferenceEntry`, `load_vault_reference_entries`,
  `extract_prompt_reference_keys`, `normalize_relative_path_for_storage` (from
  `vault_ref`/`utils`/`runebound_models` — external, unchanged).

### npc.rs
- `NPC_GEN_SAMPLING` (95); `NpcSeed` (1399, **pub**)
- `recent_name_set` (2693), `occupation_tokens` (2701), `occupation_anchor` (2716, **pub(crate)**),
  `recent_occupation_anchor_set` (2723, **pub(crate)**), `describe_recent_npc_seeds` (2739),
  `describe_recent_npc_occupation_anchors` (2756, **pub(crate)**)
- `impl`: `generate_npc_seed` (237–351)
- **Tests:** `occupation_anchor_ignores_descriptive_fillers`,
  `recent_occupation_anchor_set_collects_unique_roots`,
  `describe_recent_occupation_anchors_is_compact_and_unique`

### location.rs
- `LOCATION_GEN_SAMPLING` (100)
- `LocationBranch` (1417), `location_branch` (1429), `location_subfolder` (1442),
  `location_dir_for_kind` (1454) — all **pub**
- `LocationWizardInputs` (1945, **pub**); `opt_clause` (1981), `geography_clause` (1985),
  `locked_danger_clause` (1994), `LOCATION_PROSE_LEASH` (2003),
  `wizard_location_system_prompt` (2008), `wizard_location_schema` (2088),
  `build_wizard_user_prompt` (2143, **pub(crate)**)
- `LocationSeed` (2230, **pub**)
- `describe_recent_location_seeds` (2681), `recent_location_name_set` (2731)
- `impl`: `generate_location_seed` (352–458), `generate_location_seed_for_wizard` (459–582)
- **Tests:** `location_subfolder_maps_each_branch_and_flattens_other`,
  `location_dir_for_kind_appends_subfolder_or_stays_flat`, `settlement_prompt_*`,
  `settlement_none_export_*`, `holy_site_grounds_*`, plus the `guildhall_*`/`hideout_*`
  prompt tests **if** they exercise `build_wizard_user_prompt` (move each test next to
  the function it calls — check the body).

### faction.rs
- `FACTION_GEN_SAMPLING` (105)
- `FactionCategory` (1465), `faction_category` (1474), `faction_category_str` (1488),
  `faction_subfolder` (1502), `faction_dir_for_kind` (1511) — all **pub**
- WOAC consts **pub**: `LORD_TYPES` (1529), `CONTROL_TYPES` (1540), `MANDATES` (1544),
  `REACH` (1554), `HOUSE_BRANDS` (1558)
- WOAC fact fns: `lord_type_facts` (1565), `control_type_facts` (1597),
  `mandate_facts` (1625), `loyalty_fault` (1657), `reach_phrase` (1673)
- `FactionWizardInputs` (1691, **pub**); `FACTION_WOAC_LEASH` (1715),
  `wizard_faction_system_prompt` (1722), `wizard_faction_schema` (1849),
  `build_faction_wizard_user_prompt` (1875, **pub(crate)**)
- `FactionSeed` (2261, **pub**)
- `recent_faction_name_set` (2641), `describe_recent_faction_seeds` (2649)
- `impl`: `generate_faction_seed` (583–690), `generate_faction_seed_for_wizard` (691–783)
- **Tests:** `faction_subfolder_maps_all_nine_kinds`, `faction_dir_for_kind_*`,
  `faction_wizard_prompt_emits_*` (× several), `faction_wizard_grounds_*`,
  `wizard_faction_schema_omits_*`, `wizard_faction_system_prompt_*` (× several)
- *Largest module (~700 lines)* — expected; faction carries the most domain knowledge.

### god.rs
- `GOD_GEN_SAMPLING` (110); `GodSeed` (2283, **pub**)
- `recent_god_name_set` (2661), `describe_recent_god_seeds` (2669)
- `impl`: `generate_god_seed` (784–883)

### item.rs
- `ITEM_GEN_SAMPLING` (115); `ItemSeed` (2302, **pub**)
- `impl`: `generate_item_seed` (884–985)  *(no recency helpers exist for items)*

### event.rs
- `EVENT_GEN_SAMPLING` (120); `EventSeed` (2317, **pub**)
- `recent_event_title_set` (2621), `describe_recent_event_seeds` (2629)
- `impl`: `generate_event_seed` (986–1072)

### dungeon.rs
- `DungeonBeatSeed` (2323), `DungeonSeed`+impl (2336), `DungeonStory`+impl (2395) — DTOs **pub**;
  `DungeonStructured` (2413), `DungeonStructuredBeat` (2419) — private
- `describe_recent_dungeon_stories` (2430), `anchor_story_phrase` (2447),
  `anchor_mechanic` (2472, **pub(crate)**), `overlay_phrase` (2496), `topology_shape` (2510),
  `twist_directive` (2527), `pass1_elements_block` (2539), `pass2_assignment_block` (2561)
- `impl`: `generate_dungeon_story` (1073–1250), `structure_dungeon_story` (1251–1398)
- *Note:* the dungeon methods are **bespoke** — they call `capacity_notice` but **not**
  `run_seed_attempts` (two-pass plan→structure flow), and there is no
  `DUNGEON_GEN_SAMPLING` const. Move both methods wholesale; let the compiler tell you
  which engine items they need.
- *Imports to bring along:* `DungeonContentPlan`, `DungeonBeat` (from `runebound-models`).

---

## 7. Phased execution (compile green after each phase)

Work incrementally so each phase is an independently reviewable, green diff. After
**every** phase run: `cargo test --manifest-path desktop/src-tauri/Cargo.toml` and
`cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings`.

- **Phase 0 — establish the directory (pure move).**
  `git mv desktop/src-tauri/src/services/ai_generation.rs desktop/src-tauri/src/services/ai_generation/mod.rs`.
  No content change. `services/mod.rs` already says `pub mod ai_generation;` — unchanged.
  Build green. *(This is the safety net: from here every phase only moves code between
  files within the directory.)*

- **Phase 1 — extract `engine.rs`.** Move the §6 engine items out of `mod.rs` into
  `engine.rs`; add `mod engine;` + `pub use engine::*;` + `pub(crate) use engine::parse_recent_seeds;`
  to `mod.rs`; apply the `pub(super)` visibilities from §5; move the three engine tests.
  Build green.

- **Phase 2 — extract `reference.rs`.** Same pattern. Build green.

- **Phase 3 — extract the kind modules, one per sub-step:** `dungeon` → `npc` →
  `god` → `item` → `event` → `location` → `faction`. For each: move the DTO + sampling
  + helpers + prompt builders + the `impl AiGenerationService { … }` block + that
  kind's tests; add `mod x; pub use x::*;` and any `pub(crate) use` lines; build green
  before starting the next. (Do dungeon first — it's the most self-contained; do
  faction last — it's the largest.)

- **Phase 4 — finalize `mod.rs`.** It should now contain only: the 9 `mod` decls, the
  `AiGenerationService` struct, and the re-export wall from §4. Confirm it matches §4.

- **Phase 5 — verify (see §8).**

---

## 8. Verification / definition of done

1. `cargo fmt --check --manifest-path desktop/src-tauri/Cargo.toml` — clean.
2. `cargo clippy --manifest-path desktop/src-tauri/Cargo.toml --all-targets -- -D warnings` — **0 warnings**.
   (Watch for new `unused_imports` from items that no longer need to cross modules,
   and for over-wide `pub(crate)` that clippy/`unreachable_pub` may flag — tighten to `pub(super)`.)
3. `cargo test --manifest-path desktop/src-tauri/Cargo.toml` — **same test count, all
   passing**. No test body was edited; if a test fails, you changed behavior — revert
   and re-move.
4. `make lint` — clean across both workspaces.
5. **Consumer diff is empty.** `git diff --stat` should show changes only under
   `services/ai_generation/` (+ the `git mv`). If any of the 11 consumer files in §4
   changed, the re-export wall is incomplete — fix `mod.rs`, don't patch the consumer.
6. No single file in the new directory exceeds ~700 lines (faction); `mod.rs` is thin.

---

## 9. Future work (explicitly OUT OF SCOPE for this pass)

Capture, don't do — these would turn a mechanical diff into a risky one:

- **De-duplicate the generate methods.** Once co-located, the six `run_seed_attempts`-based
  methods (npc/location/faction/god/item/event) share a near-identical spine:
  `build_reference_context → reference_system_suffix → estimate tokens → capacity_notice
  → build_seed_payload → run_seed_attempts`. A shared helper (or a small macro) could
  collapse much of it. Do this as a **separate** follow-up PR, against the now-split
  modules, with its own review.
- **Consider a `SeedGenerator` trait** (per-kind `system_prompt`/`schema`/`sampling`/
  `user_prompt`) if the dedup above wants a uniform shape.
- The two-pass dungeon path is bespoke; leave it until the simpler path is unified.
