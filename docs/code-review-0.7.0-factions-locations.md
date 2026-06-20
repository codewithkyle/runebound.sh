# Code Review — Faction Refactor & Location Expansion (v0.7.0)

**Reviewer lens:** senior engineer / architect, focused on hackiness, idiom, and extensibility.
**Date:** 2026-06-20 · **Branch:** `wip/0.7.0`
**Primary focus (per request):** the "wizard" code as used for locations and factions.

**Files read in full for this review**

| File | LOC | Role |
|---|---|---|
| `wizard/src/{lib,wizard,runtime,session,registry,prompt}.rs` | ~1,000 | The wizard framework crate |
| `desktop/src-tauri/src/wizards/faction.rs` | 1,609 | Faction wizard (focus) |
| `desktop/src-tauri/src/wizards/location.rs` | 1,923 | Location wizard (focus) |
| `desktop/src-tauri/src/wizards/{dungeon,entity_link,mod}.rs` | ~1,400 | Reference wizard + shared link helpers + registration |
| `services/ai_generation.rs`, `entity_reroll.rs`, `entities/domains/{faction,location}_domain.rs`, `runebound-models/src/utils.rs` | ~6,200 | Supporting generation/domain layer (surveyed) |

---

## 1. Verdict

This is **good code**, and unusually disciplined for LLM-glue work. The wizard *framework* (the `wizard` crate) is genuinely well-architected: host-agnostic generics, a clean transition vocabulary, clickability-by-construction, and real tests for its hardest path (the native-action round-trip). The faction and location wizards are readable, heavily commented with *why*, and well unit-tested at the helper level. The recent "defensive sanitization" commit is a textbook centralized cleanup (`normalize_name`/`strip_code_formatting` in one place, threaded through the existing normalizer funnel).

So this review is **not** a list of things that are broken. It is a list of places where the *two new wizards* drifted from the framework's own idioms, where positional coupling and stringly-typed routing create silent-breakage risk, and where "duplicate per spec" was chosen over the shared home that already exists. These are exactly the seams that bite when you add the *next* wizard — which is the stated value ("clear, idiomatic, extendable").

**Severity tally:** 3 High (structural/extensibility), 3 Medium, 3 Low, plus supporting notes on `ai_generation.rs`.

The three High findings share one root cause: **the flow graph and its data are coupled by position and by bare strings, and almost none of that coupling is enforced by a test or the type system.** Fix that theme and the wizard layer becomes genuinely safe to extend.

---

## 1a. Alignment with the documented standards (`docs/`)

I re-read the governing docs — `architecture.md` §4 (Wizard Framework), `feature-development.md` §7 (Playbook F: Add a New Wizard), and `expanded-factions-{design,spec}.md` (the D-decisions + the §5.3 build plan) — and checked every finding against them. Net effect: most findings are **validated or strengthened**, two touch **deliberate, documented decisions** (reframed, not "smells"), and **one finding (H2) was overstated and is corrected.**

**Process conformance is good.** The feature satisfies the §7 Playbook F mechanics: additive registration (one `register` line + `pub mod`, per §5.4), prompts built only via the sanctioned `wizard::prompt` helpers, `awaiting_llm_label()` set on the generating step for the spinner (the §8 wizard exception), and per-step routing unit tests as the §5.3 checkpoint requires. None of the §9 anti-patterns are present (no `router.rs`/`main.rs` logic, no direct DB access from steps).

**Strengthened by the docs**

- **M1 (duplicated helpers) — strengthened.** Spec §5.3 (lines 384-386) says the `numbered_choices`/`pick_value` helpers "**should be lifted to a shared module or duplicated**" — and lists *shared module first*. So the in-code comment "Duplicated … per spec §5.3" (`faction.rs:1212`) picks the spec's *second* option and cites the spec as if it mandated it. The spec's entire framing (§5.1-5.3, design §9) is "mirror/reuse the location wizard." Lifting these into `wizard::prompt` **is the spec's own preferred path** — M1 isn't second-guessing the spec, it's enforcing it.
- **H1 (label/value drift) — consistent with §1.4.** The spec says the canonical vocab is "**defined once**" (§1.4), split between `utils.rs` (persisted) and `ai_generation.rs` (wizard-prompt vocab). The menu *label* arrays (`POWER_BASE_LABELS`, …) are a **second, parallel representation the implementation introduced**, which the docs don't cover and which can drift from the "defined once" values. H1 is squarely in the spirit of the spec's own single-source rule; the docs are simply silent on labels-vs-values, which is the gap.
- **H3 (back / state-bleed) — a gap in an explicitly-deferred area.** Spec §5.4 says "**No other plumbing edits** (… nav verbs … work unchanged)," and the §5.3 checkpoint only *manually walks* `back`/`cancel`. The spec **assumed** the engine's `back` "just works" and never analyzed its interaction with the **new** state this feature added — the D4 repeatable allies/rivals picker and the `awaiting_custom_*` flags. That unexamined interaction is exactly H3: the risk lives precisely where the spec waved it through.

