# Review: `create dungeon` / `start setup` Architectural Alignment

> **Purpose:** A point-in-time review (not a guideline doc) of how the `create dungeon`
> flow drifts from the established entity-creation pattern (`create faction`) and from the
> rules in `docs/architecture.md`, `docs/cli.md`, `docs/command-contexts.md`, `docs/render.md`,
> and `docs/feature-development.md`. Written on the `feature/dungeons` branch before merge so the
> drift can be paid down deliberately rather than shipped.

> **✅ Resolved (2026-06-17).** Findings 1–5 were paid down by the wizard-framework refactor
> (`docs/create-wizard-refactor.md`, Option B): `create dungeon`'s intake half now runs on the generic
> wizard engine (`desktop/src-tauri/src/wizards/`). The bespoke `commands/dungeon_flow.rs`,
> `DungeonCreationFlow`, and the `main.rs` interceptor are deleted. The framework itself is documented in
> `docs/architecture.md` §4 (Wizard Framework) + §8D, `docs/command-contexts.md` §1/§4, `docs/cli.md`
> §4/§5, `docs/feature-development.md` §7, and `docs/render.md` §3. Per-finding status is noted inline below.

---

## TL;DR

`create dungeon` was modeled on the bespoke `start setup` onboarding wizard — a raw-line
state machine intercepted *before* registry dispatch. That choice is the root cause of the two
regressions noticed in the field:

1. **Step prompts aren't clickable.** Actionable verbs (`generate`, `continue`, `reroll`,
   `set room`, `cancel`, menu numbers) are emitted as literal back-tick text inside paragraphs,
   never as `InlineNode::CommandRef`. They render as non-clickable code spans.
2. **No typeahead/autocomplete inside the flow.** The wizard has no `InputContext` and is gated
   by a parallel `DungeonCreationFlow.active` boolean the suggestion service never consults, so
   autocomplete resolves to `Default` and tries to complete step answers against the normal manifest.

Both are *direct, predictable consequences* of copying the wizard route. The codebase already has
a fully-aligned pattern for this exact job (`create faction` → draft → `EntityEditor` context), and
**the dungeon feature already uses it for the post-draft editor half** — it just bypasses it for the
intake half.

**The misalignment is bounded.** `create dungeon` has two halves:

| Half | Files | Verdict |
|---|---|---|
| **Intake wizard** (steps 1–7: premise → tone → twist → context → topology → plan review → story review) | `commands/dungeon_flow.rs` (719 lines), `DungeonCreationFlow` state, `main.rs` interceptor | **Misaligned** — the subject of this review |
| **Editor** (the `dungeon` draft opened after the wizard finalizes) | `commands/dungeon_commands.rs`, `entities/domains/dungeon_domain.rs`, manifest spec, suggestion tests | **Aligned** — a first-class entity like npc/faction/item |

So the remediation target is narrow: realign the intake half; the editor half is already correct.

---

## 1. The reference pattern — what "aligned" looks like (`create faction`)

`create faction` is a single, registry-dispatched command that does *not* intercept input:

- `create` is one registered handler (`commands/mod.rs:77,222-228`); `router.rs:27-37` dispatches it
  generically with no special-casing.
- `create_faction` (`commands/create_commands.rs:216-282`) is one-shot: one LLM seed call →
  build `FactionDraftSession` → `editor.set_faction(draft)` → return summary + card event.
- `set_faction` → `set_active_draft` sets `active_kind = Some(EntityKind::Faction)`
  (`app_state.rs:238-242,324-326`), which activates the **`EntityEditor(Faction)`** input context.
- The card footer emits **explicit `command_ref` nodes** (`runebound-models/src/drafts.rs:531-537`):

  ```rust
  .with_block(paragraph_with_inlines(vec![
      text_node("Use "),
      command_ref("save", "save"),
      text_node(" to persist this faction, or "),
      command_ref("reroll", "reroll"),
      text_node(" to regenerate it."),
  ]))
  ```

- Autocomplete + context-aware help come **for free**: the suggestion service resolves
  `active_kind()` → `EntityEditor("faction")` (`services/suggestions.rs:60-82`) and filters by
  `command_availability(name).is_visible_in(context)` (`suggestions.rs:84-99`,
  `command-specs/src/lib.rs:172-187`). No per-command wiring.

