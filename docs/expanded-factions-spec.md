# Expanded Factions — Implementation Spec (v0.7.0)

> **Status:** ready to implement. This translates `docs/expanded-factions-design.md`
> (the locked design) into a concrete, multi-phase change set. It mirrors the v0.6.0
> location wizard everywhere it can, follows the `docs/architecture.md` §8C (entity)
> and §8D (wizard) playbooks, and obeys the **model-first** rule (§7): shared models
> change first, then the layers that consume them.
>
> **How to read it:** §1 is the destination (the new faction shape + enums). Phases
> 1–8 are the ordered work, each independently buildable with its own checkpoint.
> Appendices A–C are the lookup tables (steps, vulnerability seeds, files touched).
> File:line anchors point at the *current* code to change.

---

## 0. Scope, approach, and open decisions

**Approach.** The faction rework is the same dual-pattern move locations got: keep a
**one-shot lane** (`create faction <prompt>`) and add a **branching wizard** (`create
faction`) whose `finalize()` hands a `FactionDraft` to the existing faction editor
(`docs/architecture.md` §4 "Combining the two"). Nearly every piece has a location
analogue to copy — they are cited inline.

**Breaking change, by decision.** Per the design §10 and the user's call, old factions
are **not** migrated. The schema migration **drops and recreates** the `factions`
table (test data is wiped between releases). This keeps the new columns clean rather
than threading nullable add-columns through a live table.

**Decisions baked into this spec** (each resolves an ambiguity in the design; flagged
so they can be overridden in review):

| # | Decision | Rationale |
|---|---|---|
| D1 | **Drop `kind_custom`** from the faction model entirely. | The new scheme has 9 fixed kinds and **no `other`** (design §3). Unlike locations (which keep `other`/`kind_custom` for freeform one-shots), factions have no freeform kind — the one-shot just picks one of the 9. Removing it deletes a whole validation branch. |
| D2 | **`category` is derived from `kind_type`, not user-settable.** Stored in frontmatter + DB row (computed at save), but absent from `FactionDraft` and not a schema field. | Single-sourced from kind (`faction_category()`), so it can't drift; persisting it lets Obsidian/dataview and the DB filter by category. Mirrors how location derives its subfolder from `kind_type`. |
| D3 | **Relational/place fields are settable but never rerollable / never LLM-generated:** `leader`, `allies`, `rivals_enemies`, `liege`, `loyalty_type`, and the rendered-only `headquarters`. | Design §7 — avoid an ever-growing web of invented names. These are wizard-picked or left as blank stubs. |
| D4 | **Allies and Rivals are repeatable single-link picker steps** (link one → "link another / done"), accumulating into the existing `Vec<String>` fields; `skip`/`done` with none linked leaves a blank published stub. | Matches the `Vec` shape and the design's "link or leave blank." A repeat loop is the minimal way to allow several links without an auto-generated web. |
| D5 | **Two routing steps:** Step 1 picks **category** (3 choices), then a per-category **kind** step. (Location uses one flat kind step.) | The three categories have genuinely different question sets (design §8.1–8.3); a category router keeps each branch self-contained and lets `houses` sub-route by political layer. |

> **The only items worth a second look before coding** are D1 (drop `kind_custom`)
> and D2 (derive vs. store `category`) — both are reversible but shape the model. The
> rest follow directly from the design.

---

## 1. Target data model (the destination)

### 1.1 Field-by-field: the new `FactionDraft`

`runebound-models/src/drafts.rs:93` (`FactionDraft`) becomes:

| Field | Type | Origin | Notes |
|---|---|---|---|
| `id` | `String` | — | unchanged |
| `seed_prompt` | `Option<String>` | — | unchanged (reroll bias) |
| `name` | `String` | LLM | unchanged |
| `slug` | `String` | — | unchanged |
| `vault_path` | `String` | — | unchanged |
| `kind_type` | `String` | GM-locked (wizard) / LLM (one-shot) | now one of the **9** kinds (§1.4) |
| `public_description` | `String` | LLM | visible face |
| `reputation` | `String` | LLM | visible face |
| `symbol_description` | `String` | LLM | visible face |
| `want` | `String` | GM-seed or LLM | **WOAC** — absorbs `true_agenda` (+ goals laddering) |
| `obstacle` | `String` | LLM (vuln-seeded) | **WOAC** — absorbs `current_tension` |
| `action` | `String` | LLM | **WOAC** — absorbs `methods` |
| `consequence` | `String` | LLM | **WOAC** — new |
| `leader` | `String` | **picker / blank** | was `leadership`; now an NPC link name or free text, never LLM |
| `sphere_of_influence` | `String` | LLM | scaled by layer/reach |
| `resources_assets` | `Vec<String>` | LLM | unchanged |
| `allies` | `Vec<String>` | **picker / blank** | never LLM |
| `rivals_enemies` | `Vec<String>` | **picker / blank** | never LLM |
| `liege` | `Option<String>` | **picker / free-text** | houses Vassal/Lord only; faction link name |
| `loyalty_type` | `Option<String>` | **enum / random** | houses Vassal/Lord only; one of the 7 loyalty types (§1.4) |
| `wizard_subfoldered` | `bool` | wizard=true, one-shot=false | **new**, mirrors `LocationDraft.wizard_subfoldered`; gates category subfoldering at save |

