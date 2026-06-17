# Refactor Plan: A First-Class Wizard Context

> **Purpose:** A multi-phase plan to turn the bespoke multi-step "flow" pattern (today: `start setup`
> onboarding and `create dungeon`) into a first-class, registerable **wizard framework** — so that
> spinning up a new multi-step wizard is as idiomatic and low-drift as spinning up a one-shot `create`
> command is today. Then migrate `create dungeon` onto it.
>
> **Read first:** `docs/review-dungeon-alignment.md` (why the current dungeon flow drifted). This plan is
> the remediation, generalized. **Goal state:** the codebase can bootstrap *both* one-shot creates and
> multi-step wizards from data + a small trait impl, with autocomplete, clickability, help, and dispatch
> handled by the framework, not hand-rolled per feature.

---

## 1. Guiding principle — wizards should mirror entities

One-shot `create` commands are already data-driven and bootstrappable: you define a `EntityKind`, a
schema, implement the `EntityDomain` trait, register it once, and autocomplete/help/clickability/context
all come for free from `command_availability` + the suggestion service. A wizard should work the same way.

| One-shot create (today, idiomatic) | Multi-step wizard (target) |
|---|---|
| `EntityKind` enum variant | `WizardId` (stable string id, e.g. `"dungeon"`) |
| `EntitySchema` + `EntityFieldSpec` (`entities/schema.rs`) | `WizardDef` = ordered `WizardStep`s (declarative) |
| `EntityDomain` trait (`entities/domain.rs`) | `Wizard` trait (`wizards/wizard.rs`) |
| `EntityDomainRegistry` + `build_default_registry()` (`entities/registry.rs`) | `WizardRegistry` + `build_default_wizard_registry()` (`wizards/registry.rs`) |
| `EditorSession` (`active_kind` + `HashMap<EntityKind, DraftEnvelope>`) | `WizardSession` (`active_id` + cursor + substate + type-erased state) |
| `InputContext::EntityEditor(kind)` | `InputContext::Wizard(id)` |
| `CommandAvailability::EntityScoped` / `EntityEditorOnly` / `AnyEditor` | `CommandAvailability::AnyWizard` (+ `WizardScoped` if ever needed) |
| Card footer emits `command_ref` (`drafts.rs`) | Step prompt emits `command_ref` **by construction** |
| Generic verb handlers `handle_save/handle_cancel` (`system_commands.rs`) | Generic verb handlers `continue/back/cancel` |
| Field-arg suggestions from schema (`suggestions.rs:546-592`) | Step-token suggestions from the active step |

The win is the same win entities already have: **adding a wizard is additive data + one trait impl; the
plumbing never changes.** That is the literal definition of "bootstrappable" the user is asking for.

---

## 2. Target architecture (the engine)

All new code lives under a new `desktop/src-tauri/src/wizards/` module, mirroring `entities/`. (Onboarding
is the one exception — it lives in core; see §6 for why it does not move in this plan.)

### 2.1 Shared contracts (`command-specs/src/lib.rs` + `runebound-models`)

- **`InputContext::Wizard(String)`** — add the variant next to `EntityEditor(String)`
  (`command-specs/src/lib.rs:112-118`). Tag = the active wizard id. This is the keystone: it makes the
  wizard a real context the autocomplete + help index already understand. The generic availability filter
  (`suggestions.rs:84-99`) and help-index filter (`core/src/command.rs:181-184`) need **zero** changes
  once the variant and arms exist — they're already generic over `InputContext`.
- **`CommandAvailability::AnyWizard`** — add the arm to the enum and to `is_visible_in`
  (`command-specs/src/lib.rs:125-165`): `matches!(context, InputContext::Wizard(_))`. Used by the shared
  navigation verbs. (Defer `WizardScoped(&'static str)` until a wizard actually needs a verb scoped to
  only itself — YAGNI; the symmetry is noted but not built.)
- **Navigation verb specs** — add `CommandSpec`s for `continue` and `back` in `command_manifest()`, with
  `command_availability` arms `"continue" | "back" => AnyWizard` (and `ConfigEditor` too if/when onboarding
  joins — see §6). Extend `cancel`'s arm so it's visible in a wizard (today `"save" | "cancel" => AnyEditor`;
  make cancel `AnyEditor | AnyWizard` via a small helper or a dedicated arm). `continue` is currently *not*
  a manifest command at all — it's faked by the bespoke onboarding branch (`suggestions.rs:181-205`);
  promoting it to a real, availability-gated command is part of the cleanup.