**Deliberate, documented decisions (reframed below, not deviations)**

- **The multiplexed faction picker is prescribed, not accidental.** Spec §5.3 "Shared pickers" (lines 387-394) says: "*one step each, parameterized by `link_return` … a faction picker (liege / patron / allies / rivals), an NPC picker (leader), a god picker.*" The implementation matches this exactly. So **M2's design observation stands but the 4-mode picker is intended** — my narrower, still-valid point is that the *mechanism* for the repeatable allies→rivals flow (mutate `link_return` + `Stay` in place) is an implementation detail the spec left open, and it's where the H3 back-stack risk concentrates.
- **Stringly-typed routing is the documented engine convention.** `link_return: Option<&'static str>` is named in §5.3 (line 359) and `Goto(step_id)` routing is the framework's documented model (architecture §4; spec Appendix A). So **M3 is not a deviation or a smell** — it's the inherited, blessed pattern. Downgraded below to optional hardening.
- **Faction's category router is decision D5.** Faction using a category→kind router where location uses a single flat kind step is **D5**, with a stated rationale ("the three categories have genuinely different question sets"). Intentional.

**Corrected — I overstated one finding**

- **H2 (`Next` vs `Goto`) — the "contradicts the documented convention" claim was wrong.** Spec **Appendix A** explicitly says "*Transitions use the engine's `Goto`/`Next`/`Complete`*," and architecture §4 designates `Goto` only for **branch routing** (which the location wizard *does* do). Using `Next` for the *linear* moves inside a branch is therefore **doc-sanctioned**, not a deviation. What survives is the narrower, undocumented risk: intra-branch `Next` couples correctness to the physical order of the interleaved `steps` Vec, and nothing tests it. H2 drops from "contradicts the docs" to "a real but undocumented fragility — add the flow-graph/Next-chain test." Corrected in §3 below.

---

## 2. What is strong (keep doing this)

- **`wizard` crate is the right abstraction.** Generic over host `H`, with `accept()`/`finalize()`/`seed()` as the *only* host-coupled surface (`wizard/src/wizard.rs:10-12`). The engine is its own crate precisely because everything else is host-agnostic — that is the correct boundary and it pays for itself with the onboarding port.
- **Clickability by construction** (`wizard/src/prompt.rs:1-5`): a `WizardChoice` can only render as a `command_ref`, so a non-clickable choice is unrepresentable. This is the right way to kill a regression class — make it impossible, not policed.
- **Two-state `WizardSession` enum** (`session.rs:42-49`) so "a wizard is running" and "its accumulator exists" are one fact, not a cross-field invariant guarded by `expect`. The `std::mem::take` in `complete()` (`runtime.rs:273`) is a clean ownership move.
- **`run_transition` as a loop, not recursion** (`runtime.rs:181-263`), so a `Native` resubmit drives the next transition without re-locking the session. Subtle and correct.
- **Generation layer is better-factored than typical.** `run_seed_attempts` (`ai_generation.rs:184`) writes the 5-attempt retry/dedup loop *once* for all 9 generators; `entity_reroll.rs` collapses per-field reroll into `(kind, field)` tables. The duplication that remains is mostly acknowledged in-code ("Mirrors `generate_location_seed_for_wizard`").
- **Tests target the pure helpers** (`pick_value`, `resolve_anchor`, `is_structured`, brand/kind coverage). The instinct to extract pure, testable functions out of the async step bodies is correct and consistent.

---

## 3. High-severity findings

### H1 — Menu labels and their generation values are coupled by array position, across file *and crate* boundaries, unchecked