That is the contract every other entity (`npc`, `location`, `item`, `event`, `god`) follows. Refinement
happens through real, manifest-backed commands inside the editor, not a pre-draft Q&A.

---

## 2. How the dungeon intake wizard diverges

`create dungeon` deliberately steps off the pattern. The entry arm says so
(`commands/create_commands.rs:58-62`):

```rust
if lowered == "create dungeon" || lowered.starts_with("create dungeon ") {
    // Dungeon breaks from the single-command pattern: it starts a guided,
    // multi-step flow (steps A–E) rather than generating in one shot.
    return crate::commands::dungeon_flow::start_dungeon_flow(invocation.state.inner()).await;
}
```

…and the flow module's own doc comment states the lineage (`commands/dungeon_flow.rs:1-4`):

> *"The guided `create dungeon` flow (steps A–E). Modeled on the bespoke setup wizard
> (`core::command::try_execute_onboarding`): a small step counter with typed answer fields,
> intercepted before registry dispatch while active."*

### Finding 1 — Step prompts emit back-tick text, not `command_ref` (the clickability regression) — ✅ Resolved
*Fixed by construction: step prompts are built with `wizards/prompt.rs` (`wizard_menu`/`action_row`), which render every `WizardChoice` as a `command_ref`.*

Every step prompt builds its `OutputDoc` from `heading` / `paragraph_text` / `image` /
`list(text_node)` only — never `command_ref`. The actionable verbs are literal back-ticks inside
a paragraph string:

- Step 1 premise (`dungeon_flow.rs:110-116`): `"…or type \`generate\` to have the oracle invent one."`
- Plan review (`dungeon_flow.rs:643-664`): `"Type \`continue\` to write the story, \`reroll\` for a
  new roll, \`set room <room> <type>\` to pin one…"`
- Story review (`dungeon_flow.rs:574-592`): `"Type \`continue\` to build the cards, \`reroll [hint]\`…"`
- Menus (`dungeon_flow.rs:688-696`): options like `"1: Tragedy"` are plain paragraph text.

These render as non-clickable code spans (`renderer.tsx:117-126` only makes `InlineNode::CommandRef`
clickable; the plain-text linkifier in `App.tsx` only recognizes real command roots, and `2` /
`continue` / `reroll` / a free-text premise are not roots).

**Rules violated:**
- `docs/cli.md §5`: *"Actionable command text should be clickable. Preferred path: backend emits
  `InlineNode::CommandRef`. For any new command, ensure at least one explicit `command_ref` path
  exists in guidance output."*
- `docs/render.md §3` (CommandRef Rule) and `docs/render.md §8` / `docs/architecture.md §9`
  anti-patterns: *"Depending on markdown heuristics for command links → Emit explicit `command_ref` nodes."*
- `docs/feature-development.md §8`: *"Use `command_ref` for actionable text."*

> Note: the dungeon **card** (post-finalize) *does* use `command_ref` correctly
> (`drafts.rs:710-718`), which is why the editor half feels right and only the wizard feels broken.

### Finding 2 — The wizard has no `InputContext`; autocomplete is blind to it (the typeahead regression) — ✅ Resolved
*Fixed: `InputContext::Wizard(id)` is resolved by `AppState::resolve_input_context()`, and the active step's tokens are surfaced via `active_step_choices()` / `wizard_step_suggestions()`.*

`InputContext` has exactly three variants — `Default`, `ConfigEditor`, `EntityEditor(String)`
(`command-specs/src/lib.rs:112-118`). **None represents "the dungeon wizard is active."** The wizard
is gated by a separate boolean `DungeonCreationFlow.active`, which neither the suggestion service nor
the help index ever consults (`grep dungeon_flow` in `services/suggestions.rs` → 0 matches). While the
wizard runs, no draft is open and onboarding is inactive, so context resolves to `Default`
(`suggestions.rs:60-82`) and the suggester tries to complete `"2"` / `"continue"` / premise prose
against the normal command manifest. There is no typeahead for the menu options or flow verbs.

**Rules violated:**
- `docs/command-contexts.md §1-2` and `docs/cli.md §4`: visibility is driven by `InputContext` +
  `command_availability` — *"the single source of truth shared with the help index."* The wizard sits
  entirely outside this model.
