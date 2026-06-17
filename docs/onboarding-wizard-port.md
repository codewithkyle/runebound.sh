# Refactor Plan: Port Onboarding (`start setup`) onto the Wizard Engine

> **Purpose:** Retire core's bespoke onboarding state machine (`try_execute_onboarding`,
> `OnboardingSession`, the substate enums, the input-rewrite shim) by re-expressing `start setup` and
> `setup vault|llm|model` as **registered wizards on the engine** built in `docs/create-wizard-refactor.md`.
> End-state: **one** wizard engine in the codebase; onboarding gets clickable prompts, generic autocomplete,
> and the single wizard dispatch route for free — the same wins dungeon got.
>
> **Prerequisite:** the wizard refactor (`docs/create-wizard-refactor.md`, Phases 1–4) is merged and proven
> on `create dungeon`. This is the **next spike, same sprint**.
>
> **Read first:** `docs/create-wizard-refactor.md` (the engine), `docs/command-contexts.md §3-5` (dispatch
> routes + the onboarding seed-parity invariant), `docs/config.md` (what setup persists).

---

## 1. Why this is its own spike — the delta over dungeon

The dungeon wizard exercised the engine's happy path. Onboarding needs strictly more, and each item below is
a real design task, which is why it is sequenced after — not inside — the dungeon refactor:

| Onboarding need | Dungeon? | Engine answer (built in this spike) |
|---|---|---|
| Runs in **core/CLI**, not just desktop | No (desktop-only) | Promote engine to a shared `wizard` crate, generic over a host context |
| **Menu-vs-free-text substates** (`VaultStepState`/`OllamaStepState`) | No | "A substate is just another step" — decompose into two declarative steps |
| **Sub-flows** (`Full`/`Vault`/`Llm`/`Model`) that save-and-exit | No | Multiple `Wizard` registrations reusing shared `WizardStep` *values* |
| **Native folder picker** triggered from a step | No | `WizardTransition::Native` → host-fulfilled (desktop picker / CLI degrade) |
| **Config seeding** from `load_effective` on entry | No (empty start) | `Wizard::seed(ctx) -> WizardData` hook |
| **Async server probe** mid-step (Ollama models) | (LLM, but linear) | `accept()` is already async; spinner via the structured `WizardView` signal |
| Collapse the special **`ConfigEditor`** input context | No | Replace with `InputContext::Wizard("setup")`; migrate availability arms |
| Remove the **input-rewrite shim** (`rewrite_onboarding_tokens`) | No | Generic interceptor runs before the registry, so raw lines reach the step directly |

---

## 2. Current state we are deleting (inventory)

All citations are the bespoke onboarding implementation to be removed or relocated.

- **State machine:** `OnboardingSession` (`core/src/session.rs:44-71`) + `OnboardingFlow` (`:6-16`),
  `VaultStepState` (`:21-29`), `OllamaStepState` (`:34-42`); `SessionState.onboarding` (`:74-77`).
- **Interceptor:** `try_execute_onboarding` (`core/src/command.rs:333-852`) — entry verbs (`:351,386,402,413,475`),
  active guard (`:506`), global verbs `show setup`/`cancel` (`:510,543`), step verbs (`:552,565,580,596,616,634`),
  substate blocks (`:648,684,698,731,752`), `save` (`:772`), fallback (`:849`).
- **Prompt builders (plain text, not `command_ref`):** `vault_menu_text` (`:872-884`), `ollama_menu_text`
  (`:886-896`), `ollama_url_prompt_text` (`:898-904`), `enter_ollama_menu` (`:907-912`), `model_step_text`
  (`:914-937`), `save_prompt_text` (`:939-946`). Rich save docs: full save (`:821-846`), `save_llm_section`
  (`:981-1034`), `save_vault_section` (`:948-979`), `save_model_section` (`:1049-1074`).
