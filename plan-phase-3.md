## Phase 3 – Modular Desktop Execution Pipeline

### What’s Broken
- `desktop/src-tauri/src/router.rs` is ~2.5k lines of `if/else` branches covering every desktop command, vault sync, reroll, and history feature. There are no seams for testing or reuse, and state mutations (`editor_session`, DB writes, vault edits) are interwoven with prompt text and telemetry logging.
- Autocomplete, history, and command execution all live in the same file, so a small change (e.g., tweaking `npc reroll`) risks regressions elsewhere.
- There’s no dependency injection: every branch shells directly into SQLite and the filesystem, making it impossible to mock for tests or to swap implementations (e.g., remote execution) later.

### What Needs to Change
- Decompose the router into modules grouped by domain (NPC, location, faction, history, setup) with dedicated service structs responsible for DB/vault coordination.
- Introduce a dispatcher layer that routes parsed commands to these modules (building on the Phase 1 registry) so execution code only deals with typed inputs, not raw strings.
- Wrap external resources (vault IO, SQLite pool, AI generation) behind traits injected into services to enable unit tests and future backends.

### Implementation Notes
- After Phase 1, each desktop handler can live under `desktop/src-tauri/src/commands/<domain>/`. Give each module a focused API (e.g., `NpcCommandService::generate(prompt)`), keeping router code as thin orchestration.
- Move DB queries out of the router into repository structs; inject `Arc<dyn NpcRepository>` etc. via `AppState`. Use `Mock` implementations in tests to validate command behavior without touching disk.
- Split autocomplete into smaller providers (command suggestions, entity search, location travel). Register them similarly to command handlers so UI logic isn’t special-casing editor modes.

### Refactor Checklist
- [ ] Create domain services and repositories for NPC/location/faction commands.
- [ ] Move router logic into per-command modules, wired through the common registry introduced in Phase 1.
- [ ] Abstract database and vault operations behind traits; provide prod + test implementations.
- [ ] Break autocomplete into composable providers; ensure Solid client only handles presentation.
- [ ] Add unit/integration tests per module to lock behavior before future changes.