- This is *strictly worse than the wizard it copied*: `start setup` at least gets the `ConfigEditor`
  context and surfaces `continue` in suggestions (`suggestions.rs:181-191`). The dungeon flow gets none.

### Finding 3 — Flow verbs live outside the manifest (no help, no completion, no clickability fallback) — ✅ Resolved
*Fixed: `continue`/`back` are manifest commands (`AnyWizard`), `cancel` is `AnyEditorOrWizard`; per-step tokens come from the active step's `choices()`.*

The wizard verbs (`generate`, `skip`, `1`/`2`/`3`, topology `0`-`9`, `continue`, `accept`,
`reroll [hint]`, `set room <room> <type>`, `redo`) are raw-string matches inside `dungeon_flow.rs`
(`handle_step_f_plan:293-304`, `handle_step_g_story:447-474`, `set_room_type:310-337`). None appears in
`command-specs/src/lib.rs`, has a `command_availability` arm, or a `canonical_help_command`.

Consequences: no phrase help (`continue help` is meaningless), no autocomplete, no clickability-fallback
resolution. In particular `set room <room> <type>` re-implements field parsing and validation by hand
(`SETTABLE_ROOM_TYPES`, `resolve_room_index`) instead of routing through the schema + domain `set_field`
machinery the editor half already uses.

**Rule violated:** `docs/architecture.md §5` / `docs/cli.md §2`: *"all command metadata lives in
`command-specs/src/lib.rs`"* (single source of truth for names, subcommands, availability, help).

### Finding 4 — A third, undocumented dispatch route inlined into `main.rs` — ✅ Resolved
*Fixed: replaced by the single generic `try_execute_active_wizard` route (`wizards/runtime.rs`), documented in `docs/command-contexts.md` §4 (route 4). One route serves all wizards.*

`docs/architecture.md §2` and `docs/command-contexts.md §4` enumerate exactly **two** intentional
divergences from registry dispatch (desktop-override; onboarding interception) and the latter doc
literally titles the section *"Dispatch routes (there are three, not one)."* The dungeon flow adds a
**third interceptor** — gated on `dungeon_flow.active`, inlined into `run_command`
(`main.rs:96-114`), comment: *"bypass registry dispatch (exactly how onboarding does)"* — and neither
doc was updated to reflect it.

It also pushes command logic (the active-check, the flow call, and history-push) into `main.rs`,
brushing against `docs/cli.md §7` / `docs/feature-development.md §9`: *"Do not add command business
logic to `main.rs` or `router.rs`."* (Onboarding's equivalent logic lives in core's
`execute_line`/`try_execute_onboarding`, `core/src/command.rs:261,333` — `main.rs` only forwards to it.)

### Finding 5 — Fragile cross-language string coupling for the spinner — ✅ Resolved
*Fixed: the spinner reads the structured `CommandResponse.wizard: WizardView { step_id, awaiting_llm_label }`; `detectDungeonFlowScreen` and the marker-string matching are deleted.*

The frontend infers which wizard screen it's on by substring-matching the *rendered prompt text*
(`App.tsx:1076-1084` → drives `commandSpinnerLabel` `:1096-1110`). The matched literals duplicate the
Rust marker constants (`dungeon_flow.rs:25-31`) by hand and are already inconsistent:
`PLAN_REVIEW_MARKER` is `"Create Dungeon — Step 6 of 6 — Room Plan"` but the frontend matches only the
suffix `"Step 6 of 6 — Room Plan"`. Renumbering a step silently breaks the spinner with no
compile-time error, and the em-dash must byte-match across both languages.

A spinner *does* exist (so `docs/feature-development.md §8` "LLM commands must show a spinner" is
nominally met), but it's wired through exactly the *"markdown/heuristic dependence"* the rendering docs
warn against (`docs/render.md §8`) instead of keying off a structured signal or a real command name.

---

## 3. Where `start setup` fits in

`start setup` is the *blessed* precedent the dungeon flow copied — but it's blessed precisely because it
is a **rare, one-time config bootstrap**, and the cost of its wizard route is **explicitly documented**
(`docs/architecture.md §2`, `docs/command-contexts.md §4-5`, `docs/config.md`). Even so, it shares the
clickability gap: its menu prompts are plain `CommandOutput::text` with no `output_doc`/`command_ref`
(`core/src/command.rs:872-937`); only the final `save` step emits a single `command_ref`
(`command.rs:846`).