- **Structured wizard signal** — add an optional `wizard: Option<WizardView>` to `CommandResponse`
  (`runebound-models/src/events.rs:46-55`), where `WizardView { id, step_id, awaiting_llm_label: Option<String> }`.
  `awaiting_llm_label` is the spinner label to show when the user submits from this screen
  (e.g. `"generating story"`). This **replaces the fragile text-sniffing** in
  `App.tsx:detectDungeonFlowScreen` (the marker-string coupling flagged as Finding 5 in the review).
  Regenerate TS via `cargo build -p runebound-models`.

### 2.2 The `Wizard` trait + declarative steps (`desktop/src-tauri/src/wizards/`)

```text
wizards/
|- mod.rs        # re-exports + build_default_wizard_registry()
|- wizard.rs     # Wizard trait, WizardStep, WizardTransition, WizardOutcome
|- session.rs    # WizardSession (active id, cursor, substate, type-erased state bag)
|- registry.rs   # WizardRegistry (HashMap<WizardId, Arc<dyn Wizard>>) — mirrors entities/registry.rs
|- runtime.rs    # try_execute_active_wizard(): the single generic interceptor
|- prompt.rs     # wizard_menu()/wizard_prompt() helpers that emit command_ref BY CONSTRUCTION
`- dungeons/     # the dungeon wizard impl (steps), ported from commands/dungeon_flow.rs
```

Recommended trait shape (declarative steps; the engine owns dispatch, the author owns step content):

```rust
#[async_trait]
pub trait Wizard: Send + Sync {
    fn id(&self) -> &'static str;                 // "dungeon"
    fn title(&self) -> &'static str;              // "Create Dungeon"
    fn steps(&self) -> &[Arc<dyn WizardStep>];    // ordered; the engine walks these
    /// Called on the terminal step's Complete: build the draft / write config and hand off.
    async fn finalize(&self, state: &AppState, data: &WizardData) -> CommandResult;
}

#[async_trait]
pub trait WizardStep: Send + Sync {
    fn id(&self) -> &'static str;                 // "tone", "topology", "plan_review"
    /// Build the prompt. MUST use the prompt.rs helpers so every choice is a command_ref.
    fn prompt(&self, data: &WizardData) -> OutputDoc;
    /// Choices that should autocomplete AND be clickable (label + the literal token submitted).
    fn choices(&self, data: &WizardData) -> Vec<WizardChoice>;     // [{label:"1: Tragedy", token:"1"}, ...]
    /// Spinner label if submitting from this step triggers an LLM call (None = instant).
    fn awaiting_llm_label(&self) -> Option<&'static str>;
    /// Validate + apply input, decide where to go next. May call services (async/LLM).
    async fn accept(&self, input: &str, data: &mut WizardData, state: &AppState) -> WizardTransition;
}

