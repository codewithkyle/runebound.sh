# v0.5.0 Cleanup Plan

> **Purpose:** A phased punch-list to get the codebase into tip-top shape before tagging v0.5.0.
> Findings come from a full pre-release review (all crates + Tauri backend + React frontend).
> Work the phases **top to bottom, one at a time** — each phase is independently shippable and ends
> with a verification step. Check items off as they land.
>
> This document is the working tracker. When a phase is fully done and verified, mark its heading
> `DONE`. When the whole plan is complete, fold the resolved items into `docs/architecture.md` §10
> (Known Friction Points) and archive this file.

*Created: 2026-06-17. No external users yet, so we can take the time to do this right.*

---

## How to read the findings

Each finding has an ID (`P<phase>.<n>`), a severity, and a **verification tag**:

- ✅ **Verified** — confirmed by direct source read during the review. High confidence.
- 🔶 **Reported** — surfaced by a review pass, plausible and consistent with the code, but not line-verified end-to-end. **Confirm at fix time** before changing behavior.
- ❓ **Depends on external fact** — correctness hinges on something outside the repo (e.g. donjon's output format). Verify the assumption first.

Severity: **Blocker** (fix before going live) · **High** · **Medium** · **Low**.

---

## Baseline (as of 2026-06-17)

| Check | State |
|---|---|
| Workspace tests (`cargo test --workspace`) | ✅ green |
| Desktop crate tests (`cargo test --manifest-path desktop/src-tauri/Cargo.toml`) | ✅ 114 passed |
| Workspace clippy | ⚠️ 7 warnings (`dnd-core`, incl. `identity_op`) |
| Desktop clippy (`cargo clippy --manifest-path desktop/src-tauri/Cargo.toml`) | ⚠️ **59 warnings** |
| `make build` | uses `cargo check`, **does not run clippy** — warnings never gate |
| Version | `0.4.0` in `Cargo.toml`, `desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, `package.json` |

> Note: the desktop crate is `exclude`d from the workspace, so `--workspace` commands skip it.
> Always run the two test/clippy commands as a pair.

---

## What's already good (do not "fix")

Credit where due — these are the patterns the rest of the cleanup should aspire to:

- The **`wizard` crate** engine + `desktop/.../wizards/dungeon.rs` reference impl: declarative, host-agnostic, spinner driven by structured `WizardView.awaiting_llm_label` (not heading-string matching).
- The **`EntityDomain` / `EntityDomainRegistry` / `EntitySchema`** design and its contract tests.
- **Manifest as single source of truth** with the `default_surface_commands_are_an_explicit_known_set` sentinel test that converts the "missing availability arm" regression into a CI failure.
- **Repository boundary respected** — no `core::db` calls leak into handlers (verified).
- `publish.rs` `EntityLinker` correctly documents *why* it uses `to_ascii_lowercase` (byte-length-preserving) — the exact discipline `vault_ref.rs` is missing (see P1.1).
- The docs themselves are thorough and mostly track the code.

The core architecture is sound. This plan is about **finishing the additive refactor that was already started** and **closing the string/heuristic coupling seams** — not redesigning.

---

## Findings index

| ID | Severity | Verify | Area | Phase |
|---|---|---|---|---|
| P1.1 | Blocker | ✅ | `vault_ref` Unicode slice panic | 1 |
| P1.2 | Blocker | ✅ | Calendar weekday ignores year | 1 |
| P1.3 | Blocker | ✅ | Soft-delete writes undo record last | 1 |
| P1.4 | High | ✅/❓ | `lunar_shf` u32 vs i32 mismatch | 1 |
| P1.5 | Medium | ✅ | Moon phase floor-bucketing | 1 |
| P1.6 | Medium | 🔶 | `vault.rs` stale required dirs / missing `.trash` | 1 |
| P1.7 | Low | 🔶 | `ai_generation` unchecked `plan.anchors[i]` | 1 |
| P1.8 | Low | ✅ | `ordinal_suffix` wrong for day ≥ 100 | 1 |
| P2.1 | High | ✅ | Clippy not clean (66 warnings) + not gated | 2 |
| P2.2 | High | ✅ | App.tsx dead write-only draft state (~115 lines) | 2 |
| P2.3 | Low | ✅ | Doubled `#[cfg(test)]` | 2 |
| P2.4 | Low | 🔶 | Scattered `#[allow(dead_code)]` audit | 2 |
| P2.5 | Low | 🔶 | Magic LLM sampling constants ×8 | 2 |
| P3.1 | High | ✅ | Plain-text entity responses skip `OutputDoc`; frontend regex-parses commands | 3 |
| P3.2 | Medium | ✅ | `output_doc_from_error_text` string-sniffs error | 3 |
| P3.3 | High | ✅ | `suggestions.rs` hard-coded arg byte-offsets | 3 |
| P3.4 | Medium | ✅ | Frontend branches on rendered English | 3 |
| P3.5 | Medium | ✅ | `commandSpinnerLabel` re-encodes command taxonomy | 3 |
| P4.1 | High | ✅ | `build.rs` hand-transcribes Rust→TS, no drift guard | 4 |
| P4.2 | High | ✅ | `slug` required in TS but `#[serde(default)]` in Rust | 4 |
| P4.3 | Medium | 🔶 | `WIKILINK_UNSAFE` const duplicated | 4 |
| P4.4 | Low | 🔶 | Mention grounding uses substring, not word-boundary | 4 |
| P5.1 | High | ✅ | `EntityType` duplicates `EntityKind` | 5 |
| P5.2 | High | ✅ | Services re-enumerate 7 kinds (admin/reroll/persistence) | 5 |
| P5.3 | High | ✅ | 6 near-identical entity command modules | 5 |
| P5.4 | Medium | ✅ | `entity_commands` triple-encodes fields (load/card-doc/card-text) | 5 |
| P5.5 | High | ✅ | `db.rs` per-entity CRUD copy-paste | 5 |
| P5.6 | Medium | ✅ | `db.rs` `LIKE` wildcards unescaped | 5 |
| P5.7 | Medium | ✅ | `db.rs` nondeterministic name tie-break | 5 |
| P5.8 | Low | ✅ | `entity_store.ensure_dirs` repetition | 5 |
| P5.9 | Low | 🔶 | `create`/`system` seed→draft duplication + `clear_kind` cascades | 5 |
| P6.1 | High | ✅ | No transaction boundary (vault+db+index) | 6 |
| P6.2 | Medium | 🔶 | Blocking sync IO on async paths (incl. per-keystroke) | 6 |
| P6.3 | Medium | 🔶 | `vault_sync` is store→db only, not vault↔db | 6 |
| P6.4 | High | 🔶 | `ollama_chat` ignores truncation / 200-with-error | 6 |
| P6.5 | Low | 🔶 | `link_prose` re-allocates per (position × name) | 6 |
| P6.6 | Low | 🔶 | `resources_assets` parsed by heuristic (model-first) | 6 |
| P7.1 | Medium | ✅ | `config.rs` `Partial*` mirror + `apply_partial` | 7 |
| P7.2 | Medium | ✅ | Dual help renderers (string + doc) | 7 |
| P7.3 | Low | ✅ | `config_paths(workspace_root)` param is vestigial | 7 |
| P7.4 | Medium | 🔶 | Wizard `session.data` invariant via scattered `.expect()` | 7 |
| P7.5 | Low | 🔶 | `command-handler` `into_iter`/`Default` ergonomics | 7 |
| P7.6 | Low | ✅ | Core `execute_line` doesn't expand inline `+1d` | 7 |
| P8.x | — | — | Version bump + final verification + docs | 8 |

---

## Phase 1 — Correctness & data-safety — **DONE ✅ (2026-06-17)**

*Goal: eliminate the verified user-facing bugs and data-loss paths. Smallest, highest-value changes first.*

> **Outcome:** All items resolved. P1.7 was a false positive (`anchors` is `[String; 5]`, statically bounded — no change). P1.6 downgraded from Medium→Low after confirming `move_vault_file` creates the trash parent dir (`vault_sync.rs:693`), so the stale dir list never caused a runtime failure — fixed for skeleton/health consistency anyway. Verified: `cargo test -p dnd-core` (90 passed, incl. 6 new calendar tests) + `cargo test --manifest-path desktop/src-tauri/Cargo.toml` (**117 passed**, incl. 3 new `vault_ref` tests). No new clippy warnings introduced (full clippy sweep is Phase 2).

- [x] **P1.1 — `vault_ref` Unicode slice panic** · Blocker · ✅ — *fixed: `to_ascii_lowercase` at vault_ref.rs:115/128/149/198 + 3 regression tests*
  `services/vault_ref.rs:149–185`. `prompt.to_lowercase()` (Unicode, **not** byte-length-preserving) is sliced using byte offsets derived from the original string: `tail_start = next_at + 1` (used on `prompt_lower`, line 164) and `boundary_index = tail_start + candidate.key.len()` (the *original-cased* key length, line 171–172). A non-ASCII char before an `@` (e.g. `Élodie`) can make a slice land mid-codepoint → **panic** on the AI-context and per-keystroke autocomplete paths.
  **Fix:** use `to_ascii_lowercase()` consistently at lines **115, 128, 149** (and anywhere `key_lower` is built), matching `publish.rs`'s documented invariant. Add a test with an accented prompt + accented key.

- [x] **P1.2 — Calendar weekday ignores the year** · Blocker · ✅ — *fixed: `weekday_index` now derives from `total_days_since_epoch` (+ guards underflow/OOB); cross-year + malformed-state tests added*
  `core/src/calendar.rs:700–708`. `weekday_index` accumulates month lengths *within the current year* + `day-1` but never folds in `state.year`. For a 365-day / 7-day calendar the weekday must advance 1 per year; it doesn't, so it's wrong for any multi-year campaign and disagrees with the moon math (which counts years via `total_days_since_epoch`).
  **Fix:** derive from the absolute day count, e.g. `((def.first_day as i64 + total_days_since_epoch(state, def)).rem_euclid(week_len as i64)) as usize`. While here, harden the same fn: `state.day.saturating_sub(1)` (avoid underflow at line 706) and `def.months.get(i)` (avoid OOB at line 703). Collapse its inner loop into the existing `days_before_month` helper. Add tests: weekday advances across a year boundary; malformed `day=0` state degrades instead of panicking.

- [x] **P1.3 — Soft-delete writes its undo record last** · Blocker · ✅ — *fixed: all 7 arms now insert the `SoftDeleteRow` before the destructive move/delete; `undo` audited (marks-undone-last is already the safe order)*
  `services/entity_admin.rs:766–812`. Order is: move file to trash → `delete_by_id` → `delete_by_vault_path` → **then** insert `SoftDeleteRow`. If that final insert fails, the entity is destroyed with **no undo record**. The publish path (`soft_delete_for_publish`, ~line 1760) already does it in the safe order and comments on why.
  **Fix:** write the recovery `SoftDeleteRow` **before** the destructive move/delete, mirroring `soft_delete_for_publish`. (Full transactional integrity is P6.1; this reorder is the cheap, high-value mitigation.) Apply the same audit to `undo_last_soft_delete` ordering.

- [x] **P1.4 — `lunar_shf` signedness mismatch** · High · ✅ — *fixed: import struct field is now `HashMap<String, i32>`; negative-shift import test added*
  `core/src/calendar.rs:607` imports `lunar_shf: HashMap<String, u32>`; `lunar_shifts` (`:557`) reads it back as `HashMap<String, i32>`. Round-trips fine for non-negative values. **If** donjon ever emits a negative shift, import (`:616`) hard-fails with "invalid lunar_shf data".
  **Fix:** change the import struct field to `HashMap<String, i32>` so both ends agree and negatives survive. ❓ First confirm whether donjon actually emits negative shifts (check a real export) — either way the unified type is correct and safer.

- [x] **P1.5 — Moon phase floor-bucketing** · Medium · ✅ — *fixed: `round` (not `floor`) centers the principal phases; `phase_from_age` centering test added. Chose option (b) — centered phases.*
  `core/src/calendar.rs:573–589`. `bucket = (fraction * 8.0).floor()` puts named phases at bucket **starts**, not centers — "Full" spans `[0.5, 0.625)` of the cycle instead of centering on the midpoint. Defensible as "8 equal eighths," but undocumented and untested at `age == cycle/2`.
  **Fix (decide):** either (a) document it as 8 equal eighths from new moon and add a test pinning the labels, or (b) center the principal phases (round to nearest eighth) so `age 0 = New`, `age cycle/2 = Full`. Add a `phase_from_age(cycle/2) == Full` test regardless.

- [x] **P1.6 — `vault.rs` required-dirs are stale** · ~~Medium~~ → Low · ✅ — *fixed: `ensure_structure` now derives from one `pub const ENTITY_DIRS` (all 7 kinds + matching `.trash`); `health.rs` detail message derives from the same list. No runtime bug existed (move_vault_file creates parents).*
  `core/src/vault.rs:6–14` `REQUIRED_TOP_LEVEL_DIRS` ensures dirs for only 4 kinds (+ partial `.trash`, no `.trash/items`); the app has 7 (`EntityStore`, `entity_store.rs:12–18`). Events/gods/dungeons dirs and `.trash/items` are never ensured.
  **Fix:** confirm whether soft-delete of items/events/gods/dungeons targets an un-ensured trash dir (does `move_vault_file` create parents?). Derive the dir list from one source (the entity-kind list) so it can't drift again. Folds naturally into P5 once the kind list is centralized.

- [x] **P1.7 — Unchecked `plan.anchors[i]` index** · ~~Low~~ → **Not a bug** · ✅ verified
  **Resolution (no change):** `DungeonContentPlan.anchors` is `[String; 5]` (`runebound-models/src/dungeon_plan.rs:110`), a fixed-size array — not a `Vec`. The beat count is checked `== DUNGEON_FUNCTIONS.len()` (5), and every `i` ranges over the 5-element beats or `anchors.iter()`, indexing into `[_; 5]` arrays (`LABELS`/`ROLES`/`LOOT_RULES`). All indexes are statically bounded; no panic is reachable. False positive from the review (assumed `Vec`); the type already enforces the invariant.

- [x] **P1.8 — `ordinal_suffix` wrong for day ≥ 100** · Low · ✅ — *fixed: now computes on `day % 100` / `day % 10`; large-day test added*
  `core/src/calendar.rs:710–717`. Custom months can exceed 100 days; day 121 returns "th" (should be "st").
  **Fix:** compute on `day % 100` (11/12/13 exception) then `day % 10`.

**Phase 1 verify:** `cargo test --workspace && cargo test --manifest-path desktop/src-tauri/Cargo.toml`; manually walk `date`/`date set`/`+1y`/`moon` on an imported donjon calendar across a year boundary; soft-delete + `undo` for each entity kind; an `@`-reference with an accented name.

---

## Phase 2 — Tooling & dead-code sweep — **DONE ✅ (2026-06-17)**

*Goal: cheap, low-risk wins that make every later phase safer to verify.*

> **Outcome:** Clippy is clean across the workspace and the desktop crate under
> `-D warnings`, and `make build` now gates on it (plus `cargo fmt --check`) via a
> new `make lint` target. Decisions taken with the user: (1) the gate is a **hard
> fail** (`-D warnings`); (2) the whole tree was `cargo fmt`'d first — it carried
> ~63 pre-existing rustfmt deviations — as a **separate commit**, so the clippy
> diff stays focused; (3) **P2.5 deferred → P5.2** (the LLM sampling literals live
> in the `ai_generation`/`entity_reroll` fan-out P5.2 collapses).
>
> The live clippy counts were higher than the stale baseline (~11 workspace lib +
> 59 desktop). Most cleared via `cargo clippy --fix`; the rest hand-fixed. A few
> are **allowed-with-note**, each tagged for removal when its owning phase lands:
> command-handler `into_iter` (`should_implement_trait`) → **P7.5**;
> `ai_generation`/`entity_reroll` `ptr_arg` + `wrong_self_convention` → **P5.2**;
> `entities/common` `result_large_err` → **P5.2**; `repositories::upsert_index`
> `too_many_arguments` → **P6**. New finding (not in the original review): 6 dead
> `render_*_markdown` thin wrappers in `publish.rs` — deleted god/event
> (unreferenced) and `#[cfg(test)]`-gated the other four (test-only).
>
> Verified: both clippy targets exit 0 under `-D warnings`; dnd-core 90 + desktop
> 117 tests; `tsc --noEmit` + `vite build` clean; `make lint` exits 0 on a clean
> tree and **non-zero** on a reintroduced warning (smoke-tested); `make build`
> exits 0. *Remaining manual check (left to the user): launch the app and confirm
> entity create/show/save/cancel still render after the App.tsx deletion.*

- [x] **P2.1 — Clippy clean + gated** · High · ✅ — *done: clean on both targets under `-D warnings`; new `make lint` target gates `make build` on clippy + `cargo fmt --check`. Whole tree `cargo fmt`'d first (separate commit). Allow-with-note items tagged for P5.2/P6/P7.5 (see Outcome).*
  7 warnings in `dnd-core`, 59 in `dnd-desktop`; `make build` runs `cargo check`, so they never gate.
  **Fix:** clear the warnings (`cargo clippy --fix` for the mechanical ones — `identity_op`, `needless_return`, etc. — hand-fix the rest), then add a clippy step to `make build` / CI, ideally `-D warnings` once clean.

- [x] **P2.2 — Delete App.tsx dead draft state** · High · ✅ — *done: deleted the 8 write-only signals + their `applyClientEvent` cases + 7 draft imports (net −97 lines); `outputDocFromClientEvent`/`entity_card` render path unchanged; `tsc` + build clean.*
  `desktop/src/App.tsx:94–101` declares `editorMode` + 7 `*Draft` signals that are **set** in `applyClientEvent` (`:533–613`) but **never read** (zero getter call sites). The visible card renders from `client_event.entity_card` (`:644`).
  **Fix:** delete the 8 signals and collapse `applyClientEvent` to just `clear_terminal` / `exit_requested` (+ no-op default). Removes ~115 lines and the frontend's accidental mirror of the backend per-entity branching. Drop the now-unused `*Draft` type imports.

- [x] **P2.3 — Doubled `#[cfg(test)]`** · Low · ✅ — *done: removed the duplicate attribute in `suggestions.rs` (now :897 after fmt).*
  `services/suggestions.rs:902–903`. Delete the duplicate attribute line.

- [x] **P2.4 — Audit `#[allow(dead_code)]`** · Low · 🔶 — *done: the P5-groundwork allows (schema/registry/`ALL_ENTITY_KINDS`/wizard variants/`DonjonCalendarJson`/etc.) are intentional and kept (re-audit after P5/P7). The one the audit flagged as "genuinely dead" — `DesktopHandlerInvocation::tokens` — was a **false positive**: it's used by the date/time-delta handlers (which need original casing), so only the stale `#[allow(dead_code)]` was removed, not the field.*
  Scattered on `EntityDomain::schema()`, `EntityFieldSpec::value_kind`, `EntitySchema::kind`, `EntityKind::as_str`, `ALL_ENTITY_KINDS`, `WizardTransition`/`NativeAction` variants, `DonjonCalendarJson` fields, etc. Several mark metadata that *should* be live (and will become live in P5).
  **Fix:** for each, either wire it up or delete it. Track which are "becomes-live-in-P5" vs genuinely dead. Re-run after P5/P7 to remove the rest.

- [ ] **P2.5 — Name the magic LLM sampling constants** · Low · 🔶 — **DEFERRED → P5.2** *(by decision: the literals live in the `ai_generation` (8×) + `entity_reroll` (7×) fan-out P5.2 collapses; hoisting now would churn ~15 call sites P5 rewrites. Folds into P5.2's generator/reroll-loop extraction.)*
  `services/ai_generation.rs` repeats `"temperature"/"top_p"/"repeat_penalty"` literals ~8× with small per-kind variations and no rationale.
  **Fix:** hoist to named consts (or per-kind config). Best done alongside the P5 generator-loop extraction so they land in one place.

**Phase 2 verify:** both clippy commands clean; both test suites green; launch the app, confirm entity create/show/save/cancel still render correctly after the App.tsx deletion.

---

## Phase 3 — De-couple: kill the string/markdown heuristics — **DONE ✅ (2026-06-17)**

*Goal: close the `docs/architecture.md` §9 anti-patterns where behavior is coupled to rendered prose. Make the backend the authority and emit structured nodes.*

> **Outcome:** The frontend no longer interprets rendered prose at all — it renders
> backend `OutputDoc`s, and clickability comes exclusively from backend-authored
> `command_ref` nodes. Two decisions taken with the user: (1) **thorough /
> backend-authoritative** — every response carries a doc and the *entire* frontend
> parser (`output/markdown.ts`, 250 lines) was deleted; (2) the **setup gate stays
> `ok:false`** (a non-zero exit for scripting) but is built from typed data and
> styled by the doc's own `Status` tone.
>
> Patterns established (each a single source of truth, mirroring the existing
> `command_availability(name)` idiom rather than touching the 107 manifest
> literals): `command_argument_kind(name)` (P3.3), the `SetupRequired` typed error
> (P3.2), and `spinner_hints()` + the serialized `CommandManifest.spinner_hints`
> (P3.5). Findings: `detectOllamaPrompt` + the `ollamaPrompt` signal were already
> **dead** (the onboarding *wizard* owns those steps via `awaiting_llm_label`, and
> the wizard path short-circuits the spinner) → deleted, not converted; the old
> spinner ladder's `"test ollama"` entry referenced no real command → dropped.
>
> Verified: command-specs 22 + dnd-core 92 + desktop 120 tests green; `make lint`
> (clippy `-D warnings` + `cargo fmt --check`, both targets) exit 0; `tsc --noEmit`
> + `vite build` clean (frontend bundle shrank); grep gates for parser refs,
> rendered-English branching, and magic byte-offsets all return nothing. *Remaining
> manual check (left to the user): launch the app and confirm entity/help/setup/
> history clickables are real command_refs, the setup gate renders friendly (not
> error-red), and spinners still appear on create/reroll/save/publish + the Ollama
> probes and onboarding wizard.*

- [x] **P3.1 — Entity text responses must carry `OutputDoc` + `command_ref`** · High · ✅ — *done: `ok_response` (desktop) + the core `execute_line` Ok arm auto-wrap a bare message in a paragraph doc; `no_active_draft_doc` (clickable `create <root>`) and `render_history_doc` (clickable history lines) supply explicit command_refs. Deleted `output/markdown.ts` entirely + the clickability-guessing infra (`resolveClickableCommandTarget`/`isValidCommandLike`/`buildCommandMeta`/`commandMeta`); the new `output/entry-doc.ts::buildEntryDoc` is a non-parsing fallback for frontend-origin entries; the banner is now a structured `BANNER_DOC`.*
  `commands/mod.rs` `ok_response` hard-coded `output_doc: None`; the frontend regex-guessed clickable commands via `parseFreeText`/`tryBuildSingleCommandInline`/`parseMarkdownInspiredBlocks`. The largest §9 cleanup.

- [x] **P3.2 — `output_doc_from_error_text` string-sniffs the setup error** · Medium · ✅ — *done: `execute_status` returns a typed `SetupRequired { issues, global_config_path }` error; `output_doc_from_error(&err)` downcasts it and builds a Warning-toned doc from `issues` directly (with a clickable `start setup`). Deleted `extract_missing_values`. `Display` still says "First-time setup required" for the plain-text `error` field / CLI. Kept `ok:false` by decision.*

- [x] **P3.3 — `suggestions.rs` hard-codes argument byte-offsets** · High · ✅ — *done: added `command_argument_kind(name)` (SSOT, like `command_availability`); the suggestion query is derived by stripping the parsed root token (`trimmed[root.len()..]`) instead of `[4..]`/`[6..]`/`[7..]`; `npc_travel_location_query` strips `"npc travel to ".len()`; dungeon beat completions come from `DUNGEON_FUNCTIONS`.*

- [x] **P3.4 — Frontend branches on rendered English** · Medium · ✅ — *done: deleted `isBootstrapSetupMessage` (the error branch styles by the response doc's leading `Status` tone — a Warning lead = soft gate → rendered as output, not red). Deleted `detectOllamaPrompt` + the `ollamaPrompt` signal + branches — a **dead-code finding**: their trigger strings come only from the onboarding wizard, whose Ollama steps declare `awaiting_llm_label`, so the wizard spinner path already owned them.*

- [x] **P3.5 — `commandSpinnerLabel` re-encodes the command taxonomy** · Medium · ✅ — *done: `spinner_hints()` + `CommandManifest.spinner_hints` carry the taxonomy; `commandSpinnerLabel` (~100 → ~20 lines) keeps the wizard short-circuit, skips `help`, then matches the longest spinner-hint prefix of the user input. The bare-`reroll <beat>` nuance collapses to "rerolling draft" (`dungeon reroll` still → "rerolling beat"); `create dungeon` is wizard-driven and intentionally absent.*

**Phase 3 verify:** both test suites green; `make lint` exit 0; `tsc`/`vite build` clean; grep gates (parser refs, rendered-English branching, magic offsets) all empty. Manual in-app smoke left to the user (clickables, setup gate styling, spinners).

---

## Phase 4 — Shared-contract & generation hardening

*Goal: make the Rust↔TS contract single-sourced and drift-proof.*

- [ ] **P4.1 — Replace hand-written TS generation** · High · ✅
  `runebound-models/build.rs` (351 lines of `push_str`) transcribes every struct field by hand, with no test asserting it matches the Rust types. It also writes into a sibling crate's source tree from a build script (non-hermetic — part of why desktop is a separate workspace).
  **Fix:** adopt `ts-rs` or `typeshare` (derive TS from the structs), making the Rust definitions the literal single source. If avoiding a new dep, at minimum add an integration test that serializes a sample of each struct and asserts JSON keys match the emitted TS keys, and anchor the output path on `CARGO_MANIFEST_DIR`. Prefer the derive route.
  *Phase 3 note:* the `CommandManifest` TS type is **hand-maintained** in `desktop/src/command/parser-client.ts` (not `generated/models.ts`), and Phase 3 added `spinner_hints: SpinnerHint[]` to both the Rust struct and that hand-written TS type. Whatever single-source mechanism P4 adopts should cover the manifest types too (incl. `SpinnerHint`), since the manifest crosses the boundary via `get_command_manifest`.

- [ ] **P4.2 — `slug` required in TS but `#[serde(default)]` in Rust** · High · ✅
  `build.rs:14` (`NpcDraft.slug: string`) and `:86` (`EventDraft.slug: string`) vs `drafts.rs:38`/`:135` (`#[serde(default)] pub slug`). TS contract is stricter than the wire contract.
  **Fix:** falls out automatically once P4.1 uses a derive (it'll emit `slug?: string`). If staying hand-rolled, emit these optional. Decide whether `slug` should instead be guaranteed-populated before crossing to the frontend.

- [ ] **P4.3 — Dedupe `WIKILINK_UNSAFE`** · Medium · 🔶
  `services/mention_extraction.rs:23` and `services/publish.rs:332` define identical `&['[', ']', '|', '#', '^']` consts, each commenting that it "mirrors the other."
  **Fix:** hoist one `pub const WIKILINK_UNSAFE_CHARS` and reference from both.

- [ ] **P4.4 — Mention grounding uses substring, not word-boundary** · Low · 🔶
  `services/mention_extraction.rs:88,103` use `prose_lower.contains(&lower)`, so "Vex" is "grounded" by prose containing "Vexley".
  **Fix:** reuse `publish.rs`'s `boundary_before`/`boundary_after` word-boundary check for consistency between the two layers.

**Phase 4 verify:** `cargo build -p runebound-models` regenerates `desktop/src/generated/models.ts` cleanly; the new parity test passes; frontend `tsc`/build is green; diff the regenerated TS to confirm no unexpected shape changes.

---

## Phase 5 — Entity-fan-out unification (the big one)

*Goal: finish the additive design. Today the command-behavior layer uses the `EntityDomain` registry, but `db.rs` and the three big services re-enumerate all 7 kinds by hand. Each new entity currently costs ~250 LOC in db + ~450–550 in services + ~100 per command module. Drive everything from the schema/registry so "add an entity" is truly additive.*

> Do the **enablers (P5.1)** first. If a new entity type is planned for v0.5.0, do this phase **before** adding it (don't create an 8th copy).

- [ ] **P5.1 (enabler) — Merge `EntityType` into `EntityKind`** · High · ✅
  `services/entity_admin.rs:1924` defines `EntityType`, an exact twin of `EntityKind` (`entities/kind.rs`). Two enums, same variants, same `as_str`.
  **Fix:** delete `EntityType`, use `EntityKind` everywhere. Mechanical, removes a whole "which enum?" class. Centralize the canonical kind list (`ALL_ENTITY_KINDS`) so P1.6 / P5.5 / P5.8 can derive from it.

- [ ] **P5.2 — Services should dispatch through the domain registry** · High · ✅
  `services/entity_admin.rs` (`resolve_entity` `:212–723`, `soft_delete_entity` `:761–1183`, `undo_last_soft_delete` `:1229–1672`), `entity_persistence.rs` (7× `save_*_draft`), `entity_reroll.rs` (7× `reroll_*_field` + `canonical_*_reroll_field` + `*_context_summary`). ~3,600 of ~5,200 logic lines are 7-way fan-out. The 70-field `EntityDetails` god-struct (`:1948`) has each arm write ~60 explicit `None`s.
  **Fix:** add object-safe methods to `EntityDomain` — `resolve`, `soft_delete`, `restore`, `save`, `reroll` — so services become `for kind in ALL_ENTITY_KINDS { registry.domain(kind).op(...).await? }`. Replace `EntityDetails`'s 70 fields with an enum-of-structs or `serde_json::Value` so arms stop writing `None`s. Drive the reroll retry loop and `set_field` from `EntitySchema.value_kind` (this retires the `#[allow(dead_code)]` on `value_kind`). Move per-field reroll instruction strings into the spec.

- [ ] **P5.3 — Collapse the 6 near-identical command modules** · High · ✅
  `commands/{location,faction,item,god,dungeon}_commands.rs` are character-for-character identical modulo the entity-name string and rename byte-offset; `npc_commands.rs` only adds `travel`. ~500 lines that should be ~80.
  **Fix:** one `dispatch_entity_command(kind, invocation)` driving the verb ladder generically (root = `kind.command_root()`, so rename/set/reroll parse off `root.len()` — removes the magic offsets). Add `fn has_draft(&self, state) -> bool` to `EntityDomain` (replaces the only per-entity line in the `help` branch). Register per-entity via a closure in `commands/mod.rs`. `npc travel` stays a pre-check or a per-domain `extra_verbs` hook; event's narrative-only reroll folds in.

- [ ] **P5.4 — `entity_commands` triple-encodes fields** · Medium · ✅
  `commands/entity_commands.rs:166–622`: `build_load_response`, `build_entity_card_doc`, `build_entity_card_text` each re-list every field per entity — the doc and text encode the **same** fields twice (drift hazard).
  **Fix:** drive the card (doc + text fallback) and the load mapping from one per-entity field descriptor (the schema, or a domain method), so the text fallback derives from the doc.

- [ ] **P5.5 — `db.rs` per-entity CRUD copy-paste** · High · ✅
  `core/src/db.rs` (1857 lines): `search/find_by_name_or_slug/find_by_slug/find_by_id/list/upsert/delete/row_to_*` per entity, with column lists restated 5–7× and order-sensitive `?N` placeholders edited by hand. ~250 LOC per new entity, six coordinated edits per column add.
  **Fix (incremental):** (a) one `const COLUMNS: &str` per entity reused in every query; (b) `#[derive(sqlx::FromRow)]` to delete the hand-written `row_to_*` block; (c) a small `EntityTable` trait or `impl_entity_table!` macro generating the CRUD set. Also fold `find_by_slug`/`find_by_id`/`find_by_name_or_slug` into one `find(Key)`.

- [ ] **P5.6 — `db.rs` `LIKE` wildcards unescaped** · Medium · ✅
  `core/src/db.rs:173` et al. `format!("%{}%", query…)` — `%`, `_`, `\` in a query act as wildcards (searching `_` matches every 1-char name). Not injection (values are bound), but wrong results.
  **Fix:** escape `\ % _` in the user portion and append `ESCAPE '\\'`; factor into one helper (repeats 7×). Lands naturally with P5.5.

- [ ] **P5.7 — Nondeterministic name tie-break** · Medium · ✅
  `core/src/db.rs:262` et al. `find_*_by_name_or_slug` / `search_*` lack a secondary sort key; two rows with the same lowercased name resolve arbitrarily across runs (no DB uniqueness on `name`).
  **Fix:** add `, id ASC` (or `, slug ASC`) to the `ORDER BY`.
  *Related design note:* `resolve_entity`/`soft_delete_entity` walk kinds in a fixed order and return the first name hit, so an NPC and Location both named "Raven" → the Location is unreachable by bare name and `delete Raven` always hits the NPC. Consider a disambiguation error when multiple kinds match. (Decide during P5.2.)

- [ ] **P5.8 — `entity_store.ensure_dirs` repetition** · Low · ✅
  `core/src/entity_store.rs:37–81` repeats the `create_dir_all(...).with_context(...)` block 7×.
  **Fix:** iterate the centralized kind list (from P5.1). Resolves P1.6's drift at the same time.

- [ ] **P5.9 — `create`/`system` seed→draft duplication + `clear_kind` cascades** · Low · 🔶
  `commands/create_commands.rs:96–472` (7× `create_*`) and `commands/system_commands.rs:229–474` (7× `reroll_current_*`) duplicate the seed→draft field copy; each does a `set_<kind>` then `clear_kind` for the other six. But `EditorSession` is designed to *retain* multiple drafts (`app_state.rs:218–228`, with a test asserting "second draft switches active but keeps both") — so the cascades may be discarding drafts the design means to keep.
  **Fix:** add an `EntityDomain::generate_draft(prompt, state)` so create and reroll share one builder. **Decide intent on multi-draft:** if single-draft, use a clear `clear_all()` + `set`; if multi-draft, the cascades are a latent bug. Confirm before changing.

**Phase 5 verify:** both test suites green (the schema/registry contract tests + 114 desktop tests are the guardrail here); for **every** entity kind, manually walk create → show → set → reroll → save → load → delete → undo, and confirm autocomplete + help still list the right commands per context. Diff line counts to confirm the collapse landed.

---

## Phase 6 — Data integrity & async hygiene

*Goal: make persistence atomic and keep the async runtime responsive.*

- [ ] **P6.1 — No transaction boundary across vault + db + index** · High · ✅ (pattern)
  The repository layer exposes no transaction primitive. `save_*` = vault write + (rename: delete old slug) + `repo.upsert` + `document_repo.upsert_index`, each independently fallible with no rollback; soft-delete/undo/sync are the same shape. A mid-sequence failure leaves persistent partial state (e.g. rename deletes the old canonical file before the new row's upsert succeeds).
  **Fix:** introduce a DB transaction on the `Database` handle, wrap the DB-side mutations (upsert + index) so they commit atomically, do the vault FS write last, and treat a post-commit FS error as a logged warning. (P1.3 is the cheap first slice of this.)

- [ ] **P6.2 — Blocking sync IO on async paths** · Medium · 🔶
  `build_reference_context` / `vault_ref::load_vault_reference_entries` / `EntityStore` do recursive `fs::read_dir` + TOML loads **synchronously inside `async fn`s**, including on the per-keystroke autocomplete path (`suggestions.rs`).
  **Fix:** wrap in `tokio::task::spawn_blocking` and/or cache the `@reference` index instead of rebuilding it per keystroke.

- [ ] **P6.3 — `vault_sync` is store→db only, not vault↔db** · Medium · 🔶
  `services/vault_sync.rs:116–167` lists the canonical TOML store and projects into the DB; it never scans the Obsidian markdown vault, so deleted/renamed `.md` files go undetected despite the "reconcile vault ↔ db" framing. The reap loop is also non-transactional (P6.1) and runs after `finalize_pending_publishes`, widening the window where a partial failure resurrects a reaped entity.
  **Fix (decide):** either correct the docs/naming to "project canonical store → db," or add the missing disk-scan half. Make the reap atomic regardless. *(The 7 `SyncRepository` impls are boilerplate — optional macro, but the trait split itself is the good kind of generalization; leave unless it bothers you.)*

- [ ] **P6.4 — `ollama_chat` ignores truncation / 200-with-error** · High · 🔶
  `services/ollama_chat.rs:80–101` `post_chat_for_content` returns `message.content` without checking `done_reason == "length"` (truncated → invalid JSON → silent retry miss) or a top-level `{"error": …}` body on a 200.
  **Fix:** detect and surface both; distinguish truncation from a genuine parse miss so the capacity notice is accurate. *(Good news: no `unwrap`/`expect` on model output anywhere — the `0..5` retry-with-repair loops are the right shape; this is about diagnostics, not crashes.)* Also `:67–73` `attempt_seed` truncates a micros clock to `i32` — fine for intra-call divergence, but comment/clean.

- [ ] **P6.5 — `link_prose` re-allocates per (position × name)** · Low · 🔶
  `services/publish.rs:400–452` calls `name.to_ascii_lowercase()` inside the inner loop over all names, for every boundary position — O(text × names × len) with a heap alloc each time.
  **Fix:** store `(canonical, lowercased)` pairs in `EntityLinker::new` (the lowercased form is already computed there and thrown away). Self-contained win.

- [ ] **P6.6 — `resources_assets` parsed by heuristic** · Low · 🔶
  `services/publish.rs:529–568` guesses "is it JSON or delimited text?" and splits on `\n;,`, which would wrongly fragment `"vaults, both hidden and warded"`.
  **Fix:** make `resources_assets` a `Vec<String>` in the model (model-first) like the other list fields, instead of a free-text blob publish has to reverse-engineer.

**Phase 6 verify:** both test suites green; kill-test a save mid-flight (e.g. point the vault at a read-only dir) and confirm no half-written state survives; large-vault autocomplete stays responsive; feed `ollama_chat` a truncated/`error` body and confirm it's surfaced, not silently retried into a generic failure.

---

## Phase 7 — Structural simplifications

*Goal: remove drift-prone parallel structures now that behavior is unified.*

- [ ] **P7.1 — Drop the `config.rs` `Partial*` mirror** · Medium · ✅
  `core/src/config.rs:9–142` mirrors every config struct with an `Option`-wrapped twin + hand-written `apply_partial` (`:299–342`). Since the base already uses `#[serde(default)]`, deserialize straight into `AppConfig`.
  **Fix:** delete the `Partial*` hierarchy + `apply_partial`. The only thing they buy — `ensure_config_sections_persisted`'s "was `[generation]` literally present?" probe (`:233`) — can use a `toml::Value`/`Option<toml::Table>` probe instead.

- [ ] **P7.2 — Collapse dual help renderers** · Medium · ✅
  `core/src/command.rs:550–706`: a markdown-string help renderer (`render_command_help`/`render_subcommand_help`) and a structured `OutputDoc` renderer (`command_help_doc`/`root_help_doc`) walk the same manifest in parallel and already nearly drift.
  **Fix:** write one doc→text renderer, derive the plain-text `output` from the `OutputDoc`, delete the string builders (~150 lines). Pairs well with P3.

- [ ] **P7.3 — `config_paths(workspace_root)` is vestigial** · Low · ✅
  `core/src/config.rs:96–101,242–256`: the `_workspace_root` param is ignored (everything derives from `dirs::config_dir()`), yet `workspace_root` is threaded through many signatures implying workspace-scoped config.
  **Fix:** drop the param (and the threading) or honor it; at minimum document that config is global.

- [ ] **P7.4 — Wizard `session.data` invariant via scattered `.expect()`** · Medium · 🔶
  `wizard/src/runtime.rs:66,161,229,244,253,283` `.expect("active wizard data")`; the host downcast helpers also `.expect()`. Safe today, but a future transition that leaves the session active after `data.take()` becomes a host crash.
  **Fix:** model `WizardSession` as `Inactive | Active { data, cursor, history }` so data presence is a type guarantee, removing all the `expect`s. Add a `WizardData` doc note that a failed downcast is a construction bug (the type-erasure invariant). Add tests for `Native` resubmit → `Stay`/`Next` history behavior.

- [ ] **P7.5 — `command-handler` ergonomics** · Low · 🔶
  `command-handler/src/lib.rs:147–181`: `into_iter(self)` is an inherent method shadowing the `IntoIterator` convention; `new()` without `impl Default` (clippy `new_without_default`).
  **Fix:** rename to `into_values()` (or impl `IntoIterator`); add `impl Default`. Add a doc comment clarifying the intentional `CommandHandler` vs `HandlerBridge` split (and the `ExecutionTarget`/`HandlerMetadata` ↔ `command-specs` `From`-bridge duplication) so the layering reads as deliberate.

- [ ] **P7.6 — Core `execute_line` doesn't expand inline `+1d`** · Low · ✅
  `core/src/command.rs:404–408` (`execute_line_internal`) runs `shell_words::split` → `normalize_alias_tokens` but **not** `expand_inline_delta_root_tokens` (`command_parse.rs:89–104,157`), so the parse view (`+`,`1d`) and core dispatch (`+1d`) disagree. **Not user-facing** — the desktop seam (`main.rs:50`) dispatches via `parse_command_input` (which expands) → registered `+` handler. Only bites if core is exercised standalone (CLI/tests).
  **Fix:** run the same delta expansion (ideally inside `normalize_alias_tokens` so both paths share it) for consistency.

**Phase 7 verify:** both test suites green; config round-trips (load → modify → save → reload) and the "section was/wasn't present" persistence behavior still holds; help output (plain text + clickable) unchanged across contexts; wizard back/cancel/native flows still work.

---

## Phase 8 — Release

*Goal: tag v0.5.0 from a clean tree.*

- [ ] **P8.1 — Bump version `0.4.0` → `0.5.0`** in `Cargo.toml` (workspace), `desktop/src-tauri/Cargo.toml`, `desktop/src-tauri/tauri.conf.json`, `desktop/package.json`. (Confirm `Cargo.lock` updates.)
- [ ] **P8.2 — Full verification matrix:** `cargo test --workspace` + `cargo test --manifest-path desktop/src-tauri/Cargo.toml` + both clippy commands clean + `make build` + frontend build + a manual smoke pass of onboarding, each entity lifecycle, the dungeon wizard, calendar/moon, and publish.
- [ ] **P8.3 — Docs:** update `docs/architecture.md` §10 (resolved friction), update any playbooks the P5 unification changed (the "Add a New Entity" checklist should now be much shorter), and archive this file.
- [ ] **P8.4 — Tag and cut the release.**

---

## Notes on confidence

The Blockers (P1.1–P1.3), P2.1/P2.2, P3.1/P3.2, P4.1/P4.2, P5.1–P5.7, P6.1, P7.1–P7.3/P7.6 were **verified against source** during the review. Items tagged 🔶 are consistent with the code but should be re-confirmed at the top of their phase before changing behavior — especially the ones that hinge on intent (P5.9 multi-draft, P6.3 sync direction) or external format (P1.4 donjon negatives). When in doubt, write the failing test first.
