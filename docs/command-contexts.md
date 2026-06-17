# Command Contexts, Availability, and Dispatch Semantics

> **Purpose:** This document is the single conceptual reference for *which* commands are offered where, *how* the parser decides argument vs. subcommand, and *why* the setup wizard is dispatched differently. Read it before changing command visibility, the parser, help, autocomplete, or the onboarding flow. These rules are subtle and were the root cause of several 0.4.0 regressions; documenting them is how we stop re-introducing them.

---

## 1. The input contexts

Every command surface (autocomplete and help) is gated by an **input context**, defined by `InputContext` in `command-specs/src/lib.rs`:

| Context | When active | Detected from |
|---|---|---|
| `Default` | No editor open — the normal command surface | none of the contexts below is active |
| `EntityEditor(kind)` | An entity draft is open; tag is the command root (`"npc"`, `"location"`, …) | `EditorSession::active_kind()` (desktop `AppState`) |
| `Wizard(id)` | A multi-step wizard is running; tag is the wizard id (`"dungeon"`, `"setup"`, `"setup-vault"`, …) | `wizard_session.active_id` (desktop `AppState`) |

Onboarding (`start setup` and the `setup vault|llm|model` sub-flows) is a **wizard** like any other, so there is no separate config context — it resolves to `Wizard("setup")` etc.

Precedence when resolving the live context (in `AppState::resolve_input_context`): an open entity draft (`EntityEditor`) wins, then an active wizard (`Wizard`), else `Default`. A wizard and the entity editor it finalizes into are never active at once.

> **Where context is resolved:** entity-editor and wizard state live in the **desktop** `AppState`, not core's `SessionState`. Core alone only ever renders the `Default` surface. Anything that must know about an open entity editor or active wizard (the suggestion service, the context-aware `help`) is therefore computed in the desktop layer. **`AppState::resolve_input_context()` is the one canonical resolver** — call it (the suggestion service and the desktop `help` handler both do) rather than re-deriving context ad hoc.

---

## 2. `command_availability` is the single source of truth

`command_availability(name) -> CommandAvailability` (in `command-specs/src/lib.rs`) declares where each command appears. **Every** visibility consumer asks this function; nothing hard-codes per-command visibility.

```rust
pub enum CommandAvailability {
    Default,                 // default surface only (create, calendar, undo, load, …)
    Always,                  // every context (help, clear)
    DefaultOrEntityEditor,   // default surface + any entity draft (publish)
    EntityEditorOnly,        // any entity draft (reroll, save)
    EntityScoped(&'static str), // only the matching entity kind's editor (npc, location, …)
    AnyWizard,               // any active wizard (continue, back)
    AnyEditorOrWizard,       // any entity draft or active wizard (cancel)
}
```

Consumers:

- **Autocomplete** — `services/suggestions.rs` retains only suggestions whose root `is_visible_in(&context)`.
- **Help index** — `render_root_help` / `root_help_doc` (core) and the desktop `help` override list only commands visible in the current context.

### Rules and footguns

- **Execution is NOT context-gated.** Contexts filter *autocomplete* and *help* only. Any registered handler still runs in any context if the user types it (dispatch looks up by root name, not context). Never rely on a context to "block" a command — if a command must be refused in some state, guard inside the handler.
- **Help unions in the default surface.** In an `EntityEditor`, the help index shows the `Default` commands **plus** that editor's context-specific commands (`location`, `reroll`, `publish`, `save`, `cancel`). This matches the fact that global commands remain runnable inside an editor. The predicate is `is_visible_in(context) || is_visible_in(Default)` (`help_lists_command` in `core/src/command.rs`).
- **The `_ => Default` fallthrough is a footgun.** A command with no explicit arm is `Default`-only and therefore invisible in every editor context. Add an explicit arm when introducing a command unless `Default`-only is genuinely correct. *(A missing/incorrect arm is what once dropped `undo` from the default help and made `publish` behave inconsistently.)*
- **`show_in_autocomplete: false` hides from both surfaces.** The hidden delta roots (`+`, `-`) set this; they are runnable but never listed.

---

## 3. Parser: subcommand vs. free-form argument

`core/src/command_parse.rs` decides whether the **second token** is a subcommand or an argument using the root's `requires_subcommand` flag:

| `requires_subcommand` | Unrecognized second token | Use for |
|---|---|---|
| `true` | Error: `unknown subcommand for <cmd>: <token>` | Menu-style roots: `calendar`, `date`, `npc`, `location`, … |
| `false` | Treated as a **free-form argument** | Roots that take a name/value: `publish <name>`, `load <name>`, `history <limit>` |

Known subcommands always match first regardless of the flag. So a `requires_subcommand: false` command can still expose a `help` subcommand **and** accept a free-form argument:

```
publish help            -> matches the `help` subcommand
publish The Brotherhood -> "The" is an argument, dispatched to the publish handler
```

> **Invariant:** if a command takes a free-form argument, it MUST be `requires_subcommand: false`, even if it also declares a `help` subcommand. Setting it `true` (or the parser ignoring the flag) makes the argument get rejected as an unknown subcommand. This was the `publish The Brotherhood` regression.