- **Lifecycle:** `reset_onboarding` (`:854-861`); token rewriter `rewrite_onboarding_tokens` (`:1196,1203`).
- **Desktop native interplay:** the onboarding block in `run_command` (`desktop/src-tauri/src/main.rs:66-94`);
  `pick_vault_folder` + `FolderPick` (`desktop/src-tauri/src/commands/setup_commands.rs:6-34`).
- **Suggestions:** the bespoke `continue` branch (`services/suggestions.rs:181-205`) and the
  `onboarding.active → ConfigEditor` context resolution (`suggestions.rs:72-80`, mirrored in
  `system_commands.rs:32-45`).
- **Manifest:** `InputContext::ConfigEditor` and every `CommandAvailability::ConfigEditor` / `AnyEditor` arm
  that exists to serve onboarding (`command-specs/src/lib.rs`).

**Keep as-is (NOT a wizard):** `setup verbosity <level>` (`command.rs:445`) is a direct config write, not an
interactive flow. It stays a normal command.

---

## 3. The four hard problems & their solutions

### 3.1 Cross-layer home → promote the engine to a shared `wizard` crate

Add a workspace crate `wizard` (the parallel of `command-handler`, described in `docs/architecture.md §1` as
"Generic dispatch primitives"). It owns the engine from the dungeon refactor — `Wizard`/`WizardStep`/
`WizardTransition`/`WizardSession`/`WizardRegistry`/runtime/prompt helpers — with **no** knowledge of
`AppState` or `SessionState`. The host state is a generic parameter:

```rust
// wizard crate
#[async_trait]
pub trait Wizard: Send + Sync {
    type Ctx;                                   // AppState (desktop) | OnboardingCtx (core)
    fn id(&self) -> &'static str;
    fn steps(&self) -> &[Arc<dyn WizardStep<Ctx = Self::Ctx>>];
    fn seed(&self, _ctx: &Self::Ctx) -> WizardData { WizardData::default() }   // §3.5
    async fn finalize(&self, ctx: &Self::Ctx, data: &WizardData) -> CommandResult;
}
```

`session.rs`, `prompt.rs`, and the `WizardTransition`/`WizardData` types depend only on `runebound-models`
output types, so they move unchanged. The dungeon wizard re-points with `type Ctx = AppState`. (The
forward-compat nudge in the dungeon refactor — keeping `accept()`/`finalize()` the only `AppState`
touchpoints — makes this promotion mechanical.) Update the crate table in `docs/architecture.md §1`.

### 3.2 Substates → "a substate is just another step"

The `VaultStepState`/`OllamaStepState` enums exist only because one step has a "show menu" mode and a "now
type a value" mode that read the same raw input differently. With declarative steps, each mode is simply its
own step, and the enums vanish:

| Bespoke step + substate | Declarative steps |
|---|---|
| Vault `MenuShown` | `vault_menu` — choices `1/2/3` |
| Vault `AwaitingPath` | `vault_path` — free-text answer |
| Ollama `MenuShown` | `ollama_menu` — choices `1/2` |
| Ollama `AwaitingUrl` | `ollama_url` — free-text answer |
| Model `step==3` | `model` — numbered list + free text |
| Save `step==4` | `save` — confirm |

"Keep current / continue" stops being a magic substate case: it's a menu **choice** whose `accept()` returns
`Goto(next_section)`. `back` (engine verb from the dungeon refactor) lets the user return to a menu to change
an answer — a capability onboarding never had.

### 3.3 Sub-flows → reuse step *values* across multiple registrations

Because steps are values, the four entry points are four `Wizard` registrations sharing the same step objects:

| Wizard id | Steps | `finalize` writes |
|---|---|---|
| `setup` (Full) | `vault_menu, vault_path, ollama_menu, ollama_url, model, save` | vault + ollama + model |
| `setup-vault` | `vault_menu, vault_path` | vault section (save-and-exit) |
| `setup-llm` | `ollama_menu, ollama_url, model` | ollama + model |
| `setup-model` | `model` | model section |