**Removed** (vs. current): `true_agenda`, `methods`, `current_tension`,
`goals_short_term`, `goals_long_term`, `headquarters`, `kind_custom`.
**Added:** `want`, `obstacle`, `action`, `consequence`, `liege`, `loyalty_type`,
`wizard_subfoldered`. **Renamed:** `leadership` → `leader`.
**Derives `#[derive(Debug, Clone, Serialize, Deserialize, TS)]`** stays; keep
`#[serde(default)]` on the `Option`/`Vec`/`bool` fields.

### 1.2 `FactionFrontmatter` (`drafts.rs:280`)

Same field set as the draft **minus** `seed_prompt`/`wizard_subfoldered`, **plus** the
persisted metadata. New/changed fields: `category: String` (computed at save, D2),
`want`/`obstacle`/`action`/`consequence`, `leader`, `liege: Option<String>`,
`loyalty_type: Option<String>`. Keep `created_at`/`updated_at`/`published_at` and the
`#[serde(rename = "type")] doc_type`. Keep the `string_or_seq_list` back-compat
deserializer on `resources_assets`/`allies`/`rivals_enemies` (it's harmless and the
store may hold scalar legacy text).

### 1.3 `FactionRow` (`core/src/db.rs:62`) + table

Columns become: `id, slug, name, vault_path, kind_type, category,
public_description, reputation, symbol_description, want, obstacle, action,
consequence, leader, sphere_of_influence, resources_assets, allies, rivals_enemies,
liege, loyalty_type, created_at, updated_at`. List columns stay JSON-text (the
`faction_list_to_db_text` convention). `liege`/`loyalty_type` are nullable
(`Option<String>`); `category` is `NOT NULL`.

### 1.4 Enum vocabularies (the canonical string lists)

Defined once in `runebound-models/src/utils.rs` (kinds + loyalty, which persist) and in
`services/ai_generation.rs` (lord-type / control-type / mandate / reach / brand, which
are wizard-prompt vocab, not persisted). All lowercase, `snake_case`.

```
FACTION_KIND_TYPES (9, replaces the old 10):
  great_house, major_vassal, minor_vassal, individual_lord,   // houses
  guild, company, criminal_syndicate,                          // establishments
  temple, cult                                                 // religion

FACTION_CATEGORIES (3):  houses, establishments, religion
LOYALTY_TYPES (7):  reward, marriage, military, economic, shared_enemy, oath, secret
LORD_TYPES (6):  chokepoint, surplus, junction, specialist, march, extraction
CONTROL_TYPES (5):  craft, service, trade, vice, knowledge
MANDATES (6):  devotion, sacrifice, conquest, purity, secret_knowledge, cycle
REACH (3):  local, regional, realm
HOUSE_BRANDS (6 + custom):  wealth, loyalty, martial, piety, cunning, lineage
```

### 1.5 Derived helpers (new, in `services/ai_generation.rs`, mirroring `location_*`)

```rust
pub enum FactionCategory { Houses, Establishments, Religion }
pub fn faction_category(kind_type: &str) -> Option<FactionCategory> // None = freeform one-shot
pub fn faction_category_str(kind_type: &str) -> &'static str         // "houses" | … | "" for the frontmatter/row
pub fn faction_subfolder(kind_type: &str) -> Option<&'static str>    // Some("houses") | … | None
pub fn faction_dir_for_kind(base: &str, kind_type: &str) -> String   // base/sub or base
```

`faction_subfolder` maps each of the 9 kinds to its category folder; everything else
→ `None` (flat). This is the exact analogue of `location_subfolder`
(`ai_generation.rs:1343`) / `location_dir_for_kind` (`:1355`).

---

## Phase 1 — Shared models & contracts (`runebound-models`)

**Goal:** land the new model so every downstream layer compiles against it. Model-first
(architecture §7).

1. **`src/utils.rs`**
   - Replace `FACTION_KIND_TYPES` (`:82`) with the 9 kinds (§1.4).
   - Add `FACTION_CATEGORIES` and `LOYALTY_TYPES` consts.
   - Rewrite `normalize_faction_kind_type` (`:481`) to validate against the new 9
     (drop the hyphen→underscore is fine to keep; drop any `other` handling).
   - Add `normalize_loyalty_type(&str) -> Result<String, String>` (validates against
     `LOYALTY_TYPES`).
   - Keep `string_or_seq_list` (`:567`) untouched.