The pervasive pattern in both wizards is two parallel `const` arrays: a `*_LABELS` array shown in the menu, and a `*_VALUES`/types array used to map the picked index to the stored value. `numbered_choices(&LABELS)` builds the menu; `pick_value(input, &VALUES)` resolves it. Correctness depends entirely on the two arrays staying the same length **and in the same order**.

In the faction wizard this is worse than parallel-arrays-in-one-file, because the **labels live in `faction.rs` while the values live in another crate/module**:

| Menu labels (`wizards/faction.rs`) | Mapped values | Defined in |
|---|---|---|
| `POWER_BASE_LABELS` (`:250`) | `LORD_TYPES` | `ai_generation.rs:1529` |
| `CONTROL_LABELS` (`:518`) | `CONTROL_TYPES` | `ai_generation.rs:1540` |
| `MANDATE_LABELS` (`:647`) | `MANDATES` | `ai_generation.rs:1544` |
| `REACH_LABELS` (`:732`) | `REACH` | `ai_generation.rs:1554` |
| `LOYALTY_LABELS` (`:412`) | `LOYALTY_TYPES` | `runebound-models/src/utils.rs:107` |

`PowerBaseStep::accept` (`faction.rs:290`) shows `POWER_BASE_LABELS` to the user but stores `LORD_TYPES[n-1]`. Today they align (`LORD_TYPES[0] == "chokepoint"`, `POWER_BASE_LABELS[0]` starts with "chokepoint"). But **nothing asserts that alignment.** Reorder `LORD_TYPES` in `ai_generation.rs` — a file a different person edits for a different reason — and the faction menu silently mislabels every power base, with no compile error and no failing test. The existing `pick_value_maps_one_based_index` test (`faction.rs:1492`) checks `LORD_TYPES` *indexing*, not that `POWER_BASE_LABELS` is parallel to it.

Location has the same shape (`CONTROL_LABELS`/`CONTROL_VALUES` `:309-323`, `SITE_DRAW_LABELS`/`VALUES` `:531-548`, `BASE_OWNER_LABELS`/`VALUES` `:674-685`, `DANGER_LABELS`/`VALUES` `:1377-1378`), but at least keeps both arrays adjacent in the same file and *does* guard a couple of them (`guildhall_role_offers_archetypes_then_skip` asserts equal length; `holy_site_draw_index_matches_its_label_and_value` pins the one index that routing depends on). That coverage is partial and inconsistent — `CONTROL`, `SITE_FOCUS`, and `BASE_OWNER` have no alignment guard at all.

**Why it matters:** silent data corruption on an unrelated edit is the worst kind of fragility — it ships. This is the single most likely way a future change breaks faction generation.

**Recommendations (pick one, apply uniformly):**
1. **Collapse each pair into one source of truth.** A `const X: [(&str, &str); N]` of `(label, value)` tuples, with `numbered_choices`/`pick_value` operating on `.0`/`.1`. Drift becomes impossible because there is one array.
2. If the value arrays must stay in `ai_generation.rs` (shared with the one-shot path), **add an alignment test per pair** asserting `LABELS.len() == VALUES.len()` *and* a representative mapping (as `holy_site_draw_index_*` already does for one case). Cheap, and it converts a silent prod failure into a red CI run.

I'd push for option 1 for the in-file pairs and option 2 for the cross-crate ones.

---

### H2 — Intra-branch flow correctness is coupled to the physical order of the `steps` `Vec`, and nothing tests it

> **Doc check (corrects my first draft):** `Next` is *not* a deviation. Spec Appendix A says "*Transitions use the engine's `Goto`/`Next`/`Complete`*," and architecture §4 designates `Goto` only for **branch routing** — which the location wizard does do. So using `Next` for linear intra-branch moves is doc-sanctioned. The finding below is the narrower, **undocumented** risk: the *untested coupling to declaration order*, not the choice of `Next`.

`docs/architecture.md:169` designates the location wizard as the reference for branching flows, routing "via `WizardTransition::Goto`." The location wizard does route *across* branches with `Goto`, but *within* a branch it uses `Next` — and `Next` is literally `cursor += 1` (`runtime.rs:200`). So each branch only works because its steps are declared **contiguously and in order** inside an 18-element `Vec` where the five branches are interleaved (`location.rs:1257-1304`):

