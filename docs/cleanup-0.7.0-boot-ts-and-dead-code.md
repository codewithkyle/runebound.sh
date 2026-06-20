# Cleanup Plan — boot.rs TS de-duplication + conservative dead-code removal (v0.7.0)

> **For the implementing agent.** This is a self-contained work order; you do not need
> prior conversation context. Do the two phases in order. Phase 1 is frontend+codegen,
> Phase 2 is backend-only — they don't overlap. Line numbers are accurate as of branch
> `wip/0.7.0`; re-grep if the tree has moved.

## Orientation (read first)

- **Two cargo workspaces.** The root workspace (`dnd-core`, `command-handler`,
  `command-specs`, `runebound-models`, `wizard`) **excludes** the Tauri crate. The
  desktop backend at `desktop/src-tauri` is its own workspace. You must build/test
  **both** separately — `cargo build --workspace` at the root does NOT compile
  `desktop/src-tauri`.
- **ts-rs drift guard.** Rust↔TS contracts are single-sourced with `ts-rs`: a
  `#[cfg(test)]` module generates a `.ts` file and a plain `cargo test` asserts the
  on-disk file matches the Rust types. `runebound-models` → `desktop/src/generated/models.ts`;
  the desktop crate's `services/ts_export.rs` → `desktop/src/generated/manifest.ts`.
  Regenerate the desktop side with:
  `UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml`.
- **Out of scope / do not touch.** The `@reference` LLM-grounding path in
  `services/ai_generation.rs` and `services/vault_ref.rs` is unrelated to this work.
- **Do NOT commit.** The repo owner reviews locally; stage only if asked, never
  `git commit`. Keep `cargo clippy` and all tests green (both workspaces are currently
  clean: 0 warnings, ~399 tests passing).

---

## Context / why

The v0.7.0 review found two loose ends:

1. **boot.rs TS duplication.** `BootTaskInfo`, `BootPlan`, `BootTaskResult`
   (`desktop/src-tauri/src/boot.rs`) cross the Tauri boundary but derive only
   `serde::Serialize`, not ts-rs `TS`. They're therefore hand-redeclared in TypeScript at
   `desktop/src/App.tsx:78-80` — the documented "duplicated cross-layer type"
   anti-pattern (`docs/architecture.md §9`, `docs/render.md §8`). The hand-written
   `tone: string` is also looser than the Rust comment's `"success" | "warning" | "error"`.
2. **Stale/orphaned `#[allow(dead_code)]`.** 18 sites remain. A handful are genuinely
   orphaned; the rest are either the documented entity-schema extension surface
   (`docs/architecture.md §8C/§10`) or legitimate library-API / external-DTO allows.

**Chosen scope: conservative.** Remove only genuinely-orphaned code; keep the documented
extension surface and legitimate allows (do not churn them).

---

## Phase 1 — Single-source the boot types through ts-rs

### Step 1.1 — Derive `TS` and tighten `tone` in `boot.rs`

In `desktop/src-tauri/src/boot.rs`:

- Add `ts_rs::TS` to the derive on all three structs. Current (lines 19, 26, 34):
  `#[derive(Debug, Clone, Serialize)]` → `#[derive(Debug, Clone, Serialize, ts_rs::TS)]`.
- Replace the stringly-typed tone. Change `BootTaskResult.tone` (line 39) from
  `pub tone: String` to `pub tone: BootTone`, and add the enum:

  ```rust
  #[derive(Debug, Clone, Serialize, ts_rs::TS)]
  #[serde(rename_all = "snake_case")]
  pub enum BootTone {
      Success,
      Warning,
      Error,
  }
  ```

  The `snake_case` rename keeps the wire form identical (`"success"`/`"warning"`/`"error"`),
  so the frontend payload is unchanged; the generated TS becomes a union type.
- Update the construction sites in `run_boot_task` to use the enum instead of string
  literals:
  - `"cleanup"` arm → `tone: BootTone::Success` (line ~88).
  - `"calendar"` arm → `Ok` branch `BootTone::Success`, `Err` branch `BootTone::Warning`
    (lines ~99 / ~104).
  - `"llm"` arm → the `let tone = if … { "success" } else { "warning" }` block (lines
    ~113-117) becomes `BootTone::Success` / `BootTone::Warning`; drop the `.to_string()`
    at the `BootTaskResult { … tone, … }` site (line ~121).