pub enum WizardTransition {
    Stay,                       // reprompt (invalid input)
    Goto(&'static str),         // jump to a step by id (supports the plan/story review loops)
    Next,                       // advance to the next step in order
    Back,                       // step backward to the prior step, restoring its answer for editing
    Complete,                   // run Wizard::finalize
    Cancel,                     // reset + exit
    // Native(NativeAction)     // (future) desktop side-effect hook for onboarding's folder picker
}
```

`back` is a first-class engine verb (decision §8.3): `WizardSession` keeps a cursor **history stack** so the
engine can pop to the previous step and re-render its prompt with the previously-entered answer in
`WizardData` intact (the user edits rather than re-derives). The `back` command is a real manifest entry with
`AnyWizard` availability, so it autocompletes and is clickable like any other nav verb.

`WizardData` is the type-erased accumulator (a per-wizard struct behind `Box<dyn Any>` or an enum, mirroring
how `DraftEnvelope` type-erases drafts). For dungeon it carries exactly the fields `DungeonCreationFlow`
holds today (`app_state.rs:30-46`).

### 2.3 `WizardSession` on `AppState`

Replace the bespoke `dungeon_flow: Mutex<DungeonCreationFlow>` field (`app_state.rs:508`) with a generic
`wizard_session: Mutex<WizardSession>` and add `wizards: Arc<WizardRegistry>` + `fn wizards()`, exactly
mirroring `domains: Arc<EntityDomainRegistry>` + `fn domains()` (`app_state.rs:506,563-565`). The session
holds: `active_id: Option<&'static str>`, `cursor: usize` (or current step id), per-step `substate`
(the menu-shown vs awaiting-free-text distinction onboarding needs — see §6), and the `WizardData` bag.

### 2.4 Single generic interceptor (dispatch route consolidation)

`try_execute_active_wizard(line, state)` in `wizards/runtime.rs`, returning `Result<Option<CommandResponse>>`
(the same handled/not-handled/failed contract as `try_execute_onboarding` and `try_execute_dungeon_flow`).
It: reads `wizard_session.active_id`; if none, `Ok(None)` (fall through); else handles global verbs
(`cancel`, `back`), then delegates to the active step's `accept()`, applies the `WizardTransition`, and
renders the resulting step's prompt (or `finalize`). In `main.rs run_command`, the current **dungeon-flow
block** (`main.rs:99-114`) is replaced by a single call to this interceptor. Net effect: the dungeon's
bespoke third dispatch route collapses into one generic, documented wizard route that scales to N wizards
without new interceptors. (Onboarding's core interception remains its own route for now; §6.)

### 2.5 Clickability by construction (`wizards/prompt.rs`)

The root cause of the clickability regression is that step prompts were hand-built with `paragraph_text`
containing back-tick literals. The fix is to make it *impossible* to author a non-clickable choice: the
only sanctioned way to render a menu/prompt is via helpers that take `WizardChoice`s and emit
`command_ref` nodes:

```rust
pub fn wizard_menu(title: &str, intro: &str, choices: &[WizardChoice]) -> OutputDoc { /* command_ref per choice */ }
pub fn wizard_prompt(title: &str, body: Vec<InlineNode>, actions: &[WizardChoice]) -> OutputDoc { /* ditto */ }
```

A menu choice `{label:"1: Tragedy", token:"1"}` renders as `command_ref("1: Tragedy", "1")` — clicking
submits `1`, which the interceptor handles. `continue` / `reroll` / `cancel` / `set room …` in review
screens become real clickable refs. Because the frontend already makes `InlineNode::CommandRef` clickable
(`renderer.tsx:117-126`), **no frontend change is needed for clickability** — it falls out of emitting the
right nodes. This closes Findings 1 of the review.

### 2.6 Autocomplete integration (`services/suggestions.rs`)

Two plug-in points, both mirroring existing entity logic:
1. **Context resolution** (`suggestions.rs:60-82` *and* the duplicate in `system_commands.rs:32-45`): add a
   branch that produces `InputContext::Wizard(id)` when `wizard_session.active_id` is set. **Extract this
   shared match into one `resolve_input_context(state) -> InputContext` helper** so the two copies can't
   drift (the docs already warn they must stay in sync). Decide precedence: an active wizard should rank
   like `ConfigEditor` does today (entity editor wins only if a draft is somehow open, which it isn't
   mid-wizard).
2. **Step-token suggestions**: add a stage (alongside the entity field-arg block at `suggestions.rs:497-507`)
   that, when a wizard is active, offers the current step's `choices()` tokens/labels. This generalizes the
   bespoke onboarding `continue` branch (`suggestions.rs:181-205`), which should then be deleted in favor of
   the generic path. The generic availability filter (`suggestions.rs:84-99`) already gates `continue`/`back`/`cancel`
   correctly once their `AnyWizard` arms exist. Add suggestion tests (`cargo test suggestions`) per the docs.

This closes Finding 2 (no typeahead) and Finding 3 (verbs outside the manifest) of the review.

---

## 3. Division of labor — the "bootstrap surface"

Once the engine exists, adding a wizard touches only the right-hand column:

| The engine owns (write once) | A wizard author writes (per wizard) |
|---|---|
| Interception + dispatch route (`runtime.rs`) | `Wizard` impl: `id/title/steps/finalize` |
| Navigation verbs (`continue`/`back`/`cancel`) | Each `WizardStep`: `prompt/choices/accept` |
| `command_ref` rendering of choices (`prompt.rs`) | The `WizardData` accumulator struct |
| `InputContext::Wizard` + availability filtering | Registering the wizard in `build_default_wizard_registry()` |
| Autocomplete context + step-token suggestions | Launch call from its entry command (`create dungeon`) |
| Structured spinner signal | Per-step `awaiting_llm_label()` (one line if LLM-backed) |
| `cancel`/reset semantics | `finalize()` hand-off (open a draft / write config) |

This is intentionally the same shape as the "Add a New Entity Type" playbook (`docs/architecture.md §8C`):
mostly additive, no plumbing edits.

---

## 4. Phased plan

Each phase is independently shippable, compiles, and keeps existing behavior until the phase that flips it.

### Phase 0 — Lock decisions & acceptance criteria (no code)
- Resolve the open decisions in §8.
- Freeze the **acceptance criteria** (these are the review's five findings, inverted):
  1. Every wizard step prompt's actionable tokens are `command_ref` (clickable).
  2. Autocomplete works inside a wizard via `InputContext::Wizard`.
  3. Wizard nav verbs are real manifest commands with availability arms; per-step tokens are suggested
     from the active step.
  4. Exactly one generic wizard dispatch route; documented in `command-contexts.md`.
  5. Spinner is driven by the structured `WizardView` signal, not prompt-text matching.

### Phase 1 — Shared-contract plumbing (safe, no behavior change)
- Add `InputContext::Wizard(String)`, `CommandAvailability::AnyWizard` + `is_visible_in` arm, `continue`/`back`
  `CommandSpec`s + availability arms, and extend `cancel` to be wizard-visible (`command-specs/src/lib.rs`).
- Update the manifest regression sentinels (`default_surface_commands_are_an_explicit_known_set`
  ~`lib.rs:1351`, scoping tests ~`lib.rs:1269`) so the new roots are accounted for.
- Add `wizard: Option<WizardView>` to `CommandResponse` (`runebound-models/src/events.rs`); regenerate TS.
- Extract `resolve_input_context(state)` and call it from both `suggestions.rs` and `system_commands.rs`.
- **Acceptance:** `make build` green; existing flows unchanged; `cargo test` (specs + suggestions) green.

### Phase 2 — Wizard engine + dungeon wizard (the bulk)
- Create `desktop/src-tauri/src/wizards/` (`wizard.rs`, `session.rs`, `registry.rs`, `runtime.rs`, `prompt.rs`).
- Add `wizards: Arc<WizardRegistry>` + `wizard_session: Mutex<WizardSession>` to `AppState`; build the
  registry in `main.rs` (mirror the `domains` wiring at `main.rs:183,201`).
- Implement the **dungeon wizard** under `wizards/dungeons/` by porting `commands/dungeon_flow.rs`
  step-for-step (mapping in §5). Reuse the existing services unchanged
  (`AiGenerationService::generate_dungeon_story` / `structure_dungeon_story`, `roll_dungeon_content_plan`).
- Point `create dungeon` (`create_commands.rs:58-62`) at `start_wizard("dungeon", state)` instead of
  `dungeon_flow::start_dungeon_flow`.
- Wire `try_execute_active_wizard` into `main.rs` **alongside** the old dungeon block, but register dungeon
  on the new engine — i.e. the new path is live, the old `dungeon_flow.rs` is now dead code (removed in Phase 4).
- **Acceptance:** `create dungeon` runs entirely on the engine; manually verify the full flow + `cancel` at
  every step; `finalize` opens the same `EntityEditor(Dungeon)` draft as before (the aligned editor half is
  untouched).

### Phase 3 — Frontend + autocomplete
- Replace `App.tsx:detectDungeonFlowScreen` / marker-string matching with reading `response.wizard` (the
  structured signal); drive `commandSpinnerLabel` from `wizard.awaiting_llm_label`.
- Add the wizard branch to `resolve_input_context` consumers and the step-token suggestion stage in
  `suggestions.rs`; delete the bespoke onboarding `continue` branch *only if* onboarding is migrated
  (else leave it; see §6). Add suggestion tests.
- **Acceptance:** typeahead shows step choices/`continue`/`cancel` inside the wizard; menu options and review
  verbs are clickable; spinner shows correctly with no text matching; `cargo test suggestions` green.

### Phase 4 — Delete the bespoke path + write the guidelines
- Delete `commands/dungeon_flow.rs`, `DungeonCreationFlow` (`app_state.rs:30-46`), the dungeon block in
  `main.rs`, the `STEP_E_MARKER`/`PLAN_REVIEW_MARKER`/`STORY_REVIEW_MARKER` constants, and the dead frontend
  detection code.
- **Docs (the deliverable the user wants):**
  - `docs/architecture.md` — add a "Wizard Framework" section (module layout, the entity↔wizard parallel)
    and an "Add a New Wizard" playbook (§8 sibling to "Add a New Entity Type").
  - `docs/command-contexts.md` — document `InputContext::Wizard`, the generic wizard dispatch route (update
    "there are three routes" → describe the wizard route as a first-class generalization, not a special case),
    and wizard availability.
  - `docs/cli.md` + `docs/feature-development.md` — add wizard verbs to the surfaces list and a "Add a New
    Wizard" feature playbook; note the clickability-by-construction rule.
  - `docs/render.md` — note `wizard_menu`/`wizard_prompt` as the sanctioned prompt builders.
  - Mark the relevant items resolved in `docs/review-dungeon-alignment.md`.
- **Acceptance:** no references to `dungeon_flow` remain; `make build` + full test suite green; docs updated
  in the same PR (per the docs' own "update in the same PR" rule).

### Phase 5 — Onboarding: full port (separate spike, same sprint)
- Onboarding is **not** retrofitted in place — it gets a **full port** onto this engine, tracked as its own
  spike in **`docs/onboarding-wizard-port.md`**. That spike promotes the engine to a shared `wizard` crate
  (so core/CLI can host it too) and retires core's bespoke `try_execute_onboarding`.
- It runs **after** Phase 4 here (the engine is proven on dungeon first), in the **same sprint**.
- **Forward-compat nudge for this refactor:** keep the engine's `session.rs`/`prompt.rs`/transition types
  free of `AppState` coupling (only `accept()`/`finalize()` touch host state). That makes the crate
  promotion in the onboarding spike mostly mechanical (`&AppState` → a generic host context).

---

## 5. Dungeon flow → declarative steps (concrete migration map)

The current `dungeon_flow.rs` step machine maps 1:1 onto `WizardStep`s. The accumulator `WizardData` ==
today's `DungeonCreationFlow` fields.

| Today (`dungeon_flow.rs`) | Step id | `choices()` (clickable) | `awaiting_llm_label` | Transition |
|---|---|---|---|---|
| Step A `handle_step_a` | `premise` | `generate` | — | `Next` (store premise/None) |
| Step B `handle_step_b` | `tone` | `1: Tragedy`, `2: Comedy` | — | `Next` |
| Step C `handle_step_c` | `twist` | `1`/`2`/`3` | — | `Next` |
| Step D `handle_step_d` | `context` | `skip` | — | `Next` |
| Step E `handle_step_e` | `topology` | `0`–`9` (+ the `image("topology",…)` block) | — | `Goto("plan_review")` after rolling the plan |
| Step F `handle_step_f_plan` | `plan_review` | `continue`, `reroll`, `set room <room> <type>`, `cancel` | `generating story` (on `continue`) | `continue`→`Goto("story_review")`; `reroll`→re-roll, `Stay`; `set`→`Stay` |
| Step G `handle_step_g_story` | `story_review` | `continue`, `reroll [hint]`, `cancel` | `generating dungeon` (continue) / `generating story` (reroll) | `continue`→`Complete`; `reroll`→`Stay` |
| `finalize_dungeon` | — (Wizard::finalize) | — | — | build `DungeonDraftSession`, `editor.set_dungeon`, emit `LoadDungeonDraftWithCard` |

Key continuity points:
- `finalize` does exactly what `finalize_dungeon` does today (`dungeon_flow.rs:478-563`): structure the story
  (Pass 2), stamp plan meta onto beats, open the dungeon draft. **The aligned editor half is reused verbatim** —
  the dungeon `EntityDomain`, manifest spec, suggestion coverage, and `command_ref` card all stay.
- `set room <room> <type>` keeps its validation but lives in the `plan_review` step's `accept()`; consider
  routing through the dungeon schema's settable types rather than the local `SETTABLE_ROOM_TYPES` const to
  remove that duplication (optional cleanup).
- The plan→story→finalize review loop is expressible with `Goto`/`Stay`, so no expressiveness is lost vs the
  current `step: u8` counter.

---

## 6. Onboarding: full port in a follow-up spike

Onboarding is the *other* bespoke wizard, and the end-state goal is **one wizard engine** in the codebase —
so onboarding gets fully ported onto this engine, not left bespoke and not merely retrofitted. That work is
its own spike, **`docs/onboarding-wizard-port.md`**, run **after** this refactor in the **same sprint**
(decision §8.1).

It is a separate spike because it requires strictly more than dungeon does, and that "more" is a project of
its own:

- **It lives in core (`core/src/command.rs` / `core/src/session.rs`), not desktop.** This engine mirrors
  `EntityDomain`, which is desktop, and the dungeon wizard is desktop-only. Onboarding must also work in the
  core/CLI path, so hosting it requires **promoting the engine into a shared `wizard` crate** parameterized
  over a host context (the parallel of the existing `command-handler` "generic dispatch primitives" crate).
- **It needs features the dungeon wizard never exercises:** per-step substates
  (`VaultStepState`/`OllamaStepState`, menu-vs-free-text), sub-flows (`Full`/`Vault`/`Llm`/`Model` scopes
  that save-and-exit), a **desktop-native side-effect hook** (the folder-picker rewrite in `main.rs:66-94`),
  config seeding from `load_effective`, async server probes, and the `ConfigEditor`-context collapse. The
  trait in §2.2 is deliberately *designed to accommodate* these as extension points (substate-as-steps,
  `WizardTransition::Native`, a `seed()` hook), but building and proving them belongs to the onboarding
  spike, not the tail of this one.

This refactor's job is to **build and prove the engine on dungeon**; the onboarding spike's job is to
**generalize the proven engine and retire `try_execute_onboarding`**. Keeping them as two spikes means this
PR stays reviewable and the engine ships value (dungeon realignment) before the larger core surgery begins.

---

## 7. Risks & rollback

- **Scope creep into a generic engine nobody needs yet.** Mitigation: build *only* what the dungeon wizard
  requires; list onboarding's extra needs as designed-for extension points, implemented only on demand.
- **Behavior regressions in the dungeon flow.** Mitigation: port step-for-step (§5 map), keep the same
  services and `finalize`, and the old path stays compiled until Phase 4 — Phases 2–3 can be A/B'd.
- **Context-resolution drift** (the two copies). Mitigation: the `resolve_input_context` extraction in
  Phase 1 removes the duplication permanently.
- **TS/Rust contract drift** for the new `WizardView`. Mitigation: model-first in `runebound-models`,
  regenerate TS, no hand-rolled TS interface (per `docs/architecture.md §7`).

---

## 8. Decisions (resolved)

1. **Onboarding scope** — ✅ **Full port**, as a separate spike run after this refactor in the same sprint.
   Tracked in `docs/onboarding-wizard-port.md` (§6).
2. **Step model** — ✅ **Declarative `WizardStep` objects** (§2.2), not a trait-with-internal-counter. The
   declarative form is what makes a wizard "data you register," matching the entity-schema feel — and it's
   what lets onboarding reuse step *values* across its sub-flows in the port spike.
3. **`back` navigation** — ✅ **In the engine from the start.** Add `WizardTransition` support for stepping
   backward (`Goto(previous)`), a `back` nav verb (manifest + `AnyWizard` availability), and cursor history
   on `WizardSession` so `back` restores the prior step's accumulated answer for editing.
4. **Wizard id vs entity tag** (minor, not blocking) — the dungeon *wizard* and dungeon *editor* are never
   active simultaneously (the wizard finalizes into the editor). Reuse id `"dungeon"`;
   `InputContext::Wizard("dungeon")` and `InputContext::EntityEditor("dungeon")` are distinct variants, so
   there is no real collision.

---

## 9. Acceptance checklist (closes the review's findings)

- [ ] Wizard step prompts emit `command_ref` for every action (Review Finding 1) — via `prompt.rs`, enforced by construction
- [ ] Autocomplete works inside a wizard via `InputContext::Wizard` (Finding 2)
- [ ] Nav verbs (`continue`/`back`/`cancel`) are manifest commands with availability arms; step tokens suggested from the active step (Finding 3)
- [ ] One generic wizard dispatch route, documented in `command-contexts.md` (Finding 4)
- [ ] Spinner driven by structured `WizardView`, no prompt-text matching (Finding 5)
- [ ] `commands/dungeon_flow.rs` + `DungeonCreationFlow` + markers deleted
- [ ] "Add a New Wizard" playbook written in `docs/architecture.md` + `docs/feature-development.md`
- [ ] `cargo test` (command-specs + suggestions) and `make build` green
- [ ] Full `create dungeon` flow + `cancel` manually verified; `finalize` opens the unchanged dungeon editor

---

*Drafted: 2026-06-16 · branch `feature/dungeons` · companion to `docs/review-dungeon-alignment.md`.*