2. **`src/drafts.rs`**
   - Rewrite `FactionDraft` (`:93`) and `FactionFrontmatter` (`:280`) per §1.1–1.2.
   - Rewrite `faction_entity_card` (`:530`) row order to: Name, Slug, Kind, Category
     (derived — call a small `faction_category_str` or inline the map; the card is
     display-only so deriving here is fine), Public Face, Reputation, Symbol, Want,
     Obstacle, Action, Consequence, Leader, Sphere of Influence, Resources, Allies,
     Rivals, Liege *(only if `Some`)*, Loyalty *(only if `Some`)*, Path. Footer
     `command_ref` (`save`/`reroll`) unchanged.
3. **Regenerate TS** — `UPDATE_MODELS=1 cargo test -p runebound-models` (architecture
   §8C.2). This refreshes `desktop/src/generated/models.ts`; the ts-rs drift guard
   (`services/ts_export.rs`) will fail the build if skipped.

**Checkpoint:** `cargo build -p runebound-models` + the model crate's tests pass; TS
regenerated. (Downstream crates won't compile yet — expected.)

---

## Phase 2 — Entity schema & domain (`desktop/src-tauri/src/entities`)

**Goal:** drive `set` / `reroll` / the card off the new fields. The schema is the
single source the suggestion service and field dispatch consume (architecture §4).

1. **`schema.rs`** — rewrite `FACTION_FIELDS` (`:273`). The settable/rerollable matrix
   is the teeth of D3:

   | Field | display / aliases | value_kind | settable | **rerollable** |
   |---|---|---|---|---|
   | `name` | name | Text | ✓ | ✓ |
   | `kind_type` | kind / kind_type | Enum (9) | ✓ | ✓ |
   | `category` | — | — | — | — *(derived; not a field)* |
   | `public_description` | public | Text | ✓ | ✓ |
   | `reputation` | reputation | Text | ✓ | ✓ |
   | `symbol_description` | symbol / sigil / banner | Text | ✓ | ✓ |
   | `want` | want / agenda | Text | ✓ | ✓ |
   | `obstacle` | obstacle / tension | Text | ✓ | ✓ |
   | `action` | action / methods | Text | ✓ | ✓ |
   | `consequence` | consequence | Text | ✓ | ✓ |
   | `sphere_of_influence` | influence | Text | ✓ | ✓ |
   | `resources_assets` | resources | List | ✓ | ✓ |
   | `leader` | leader / leadership | Text | ✓ | **✗** |
   | `allies` | allies | List | ✓ | **✗** |
   | `rivals_enemies` | rivals | List | ✓ | **✗** |
   | `liege` | liege | Text | ✓ | **✗** |
   | `loyalty_type` | loyalty | Enum (7) | ✓ | **✗** |

   Keep the old aliases (`agenda`, `tension`, `methods`, `leadership`) pointed at the
   new fields so muscle-memory commands still resolve. Update `reroll_instruction`
   strings for the rerollable fields (e.g. `want`: "Generate the faction's deep aim in
   1–2 sentences."; `obstacle`: "Generate the obstacle in its way in 1–2 sentences.";
   etc.). Non-rerollable fields need no instruction.
2. **`domains/faction_domain.rs`** (`:29`) — rewrite the `set_field` and `reroll_field`
   match arms to the new field set (drop removed fields, add WOAC + `liege`/
   `loyalty_type`, rename `leadership`→`leader`). `reroll_field` only handles the
   rerollable subset; `set_field` handles all. Normalize `kind_type` via the model's
   `normalize_faction_kind_type`; normalize `loyalty_type` via `normalize_loyalty_type`.
   Drop the local duplicate `normalize_faction_kind_type` (`:384`) in favor of the
   model crate's. Update `faction_summary_text` (`:408`) and `faction_event_from_draft`
   (`:433`) to the new fields.
3. **`kind.rs`** — no change (`EntityKind::Faction` stays).

**Checkpoint:** `cargo test -p` the desktop entities module; the schema/registry
contract tests should pass once Phases 3–4 land. (Still won't fully link — DB/services
pending.)

---

## Phase 3 — DB & persistence

**Goal:** store and project the new shape.

1. **Migration** `core/migrations/0010_factions_woac.sql` (sqlx auto-discovers
   `NNNN_*.sql` in order; `0009` is checksummed and must not be edited):
   ```sql
   DROP TABLE IF EXISTS factions;
   CREATE TABLE factions ( …new columns from §1.3… );
   CREATE INDEX idx_factions_slug ON factions(slug);
   CREATE INDEX idx_factions_name ON factions(name);
   CREATE INDEX idx_factions_category ON factions(category);
   ```
2. **`core/src/db.rs`** — rewrite `FactionRow` (`:62`) per §1.3 and the
   `impl_entity_table!` column list (`:251`): `strict`/`lenient`/`opt` per nullability
   (`liege`/`loyalty_type` → `opt`; `category` → `lenient "".to_string()` or a strict
   value). The macro regenerates the full CRUD set — no hand SQL.
3. **`repositories/mod.rs`** — `FactionRepository` + `ProdFactionRepository` (`:305`)
   are field-agnostic (they pass whole `FactionRow`s); **no change**.
4. **`services/entity_persistence.rs`** — rewrite the `impl_entity_persistence!`
   invocation (`:212`):
   - Add the **`vault_dir:`** arg (the subfoldering hook, copied from location at
     `entity_persistence.rs:136`):
     ```rust
     vault_dir: crate::services::ai_generation::faction_dir_for_kind(
         "factions",
         if draft.wizard_subfoldered { &kind_type } else { "" },
     ),
     ```
     (`""` → `faction_dir_for_kind` returns the flat base, so one-shots stay in
     `factions/`; the wizard subfolders by category.)
   - `normalize` block: validate `kind_type`; compute `let category =
     faction_category_str(&kind_type).to_string();`; normalize the WOAC text fields,
     `leader`, `sphere_of_influence`, `reputation`, lists; normalize `liege`
     (trim→`Option`) and `loyalty_type` (validate→`Option`). Drop all the removed
     fields and the `kind_custom`/`other` branch.
   - `frontmatter_fields` / `row_fields`: list the new columns incl. `category`,
     `liege`, `loyalty_type`. (A forgotten field is a struct-literal compile error — the
     macro's safety net.)
5. **`services/vault_sync.rs`** — rewrite `faction_row_from_frontmatter` (`:609`) to map
   the new frontmatter→row fields (incl. `category`, `liege`, `loyalty_type`, lists via
   `faction_list_to_db_text`). `FactionSync` (`:312`) is generic; no change.

**Checkpoint:** `cargo build -p dnd_core` and the desktop crate compile; run the app
once so the migration applies (drops/recreates `factions`); `vault_sync` round-trips a
hand-written TOML faction. Note `faction_dir_for_kind` lives in `ai_generation.rs`
(Phase 5's helpers) — land §1.5's helper stubs early (they're tiny) so Phase 3 links.

---

## Phase 4 — One-shot generation & reroll

**Goal:** `create faction <prompt>` and per-field reroll produce the new shape.

1. **`services/ai_generation.rs`**
   - Rewrite `FactionSeed` (`:1642`) to the **LLM-filled** fields only: `name`,
     `kind_type` (one-shot needs the model to pick; `#[serde(default)]` so the wizard
     can omit it from its schema and lock it — mirrors `LocationSeed.kind_type` at
     `:1620`), `public_description`, `reputation`, `symbol_description`, `want`,
     `obstacle`, `action`, `consequence`, `sphere_of_influence`, `resources_assets`.
     **Not** in the seed: `leader`, `allies`, `rivals_enemies`, `liege`,
     `loyalty_type`, `headquarters` (D3).
   - Rewrite `generate_faction_seed` (`:583`) — the one-shot path: new JSON schema +
     system prompt naming the WOAC fields and the 9 kinds; keep `FACTION_GEN_SAMPLING`
     (`:105`), the recent-seed dedup, and `@reference` reuse. The one-shot draft's
     `leader`/`allies`/`rivals_enemies` default empty; `liege`/`loyalty_type` `None`.
2. **`services/entity_reroll.rs`** — rewrite `FactionRerollContext` (`:1179`) to the
   full new field set (context carries everything for grounding, even non-rerollable
   fields). `reroll_faction_field` (`:669`) only *generates* the rerollable subset
   (§2.1 matrix); update `faction_context_summary` (`:1361`) to emit the new
   key=values. Keep `FACTION_SAMPLING`.
3. **`commands/create_commands.rs`** — update `create_faction` (`:204`) to build the
   new `FactionDraftSession` (drop removed fields, add WOAC + empty relational fields +
   `wizard_subfoldered: false`). Mirror `create_location`'s one-shot block (`:149`).

**Checkpoint:** `create faction a maritime smuggling cartel` produces a draft;
`faction show` renders the new card; `faction reroll obstacle` rerolls; `faction set
loyalty oath` validates; `faction save` writes flat `factions/<slug>.md` with WOAC
sections.

---

## Phase 5 — The faction wizard (`desktop/src-tauri/src/wizards/faction.rs`)

**Goal:** the branching guided flow. This is the centerpiece; everything else is
plumbing it rides on. Built entirely on the existing engine (architecture §8D) — the
only edits outside this file are one registry line and the `create` launch arm.

### 5.1 Generation helpers (in `ai_generation.rs`, mirroring the `location_*` wizard chain)

- `FactionCategory` enum + `faction_category` / `faction_subfolder` /
  `faction_dir_for_kind` (§1.5).
- `FactionWizardInputs` struct (analogue of `LocationWizardInputs` `:1366`) — every
  locked answer: `kind_type`, `category`, `power_base` (lord-type) + `power_specifics`,
  `brand`, `liege` + `loyalty_type`, `control_type` + `control_specifics`, `reach`,
  `god` + `mandate` + `mandate_specifics`, `patron`, `want` (GM ambition seed), and a
  reroll `hint`.
- `build_faction_wizard_user_prompt(&FactionWizardInputs) -> String` (analogue of
  `build_wizard_user_prompt` `:1547`): a concise restatement of the locked answers that
  **doubles as the `@reference` probe**. Emits `@gods/<god>`, `@factions/<liege>`,
  `@factions/<patron>` tokens so the deity's domain and the liege/patron house metadata
  get pulled into context (exactly how guildhall threads `@factions/<name>` and
  `@locations/<sub>/<anchor>` at `:1585`/`:1597`). Reused by the wizard's
  `build_seed_prompt` to persist GM intent as reroll bias.
- `wizard_faction_system_prompt(&FactionWizardInputs, FactionCategory) -> String`
  (analogue of `wizard_location_system_prompt` `:1425`): per-category framing that bakes
  in the design's generation rules — **Obstacle pre-seeded from the chosen
  lord-type/control-type/mandate vulnerability** (Appendix B), the visible/hidden gap
  (public_description = the claim; the leverage shows in want/action), Great House =
  no direct peer assault, Vassal/Lord = liege + loyalty fault line, Cult widens the
  public/true gap. `want` is locked when GM-seeded, else LLM-inferred.
- `wizard_faction_schema(FactionCategory) -> serde_json::Value` (analogue of
  `wizard_location_schema` `:1492`): the WOAC schema **omitting** `kind_type` (GM-locked)
  and all relational fields (never generated). Same across categories (the *prompt*
  differs, the field set doesn't) — one schema is fine.
- `generate_faction_seed_for_wizard(&FactionWizardInputs, …) -> SeedGeneration<FactionSeed>`
  (analogue of `generate_location_seed_for_wizard` `:459`): branch via
  `faction_category`, build prompt/schema, run `run_seed_attempts`, and in the accept
  closure **lock `kind_type` from inputs** and **override `want` with the GM seed when
  present** (mirrors the `seed.kind_type = kind_type.clone()` lock at `:527` and the
  `seed.authority = faction_name` override at `:564`).

### 5.2 Pickers — extend `wizards/entity_link.rs`

The faction picker already exists (`load_linkable_factions` `:113`) and is reused for
**liege**, **patron**, **allies**, **rivals**. Add two siblings (copy the
`load_linkable_factions` body, swapping repo + folder):

- `load_linkable_npcs(&AppState) -> Vec<(String,String)>` — `state.npc_repo().list_all`
  + `load_published_entity_names("npcs")`. For the **leader** picker.
- `load_linkable_gods(&AppState) -> Vec<(String,String)>` — `state.god_repo().list_all`
  + `load_published_entity_names("gods")`. For the **god** picker.

All matching/typeahead (`entity_suggestions`, `match_entity`, `EntityMatch`,
`merge_linkable`) is already generic over `(name, slug)` — reused as-is.

### 5.3 The wizard (`wizards/faction.rs`) — structure mirrors `wizards/location.rs`

- **`FactionWizardData`** accumulator (analogue of `LocationWizardData` `location.rs:60`):
  one field per answer in §5.1's inputs, plus the picker working sets (`npcs`, `gods`,
  `factions: Vec<(String,String)>`), the picked link names/slugs, a
  `link_return: Option<&'static str>` (which step requested a shared picker, like
  `faction_link_return`), the accumulated `allies`/`rivals` Vecs, and the generated
  `seed`/`notice`. `as_inputs(hint)` projects it into `FactionWizardInputs`.
- **Steps** (each an `impl WizardStep<AppState>`; build prompts only with
  `wizard::prompt::{wizard_menu, action_row, choice_lines}` so every choice is a
  clickable `command_ref`):
  - `CategoryStep` (id `category`) — 3 choices → `Goto` the category's kind step (D5).
  - **Houses:** `HouseLayerStep` (`houses_layer`, the 4 kinds) → routes A
    (`great_house`) vs B (vassal/lord); `PowerBaseStep` (`power_base`, 6 lord-types, no
    random); `PowerSpecificsStep` (`power_specifics`, optional free text);
    `BrandStep` (`brand`, Great House only, 6 + custom); the liege picker +
    `LoyaltyTypeStep` (`loyalty_type`, 7 + `0`=random) for vassal/lord; then the shared
    tail.
  - **Establishments:** `EstKindStep` (`est_kind`, 3 kinds); `ControlTypeStep`
    (`control_type`, 5 types); `ControlSpecificsStep`; `ReachStep`; the patron picker;
    then the shared tail.
  - **Religion:** `RelKindStep` (`rel_kind`, temple/cult); the god picker; `MandateStep`
    (`mandate`, 6); `MandateSpecificsStep`; `ReachStep` (shared); the patron picker;
    then the shared tail.
  - **Shared tail** (one set of steps, reused by all three branches): `AmbitionStep`
    (`ambition`, optional GM Want seed → skip); `LeaderStep` (`leader`, NPC picker →
    skip = blank); `AlliesStep` (`allies`, faction picker, repeatable per D4);
    `RivalsStep` (`rivals`, faction picker, repeatable); `GenerateStep` (the terminal
    step — records the last optional field, runs `generate_faction_into`, returns
    `WizardTransition::Complete`). Model `GenerateStep` on `location.rs:960` (optional
    free text → generate → complete) and `ReachStep`/menu steps on the `numbered_choices`
    + `pick_value` helpers (`location.rs:1272`/`:1281`), which should be lifted to a
    shared module or duplicated.
  - **Shared pickers** (one step each, parameterized by `link_return` like
    `FactionLinkStep` `location.rs:1031`): a faction picker (liege / patron / allies /
    rivals), an NPC picker (leader), a god picker. Each loads its set via the §5.2
    helpers on entry (mirror `enter_faction_link` `location.rs:1313`), typeaheads via
    `entity_suggestions`, resolves via `match_entity`, and accepts a free-typed name as
    a fallback where the design allows (liege, god, leader). Liege is **required** for
    vassal/lord (no `skip`, free-text accepted — exactly the guildhall-mandatory mode at
    `location.rs:1027`); allies/rivals/patron/leader are **skippable**.
- **`FactionWizard`** (`impl Wizard<AppState>`, analogue of `LocationWizard`
  `location.rs:1165`): `id() = "faction"`, `title() = "Create Faction"`, `steps()` lists
  every step Arc, `seed()` returns `WizardData::new(FactionWizardData::default())`,
  `finalize()` builds the `FactionDraft` from the seed + locked answers and calls
  `editor.set_faction(draft)` (the same hand-off `create_faction` does).
- **`build_faction_draft`** (analogue of `build_location_draft` `location.rs:1437`):
  locks `kind_type` from the accumulator (never the model), sets `wizard_subfoldered:
  true`, fills `leader`/`allies`/`rivals_enemies`/`liege`/`loyalty_type` from the
  accumulator (not the seed), and `seed_prompt` from `build_seed_prompt`.
- **`generate_faction_into`** (analogue of `generate_location_into` `location.rs:1395`):
  call `generate_faction_seed_for_wizard`.
- Set `awaiting_llm_label() = Some("generating faction")` on the `GenerateStep` so the
  `WizardView` spinner fires with no frontend text-matching (architecture §8 wizard
  exception).

### 5.4 Register

`wizards/mod.rs:40` — add `registry.register(Arc::new(FactionWizard::new()));` and
`pub mod faction;`. **No other plumbing edits** (dispatch, nav verbs, context,
typeahead, spinner all work unchanged — architecture §8D.5).

**Checkpoint:** `cargo test suggestions` (step-token typeahead) + the per-step routing
unit tests (Appendix A). Manually walk each branch: choices clickable, `back`/`cancel`
at each step, spinner on generate, `finalize` opens the editor.

---

## Phase 6 — Command surface & publish

1. **`commands/create_commands.rs`** — make bare `create faction` launch the wizard and
   `create faction <prompt>` stay one-shot, mirroring the location split (`create
   location` launches `start_wizard("location", …)` at `create_commands.rs:67`; the
   `<prompt>` form falls to one-shot). Add the symmetric `start_wizard("faction", …)`
   arm.
2. **`command-specs/src/lib.rs`**
   - `create` subcommand `faction` summary already reads "Start guided faction
     creation" — keep; ensure the example shows both `create faction` and `create
     faction <prompt>`.
   - Update the `faction` `CommandSpec` examples (`:650`) to the new fields (`faction
     set want …`, `faction set obstacle …`, `faction reroll action`, `faction set
     loyalty oath`, `faction set liege House Vaurel`).
   - `command_availability("faction")` stays `EntityScoped("faction")` (`:189`); the
     wizard nav verbs (`continue`/`back`/`cancel`) already have arms. **No new command
     roots**, so no availability work beyond examples.
   - Spinner hints (`:238`): `create faction → generating faction`, `faction reroll →
     rerolling faction`, `faction save → saving draft` stay correct; the **wizard**
     path uses the `WizardView` signal, not these (architecture §8).
3. **`services/publish.rs`** — rewrite `render_faction_markdown_with_links` (`:85`) to
   the new section order, with blank-stub handling for the never-generated fields:
   - Attr lines: **Kind** (display form), **Category**.
   - Prose (`write_section`, prose-linked): **Public Description**, **Reputation**,
     **Symbol**, then **WOAC** — **Want**, **Obstacle**, **Action**, **Consequence**,
     then **Sphere of Influence**.
   - **Leadership** — `[[leader]]` wikilink when set, else a **blank stub** (emit the
     heading so the GM fills it in Obsidian). Add a `write_blank_section(out, title)`
     helper (emits `## {title}\n\n`) for the stub case.
   - **Headquarters** — **always a blank stub** (design §7: dropped from data, still
     rendered for manual fill).
   - **Resources & Assets** — `write_list_section` (plain).
   - **Allies** / **Rivals** — `write_linked_list_section` (`[[wikilink]]` each) when
     non-empty, else a blank stub.
   - **Liege** (houses vassal/lord, when `Some`) — `[[liege]]` wikilink.
   - **Loyalty** (houses vassal/lord, when `Some`) — attr line with the value.
   - Subfoldering is decided at **save** (Phase 3's `vault_dir:`), not here — publish
     is path-agnostic (it renders the body; the `.md` location is already resolved).
4. **`app_state.rs`** — the `DraftEnvelope::Faction` variant + `set/get/take_faction`
   helpers are field-agnostic; only the test fixture `faction_draft` (`:594`) needs the
   new fields. (The `FactionDraftSession` alias already tracks `FactionDraft`.)
5. **`services/suggestions.rs`** — `entity_kind_for_root` (`:610`) and field-argument
   suggestions are schema-driven; they pick up the new fields automatically. The wizard
   step-suggestion path (`active_step_suggestions`, `:77`) is generic. **No code change**
   — but add `set`/`reroll` field-completion tests for the new field names.

**Checkpoint:** `make build`; `cargo test suggestions`. Walk a published houses faction
→ confirm it lands in `factions/houses/`, WOAC sections render, HQ/allies/rivals show
blank stubs, liege/loyalty render only for vassal/lord.

---

## Phase 7 — Frontend (`desktop/src`)

Minimal — the faction card is backend-built (`faction_entity_card`) and rendered through
the generic `OutputDoc` path; the wizard rides the existing `WizardView` spinner.

1. `App.tsx` — `load_faction_draft_with_card` already returns `event.entity_card`
   (`:553`); no change. Confirm `commandSpinnerLabel` still resolves `create faction`
   from the manifest hints (it does — no edit).
2. Regenerated `models.ts` (Phase 1) is consumed automatically — verify the card renders
   the new rows and no removed field is referenced anywhere in `desktop/src`.

**Checkpoint:** create (wizard + one-shot), show, set, reroll, save, load, cancel a
faction in the desktop UI; spinner appears on the wizard generate step and on `create
faction <prompt>` / `faction reroll`.

---

## Phase 8 — Verification

Run the architecture §11 / feature-dev §10 checklist. Specifically:

- **Unit tests** in `wizards/faction.rs` (Appendix A): category→kind routing, the
  great_house vs vassal/lord split, `build_faction_draft` locks `kind_type` and sets
  `wizard_subfoldered`, `faction_subfolder` maps all 9 kinds, the liege-mandatory
  (no-skip) mode, allies/rivals repeat-accumulate, GM `want` override.
- **Generation tests** in `ai_generation.rs`: `build_faction_wizard_user_prompt` emits
  the right `@gods/`/`@factions/` tokens; `wizard_faction_schema` omits `kind_type` +
  relational fields; the vulnerability→Obstacle seed text appears per lord-type/
  control-type/mandate.
- `cargo test suggestions` (field completions + step typeahead).
- `cargo test -p runebound-models` (ts-rs drift guard).
- `cargo test --workspace` + the desktop suite (schema/registry contract tests).
- **Desktop workspace caveat** (memory): the Tauri crate is skipped by `cargo
  --workspace` at root — build `desktop/src-tauri` separately.
- `make build`.
- Manual: the full **create NPC → create faction (link NPC leader) → create location
  (guildhall linking the faction)** loop, end to end, confirming each picker reads the
  prior entity.

> **Per the project rule, do not commit — stage for review.** ([[never-commit-user-reviews-locally]])

---

## Appendix A — Per-branch step tables

Buckets: **M** = Must (required), **C** = Can (optional → skip/blank/random), **G** =
generate. Transitions use the engine's `Goto`/`Next`/`Complete`.

### A.1 Houses (design §8.1)

| id | Prompt | Bucket | Applies | On accept |
|---|---|---|---|---|
| `category` | Category | M | all | → `houses_layer` |
| `houses_layer` | House layer (4 kinds) | M | houses | great_house → `power_base` (flow A); vassal/lord → `power_base` (flow B) |
| `power_base` | Power base (6 lord-types, no random) | M | houses | → `power_specifics` |
| `power_specifics` | Specifics (free text) | C | houses | A → `brand`; B → liege picker |
| `brand` | Brand (6 + custom) | M | great_house | → `ambition` (tail) |
| *(liege picker)* | Liege (faction picker → free-text) | M | vassal/lord | → `loyalty_type` |
| `loyalty_type` | Loyalty type (7, `0`=random) | C | vassal/lord | → `ambition` (tail) |
| `ambition` | Ambition / Want | C | all | → `leader` |
| `leader` | Leader (NPC picker) | C | all | → `allies` |
| `allies` | Allies (faction picker, repeat) | C | all | → `rivals` |
| `rivals` | Rivals (faction picker, repeat) | C | all | → `generate` |
| `generate` | (optional detail) → generate | G | all | `Complete` |

### A.2 Establishments (design §8.2)

`category → est_kind (M) → control_type (M) → control_specifics (C) → reach (M) →
patron picker (C) → ambition (C) → leader (C) → allies (C) → rivals (C) → generate`.

### A.3 Religion (design §8.3)

`category → rel_kind (M) → god picker (M) → mandate (M) → mandate_specifics (C) →
reach (M) → patron picker (C) → ambition (C) → leader (C) → allies (C) → rivals (C) →
generate`.

The **tail** (`ambition → leader → allies → rivals → generate`) is identical across all
three branches — implement once, share.

---

## Appendix B — Vulnerability → Obstacle seed tables

Fed into `wizard_faction_system_prompt` so the LLM's `obstacle` is grounded in the
chosen power base's built-in fault line (design §4, §8.2, §8.3).

**Lord-types (houses):** chokepoint → an alternate route opens; surplus → spoilage /
raid / glut; junction → a rival port or route; specialist → input supply cut or
technique copied; march → peace lets the crown reclaim the autonomy (war makes you first
to fall); extraction → the vein runs dry / floods / a richer deposit opens.

**Control-types (establishments):** craft → a rival guild or cheap substitute; service →
only as good as the last job (a defeat / betrayal); trade → a rival route, new tariff,
or revoked charter; vice → the law, a rival crew, a crackdown; knowledge → a leaked
secret or a debt called in.

**Mandates (religion):** devotion → donor fatigue / a richer rival temple; sacrifice →
the supply of victims runs out / public backlash; conquest → resistance / a crusade
against them; purity → schism over who's pure / purges; secret_knowledge → the secret
leaks / rival seekers; cycle → a broken cycle / encroaching civilization.

Plus the **loyalty fault lines** for vassal/lord (fed alongside liege): reward →
slighting others & rising expectations; marriage → mixed loyalty / hostages; military →
one failure cracks it (too-strong flips to fear); economic → a rival's better terms;
shared_enemy → losing the common threat; oath → lasts only while someone still cares;
secret → blackmail makes enemies.

---

## Appendix C — Files-touched matrix

| Layer | File | Change |
|---|---|---|
| Model | `runebound-models/src/utils.rs` | kinds (9), categories, loyalty consts; normalize fns |
| Model | `runebound-models/src/drafts.rs` | `FactionDraft`, `FactionFrontmatter`, `faction_entity_card` |
| Model | `desktop/src/generated/models.ts` | regenerated (`UPDATE_MODELS=1`) |
| Schema | `desktop/src-tauri/src/entities/schema.rs` | `FACTION_FIELDS` (settable/rerollable matrix) |
| Domain | `desktop/src-tauri/src/entities/domains/faction_domain.rs` | `set_field`/`reroll_field`/summary/event |
| DB | `core/migrations/0010_factions_woac.sql` | **new** — drop + recreate table |
| DB | `core/src/db.rs` | `FactionRow` + `impl_entity_table!` columns |
| Persist | `desktop/src-tauri/src/services/entity_persistence.rs` | `impl_entity_persistence!` (+`vault_dir:`) |
| Persist | `desktop/src-tauri/src/services/vault_sync.rs` | `faction_row_from_frontmatter` |
| Gen | `desktop/src-tauri/src/services/ai_generation.rs` | `FactionSeed`, one-shot gen, **+ all §5.1 wizard helpers** |
| Reroll | `desktop/src-tauri/src/services/entity_reroll.rs` | `FactionRerollContext`, `reroll_faction_field`, summary |
| Wizard | `desktop/src-tauri/src/wizards/faction.rs` | **new** — accumulator, steps, pickers, `FactionWizard` |
| Wizard | `desktop/src-tauri/src/wizards/entity_link.rs` | `load_linkable_npcs`, `load_linkable_gods` |
| Wizard | `desktop/src-tauri/src/wizards/mod.rs` | register + `pub mod faction` |
| Cmd | `desktop/src-tauri/src/commands/create_commands.rs` | bare `create faction` → wizard; one-shot draft fields |
| Cmd | `command-specs/src/lib.rs` | `faction` examples; `create faction` example |
| Publish | `desktop/src-tauri/src/services/publish.rs` | `render_faction_markdown_with_links` + `write_blank_section` |
| State | `desktop/src-tauri/src/app_state.rs` | test fixture `faction_draft` only |
| Suggest | `desktop/src-tauri/src/services/suggestions.rs` | tests only (schema-driven) |
| FE | `desktop/src/App.tsx` | verify only (no edit expected) |

---

*Companion to `docs/expanded-factions-design.md`. Mirrors the v0.6.0 location wizard;
follows `docs/architecture.md` §8C/§8D and `docs/feature-development.md` Playbooks C/F.*