```
control → resources → export_mode → geography_settlement   (indices 1→2→3→4, all Next)
site_focus → site_danger → site_draw → geography_site       (5→6→7→8, all Next)
base_owner → base_protection → base_danger → base_purpose → geography_hideout (9→…→13, all Next)
```

`SiteDrawStep::accept` returns `Next` (`location.rs:613`) trusting that index 8 (`geography_site`) physically follows index 7. Insert a new site question, or reorder the `Vec`, and `Next` lands on the wrong step — **no compile error, no failing test.** The faction wizard, by contrast, is fully explicit: every transition is a `Goto("step_id")` (e.g. `faction.rs:246, 294, 408`), so its flow is order-independent and survives `Vec` reordering.

So the two sibling wizards use **different** routing disciplines (faction all-`Goto`, location `Goto`-between/`Next`-within). Both are individually doc-legal; the issue is only that location's `Next`-chains have an unchecked dependency on `Vec` order.

**Why it matters:** "extendable" means a new contributor can add a step without re-deriving a hidden ordering invariant. Today, adding a settlement question safely requires knowing that the settlement steps must remain a contiguous run ending in a `Complete` terminal — an invariant written nowhere and checked nowhere.

**Recommendations:**
- **Either** add a per-branch test asserting each `Next`-chain lands on the intended step id, **or** convert intra-branch `Next` to explicit `Goto(next_id)` (matching the faction wizard) so the flow is order-independent. The test is the cheaper of the two and is sufficient; the conversion is the more thorough fix. `Next` is fine for a genuinely linear wizard (dungeon).
- Add a **flow-graph test** (one per wizard, or a shared helper): collect every `Goto`/routing-key target referenced in the file and assert each resolves to an existing `step.id()`. The engine already errors on an unknown `Goto` target at *runtime* (`runtime.rs:207`); a test turns "panics the first time a GM walks that path" into "fails in CI." This also catches typo'd step-id strings (see H3/M3).

---

### H3 — `back` does not roll back accumulator mutations, despite docs saying it "restores accumulated answer" — and the new mode-flag/repeatable-picker steps depend on state that therefore bleeds

`WizardTransition::Back` is documented as "Step backward to the previous step (restoring its accumulated answer)" (`wizard.rs:62`; echoed in `session.rs`). The implementation only moves the cursor — it never touches `active.data` (`runtime.rs:141-149` and `217-225`). The accumulator is monotonically mutated for the whole run; there is no per-step snapshot. So **`back` restores nothing**; the documented behavior and the actual behavior disagree.

For simple scalar steps this is harmless (going back and re-answering overwrites the field). It stops being harmless for the *new* state the faction/location wizards introduced:

- **Boolean sub-state flags.** `awaiting_custom_brand` (`faction.rs:62`) and `awaiting_custom_kind` (`location.rs:65`) put a step into a second "now type your custom value" screen. If the GM enters that sub-state and hits `back`, the cursor moves but the flag stays `true` (nothing resets it). Re-entering the step later renders the custom-entry screen unexpectedly.
- **Multiplexed picker mode.** `link_return` (`faction.rs:94`) / `faction_link_return` (`location.rs:109`) decide which of 3–4 modes a shared picker is in. They are set on entry and never cleared; `back` doesn't restore the prior value.
- **Repeatable accumulating lists.** The allies/rivals picker pushes onto `data.allies`/`data.rivals` and *stays* (`faction.rs:985-1007`). Walking `back` into it re-renders "Linked so far: …" with the prior entries still present, because the list was never rolled back.

The most fragile case is the **allies → rivals "flip in place"** (`faction.rs:988-995`): finishing allies sets `link_return = Some("rivals")` and returns `Stay` (no history push), so the entire allies+rivals interaction is *one cursor position* whose identity mutates. `back` from the rivals phase pops history to `npc_pick` (not back to allies), while `link_return` remains `"rivals"` — a confusing, untested state.

**Why it matters:** this is the seam where the framework's "accumulator is shared and mutable" model meets the new steps' "I keep transient sub-state in the accumulator" approach. The mismatch is currently masked because nobody has written a test that drives `back` through a mode-flag or repeatable step.

