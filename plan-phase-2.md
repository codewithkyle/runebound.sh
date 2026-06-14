## Phase 2 – Shared Draft/Data Models

### What’s Broken
- NPC/location/faction drafts are defined three times: `core/src/npc.rs`, `desktop/src-tauri/src/app_state.rs`, and `desktop/src/App.tsx`. Fields drift (e.g., camelCase vs snake_case), forcing brittle conversion logic and duplicated normalization helpers.
- Frontend logic (`App.tsx`) must guess which fields exist in `CommandClientEvent` payloads and how to render them. Any backend change risks runtime errors because there's no shared schema.
- Markdown output parsing is doing domain work (building entity cards) because the frontend can't rely on structured docs coming from the backend.

### What Needs to Change
- Define canonical Rust structs for drafts and events in a crate that both core and desktop share, and generate TypeScript bindings so Solid consumes the same schema.
- Emit rich `OutputDoc` structures (already defined in `core/src/output.rs`) instead of free-form text wherever possible so the frontend stops re-parsing Markdown.
- Provide utility functions for normalization (e.g., `normalize_unknown`, slug helpers) inside the shared crate rather than reimplementing them in TS.

### Implementation Notes
- Extract `CommandClientEvent`, `NpcDraft`, `LocationDraft`, `FactionDraft`, and related helpers into a `runebound-models` crate. Re-export from both `dnd_core` and `desktop/src-tauri`.
- Use `ts-rs`, `specta`, or a JSON schema generator to emit `.d.ts` files consumed by `desktop/src` so Solid components have typed imports instead of hand-written interfaces.
- Update Tauri `invoke` responses to serialize these shared structs; in Solid, replace local types with generated ones and drop redundant normalizers (e.g., `normalizeUnknown` now lives in shared utils or is done server-side).

### Refactor Checklist
- [x] Create shared models crate with drafts, events, and normalization helpers.
- [x] Wire TypeScript codegen to emit `.d.ts` files during build.
- [x] Update core crate to use shared types (re-exports from `runebound-models`).
- [x] Update desktop Tauri backend to use shared types (type aliases in `app_state.rs`).
- [x] Remove duplicate TS interfaces from `App.tsx` (now imports from `generated/models.ts`).
- [x] Server-side normalization: move `normalize_unknown` logic from TS to Rust before serializing events.
- [x] Ensure `OutputDoc` usage replaces Markdown parsing for drafts; keep parser only for truly free-form output.

### Implementation Status (2026-06-14)

**Completed:**
- Created `runebound-models/` crate with:
  - `drafts.rs`: `NpcDraft`, `LocationDraft`, `FactionDraft`, frontmatter types
  - `events.rs`: `CommandClientEvent`, `CommandResponse`, `OutputSegment`
  - `output.rs`: `OutputDoc`, `OutputBlock`, `InlineNode`, `StatusTone`, `SpinnerState`
  - `utils.rs`: `normalize_unknown_text`, `normalize_unknown_list`, `slugify`, `make_entity_id`, etc.
  - Entity card builders: `npc_entity_card()`, `location_entity_card()`, `faction_entity_card()`
- TypeScript generation via `build.rs` script → `desktop/src/generated/models.ts`
- Core crate re-exports shared types from `runebound-models`
- Desktop Tauri backend uses shared types via type aliases
- Frontend imports types from generated `models.ts`
- **Server-side normalization**: `router.rs` now applies normalization before emitting events
- **OutputDoc for entity cards**: New `Load*DraftWithCard` variants include pre-built entity card

**Remaining:**
- None - all Phase 2 tasks complete

**Files Modified:**
- `runebound-models/src/events.rs` (added WithCard variants, entity card builder helpers)
- `runebound-models/src/drafts.rs` (added `npc_entity_card`, `location_entity_card`, `faction_entity_card`)
- `runebound-models/build.rs` (updated TS codegen for WithCard variants)
- `core/src/command.rs` (re-exports from runebound_models)
- `core/src/output.rs` (re-exports from runebound_models)
- `desktop/src-tauri/src/router.rs` (uses shared CommandClientEvent, applies normalization, emits WithCard variants)
- `desktop/src/App.tsx` (handles WithCard variants, uses pre-built entity_card, removed redundant TS normalizers)

**Generated Output:**
- `desktop/src/generated/models.ts` (auto-generated on `cargo build -p runebound-models`)