The "save-and-exit per section" behavior of the sub-flows is just each wizard's `finalize` + a shorter step
list — no special "flow scope" field needed. The entry commands (`start setup`, `setup vault`, `setup llm`,
`setup model`/`model`) each call `start_wizard(<id>, ctx)`. `model`/`setup model` must still work when no
wizard is active (they start one), matching today's pre-guard handling (`command.rs:413`).

### 3.4 Native folder picker → `WizardTransition::Native`, fulfilled by the host

This is the one genuinely cross-layer mechanism. Today the desktop layer peeks at onboarding substate, runs
the picker, and rewrites input to `set vault <path>` before forwarding to core (`main.rs:66-94`). Generalize:

- The `vault_menu` step's choice `1` returns `WizardTransition::Native(NativeAction::PickFolder { resubmit_as: "vault_path" })`.
- The engine surfaces this as a `WizardOutcome::Native(action)` from the interceptor — it does **not** perform
  the action itself (the crate has no Tauri).
- **Each host fulfills it:**
  - **Desktop** (`main.rs`): run `pick_vault_folder(app_handle)`; on `Picked(path)` re-submit the path to the
    same wizard's `vault_path` step; on `Cancelled` re-render `vault_menu`. (Exactly today's behavior, now
    routed generically.)
  - **Core/CLI:** no `app_handle`, so the host returns the graceful message ("the dialog picker is only in the
    desktop app; choose 2 to type a path") and `Stay`s — matching core's current `"1"` bail (`command.rs:650`).

This keeps the engine host-agnostic while letting a step *declare* a desktop capability. It's the keystone
that makes a core-defined wizard usable from the desktop without the engine depending on Tauri.

### 3.5 Config seeding, async probe, ConfigEditor collapse, rewrite removal

- **Seeding:** `Wizard::seed(ctx)` runs `load_effective(workspace_root)` and fills `vault_path`,
  `ollama_base_url`, `selected_model` into `WizardData`. **Invariant (do not regress):** seed
  `ollama_base_url` **unconditionally** from effective config so the menu shows the configured server, not the
  `127.0.0.1` default — this is the documented "continue with 127.0.0.1" regression
  (`docs/command-contexts.md §5`, `docs/config.md`). A test must lock it.
- **Async probe:** the `ollama_menu`/`ollama_url` steps' `accept()` probes the server (fills the model list
  consumed by the `model` step's `choices()`); `awaiting_llm_label = "checking Ollama"` drives the spinner via
  the structured `WizardView` signal — no text-sniffing.
- **ConfigEditor collapse:** replace `InputContext::ConfigEditor` with `InputContext::Wizard("setup")` (and the
  sub-flow ids). Migrate the availability arms: `save`/`cancel` already need `AnyWizard` (added in the dungeon
  refactor); any remaining `ConfigEditor`-only arms become `AnyWizard` or `WizardScoped("setup")`. This removes
  a whole special context — a net simplification — but touches the manifest sentinel tests (§5).
- **Rewrite removal:** the generic wizard interceptor runs before the registry (like dungeon), so raw lines
  reach the active step's `accept()` directly. Delete `rewrite_onboarding_tokens` and the `setup input`/`setup save`
  shims (`command.rs:1196,1203`).

---

## 4. Phased plan

### Phase A — Promote the engine to the `wizard` crate (no behavior change)
- Create the `wizard` workspace crate; move the engine modules out of `desktop/src-tauri/src/wizards/` and
  parameterize over `Ctx` (§3.1). Desktop keeps a thin `wizards/` that registers concrete wizards.
- Re-point the dungeon wizard (`Ctx = AppState`); add the crate to `docs/architecture.md §1`.
- **Acceptance:** dungeon flow byte-identical behavior on the crate; root workspace **and** `desktop/src-tauri`
  both build (see §6 — they're separate builds); engine unit tests green.

### Phase B — Engine capabilities for onboarding (still no onboarding)
- Add the `seed()` hook, `WizardTransition::Native` + `WizardOutcome::Native` (host-fulfilled), and a host trait
  method for the native-action callback. Confirm `back`/`Goto`/async `accept` cover keep-current + probe flows.
- Unit-test the native-outcome contract with a fake host (both "fulfilled" and "degraded" paths).
- **Acceptance:** capabilities covered by tests; dungeon still green; nothing onboarding-facing yet.

### Phase C — Implement onboarding as wizards (parallel to the old path)
- Add a core-side `wizards/` module (or `setup_wizard.rs`) depending on the `wizard` crate, with an
  `OnboardingCtx` (effective config + the pieces of `CommandService`/`SessionState` the steps need). Implement
  the six step values (§3.2) and the four registrations (§3.3); `finalize`s reuse the existing `save_*_section`
  logic verbatim (§5 risk).
- Route both hosts through the generic interceptor: core's `execute_line_with_session` (`command.rs:261`) and
  desktop `main.rs` call `try_execute_active_wizard` before the registry. Implement the native action in both
  hosts (§3.4). Entry commands (`start setup`, `setup vault|llm|model`, `model`) call `start_wizard`.
- Keep the old `try_execute_onboarding` compiled but unreferenced (or behind a temporary flag) so Phase C can be
  A/B'd before deletion.
- **Acceptance:** full setup + each sub-flow + cancel + desktop picker + CLI fallback all work on the new path;
  config written is identical to the old path (golden-file test, §6).

### Phase D — Cutover, delete, and document
- Delete everything in §2's inventory except the `setup verbosity` command and the relocated `save_*` logic:
  `try_execute_onboarding`, `OnboardingSession` + enums, `reset_onboarding`, `rewrite_onboarding_tokens`, the
  `main.rs` onboarding block (folded into the generic native handler), the suggestions `continue` branch, and
  the `ConfigEditor` context resolution.
- Collapse `InputContext::ConfigEditor`; migrate availability arms; update the manifest sentinel tests (§5).
- **Docs:** `docs/command-contexts.md` — dispatch routes become **one** wizard route for *all* wizards (remove
  the onboarding special-route section §4, redefine/remove `ConfigEditor` in §1-2, update the seed-parity note
  §5 to reference `Wizard::seed`); `docs/config.md` — rewrite "wizard internals" to describe the engine;
  `docs/architecture.md` — crate table + the "two registries" / dispatch-route sections; `docs/cli.md` — setup
  surface. Mark onboarding items resolved in `docs/review-dungeon-alignment.md`.
- **Acceptance:** no references to `try_execute_onboarding`/`OnboardingSession` remain; full verification (§6)
  green; docs updated in the same PR.

---

## 5. Manifest, availability & context changes (do carefully — tests are the guardrail)

- `command-specs/src/lib.rs`: remove `InputContext::ConfigEditor`; ensure `start setup`, `setup vault|llm|model`,
  `model`, `setup verbosity` keep correct specs/availability (entry commands are `Default`-surface launchers).
- Availability arms: `continue`/`back`/`cancel` already `AnyWizard` (dungeon refactor). Re-point any
  `ConfigEditor`/`AnyEditor` arms that existed for onboarding to `AnyWizard`/`WizardScoped("setup")`.
- Sentinel tests to update: `default_surface_commands_are_an_explicit_known_set` (~`lib.rs:1351`),
  `entity_roots_are_scoped_to_their_own_editor` (~`:1269`), the `ConfigEditor`-referencing visibility tests,
  and `menu_style_roots_require_a_subcommand` for any changed `setup` shape. These tests are designed to fail
  if visibility drifts — let them catch you.

---

## 6. Risks & verification

**Risks**
- **Config-write parity** — the persisted TOML must not change. Mitigation: reuse `save_*_section` logic inside
  `finalize`; add a golden-file test that scripts a full flow and asserts the written `config.toml`.
- **Seed-parity invariant** — seed `ollama_base_url` unconditionally (§3.5); explicit regression test.
- **Headless/CLI must still run setup** — the engine and `OnboardingCtx` must work with no `app_handle`; the
  `NativeAction` degradation path is the critical case. Test the core path with no desktop host.
- **ConfigEditor removal blast radius** — several availability arms + tests; do it last (Phase D) with the
  sentinels as the safety net.
- **Cancel/entry parity** — `cancel`/`cancel setup` exit; `model`/`setup model` start a wizard when none active.

**Verification**
- Build the **root workspace** *and* `desktop/src-tauri` **separately** — `cargo build` / `cargo --workspace`
  at the repo root does **not** include the Tauri crate, so a desktop-only break hides unless you build
  `desktop/src-tauri` on its own. (Known workspace gotcha.)
- `cargo test` for the new `wizard` crate, `command-specs`, and `suggestions`.
- Manual: full `start setup`; each of `setup vault|llm|model`; `cancel` mid-step; desktop folder picker +
  cancel-re-shows-menu; CLI "type a path" fallback; verify menus are clickable and autocomplete inside setup;
  confirm written config is identical and the Ollama menu shows the configured server.

---

## 7. Decisions (recommended; confirm before Phase C)

1. **Where onboarding steps live** — a **core-side `wizards/` module** depending on the `wizard` crate, with a
   small `OnboardingCtx`. (Recommended over putting them in the crate, which must stay host-agnostic.)
2. **Collapse `ConfigEditor`** entirely into `InputContext::Wizard("setup")` rather than keeping it as an alias.
   (Recommended — fewer special contexts.)
3. **Four registrations vs one scoped wizard** — four registrations sharing step values (§3.3). (Recommended —
   leans into the declarative-steps decision and keeps each `finalize` obvious.)

---

## 8. Done checklist

- [x] `wizard` crate created; engine host-agnostic (generic over `H: WizardHost`); dungeon re-pointed and unchanged
- [x] `seed(host)` (async + fallible) + `WizardTransition::Native` / `NativeOutcome` / `WizardHost::perform_native` (host-fulfilled, default = CLI degradation) implemented and unit-tested with a fake host
- [x] Onboarding expressed as `setup`/`setup-vault`/`setup-llm`/`setup-model` registrations over shared `WizardStep` values, generic over an `OnboardingHost` capability trait
- [x] Setup prompts emit `command_ref` (clickable); autocomplete works in-setup via `InputContext::Wizard`
- [x] Native picker wired on desktop (`AppState::perform_native` → Tauri folder dialog); CLI degrades via the default `perform_native` *(desktop picker pending manual verification)*
- [x] `try_execute_onboarding`, `OnboardingSession`, substate enums, `rewrite_onboarding_tokens`, the `main.rs` onboarding block, and the suggestions `continue` branch all deleted
- [x] `InputContext::ConfigEditor` (and `CommandAvailability::ConfigEditor`/`AnyEditor`) collapsed; availability arms + sentinel tests updated
- [~] Seed-parity enforced in code (unconditional `ollama_base_url` seed) and noted in docs; **config-write parity not yet locked by a golden-file test** (`save_config`/`load_effective` use the global config dir, so a write-asserting test needs a sandbox — follow-up). `finalize_*` are faithful ports of the former `save_*_section` logic.
- [x] Docs updated (`command-contexts`, `config`, `architecture`, `cli`); review-doc items resolved
- [x] Root workspace **and** `desktop/src-tauri` build; `cargo test` + `make build` green *(full manual setup verification by the user pending)*

---

*Drafted: 2026-06-16 · branch `feature/dungeons` · spike 2 of 2 (after `docs/create-wizard-refactor.md`).*
*Executed: 2026-06-17 on branch `multistep-wizard` (Phases A–D).*