**Recommendations:**
- **Fix the doc first** (cheap, correct now): `Back` restores the cursor, not accumulated answers.
- For the mode/flag bleed, the targeted fix is to **reset transient sub-state on (re)entry** — e.g. have `enter_faction_pick`/`enter_*` and the kind/brand steps clear their flags when entered — so a step's transient state is always derived from a fresh entry, never inherited. This is less invasive than per-step snapshots.
- If you want `back` to truly undo (matching the current doc), the framework change is to snapshot `WizardData` onto the history stack alongside the cursor and restore it on `Back`. Bigger change; only worth it if reversible back is a product goal. Otherwise, prefer the doc fix + reset-on-entry.
- Add at least one **`back`-through-a-repeatable-step test** to lock whichever semantics you choose.

---

## 4. Medium-severity findings

### M1 — Pure, host-agnostic helpers are copy-pasted between the two wizards, with a "duplicated per spec" comment, when a shared home already exists

`numbered_choices` and `pick_value` are byte-for-byte identical in `faction.rs:1213-1230` and `location.rs:1359-1375`, and the faction copies carry the comment *"(Duplicated from the location wizard per spec §5.3.)"*. Several more are duplicated or near-duplicated: `skip_choice`, `optional_text`, `optional_text_prompt` (faction) vs the inline equivalents in location; `trimmed_opt` (location) vs `optional_text` (faction) do the same trim-empty-to-`None` job.

These are **pure and host-agnostic** — exactly what `wizard/src/prompt.rs` is for (it already hosts `wizard_menu`, `choice_lines`, `filter_choices`). And there is already a precedent for lifting shared wizard logic into one place: `wizards/entity_link.rs` was created by extracting the location pickers' matching/loading so faction could reuse them ("First grown by the location wizard… and lifted here so other wizards reuse the exact same behavior").

So the project has both the *mechanism* (a shared crate-level prompt module and a shared `entity_link` module) and the *established habit* (entity_link) for de-duplicating — yet chose to duplicate the simplest, most obviously-shareable functions, citing a spec. That is the clearest "hacky relative to the codebase's own standard" item in the review.

**Why it matters:** every future wizard will now copy `numbered_choices`/`pick_value` a third and fourth time, and a fix to one (e.g. supporting `1.` as well as `1`) won't propagate. It also undercuts the credibility of `entity_link.rs` as "the shared home."

**The spec actually agrees.** §5.3 (lines 384-386) says these helpers "**should be lifted to a shared module or duplicated**" — listing *shared module first*. So the comment "Duplicated … per spec §5.3" (`faction.rs:1212`) doesn't reflect a spec mandate; it took the spec's secondary option and cited the spec as cover. This makes M1 a straightforward "do what the spec preferred."

**Recommendation:** lift `numbered_choices`, `pick_value`, `skip_choice`, `optional_text`/`trimmed_opt`, and `optional_text_prompt` into `wizard::prompt` (or a `wizards/common.rs` if you want to keep host-coupled helpers out of the crate). Delete the duplicates, and drop the misleading "per spec §5.3" comment.

### M2 — The two sibling wizards diverge in *when* they multiplex a step vs split it into structs

> **Doc check:** the multiplexed faction picker is **prescribed**, not accidental — spec §5.3 "Shared pickers" (lines 387-394) says "*one step each, parameterized by `link_return` … a faction picker (liege / patron / allies / rivals)*." So this finding is not "the code took a liberty"; it's "the design is intentional, but worth making a discoverable convention and worth de-risking the one sharp edge in it." Reframed accordingly.

Both wizards collapse the faction picker into one multi-mode step (`FactionPickStep` serves liege/patron/allies/rivals, `faction.rs:873`; `FactionLinkStep` serves control/base_owner/guildhall, `location.rs:1110`). But they make opposite calls elsewhere:

- Location **splits** god (`SiteGodStep`), location-anchor (`GuildhallAnchorStep`), and faction-link into separate structs, and **data-drives** its terminal step (one configurable `GenerateStep { id, title, body, field }` reused 4× — `location.rs:1039, 1263-1299`). This is clean.
- Faction **multiplexes more aggressively** — the single `FactionPickStep` also absorbs the repeatable allies/rivals flow with the in-place mode flip (H3) — and uses a one-off unit-struct `GenerateStep`.

