# Dungeons — Implementation Spec

> **Purpose:** The build plan for the dungeon feature. Translates the design in
> `docs/feature-dungeons.md` into concrete data structures, a guided creation
> flow, command surface, generation/reroll behaviors, persistence, and output —
> mapped onto the real architecture (`docs/architecture.md`,
> `docs/command-contexts.md`, `docs/feature-development.md`, `docs/render.md`,
> `docs/config.md`). Read the design doc first; this assumes it.

---

## 1. Overview & Scope

A **dungeon** is an LLM-generated 5-beat oracle. Two UX phases, two mechanisms:

1. **Guided creation flow** (the new UI). `create dungeon` starts a setup-style,
   multi-step flow that asks the five questions (A premise, B tone, C twist,
   D context, E topology — `feature-dungeons.md` §3). On the final answer it calls
   the LLM **once** to generate the whole dungeon, then loads it as an editable
   draft.
2. **Entity editor** (reuse). Once generated, a dungeon is a normal entity draft
   living in `EditorSession`, edited with `dungeon set` / `dungeon reroll <beat>` /
   `reroll`, persisted with `save`, exported with `publish`, and managed with the
   shared `load`/`show`/`delete`/`undo` commands.

This split means the dungeon reuses the entire entity-domain architecture
(`EntityKind`, `EntitySchema`, `EntityDomain`, `EditorSession`, repositories,
persistence, vault sync) and only adds **two genuinely new pieces**:

- **A guided flow state machine** (no reusable primitive exists; modeled on the
  bespoke `try_execute_onboarding` setup wizard).
- **Per-beat (array-element) reroll** — a small generalization of the existing
  scalar per-field reroll, which already sends the full draft as frozen context.

### Scope (v1)

In: guided flow A–E; whole-dungeon generation; per-beat reroll with frozen
context; dungeon-level field edit/reroll; save → TOML+DB; publish → Obsidian;
load/show/delete/undo; the dungeon output card with per-beat reroll buttons.

Out (deferred, noted inline): "back" navigation in the flow; intricate per-beat
sub-field reroll (`reroll setback.lever`); multi-dungeon stacking/linking
(`feature-dungeons.md` §9); a dedicated autocomplete context for the flow.

---

## 2. Architecture Fit

| Concern | Reuse / New | Where |
|---|---|---|
| Entity kind, schema, domain, registry | reuse pattern | `entities/` |
| Draft/editor session, draft envelope | reuse pattern | `app_state.rs` |
| Whole-entity LLM generation | reuse pattern (Event/Item) | `services/ai_generation.rs` |
| Per-field reroll w/ frozen context | reuse + generalize to array | `services/entity_reroll.rs` |
| Save (TOML + DB + index), publish, vault sync | reuse pattern (Item) | `services/{entity_persistence,publish,vault_sync}.rs` |
| Output card, client event, spinner | reuse blocks (no new variant) | `runebound-models`, `App.tsx` |
| Command spec, availability, suggestions | reuse pattern (`EntityScoped`) | `command-specs`, `services/suggestions.rs` |
| **Guided multi-step flow** | **new (bespoke, like setup)** | `app_state.rs` + `commands/dungeon_flow.rs` + `main.rs` |
| **Per-beat array reroll** | **new (generalize scalar reroll)** | `services/entity_reroll.rs` + domain |

Two parallel kind enums must both gain a `Dungeon` variant: `EntityKind`
(editor/schema side, `entities/kind.rs`) and `EntityType` (admin/persistence
side, `services/entity_admin.rs`).

---

## 3. Data Structures

### 3.1 Shared models — `runebound-models/src/drafts.rs`