The lesson is not "`start setup` is wrong" — it's that the wizard route is a **special-case tool for
config bootstrap**, and `create dungeon` generalized it onto the **repeatable entity-creation surface**,
importing all of the wizard's UX debts into a place where the codebase already has a better, fully-aligned
pattern. Hardening `start setup`'s menus to emit `command_ref` is a reasonable but **lower-priority**
follow-up (it's documented and rarely run); the dungeon flow is the urgent case because it's a core,
repeated creation path and it regressed against a sibling command users hit constantly.

---

## 4. Remediation options

### Option A — Realign to draft-first (recommended)

Make `create dungeon` behave like `create faction`: roll defaults (generate premise, default/rolled
tone·twist·topology, roll the content plan), generate the story + structure in one shot, and open the
**`EntityEditor(Dungeon)`** draft immediately. The GM then refines through real, manifest-backed editor
commands that already exist or are cheap to add:

- `dungeon set tone|twist|topology …`, `dungeon set room <room> <type>` (via schema + `set_field`)
- `dungeon reroll story [hint]`, `dungeon reroll plan`, `dungeon reroll <beat> [hint]`
- `save` / `cancel` / `dungeon show`

**Wins:** clickable `command_ref` cards, autocomplete, and context-aware help all come for free;
deletes `dungeon_flow.rs` (~719 lines), the `DungeonCreationFlow` state, the `main.rs` interceptor, and
the spinner text-sniffing; collapses the feature onto one well-trodden path; restores the third dispatch
route to two. **Cost:** the "ask one question at a time" intake UX becomes "generate, then refine" — the
same trade every other entity already makes.

### Option B — Make the wizard a first-class context (only if staged Q&A is a hard requirement)

Add an `InputContext` for the wizard (e.g. `DungeonCreation`, or a general `Flow(kind)`), register the
verbs in the manifest with a `command_availability` arm, emit `command_ref` in every step prompt, teach
the suggestion service to surface step options when the flow is active, and replace the spinner
text-match with a structured signal on `CommandResponse`. **Cost:** this is substantial net-new framework
— effectively rebuilding the context/availability/suggestion machinery for one feature — to make a
bespoke route behave like the registry it's bypassing.

### Option C — Stopgap patch (not an endpoint)

Minimally fix the two visible regressions without restructuring: (1) emit `command_ref` nodes in the
step prompts, (2) add a `dungeon_flow.active` branch to the suggestion service so menu verbs complete.
This entrenches the bespoke route and *grows* the divergence — acceptable only as a short-lived bridge,
not a destination.

**Recommendation:** Option A. It removes the most code, eliminates all five findings at once, and is the
only option that *reduces* rather than *adds* architectural surface. Use Option B only if product requires
the staged intake; never stop at Option C.

---

## 5. Checklist mapping (against the docs' own gates)

The third column is the original (pre-refactor) verdict; the fourth is after the wizard-framework refactor.

| Doc checklist item | `create faction` | `create dungeon` (intake, before) | `create dungeon` (intake, now) |
|---|---|---|---|
| Output uses `output_doc` + explicit `command_ref` for actions (`cli.md §8`, `feature-development.md §10`) | ✅ | ❌ back-tick text only | ✅ via `wizards/prompt.rs` |
| Help + autocomplete verified in every context (`architecture.md §11`) | ✅ via `EntityEditor` | ❌ no context exists | ✅ via `InputContext::Wizard` |
| Manifest updated for all command-surface changes (`cli.md §8`) | ✅ | ❌ verbs absent from manifest | ✅ `continue`/`back`/`cancel` |
| No command business logic in `main.rs`/`router.rs` (`cli.md §7`) | ✅ | ⚠️ interceptor in `main.rs` | ✅ one generic route in `wizards/runtime.rs` |
| Dispatch routes documented (`command-contexts.md §4`) | ✅ | ❌ third route undocumented | ✅ route 4 documented |
| Clickability via `command_ref`, not heuristics (`render.md §8`) | ✅ | ❌ + spinner text-sniffing | ✅ + structured `WizardView` spinner |

---

*Reviewed: 2026-06-16 · branch `feature/dungeons` · scope: intake-wizard half of `create dungeon`; the
post-draft dungeon editor is already aligned and out of scope for remediation.*