`-h` / `--help` are intentionally rejected everywhere; help is phrase-based (`help <command>` or `<command> help`). `help <command>` is normalized to `<command> help` at parse time, so the bare `help` root only ever renders the context index.

---

## 4. Dispatch routes (there are three, not one)

Most commands follow the registry path, but two routes bypass it. Know all three:

1. **Registry dispatch (the common path).** Desktop registry (`desktop/src-tauri/src/commands/mod.rs`) is tried first; on a miss it falls through to the core registry (`core/src/command.rs`), then to free-form entity resolution in `router.rs`.

2. **Desktop overrides core for the same root.** Because the desktop registry is consulted first, registering a root in *both* registries makes the desktop handler win in the desktop app. This is the supported way to give a core command access to desktop-only state. **`help` uses this**: core has a `help` handler (renders the `Default` surface), and the desktop registers a `help` override that also sees the entity editor and active wizard and renders the full context-aware index. If you add such an override, keep the two in sync via the shared core renderer (`render_help_overview`).

3. **Generic wizard route (includes onboarding).** When `wizard_session.active_id` is set, input is routed to `try_execute_active_wizard` (the `wizard` crate's `runtime.rs`) *before* registry dispatch — in desktop `run_command` (host = `AppState`) and in core's `CommandService::execute_line` (host = `CoreOnboardingCtx`). Onboarding's entry commands (`start setup`, `setup vault|llm|model`, `model`) are launched here too, via `onboarding_entry_wizard_id` → `start_wizard`, ahead of the registry. This is **one** route shared by every registered wizard — the dungeon flow's and onboarding's former bespoke interceptors were both deleted in favor of it. Consequences:
   - The active step's `accept()` consumes the raw line, so step answers (`2`, a path, a free-text premise, `save`) are never parsed as commands. The nav verbs `continue`/`back`/`cancel` are real manifest commands gated to wizard contexts (`AnyWizard` / `AnyEditorOrWizard`) — handled by the route, not by desktop handlers. `cancel` (and `cancel <id>`) exits the wizard.
   - A step may request a host capability via `WizardTransition::Native` (e.g. the onboarding vault picker); the engine calls `WizardHost::perform_native`, which opens the dialog on desktop and degrades gracefully (re-renders the step) on the CLI.
   - The response carries a structured `WizardView { id, step_id, awaiting_llm_label }` so the frontend spinner needs no prompt-text matching.
   - Adding a wizard adds **no** dispatch code — register a `Wizard` and point a launch command at `start_wizard`. See `docs/architecture.md` §4 (Wizard Framework) and §8D.

   `setup verbosity` (a one-shot config write) and `setup help` are **not** wizards — they stay normal `setup` subcommands handled by the registry.

---

## 5. Onboarding seed invariants

Each onboarding wizard seeds its accumulator from effective config on entry (`Wizard::seed(host)` → `seed_data` in `core/src/onboarding_wizard.rs`):

- **Seed parity.** Every flow seeds `ollama_base_url` **unconditionally** from `load_effective(...)`, so the Ollama menu renders `2: Continue with <configured server>` rather than the `127.0.0.1` default. Seeding "only when empty" never picks up the configured server — this was the `start setup` "continue with 127.0.0.1" regression. (Locked by a test; see `docs/onboarding-wizard-port.md` §3.5.)
- **No config write until `finalize`.** Cancelling resets the wizard session (`active_id`/cursor/data) and writes nothing. Config is written only by each wizard's `finalize` (`finalize_full`/`finalize_vault`/`finalize_llm`/`finalize_model`).

---

## 6. Change checklist

When you touch command visibility, parsing, help, or onboarding:

- [ ] New command has an explicit `command_availability` arm (or `Default`-only is deliberate and noted).
- [ ] Free-form-argument commands are `requires_subcommand: false`; menu-style roots are `true`.
- [ ] Help verified in **each** relevant context: `Default`, inside an entity editor, and (if relevant) inside a wizard. Entity-editor help shows global commands **plus** the editor's commands.
- [ ] Autocomplete filtered correctly for the same contexts (`cargo test suggestions`, run from `desktop/src-tauri`).
- [ ] If a command needs entity-editor state, it is dispatched (or overridden) in the desktop layer, not core-only.
- [ ] New wizards register a `Wizard` (no bespoke interceptor); the in-wizard command surface comes from the active step's `suggest()` + the global verbs via `active_step_suggestions` — no per-command filter added.
- [ ] Parser change covered by a `command_parse` unit test (argument-vs-subcommand cases).

---

## 7. Related docs

- `docs/cli.md` — command UX rules and per-command reference
- `docs/architecture.md` — crate/module boundaries and extension playbooks
- `docs/config.md` — config keys and the setup wizard's UX-level steps

---

*Last updated: 2026-06-15*
*Keep this aligned with `command-specs/src/lib.rs`, `core/src/command_parse.rs`, and the setup flow in the same PR as any change to those.*
</content>
</invoke>
