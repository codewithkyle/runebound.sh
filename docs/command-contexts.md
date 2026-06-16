# Command Contexts, Availability, and Dispatch Semantics

> **Purpose:** This document is the single conceptual reference for *which* commands are offered where, *how* the parser decides argument vs. subcommand, and *why* the setup wizard is dispatched differently. Read it before changing command visibility, the parser, help, autocomplete, or the onboarding flow. These rules are subtle and were the root cause of several 0.4.0 regressions; documenting them is how we stop re-introducing them.

---

## 1. The three input contexts

Every command surface (autocomplete and help) is gated by an **input context**, defined by `InputContext` in `command-specs/src/lib.rs`:

| Context | When active | Detected from |
|---|---|---|
| `Default` | No editor open — the normal command surface | neither editor below is active |
| `ConfigEditor` | The setup/onboarding wizard is running | `session.onboarding.active` (core `SessionState`) |
| `EntityEditor(kind)` | An entity draft is open; tag is the command root (`"npc"`, `"location"`, …) | `EditorSession::active_kind()` (desktop `AppState`) |

`EntityEditor` takes precedence over `ConfigEditor` when resolving the live context (an open draft wins over an in-progress wizard).

> **Where context is resolved:** entity-editor state lives in the **desktop** `EditorSession`, not core's `SessionState`. Core alone can only distinguish `Default` vs. `ConfigEditor`. Anything that must know about an open entity editor (the suggestion service, the context-aware `help`) is therefore computed in the desktop layer. See `services/suggestions.rs` and the desktop `help` handler in `commands/system_commands.rs` for the canonical resolution snippet — copy it rather than re-deriving context ad hoc.

---

## 2. `command_availability` is the single source of truth

`command_availability(name) -> CommandAvailability` (in `command-specs/src/lib.rs`) declares where each command appears. **Every** visibility consumer asks this function; nothing hard-codes per-command visibility.

```rust
pub enum CommandAvailability {
    Default,                 // default surface only (create, calendar, undo, load, …)
    Always,                  // every context (help, clear)
    ConfigEditor,            // setup wizard only
    AnyEditor,               // config or entity editor (save, cancel)
    DefaultOrEntityEditor,   // default surface + any entity draft (publish)
    EntityEditorOnly,        // any entity draft (reroll)
    EntityScoped(&'static str), // only the matching entity kind's editor (npc, location, …)
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

2. **Desktop overrides core for the same root.** Because the desktop registry is consulted first, registering a root in *both* registries makes the desktop handler win in the desktop app. This is the supported way to give a core command access to desktop-only state. **`help` uses this**: core has a `help` handler (knows only `Default`/`ConfigEditor`), and the desktop registers a `help` override that also sees the entity editor and renders the full context-aware index. If you add such an override, keep the two in sync via the shared core renderer (`render_help_overview`).

3. **Onboarding interception (setup wizard).** When `onboarding.active`, input is routed to `try_execute_onboarding` *before* registry dispatch (in core's `execute_line`, and in desktop `run_command`). The desktop registry is bypassed entirely during setup. Consequences:
   - Setup verbs (`continue`, menu numbers `1`/`2`/`3`, `set vault`, `set ollama`, `test ollama`, `use model`, `cancel`) are handled inside `try_execute_onboarding` — **not** by desktop handlers. The desktop `cancel` handler does not run during setup, so `cancel` is accepted explicitly there (both `cancel` and `cancel setup` exit the wizard).
   - `model` / `setup model` are also handled on this path (before the `active` guard), so they work outside an active wizard too.

---

## 5. Onboarding session invariants

The wizard's prompts read from `OnboardingSession` fields, which are seeded when a flow starts. The seeding must be consistent across entry points:

- **Seed parity.** `start setup` (full flow) and `setup llm` must seed `ollama_base_url` (and other shown fields) from the **same effective config**. The menu prompt renders `2: Continue with <ollama_base_url>`, so if one entry point seeds it unconditionally and another only when empty, the prompts disagree. Because `OnboardingSession::default()` is `http://127.0.0.1:11434`, an "only when empty" seed never picks up the configured server — seed unconditionally from `load_effective(...)`. This was the `start setup` "continue with 127.0.0.1" regression.
- **`reset_onboarding` clears flow state, not persisted config.** Cancelling/finishing resets `active`, `step`, substates, and the model list — it does not write config. Config is only written by the `save` step.

---

## 6. Change checklist

When you touch command visibility, parsing, help, or onboarding:

- [ ] New command has an explicit `command_availability` arm (or `Default`-only is deliberate and noted).
- [ ] Free-form-argument commands are `requires_subcommand: false`; menu-style roots are `true`.
- [ ] Help verified in **each** relevant context: `Default`, inside an entity editor, and (if relevant) during setup. Entity-editor help shows global commands **plus** the editor's commands.
- [ ] Autocomplete filtered correctly for the same contexts (`cargo test suggestions`, run from `desktop/src-tauri`).
- [ ] If a command needs entity-editor state, it is dispatched (or overridden) in the desktop layer, not core-only.
- [ ] Onboarding entry points seed shown fields identically; `cancel` exits the wizard.
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