Neither is wrong, but a reader moving between the two files has to re-learn the convention each time, and the most complex hand-written state machine (the allies/rivals flip) lives in the more-multiplexed of the two. The `prompt`/`choices`/`suggest`/`accept` of `FactionPickStep` each branch on `faction_pick_mode` — four behaviors fused into one struct, which is where the H3 state-bleed concentrates.

**Recommendation:** pick one convention and document it in §8D of the architecture doc as "the multiplexed-step pattern": *when* to multiplex (a genuinely shared picker over the same entity set) vs *when* to split (distinct entity types or distinct terminal behavior). I'd specifically **not** multiplex a repeatable accumulator (allies/rivals) into a picker that also serves single-value modes — split the repeatable list picker into its own struct so its `Stay`-loop and "Linked so far" state aren't entangled with liege/patron's single-shot logic.

### M3 — Routing is stringly-typed: step ids double as `Goto` targets *and* as routing keys, as bare literals scattered across files

> **Doc check (downgraded):** this is the **documented engine convention**, not a smell the feature introduced — `link_return: Option<&'static str>` is named in spec §5.3 (line 359) and `Goto(step_id)` is the framework's routing model (architecture §4). So treat M3 as *optional hardening*, not a correctness defect. The step-id constants below are a nice-to-have; the flow-graph test from H2 is the part that actually buys safety.

Step ids are `&'static str` literals, and the same literals are reused as routing-mode keys stored in the accumulator and compared elsewhere:

- `faction_link_return == Some("guildhall")` (`location.rs:1107`), `return_step == "base_owner"` (`location.rs:1195`), `faction_pick_mode` returns `"patron"`/`"rivals"`/… (`faction.rs:875-877`).
- `Goto("ambition")`, `Goto("loyalty_type")`, `Goto("geography_site")`, etc. — the target is a free-typed string matched against `step.id()` at runtime only.

A typo or a renamed step id breaks a path that compiles fine and only fails when a GM walks it. There is no single registry of a wizard's step ids.

**Recommendation:** introduce per-wizard step-id constants (`const STEP_AMBITION: &str = "ambition";`) or a small `enum` of step ids with a `&'static str` mapping, and use them for both `id()` and `Goto`. Even without that, the **flow-graph test from H2** mitigates most of the risk. This is the connective tissue behind H1/H2/H3 — bare strings as both identity and routing.

---

## 5. Low-severity / polish

- **L1 — Boolean sub-state-machine inside a step.** `awaiting_custom_brand`/`awaiting_custom_kind` (see H3) turn one step into two screens via a flag. It works and is contained, but it's a recurring shape that suggests a missing framework primitive: a "free-text follow-up" step or a tiny two-state helper. Worth considering if a third wizard needs the same "pick *or* type your own" affordance. Until then, at minimum tie the flag's lifetime to step entry (H3 reset-on-entry) so it can't bleed.
- **L2 — Re-downcast / borrow dances.** A few `accept` bodies bind `let data = faction_data_mut(d)` then call `faction_data_mut(d)` again inside a branch to dodge the borrow checker (e.g. `faction.rs:975-982` patron branch). Harmless, but reads awkwardly; resolving the owned `String` first, then taking one `_mut` borrow, is cleaner.
- **L3 — Duplicated wall-clock RNG.** `random_loyalty()` (`faction.rs:1296`) re-implements the dungeon wizard's `plan_seed` wall-clock-nanos trick. Both acknowledge it in comments. A single `wizard`-crate `weak_random_index(len)` helper would remove the third copy when it appears.
- **L4 — `build_seed_prompt` always returns `Some`.** `faction.rs:1354` wraps an always-present value in `Option` to mirror location's signature (where it can be `None`). Fine for symmetry, but the faction version could return `String` and let the caller wrap, or the comment could note the deliberate symmetry.

---

## 6. Supporting layer (`ai_generation.rs` & domains) — context, not focus

These are already in reasonable shape (the agent survey confirmed the shared `run_seed_attempts`/`run_field_reroll` loops do the heavy de-duplication). Flagging only what intersects faction/location maintainability:

