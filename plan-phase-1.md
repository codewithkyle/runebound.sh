## Phase 1 – Single Command Registry

### What’s Broken
- Command metadata lives in `core/src/command_manifest.rs`, but execution is split between `core/src/command.rs` (core-only roots) and `desktop/src-tauri/src/router.rs` (everything else), so the manifest is lying about what runs where.
- Adding a new command requires touching manifest, router, autocomplete, and sometimes Solid UI; there is no shared registry or trait to enforce parity.
- Desktop-only commands bypass the reusable parser/normalizer in core, so every branch re-parses strings, builds history entries, and manages help text manually.

### What Needs to Change
- Replace the ad-hoc router with a registry keyed by command root where each handler advertises execution location, help metadata, and completion hooks.
- Have both desktop and core rely on the same dispatcher API so `CommandExecution::Desktop` simply maps to a handler implemented in the desktop crate rather than a separate giant `if` chain.
- Push shared normalization (`normalize_command_input`, alias resolution, help rewrites) into a crate used by both executors to ensure identical behavior.

### Implementation Notes
- Define a `CommandHandler` trait (e.g., `fn handles(&self) -> &'static str`, `async fn execute(&self, ctx, input) -> Result<CommandOutput>`). Provide blanket helpers for simple “subcommand only” commands.
- Move manifest construction into its own crate (`command-specs`) so both Rust targets and the Solid autocomplete import from the same source (expose via JSON or generated TypeScript bindings).
- Update `dnd_core::command::execute_line_with_session` to consult the registry: `handler_registry.lookup(root)?.execute(...)`. Desktop can extend the registry at startup by injecting additional handlers.
- Ensure `CommandExecution` enum remains but is now derived from handler registration rather than duplicated switch statements.

### Refactor Checklist
- [ ] Create shared `command-handler` crate exposing registry + trait.
- [ ] Port existing core commands (`status`, `config`, `help`, `exit`, onboarding) into handler implementations.
- [ ] Move each desktop-only command out of `router.rs` into discrete handler modules registered from `desktop/src-tauri`.
- [ ] Delete the monolithic router logic once parity tests pass.
- [ ] Update autocomplete manifest generation to consume handler metadata instead of hardcoded arrays.
