## Phase 2 – Shared Draft/Data Models

### What’s Broken
- NPC/location/faction drafts are defined three times: `core/src/npc.rs`, `desktop/src-tauri/src/app_state.rs`, and `desktop/src/App.tsx`. Fields drift (e.g., camelCase vs snake_case), forcing brittle conversion logic and duplicated normalization helpers.
- Frontend logic (`App.tsx`) must guess which fields exist in `CommandClientEvent` payloads and how to render them. Any backend change risks runtime errors because there’s no shared schema.
- Markdown output parsing is doing domain work (building entity cards) because the frontend can’t rely on structured docs coming from the backend.

### What Needs to Change
- Define canonical Rust structs for drafts and events in a crate that both core and desktop share, and generate TypeScript bindings so Solid consumes the same schema.
- Emit rich `OutputDoc` structures (already defined in `core/src/output.rs`) instead of free-form text wherever possible so the frontend stops re-parsing Markdown.
- Provide utility functions for normalization (e.g., `normalize_unknown`, slug helpers) inside the shared crate rather than reimplementing them in TS.

### Implementation Notes
- Extract `CommandClientEvent`, `NpcDraft`, `LocationDraft`, `FactionDraft`, and related helpers into a `runebound-models` crate. Re-export from both `dnd_core` and `desktop/src-tauri`.
- Use `ts-rs`, `specta`, or a JSON schema generator to emit `.d.ts` files consumed by `desktop/src` so Solid components have typed imports instead of hand-written interfaces.
- Update Tauri `invoke` responses to serialize these shared structs; in Solid, replace local types with generated ones and drop redundant normalizers (e.g., `normalizeUnknown` now lives in shared utils or is done server-side).

### Refactor Checklist
- [ ] Create shared models crate with drafts, events, and normalization helpers.
- [ ] Wire specta/ts-rs (or similar) to emit TypeScript bindings during build.
- [ ] Update Tauri commands and core crate to use shared types exclusively.
- [ ] Remove duplicate TS interfaces and helper functions from `App.tsx` once bindings land.
- [ ] Ensure `OutputDoc` usage replaces Markdown parsing for drafts; keep parser only for truly free-form output.