### Step 1.2 — Emit `generated/boot.ts` from `services/ts_export.rs`

`desktop/src-tauri/src/services/ts_export.rs` already generates `manifest.ts` via a
`generate()` builder (lines 32-53) and a `manifest_ts_matches_the_rust_types` drift test
(lines 55-71). Mirror that for boot, keeping `manifest.ts` focused on commands/suggestions:

- Factor the test body (the `UPDATE_MODELS` write-or-assert against a path) into a small
  shared helper, e.g. `fn assert_or_update(path: &str, generated: &str)`, and call it from
  the existing `manifest_ts_matches_the_rust_types` test.
- Add `fn generate_boot() -> String` that emits the boot decls the same way `generate()`
  does — `BootTone::decl(&cfg)`, `BootTaskInfo::decl(&cfg)`, `BootPlan::decl(&cfg)`,
  `BootTaskResult::decl(&cfg)` — with a matching auto-generated header comment.
  Add `use crate::boot::{BootPlan, BootTaskInfo, BootTaskResult, BootTone};`.
- Add a `boot_ts_matches_the_rust_types` test that calls
  `assert_or_update(".../src/generated/boot.ts", &generate_boot())` (path built with the
  same `concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generated/boot.ts")` idiom as line 58).

### Step 1.3 — Generate the file

Run: `UPDATE_MODELS=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml`
→ creates `desktop/src/generated/boot.ts`. Then a plain `cargo test` must pass the new
drift guard.

### Step 1.4 — Consume in the frontend

In `desktop/src/App.tsx`:

- Delete the three hand-written aliases at lines 78-80 (`type BootTaskInfo = …`,
  `type BootPlan = …`, `type BootTaskResult = …`).
- Import them from the generated file, alongside the existing
  `from "./generated/models"` import (line 12):
  `import type { BootPlan, BootTaskResult, BootTaskInfo } from "./generated/boot";`
  (import only what's referenced — `BootPlan` at line 468, `BootTaskResult` at line 492;
  include `BootTaskInfo` only if used directly.)
- The `invoke<BootPlan>("boot_plan")` / `invoke<BootTaskResult>("run_boot_task", …)` call
  sites are otherwise unchanged.

### Phase 1 files
`desktop/src-tauri/src/boot.rs`, `desktop/src-tauri/src/services/ts_export.rs`,
`desktop/src/generated/boot.ts` (new, generated), `desktop/src/App.tsx`.

---

## Phase 2 — Conservative dead-code cleanup

**Per-site method.** For each `#[allow(dead_code)]` you touch: remove the attribute, then
`cargo build` + `cargo test` in **both** workspaces.
- No warning ⇒ the allow was **stale**; leave the code, keep the attribute deleted.
- Warns ⇒ genuinely dead; delete the code (and anything it orphans), re-verify.

### 2a. DELETE — genuinely orphaned (verified 0 production callers)