- **Acknowledged mirror-twin generators.** `generate_faction_seed_for_wizard` (`:691`) and `generate_location_seed_for_wizard` (`:459`) share ~90 lines of config/recent-seed/token/schema scaffolding, differing only in branch enum + locked-field overwrite. The in-code "Mirrors …" comments show this is deliberate, but it's the natural next extraction if a *third* structured entity wizard arrives — a `run_wizard_seed(inputs, branch_cfg)` shell would absorb it.
- **Repeated magic prompt fragments.** The repair note (`" Previous response was invalid or repeated. …"`) and the `@reference` grounding sentence are copy-pasted, lightly reworded, across faction/location/god/npc/item generators. Hoist to `const`s so prose tweaks stay consistent.
- **Stringly-typed entity keys.** `"faction_seed"`, `"location_seed"`, etc. appear at both the `recent_prompts(...)` call and the `entity_key` argument with no shared constant — same anti-pattern as M3, one layer down.
- **Two genuinely oversized functions** dominated by inline `format!` prompt literals: `generate_dungeon_story` (~171 lines) and `structure_dungeon_story` (~145). Outside this review's faction/location focus, but worth extracting the prompt bodies to module-level `const`/builders for readability.
- **Sanitization commit (`11871d6`) is clean** — `normalize_name`/`strip_code_formatting` centralized in `runebound-models/src/utils.rs:432-485`, wired through the existing normalizer funnel and the 7 prior `seed.name.trim()` call sites, with unit tests. No action.

---

## 7. Test gaps (specific, cheap to close)

1. **Label/value alignment** for every `*_LABELS`/`*_VALUES` pair in both wizards (H1) — length + a representative index. Only `guildhall_role` and `holy_site_draw_index` are covered today.
2. **Flow-graph integrity** per wizard (H2/M3): every `Goto`/routing-key target resolves to a real `step.id()`.
3. **`back` semantics** through a mode-flag step and the repeatable allies/rivals picker (H3) — lock whichever behavior you decide on.
4. **Cross-crate value-array coupling**: a test in `faction.rs` asserting `POWER_BASE_LABELS`/`CONTROL_LABELS`/`MANDATE_LABELS`/`REACH_LABELS`/`LOYALTY_LABELS` stay parallel to `LORD_TYPES`/`CONTROL_TYPES`/`MANDATES`/`REACH`/`LOYALTY_TYPES`.

There is **no integration test that walks an actual step graph** for either wizard; all current tests exercise pure helpers. Given five branches and multiplexed pickers, one happy-path walk per branch would be high-value.

---

## 8. Suggested sequencing

| Priority | Item | Effort | Payoff |
|---|---|---|---|
| 1 | H1 alignment tests (or tuple-pair the in-file arrays) — enforces §1.4 "defined once" | S | Stops the most likely silent prod break |
| 2 | H2/H3 flow-graph + `Next`-chain + `back`-through-repeatable-step tests | M | Closes the untested coupling the spec waved through (§5.4) |
| 3 | H3 doc fix + reset-transient-flags-on-entry | S–M | Removes back/forward state bleed |
| 4 | M1 lift `numbered_choices`/`pick_value`/etc. to `wizard::prompt` — the spec's preferred §5.3 path | S | Restores the codebase's own DRY standard |
| 5 | *(optional)* M3 step-id constants; M2 split the repeatable allies/rivals picker | S–M | Hardens a documented pattern; consistency for the next wizard |

Items 1–3 are the ones I'd gate the 0.7.0 release on; they're the difference between "works on the paths we tested" and "safe to extend," and each lives in an area the spec either assumed-works (§5.4 back) or single-sources elsewhere (§1.4 vocab). Item 4 is the spec's own preferred option. Item 5 is optional hardening of patterns the docs explicitly bless.

---

## 9. Bottom line

The faction refactor and location expansion are well-executed on a genuinely good framework, they follow the §7 Playbook F mechanics, and the *intent* is documented better than most codebases manage. Checked against `docs/`, the feature is **conformant** — the patterns I flagged hardest (the multiplexed picker, stringly-typed routing, `Next`) are all explicitly blessed by the spec/architecture, and one of my draft findings (H2's "contradicts the docs") was an overstatement now corrected. What's left is real but narrower: the new flows lean on **positional array coupling and an untested dependency on `Vec` order**, plus a `back`/state-bleed interaction the spec assumed-away (§5.4), none of it enforced by the type system or a test. Add the handful of tests, lift the copy-pasted helpers into the shared home the spec itself preferred (§5.3), and the wizard layer stays "clear, idiomatic, and extendable" for the *third* wizard, not just these two.