The draft is the in-editor working copy (carries `seed_prompt`, `slug`,
`vault_path`); the frontmatter is the persisted form (swaps `seed_prompt` for
`doc_type` + timestamps + `published_at`). This mirrors `ItemDraft` /
`ItemFrontmatter` exactly, except the beats are an **array of sub-records** — the
one real shape difference from existing flat entities.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonBeat {
    pub function: String,      // fixed skeleton: Entrance|Puzzle|Setback|Climax|Resolution
    pub content_type: String,  // one of DUNGEON_CONTENT_TYPES (the 11)
    pub idea: String,          // 1-2 lines: what happens here
    pub lever: String,         // one complication/question/hook
    #[serde(default)] pub loot: Option<String>, // conditional — None where the beat doesn't earn it
    pub read_aloud: String,    // 1-2 sentence static visual description
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonDraft {
    pub id: String,
    #[serde(default)] pub seed_prompt: Option<String>, // premise+context bias, reused by reroll
    pub name: String,
    pub slug: String,
    pub vault_path: String,
    pub premise: String,       // the spine / top-line (feature-dungeons.md §6)
    pub topology: String,      // one of DUNGEON_TOPOLOGIES, or "none"
    pub tone: String,          // "tragedy" | "comedy"
    pub twist: String,         // "false_victory" | "false_defeat" | "neither"
    pub beats: Vec<DungeonBeat>, // exactly 5, fixed order
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonFrontmatter {
    #[serde(rename = "type")] pub doc_type: String, // "dungeon"
    pub id: String, pub slug: String, pub name: String, pub vault_path: String,
    pub premise: String, pub topology: String, pub tone: String, pub twist: String,
    pub beats: Vec<DungeonBeat>,
    pub created_at: String, pub updated_at: String,
    #[serde(default)] pub published_at: Option<String>,
}
```

Plus a `dungeon_entity_card(&DungeonDraft) -> OutputDoc` builder (see §8.1) and a
`CommandClientEvent::LoadDungeonDraftWithCard { draft, entity_card }` variant in
`events.rs`.

### 3.2 Enum constants & normalizers — `runebound-models/src/utils.rs`

Used by the generation JSON Schema (`enum` constraints) and by post-parse
normalizers (mirroring `ITEM_RARITIES` / `normalize_item_rarity`).

```rust
pub const DUNGEON_FUNCTIONS: [&str; 5] =
    ["Entrance", "Puzzle", "Setback", "Climax", "Resolution"];
pub const DUNGEON_CONTENT_TYPES: [&str; 11] = [
    "combat", "cache", "sidekick", "offshoot", "foreshadowing",
    "history", "oddity", "forge", "factions", "map", "puzzle",
];
pub const DUNGEON_TONES: [&str; 2] = ["tragedy", "comedy"];
pub const DUNGEON_TWISTS: [&str; 3] = ["false_victory", "false_defeat", "neither"];
// "none" is a first-class choice = no topology imposed (feature-dungeons.md §6, step E)
pub const DUNGEON_TOPOLOGIES: [&str; 10] = [
    "none", "The Railroad", "The Moose", "The V for Vendetta", "The Arrow",
    "The Fauchard Fork", "The Evil Mule", "Foglio's Snail", "The Paw", "The Cross",
];
```

### 3.3 Editor schema — `desktop/src-tauri/src/entities/schema.rs`

The flat `EntityFieldSpec` schema covers **dungeon-level scalar fields only**.
Beat fields are addressed compositionally by the domain (§6.3), not the flat
schema. `settable`/`rerollable` here drive `dungeon set`/`dungeon reroll`
autocomplete (`settable_fields`/`rerollable_fields`).

| canonical | aliases | settable | rerollable | notes |
|---|---|---|---|---|
| name | — | ✓ | ✓ | |
| premise | spine | ✓ | ✓ | reroll = regenerate the spine line only |
| topology | — | ✓ | ✗ | structural choice; re-pick, don't reroll |
| tone | — | ✓ | ✗ | dial, not generated |
| twist | — | ✓ | ✗ | dial, not generated |

Add `DUNGEON_FIELDS`, `pub static DUNGEON_SCHEMA`, a `schema_for_kind` arm, and an
entry in the `settable_and_rerollable_field_counts_are_locked` test's `expected`
array (it hard-codes per-kind counts).

### 3.4 DB & TOML — `core/`

- **TOML is the source of truth** for full fidelity, including the beats. Path:
  `<entities-root>/dungeons/<slug>.toml`. TOML represents `beats` as an array of
  tables natively. Add `DUNGEON_DIR = "dungeons"` + `save_dungeon`/`load_dungeon`/
  `delete_dungeon`/`list_dungeons` to `core/src/entity_store.rs`.
- **DB is the search/index mirror.** Migration `core/migrations/0015_dungeons.sql`
  (next number after `0014_gods.sql`; migrations are append-only/immutable):

```sql
CREATE TABLE IF NOT EXISTS dungeons (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    vault_path TEXT NOT NULL UNIQUE,
    premise TEXT NOT NULL,
    topology TEXT NOT NULL,
    tone TEXT NOT NULL,
    twist TEXT NOT NULL,
    beats_json TEXT NOT NULL,            -- serde_json of Vec<DungeonBeat>
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_dungeons_slug ON dungeons(slug);
CREATE INDEX IF NOT EXISTS idx_dungeons_name ON dungeons(name);
```

  Beats don't fit the existing `Vec<String>` → `faction_list_to_db_text` list
  encoding (they're structs), so they round-trip through DB as a single
  **`beats_json TEXT`** column. Add `DungeonRow` + the standard CRUD set
  (`search_dungeons_by_name`, `find_dungeon_by_name_or_slug`, `list_dungeons`,
  `find_dungeon_by_id`, `upsert_dungeon`, `delete_dungeon_by_id`,
  `row_to_dungeon`) to `core/src/db.rs`.

> **Open item (§11.1):** confirm whether `entity_admin` resolve/load hydrates from
> TOML (preferred — full beats) or DB. The `beats_json` column makes DB
> round-trip lossless either way, so this is safe but should be verified.

### 3.5 Kind & session wiring — `desktop/src-tauri/src/`

- `entities/kind.rs`: add `EntityKind::Dungeon`, the `as_str`/`display_name` arms,
  and bump `ALL_ENTITY_KINDS` length.
- `app_state.rs`: `type DungeonDraftSession = DungeonDraft;`; a
  `DraftEnvelope::Dungeon` variant + `kind()` arm + `as_dungeon`/`as_dungeon_mut` +
  `From`; `EditorSession::{get_dungeon,get_dungeon_mut,set_dungeon,take_dungeon}`;
  add `Dungeon` to **every** `next_active_after` search slice plus its own arm;
  `AppState.dungeon_repo` field + accessor. **Also** the new flow state (§4.1).
- `repositories/mod.rs`: `DungeonRepository` trait + `ProdDungeonRepository`,
  wired into the `AppState` constructor.

---

## 4. The Guided Creation Flow (new UI)

There is **no reusable wizard primitive**. The setup wizard
(`core/src/command.rs::try_execute_onboarding`) is hand-rolled: a `u8` step
counter, typed answer fields, per-step sub-state enums for input disambiguation,
plain-string prompts, and interception **before** registry dispatch while active.
We build a parallel machine, but **desktop-side**, because completion calls the
desktop generation service and repositories.

### 4.1 State model — `desktop/src-tauri/src/app_state.rs`

```rust
#[derive(Debug, Clone, Default)]
pub struct DungeonCreationFlow {
    pub active: bool,
    pub step: u8,                  // 1..=5 (A..E)
    pub premise: Option<String>,   // None = "generate one"
    pub tone: Option<String>,      // DUNGEON_TONES
    pub twist: Option<String>,     // DUNGEON_TWISTS
    pub context: String,           // step D free-text (references/constraints); "" = skipped
    pub topology: Option<String>,  // DUNGEON_TOPOLOGIES incl. "none"
}
```

Held on `AppState` (e.g. `dungeon_flow: Mutex<DungeonCreationFlow>`), alongside
`EditorSession`. Like `OnboardingSession`, it is in-memory only; nothing persists
mid-flow. Answers are stored as named typed fields (not a generic map), matching
the setup precedent.

### 4.2 Entry & interception

- **Entry:** `create dungeon` (no prompt) routes through the registry to
  `create_commands::create_dungeon`, which sets `dungeon_flow.active = true`,
  `step = 1`, and returns the **Step A** prompt. (Contrast other entities, where
  `create <kind> <prompt>` generates in one shot.)
- **Interception:** in `desktop/src-tauri/src/main.rs::run_command`, after the
  existing `onboarding.active` guard, add: *if `dungeon_flow.active`, route the
  raw line to `try_execute_dungeon_flow(line, state)` and return — bypassing
  registry dispatch* (exactly how onboarding bypasses it). This keeps step answers
  like `"2"` or free-text premises from being parsed as commands.

### 4.3 Steps A–E — `desktop/src-tauri/src/commands/dungeon_flow.rs` (new)

Each step: render a prompt, capture the next raw line, validate, store, advance.
Mirror the setup helpers (`vault_menu_text`, `enter_ollama_menu`). Use `OutputDoc`
builders for prompts (heading + list), not bare strings, so they render cleanly.

| Step | Question | Input | Validation |
|---|---|---|---|
| **A** Premise | "Enter a one-line premise, or type `generate` to have the oracle invent one." | free-text or `generate` | non-empty; `generate` → `premise = None` |
| **B** Tone | menu `1: Tragedy   2: Comedy` | `1`/`2` | maps to `DUNGEON_TONES`; re-show on bad input |
| **C** Twist | menu `1: False victory   2: False defeat   3: Neither` | `1`/`2`/`3` | maps to `DUNGEON_TWISTS` |
| **D** Context | "Add references/constraints (or `skip`)." | free-text or `skip` | `skip`/empty → `context = ""` |
| **E** Topology | menu `0: None   1: The Railroad … 9: The Cross` (from `DUNGEON_TOPOLOGIES`) | `0`–`9` | maps to topology name; `0` = `"none"` |

Single-choice steps render a numbered menu and match the raw digit (the setup
pattern). Free-text steps take the next line verbatim. Only steps that both show a
menu *and* later accept free text would need a sub-state enum — none of A–E do, so
no sub-state enums are required (simpler than setup's vault/ollama steps).

### 4.4 Transitions, cancel, no-back

- **Advance:** each step hard-codes its successor (`step = n+1`) and returns the
  next prompt — no generic `advance()`, matching setup.
- **Cancel:** `cancel` / `cancel dungeon` calls `reset_dungeon_flow` (clears
  `active`, `step`, answers). Because the flow intercepts before dispatch, the
  desktop `cancel` handler never runs during the flow — handle cancel explicitly
  inside `try_execute_dungeon_flow` (the documented setup invariant,
  `config.md` notes). 
- **No "back"** in v1 (parity with setup). Re-running `create dungeon` resets to
  Step A. (Back navigation is a deferred enhancement.)
- **Seed displayed fields from collected state** when re-rendering a prompt (the
  other setup invariant) — relevant only if we add back/edit later.

### 4.5 Completion → generation → draft load

On the Step E answer:

1. Show the generation spinner (see §8.2 — handled frontend-side).
2. Build `seed_prompt` = premise (or a "generate a self-contained dungeon"
   default) joined with `context`. Persist it on the draft for later rerolls.
3. Call `AiGenerationService::generate_dungeon_seed(premise, context, tone, twist,
   topology, …)` (§5.1).
4. Mint `id = make_entity_id("dungeon")`, build `DungeonDraftSession` (set
   `tone`/`twist`/`topology`/`premise` authoritatively from the flow inputs;
   `vault_path = ""`), `editor.set_dungeon(draft)`, clear other kinds.
5. `reset_dungeon_flow()` (exit the flow; `active = false`).
6. Return `dungeon_summary_text` + emit `LoadDungeonDraftWithCard` so the frontend
   opens the editable draft. `active_kind` is now `Dungeon` → the entity editor
   (§6) takes over.

Completion is where the dungeon flow **diverges from setup**: setup persists
config; the dungeon flow generates content and opens a draft.

### 4.6 Frontend touches — `desktop/src/App.tsx`

The generic submit/render path already handles flow prompts (text/`OutputDoc`
round-trips), so the flow needs **almost no frontend code** — with two exceptions:

- **Generation spinner (mandatory).** The completing input is a bare answer
  (`"9"`), so `commandSpinnerLabel` can't match it directly. Mirror the existing
  Ollama heuristic: add `detectDungeonTopologyPrompt(text)` that recognizes the
  Step E prompt marker; set a signal; on the next submit, `commandSpinnerLabel`
  returns `"generating dungeon"`. (Same shape as `detectOllamaPrompt` →
  `ollamaPrompt()` → spinner.)
- **Clickability:** menu digits are made clickable like setup's menus; ensure
  flow answers aren't mis-linked (reuse the setup special-casing in
  `resolveClickableCommandTarget`).

No new `InputContext` is required (execution is never context-gated). Optionally a
`DungeonCreation` context could suppress unrelated autocomplete during the flow,
but defer it — it touches the `InputContext` enum and every resolution site.

---

## 5. Generation & Reroll Behaviors

The pipeline is **local Ollama only**: `POST /api/chat` with `format` set to a
JSON Schema, a retry loop (gen ≈5 attempts, reroll ≈4) with post-parse
normalization/validation and novelty rejection. No streaming, no remote provider.

### 5.1 Whole-dungeon generation — `services/ai_generation.rs`

Add `DungeonSeed` + `generate_dungeon_seed`, copying the `generate_item_seed` /
`generate_event_seed` shape (config load → reference context → recent-avoid →
`build_chat_client` → retry loop → parse/normalize/validate → persist via
`generation_repo.insert(db, "dungeon_seed", …)`).

**Output JSON Schema** (the structural guarantee — "always five beats"):

```rust
json!({
  "type": "object",
  "required": ["name", "premise", "beats"],
  "additionalProperties": false,
  "properties": {
    "name":    { "type": "string", "minLength": 1 },
    "premise": { "type": "string", "minLength": 1 },     // the spine top-line
    "beats": {
      "type": "array",
      "minItems": 5, "maxItems": 5,                       // pins the skeleton
      "items": {
        "type": "object",
        "required": ["content_type", "idea", "lever", "read_aloud"],
        "additionalProperties": false,
        "properties": {
          "content_type": { "enum": DUNGEON_CONTENT_TYPES },
          "idea":         { "type": "string", "minLength": 1 },
          "lever":        { "type": "string", "minLength": 1 },
          "loot":         { "type": ["string", "null"] }, // conditional
          "read_aloud":   { "type": "string", "minLength": 1 }
        }
      }
    }
  }
})
```

- **`function` is set by us, not the model.** After parse, assign
  `beats[i].function = DUNGEON_FUNCTIONS[i]` (validate length == 5). This
  guarantees the fixed Entrance→Resolution skeleton regardless of model behavior.
- **`tone`/`twist`/`topology` are authoritative from the flow** — set them on the
  draft directly; pass them into the prompt as constraints (they don't appear in
  the output schema).
- **System prompt** encodes the oracle constraints (`feature-dungeons.md`
  §1/§5/§8): *specific-but-unresolved*; index-card tightness; `idea` 1-2 lines;
  `lever` = one hook/question; **loot conditional** (present at Resolution,
  sometimes Climax/Cache; `null` elsewhere, especially Setback); `read_aloud` =
  1-2 sentence **static visual** only (no action/NPC behavior); **no monster
  names**; one-object-triple-duty encouraged. Inject `tone`, `twist`, `topology`,
  `premise`, `context`.
  - **Topology coupling:** if `topology != "none"`, instruct beats — especially
    the **Setback** — to respect the flow shape (a middle-entrance/looping form
    ⇒ a Setback that dumps players back toward the Entrance).
  - **Linkage placement:** if `context` points outside the dungeon, concentrate
    hooks in the **Resolution** and the connective content types
    (`map`/`foreshadowing`/`oddity`); keep the body self-contained.
- **Verbosity clamp (important):** do **not** append the standard
  `detail_directive(config.generation.verbosity)` — it pushes *toward* length and
  fights the index-card north star. Append a fixed "brief, index-card" directive
  for dungeon narrative fields instead, with the tightest leash on `read_aloud`.

### 5.2 Per-beat reroll with frozen context — `services/entity_reroll.rs` (the headline)

This generalizes the scalar per-field reroll (which already sends the whole draft
as frozen context) so the **unit is a beat** instead of a field.

- **Command:** `dungeon reroll <beat>` where `<beat>` ∈
  `{entrance,puzzle,setback,climax,resolution}` (case-insensitive; accept `1`–`5`
  too). Surfaced as a `command_ref` button under each beat card (§8.1).
- **Input:** `RerollDungeonBeatInput { beat_index, prompt, dungeon:
  DungeonRerollContext }` where the context carries **premise + topology + tone +
  twist + all five current beats**.
- **Single-beat output schema:** one beat object (`content_type`, `idea`, `lever`,
  `loot`, `read_aloud`); `function` stays fixed.
- **Prompt (frozen context):** serialize the spine, dials, topology, and the
  **other four beats verbatim** (the analog of `npc_context_summary`), then
  instruct: *"Regenerate only beat N (function = Setback). Keep it coherent with
  the spine; it must follow the {prev} beat and feed the {next} beat."* This
  satisfies the design doc's "a new Setback still follows the Entrance and feeds
  the Climax."
- **Anti-stagnation → "range across rerolls":** reuse the attempt-loop sameness
  check, but reject a reroll whose **`content_type` repeats the current beat's**
  on early attempts (forcing palette variety, not just repainted prose — the doc's
  "can't converge to goblins-trap-boss"). Accept whatever comes back on the final
  attempt.
- **Merge:** the domain writes back **only `beats[beat_index]`**; the other four
  beats remain byte-identical (the `match`/write-back pattern of
  `npc_domain.rs::reroll_field`).

### 5.3 Whole-dungeon reroll

`reroll` (field-agnostic, routed by `active_kind` in
`system_commands.rs::handle_reroll`) re-calls `generate_dungeon_seed` from the
stored `seed_prompt` + dials, replacing all five beats. Add a
`Some(EntityKind::Dungeon) => reroll_current_dungeon(...)` arm.

### 5.4 Manual edits (`set`)

- **Dungeon-level:** `dungeon set <field> <value>` for `name`/`premise`/`topology`/
  `tone`/`twist` (the flat schema fields).
- **Beat-level:** `dungeon set <beat> <field> <value>` (e.g.
  `dungeon set setback loot none`) — the domain parses `<beat>` to an index and
  `<field>` to a beat field, then mutates `beats[index]`. This is a domain-level
  extension beyond the flat schema (§6.3). Beyond this, fine-grained editing is
  expected post-publish in Obsidian.

### 5.5 Config interactions

`ollama.model` must be set (errors with "run start setup" otherwise);
`num_ctx`/`timeout_seconds` apply as usual. Five beats + frozen-context reroll are
larger prompts than other entities — the existing `capacity_notice` soft-warning
(approaching `num_ctx`) will surface via `SeedGeneration.notice`; leave it
non-blocking.

---

## 6. Commands & Surface

### 6.1 Manifest — `command-specs/src/lib.rs`

- **`dungeon` editor spec** (clone the `item` spec): subcommands
  `show`/`rename`/`set`/`reroll`/`save`/`cancel`/`help`; `requires_subcommand:
  true`; `canonical_help_command: Some("dungeon help")`; `execution: Desktop`;
  `show_in_autocomplete: true`.
- **`create dungeon`** subcommand + example added to the `create` spec.
- **Availability arm (required):** `"dungeon" => EntityScoped("dungeon")`. Omitting
  it drops `dungeon` onto the `_ => Default` fallthrough and **fails the
  `default_surface_commands_are_an_explicit_known_set` sentinel test** — a
  built-in tripwire. Also update `entity_roots_are_scoped_to_their_own_editor` and
  `menu_style_roots_require_a_subcommand` test sets.
- Optional alias (e.g. `dungeon new → create dungeon`) for discoverability.

`reroll`/`save`/`cancel` stay visible in the dungeon editor automatically via
their existing availability arms (they show whenever any draft is active).

### 6.2 Handler registration — `desktop/src-tauri/src/commands/mod.rs`

Four touch points: `pub mod dungeon_commands;`; a `dungeon_handler_entry()`
(mirror `item_handler_entry()`); `registry.register(dungeon_handler_entry())`;
and the manifest spec must exist first (`metadata_for("dungeon")` panics
otherwise). The `every_desktop_command_has_a_registered_handler` /
`every_registered_handler_maps_to_a_manifest_command` tests enforce this.

### 6.3 Entity command lifecycle — `entities/domains/dungeon_domain.rs` (new)

Implement `EntityDomain` (mirror `item_domain.rs`). Notable per-method specifics:

- `set_field`: resolve dungeon-level fields via `canonical_field_name(Dungeon,
  field, FieldAccess::Set)`; **additionally** parse the `<beat> <field>` form for
  beat edits (§5.4) before falling back to the flat resolver.
- `reroll_field`: parse `<beat>` (function name or `1`–`5`) → call
  `EntityRerollService::reroll_dungeon_beat` with full frozen context (§5.2); for
  dungeon-level `premise`/`name`, use a scalar reroll like item.
- `save`: `EntityPersistenceService::save_dungeon_draft` → `editor.clear_all()` +
  `ClearDrafts`.
- `cancel`: `editor.take_dungeon()` + `ClearDrafts`.
- Export `dungeon_summary_text` + `dungeon_event_from_draft` (emits
  `LoadDungeonDraftWithCard`).

Register in `entities/registry.rs`; declare in `entities/domains/mod.rs`. Thin
string router `commands/dungeon_commands.rs::handle_dungeon` (mirror
`item_commands.rs`).

**Shared commands** (exhaustive `match` over `EntityType`/`EntityKind` — must add
a `Dungeon` arm to each):
- `entity_commands.rs`: `build_load_response`, `build_entity_card_doc`,
  `build_entity_card_text` (hydrate `DungeonDraftSession` from resolved entity).
- `system_commands.rs`: `handle_reroll` → `reroll_current_dungeon` (§5.3).
- `create_commands.rs`: `handle_create` branch → `create_dungeon` (starts the
  flow, §4.2) + help text.

### 6.4 Suggestions — `services/suggestions.rs`

- Add `"dungeon" => Some(EntityKind::Dungeon)` to `entity_kind_for_root` so
  `set`/`reroll` field completions resolve from `DUNGEON_SCHEMA`.
- Beat addressing (`reroll setback`, `set setback loot`) is custom: extend the
  argument-suggestion stage to offer the five function names after
  `dungeon reroll` / `dungeon set` (the flat schema won't supply these).
- Add field-completion tests (template: the item/god tests). Run
  `cargo test suggestions` from `desktop/src-tauri`.

Command surface summary:

```
create dungeon                     # starts the guided flow (steps A–E)
  → (flow answers intercepted; final answer generates + loads draft)
dungeon show
dungeon set <field> <value>        # name|premise|topology|tone|twist
dungeon set <beat> <field> <value> # e.g. dungeon set setback loot none
dungeon reroll <beat>              # entrance|puzzle|setback|climax|resolution (or 1–5)
dungeon reroll premise|name        # scalar reroll
dungeon rename <name>
reroll                             # whole-dungeon regen (active draft)
save                               # persist TOML + DB + index
cancel                             # discard draft / exit flow
publish dungeon <name>             # export to Obsidian, then tweak there
load|show|preview|delete|undo <name>
```

---

## 7. Persistence & Publish

### 7.1 Save — `services/entity_persistence.rs`

Add `save_dungeon_draft` mirroring `save_item_draft`: validate `id`/`name`;
resolve slug (`dungeons` dir) and readable vault path (`dungeons/<Name>.md`);
preserve prior `published_at`; write **TOML** (`store.save_dungeon`), **DB**
(`dungeon_repo.upsert` with `beats_json`), and **document index**
(`document_repo.upsert_index("dungeon", …)`) in one call; delete stale TOML on
slug change. Returns `{ id, slug, vault_path, created_at, updated_at }`.

### 7.2 Publish — `services/publish.rs` + `commands/publish_commands.rs`

Add `render_dungeon_markdown(_with_links)` + a `dungeon_prose` helper. Suggested
Obsidian layout: premise as an intro line; a topology line; then a `##` section
per beat (`## 1. Entrance — [combat]`) with Idea / Lever / Loot (omit when `None`) /
Read-Aloud. Optional tier-2 `EntityLinker` cross-linking (gated on Ollama health),
matching item. Add the three `EntityType::Dungeon` arms in `publish_commands.rs`
(render `match`, `published_at`-stamp `match`, editor-close `match`). Publish is
**one-way**: stamp `published_at`, then `soft_delete_for_publish` removes it from
the app (it now lives in the vault, where the GM tweaks freely — the design's
"publish then tweak").

### 7.3 Vault sync — `services/vault_sync.rs`

Add `DungeonSync` (`SyncRepository` impl, `KIND = "dungeon"`) +
`dungeon_row_from_frontmatter` (serializes beats → `beats_json`) + a
`sync_entities(&DungeonSync(...))` call in `sync_from_vault`. Published records
(with `published_at`) get reaped; unpublished get upserted into DB + index;
orphaned DB rows pruned.

### 7.4 Admin — `services/entity_admin.rs`

Add `EntityType::Dungeon`; handle it in `resolve_entity`, `soft_delete_entity` /
`soft_delete_for_publish`, `undo_last_soft_delete`, and `search_entities`
(typeahead). Extend `EntityDetails` so the resolved entity carries the dungeon
fields (incl. beats), and `SuggestionHelperText`.

---

## 8. Output Rendering

No new `OutputBlock` variant and **no renderer/CSS changes** — a dungeon composes
from existing blocks. `OutputDoc` is flat (no nesting), so the five beats are five
sibling `EntityCard` blocks, and because `EntityCardRow` is plain `{label,value}`
with no inlines, **each per-beat reroll link must be its own `Paragraph` after the
card** (only `Paragraph`/`List` carry clickable `command_ref` inlines).

### 8.1 The dungeon card — `runebound-models/src/drafts.rs`

`dungeon_entity_card(&DungeonDraft) -> OutputDoc` (mirror `event_entity_card`'s
header-then-many-blocks shape):

```rust
pub fn dungeon_entity_card(draft: &DungeonDraft) -> OutputDoc {
    let mut out = doc();
    // 1. spine / premise top-line (feature-dungeons.md §6)
    out.push(heading(2, format!("{} — {}", draft.name, draft.premise)));
    // 2. topology line
    let topo = if draft.topology.is_empty() || draft.topology == "none" {
        "Topology: none (lay it out freely)".to_string()
    } else { format!("Topology: {}", draft.topology) };
    out.push(status(StatusTone::Info, topo));
    // 3. five beat cards, each followed by its own reroll Paragraph
    for (i, beat) in draft.beats.iter().enumerate() {
        let mut rows = vec![
            entity_row("Type:",  normalize_unknown_text(&beat.content_type)),
            entity_row("Idea:",  normalize_unknown_text(&beat.idea)),
            entity_row("Lever:", normalize_unknown_text(&beat.lever)),
        ];
        if let Some(loot) = &beat.loot {
            if !loot.trim().is_empty() { rows.push(entity_row("Loot:", loot.clone())); }
        }
        rows.push(entity_row("Read-Aloud:", normalize_unknown_text(&beat.read_aloud)));
        out.push(entity_card(&format!("{}. {}", i + 1, beat.function), rows));
        let key = beat.function.to_lowercase();
        out.push(paragraph_with_inlines(vec![
            text_node("Reroll this beat: "),
            command_ref(&format!("reroll {key}"), &format!("dungeon reroll {key}")),
        ]));
    }
    // 4. footer actions
    out.push(paragraph_with_inlines(vec![
        text_node("Use "), command_ref("save", "save"),
        text_node(", "), command_ref("reroll", "reroll"),
        text_node(" for a whole new dungeon, or "),
        command_ref("cancel", "cancel"), text_node(" to discard."),
    ]));
    out
}
```

`command_ref` buttons are clickable **unconditionally** (the renderer never
validates them against the manifest — only plain-text fallback is validated), so
`dungeon reroll setback` works as a button with no extra wiring.

### 8.2 Spinner labels — `desktop/src/App.tsx::commandSpinnerLabel`

Mandatory for every LLM-backed action. Add:

- **Generation:** via the Step-E heuristic (§4.6) → `"generating dungeon"`.
- `dungeon reroll <beat>` / `dungeon reroll …` → `"rerolling beat"`.
- `reroll` already → `"rerolling draft"` (or special-case `"rerolling dungeon"`).
- `dungeon save` / `save` → existing save group.
- `publish dungeon` → existing publish label.

### 8.3 Client event & exhaustiveness — `desktop/src/App.tsx`

Add `LoadDungeonDraftWithCard` to both switches (`applyClientEvent` sets the
`dungeonDraft` signal + `editorMode = "dungeon"` and nulls the others;
`outputDocFromClientEvent` returns `event.entity_card`), plus the `editorMode` /
`helperText` unions and a `dungeonDraft` signal. The `never` exhaustiveness checks
will force these. Regenerate TS: `cargo build -p runebound-models`.

---

## 9. File-by-File Change Manifest

**`runebound-models/`**
- `src/drafts.rs` — `DungeonBeat`, `DungeonDraft`, `DungeonFrontmatter`,
  `dungeon_entity_card()`.
- `src/events.rs` — `CommandClientEvent::LoadDungeonDraftWithCard`.
- `src/utils.rs` — `DUNGEON_*` enum consts + normalizers.
- (regen TS via `cargo build -p runebound-models`)

**`core/`**
- `src/npc.rs` — re-export `DungeonFrontmatter`.
- `src/entity_store.rs` — `dungeons` dir + `save/load/delete/list_dungeon`.
- `src/db.rs` — `DungeonRow` + CRUD + `row_to_dungeon`.
- `migrations/0015_dungeons.sql` — new table (append-only).

**`desktop/src-tauri/`**
- `src/entities/kind.rs` — `EntityKind::Dungeon` + arms + `ALL_ENTITY_KINDS`.
- `src/entities/schema.rs` — `DUNGEON_FIELDS`, `DUNGEON_SCHEMA`, `schema_for_kind`
  arm, locked-count test entry.
- `src/entities/domains/dungeon_domain.rs` — new domain (+ beat addressing).
- `src/entities/domains/mod.rs` / `registry.rs` — declare + register.
- `src/app_state.rs` — draft envelope/session wiring, `dungeon_repo`,
  **`DungeonCreationFlow` state**.
- `src/repositories/mod.rs` — `DungeonRepository` + `ProdDungeonRepository`.
- `src/services/ai_generation.rs` — `DungeonSeed` + `generate_dungeon_seed`.
- `src/services/entity_reroll.rs` — `reroll_dungeon_beat` + context/result structs.
- `src/services/entity_persistence.rs` — `save_dungeon_draft`.
- `src/services/entity_admin.rs` — `EntityType::Dungeon` everywhere.
- `src/services/vault_sync.rs` — `DungeonSync`.
- `src/services/publish.rs` — `render_dungeon_markdown(_with_links)`.
- `src/services/suggestions.rs` — `entity_kind_for_root` arm + beat suggestions +
  tests.
- `src/commands/create_commands.rs` — `create_dungeon` (starts flow) + branch.
- `src/commands/dungeon_commands.rs` — `handle_dungeon` router.
- `src/commands/dungeon_flow.rs` — **`try_execute_dungeon_flow` (the wizard)**.
- `src/commands/mod.rs` — module + handler entry + register.
- `src/commands/entity_commands.rs` — `Dungeon` arms (load/card).
- `src/commands/system_commands.rs` — `Dungeon` reroll arm.
- `src/commands/publish_commands.rs` — three `Dungeon` arms.
- `src/main.rs` — **intercept `dungeon_flow.active`** before registry dispatch.

**`command-specs/`**
- `src/lib.rs` — `dungeon` spec, `create dungeon`, `command_availability` arm,
  test-set updates, optional alias.

**`desktop/src/` (frontend)**
- `App.tsx` — `editorMode`/`helperText` unions, `dungeonDraft` signal, import,
  both event switches, `commandSpinnerLabel` patterns,
  `detectDungeonTopologyPrompt` heuristic, flow clickability.
- `generated/models.ts` — regenerated.

**`docs/`**
- `cli.md` command lists; `command-contexts.md` if a flow context is added later;
  `feature-development.md`/this spec kept in sync.

---

## 10. Verification

- `cargo build -p runebound-models` (regenerate TS contracts).
- `make build` (compiles backend + frontend; many tripwire tests run here).
- `cargo test suggestions` from `desktop/src-tauri`.
- Targeted tests to add: parser argument-vs-subcommand for `dungeon`; suggestions
  for `dungeon set`/`reroll` (incl. beat names); the schema locked-count entry.
- Manual flows: `create dungeon` → answer A–E → draft renders (premise line +
  topology + 5 beat cards + per-beat reroll buttons); click a beat reroll → only
  that beat changes, content_type differs, others identical; `set` a dial; `reroll`
  whole; `save`; `publish dungeon <name>` → Obsidian file correct, app draft
  cleared; `load`/`show`/`delete`/`undo`. Confirm spinners on every LLM call.

---

## 11. Open Questions, Risks & Decisions

1. **Load source (TOML vs DB).** Confirm `entity_admin` resolve/load hydrates
   beats from TOML (full fidelity) vs DB. The `beats_json` column makes DB
   lossless regardless, so this is low-risk — but verify before relying on either.
2. **Beat addressing vs the flat schema.** Beats don't fit `EntityFieldSpec`.
   Decision: keep dungeon-level fields in the flat schema; handle `<beat>` /
   `<beat> <field>` in the domain + custom suggestions. Accepted as the one
   deliberate divergence from the flat-entity pattern.
3. **Verbosity clamp.** Decision: dungeon generation ignores
   `generation.verbosity` and forces index-card brevity (esp. `read_aloud`). Risk:
   a future global verbosity change shouldn't silently re-balloon dungeon prose —
   keep the clamp local to `generate_dungeon_seed` / `reroll_dungeon_beat`.
4. **Flow state location.** Decision: desktop `AppState` (not core), because
   completion needs the desktop generation service + repos. Trade-off: a second
   bespoke state machine alongside `OnboardingSession` (duplication acknowledged in
   `architecture.md` §10). A shared "guided flow" primitive is a future refactor,
   not v1.
5. **"Generate premise" + custom premise.** When the GM types a custom premise
   (Step A), it's authoritative on the draft *and* passed as a constraint so beats
   align; when `generate`, the model produces it. Confirm the prompt handles both
   without drift.
6. **Topology "none".** `none` means no shape imposed (GM lays it out). The
   generator must not invent a topology when `none` is chosen — `topology` stays
   `"none"` and the card shows "lay it out freely."

---

## 12. Phased Implementation Plan

1. **Models & storage.** `runebound-models` structs/enums/card builder + events;
   `core` DB migration + store + CRUD; TS regen. (Compiles, nothing wired.)
2. **Entity plumbing.** `EntityKind`/`EntityType`, schema, domain (set/save/cancel
   first; reroll stubbed), registry, app_state, repository, persistence, vault
   sync, admin. Add `dungeon` manifest spec + availability + handler. (A dungeon
   can be hand-constructed, saved, loaded, published.)
3. **Generation.** `generate_dungeon_seed` + the one-shot `create_dungeon`
   (temporarily prompt-arg, pre-flow) to validate generation + the output card.
4. **Guided flow.** `DungeonCreationFlow` + `try_execute_dungeon_flow` + `main.rs`
   interception; switch `create dungeon` to start the flow; frontend spinner
   heuristic. (Full A–E UX.)
5. **Per-beat reroll.** `reroll_dungeon_beat` with frozen context + anti-stagnation
   + the per-card reroll buttons + domain write-back; whole `reroll` arm.
6. **Polish & tests.** Suggestions (beat names), spinner coverage, manual-flow
   pass, docs sync.

---

## 13. Related Docs

- `docs/feature-dungeons.md` — the design (oracle ethos, beats, the nine forms,
  tone dials).
- `docs/architecture.md` — entity-domain architecture, dispatch, extension
  playbooks (§8C is the new-entity checklist).
- `docs/command-contexts.md` — dispatch routes, availability, the setup-wizard
  interception model this flow mirrors.
- `docs/feature-development.md` — end-to-end new-entity implementation playbook.
- `docs/render.md` — output blocks, `command_ref`, spinner rule.
- `docs/config.md` — Ollama/model config, the setup-wizard invariants.

---

*Last updated: 2026-06-16*