1. **`desktop/src-tauri/src/repositories/mod.rs` — unused `VaultRepository` surface.**
   The trait (lines 22-36) has 6 methods marked dead and one live one. Remove these 6
   from **both** the `trait VaultRepository` and the `impl VaultRepository for
   ProdVaultRepository` (lines 40-109): `read_file`, `write_file`, `move_file`,
   `file_exists`, `resolve_path`, `ensure_root_exists`. **Keep `ensure_structure`** — it
   has real callers (`commands/publish_commands.rs:89`, `services/entity_admin.rs:179`).
   The trait collapses to a single method.
   - Then remove the now-orphaned free fn `normalize_relative_path` (lines 111-114) — it
     was used only by the deleted `read_file`/`move_file`. **Do not** touch
     `utils.rs::normalize_relative_path_for_storage` (a different, still-used fn).
   - Note: `ProdVaultRepository` is the **only** impl (no test fakes), so nothing else
     needs updating. The 4 grep hits for `ensure_root_exists` elsewhere are
     `Vault::ensure_root_exists` (the core type's method), not the repo method — leave them.

2. **`desktop/src-tauri/src/entities/domain.rs` — unused `EntityDomain::schema()`.**
   Remove the trait method (lines 37-38; `fn schema(&self) -> &'static EntitySchema`) — it
   has 0 callers. Then remove its 7 impls in `entities/domains/`:
   `npc_domain.rs:33`, `event_domain.rs:30`, `god_domain.rs:35`, `item_domain.rs:36`,
   `location_domain.rs:38`, `faction_domain.rs:38`, `dungeon_domain.rs:75`. **Keep** the
   `EntitySchema` constants themselves and `.fields` access (they drive set/reroll/
   suggestions) — only the unused `schema()` accessor goes.

3. **`desktop/src-tauri/src/entities/registry.rs` — unused `iter()`.**
   Remove `EntityDomainRegistry::iter()` (lines 29-34); lookups go through `domain(kind)`.

### 2b. DROP STALE ALLOW ONLY — keep the code

4. **`desktop/src-tauri/src/app_state.rs:379` — `get_event_mut` allow is stale.**
   The method **is** called (`entities/domains/event_domain.rs:93`), so the
   `#[allow(dead_code)]` on line 379 is unnecessary. Delete **just the attribute**; keep
   the method (this makes the `get_<kind>_mut` family consistent — no sibling carries the
   allow).

### 2c. KEEP — do NOT remove (documented surface / legitimate allows)

Leave these exactly as-is unless, after removing an attribute to test, the compiler shows
it is genuinely stale (in which case drop only the attribute). Do **not** delete the code:

- **`entities/kind.rs`** — `impl EntityKind` block (`as_str`/`command_root`/`display_name`,
  line 15) and `ALL_ENTITY_KINDS` (line 46). These are a live enum's helpers + the §8C
  extension lever; `as_str`/`command_root` are used, `ALL_ENTITY_KINDS` is used by the
  in-file tests. You may try dropping the blanket attrs to see if any single method is
  genuinely unused, but keep the array and the used methods.
- **`entities/schema.rs`** — `FieldAccess` enum (line 11), `EntityFieldSpec.value_kind`
  (line 34), `EntitySchema.kind` (line 52). The schema typing/access surface per §8C/§10
  (built ahead of use deliberately). Keep.
- **`wizard/src/wizard.rs`** — `WizardTransition` (line 54), `NativeAction` (line 81).
  Public library navigation API; the `Back`/`Native`/`PickFolder` forms are constructed by
  consumer crates (desktop wizards, core onboarding), so they read as dead *within* the
  `wizard` crate. The allows are required. Keep.
- **`core/src/calendar.rs`** — `DonjonCalendarJson` (line 600). An external-schema
  deserialization DTO documenting the donjon JSON contract (incl. fields not yet read).
  Keep.

### Phase 2 files
`repositories/mod.rs`, `entities/domain.rs`, `entities/domains/{npc,event,god,item,location,faction,dungeon}_domain.rs`,
`entities/registry.rs`, `app_state.rs` (attribute-only). Triage-only (likely no change):
`entities/kind.rs`, `entities/schema.rs`, `wizard/src/wizard.rs`, `core/src/calendar.rs`.

---

## Verification (run all; everything must be green)

Root workspace:
```bash
cargo build --workspace
cargo clippy --workspace          # expect 0 warnings
cargo test --workspace            # expect all pass (incl. wizard, calendar)
```

Desktop crate (separate workspace):
```bash
cd desktop/src-tauri
cargo build
cargo clippy                      # expect 0 warnings
cargo test                        # expect ~228 pass, incl. the NEW boot_ts drift test
```

Frontend + codegen:
```bash
# from desktop/src-tauri: regenerate (only needed if you edited types after generating)
UPDATE_MODELS=1 cargo test --manifest-path Cargo.toml
# confirm the generated file exists and App.tsx compiles against it
cd ../../desktop && npm run build   # tsc must pass with the hand-written aliases gone
```

Umbrella: `make build` passes.

Manual smoke (optional but recommended): launch the app and confirm the boot spinners
render with correct tones and the MOTD shows — the boot path must be behaviorally
unchanged.

## Done criteria
- [ ] boot types derive `TS`; `tone` is a `BootTone` enum; `generated/boot.ts` exists and
      `App.tsx` imports it (no hand-written boot aliases remain).
- [ ] `boot_ts_matches_the_rust_types` drift test added and passing.
- [ ] 2a deletions done (6 vault-repo methods + `normalize_relative_path`,
      `EntityDomain::schema()` + 7 impls, registry `iter()`).
- [ ] `get_event_mut` stale allow removed (method kept).
- [ ] 2c items untouched (or only a confirmed-stale attribute dropped).
- [ ] Both workspaces: build + clippy + tests all clean. No commit made.
