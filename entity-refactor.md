# Entity Domain Refactor Plan

> Purpose: Provide implementation details for agents to refactor entity domains so adding new entity types (for example `item`, `dungeon`, `quest`) requires minimal code changes and avoids cross-module duplication.

---

## 1. Problem Statement

The command and service architecture is now clean at a high level, but entity behavior is still duplicated across:

- `desktop/src-tauri/src/commands/npc_commands.rs`
- `desktop/src-tauri/src/commands/location_commands.rs`
- `desktop/src-tauri/src/commands/faction_commands.rs`
- `desktop/src-tauri/src/services/entity_admin.rs`
- `desktop/src-tauri/src/services/entity_reroll.rs`
- `desktop/src-tauri/src/services/suggestions.rs`
- `desktop/src-tauri/src/app_state.rs`

Current pain points:

1. Hardcoded field aliases and canonical field maps per entity
2. Hardcoded editor session slots (`npc_draft`, `location_draft`, `faction_draft`)
3. Repeated command subcommand flow (`show`, `rename`, `set`, `reroll`, `save`, `cancel`, `help`)
4. Large `match` branches for entity-specific details and card construction
5. High file-touch count for new entity onboarding

Goal: make entity growth mostly additive (schema + domain registration + specific implementation), not invasive edits across many existing files.

---

## 2. Refactor Outcomes (Definition of Success)

The refactor is successful when:

- Adding a new entity type does not require copy/pasting entire command/service files
- Field definitions (aliases, validation, rerollability, suggestion hints) are centralized per entity schema
- Generic handlers can execute common editor actions for all entity types
- Entity-specific behavior is encapsulated behind a domain interface
- `quest` (or another pilot entity) can be added with significantly fewer touched existing files than today

Target metrics:

- New entity requires <= 1 touch in central registry modules
- Most changes are in new files under `entities/` or `domains/`
- No new large branch chains in existing command or service modules

---

## 3. Non-Goals

- Do not change user-facing command UX (keep `npc`, `location`, `faction` roots)
- Do not rewrite all persistence to a generic JSON-only datastore
- Do not remove strong typing in `runebound-models`
- Do not collapse all entities into one giant dynamic map in frontend

---

## 4. High-Level Design

Introduce five concepts:

1. `EntityKind`
2. `EntityFieldSpec` and `EntitySchema`
3. Generic draft slot storage (`EditorSession`)
4. `EntityDomain` trait for domain operations
5. Domain registry for runtime dispatch

### 4.1 EntityKind

Create enum (new module suggested: `desktop/src-tauri/src/entities/kind.rs`):

```rust
pub enum EntityKind {
    Npc,
    Location,
    Faction,
    // future: Quest, Item, Dungeon
}
```

Include helpers:

- `as_str()`
- `command_root()` (for example `"npc"`)
- `display_name()`

### 4.2 EntityFieldSpec and EntitySchema

Create `desktop/src-tauri/src/entities/schema.rs` with:

- canonical field id
- aliases
- settable/rerollable flags
- value kind (text, enum, list, integer-like text)
- optional validator function
- optional suggestion hint category

Example shape:

```rust
pub struct EntityFieldSpec {
    pub canonical: &'static str,
    pub aliases: &'static [&'static str],
    pub settable: bool,
    pub rerollable: bool,
    pub value_kind: ValueKind,
}

pub struct EntitySchema {
    pub kind: EntityKind,
    pub fields: &'static [EntityFieldSpec],
}
```

Expose helpers:

- `canonical_field_for(kind, raw) -> Option<&'static str>`
- `settable_fields(kind) -> impl Iterator`
- `rerollable_fields(kind) -> impl Iterator`

### 4.3 Generic Draft Storage

Refactor `desktop/src-tauri/src/app_state.rs` editor session to use a map-backed model.

Current:

- `npc_draft`, `location_draft`, `faction_draft`
- mode enum with fixed variants

Target:

- `active_kind: Option<EntityKind>`
- typed draft slots in map-like wrapper

Implementation options:

1. Enum wrapper (recommended for safety):
   - `DraftEnvelope::Npc(NpcDraft)`, etc.
2. Trait object storage (not recommended initially)

Suggested API:

- `editor.set_active(kind, draft)`
- `editor.get(kind) -> Option<&DraftEnvelope>`
- `editor.clear(kind)`
- `editor.next_active_after_clear()`

### 4.4 EntityDomain Trait

Create `desktop/src-tauri/src/entities/domain.rs`:

```rust
pub trait EntityDomain: Send + Sync {
    fn kind(&self) -> EntityKind;
    fn schema(&self) -> &'static EntitySchema;

    async fn show_draft(&self, state: &AppState) -> Result<Option<CommandResponse>, String>;
    async fn rename(&self, value: &str, state: &AppState) -> Result<Option<CommandResponse>, String>;
    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> Result<Option<CommandResponse>, String>;
    async fn reroll_field(&self, field: &str, prompt: Option<String>, state: &AppState) -> Result<Option<CommandResponse>, String>;
    async fn save(&self, state: &AppState) -> Result<Option<CommandResponse>, String>;
    async fn cancel(&self, state: &AppState) -> Result<Option<CommandResponse>, String>;
    fn help_text(&self) -> String;
}
```

Note: Start with current command behavior wrapped in per-entity domain adapters. Genericization comes after parity.

### 4.5 Domain Registry

Create `desktop/src-tauri/src/entities/registry.rs`:

- `EntityDomainRegistry` keyed by `EntityKind`
- single startup builder that registers `NpcDomain`, `LocationDomain`, `FactionDomain`

Use registry in:

- generic entity editor command paths
- suggestion field generation
- common save/reroll/cancel flows in `system_commands.rs`

---

## 5. Execution Plan (Phased)

### Phase 1: Schema Extraction (Low Risk)

Deliverables:

1. Add `entities/schema.rs` and schema definitions for NPC/Location/Faction
2. Replace hardcoded field arrays in `services/suggestions.rs` with schema-driven field lists
3. Replace canonical field functions in command modules with schema lookups

Files to update:

- Add: `desktop/src-tauri/src/entities/mod.rs`
- Add: `desktop/src-tauri/src/entities/kind.rs`
- Add: `desktop/src-tauri/src/entities/schema.rs`
- Update: `desktop/src-tauri/src/services/suggestions.rs`
- Update: `desktop/src-tauri/src/commands/npc_commands.rs`
- Update: `desktop/src-tauri/src/commands/location_commands.rs`
- Update: `desktop/src-tauri/src/commands/faction_commands.rs`

Acceptance:

- All existing commands behave identically
- Field completion and validation messages still correct

### Phase 2: Session Refactor (Moderate Risk)

Deliverables:

1. Introduce `DraftEnvelope` and map-backed editor drafts
2. Add helper methods for set/get/clear/active-kind transitions
3. Migrate command modules to helper API without changing user-visible behavior

Files to update:

- `desktop/src-tauri/src/app_state.rs`
- command modules that read/write drafts
- `system_commands.rs` mode-aware logic

Acceptance:

- Mode transitions remain correct
- Save/cancel behavior unchanged

### Phase 3: Domain Trait + Adapters (Moderate/High Risk)

Deliverables:

1. Define `EntityDomain` trait
2. Build adapters for npc/location/faction using current internals
3. Create domain registry
4. Route generic flows (`show`, `set`, `reroll`, `save`, `cancel`) through domain dispatch where practical

Files to add/update:

- Add: `desktop/src-tauri/src/entities/domain.rs`
- Add: `desktop/src-tauri/src/entities/registry.rs`
- Add: `desktop/src-tauri/src/entities/domains/npc_domain.rs`
- Add: `desktop/src-tauri/src/entities/domains/location_domain.rs`
- Add: `desktop/src-tauri/src/entities/domains/faction_domain.rs`
- Update: `desktop/src-tauri/src/commands/system_commands.rs`
- Update: selected command modules to delegate into domain adapters

Acceptance:

- No behavior regressions in existing commands
- Reduced duplicate command branch logic

### Phase 4: Shared Entity Workflows (High Value)

Deliverables:

1. Consolidate repeated helper logic:
   - seed+rereoll prompt merge
   - unknown normalization wrappers
   - summary/event response scaffolding
2. Move entity-common logic into `entities/common.rs`
3. Expose shared command-layer helpers (response wrappers, reroll parsing, normalized list utilities) so CLI handlers reuse the same workflows as domains

Acceptance:

- Significant reduction in duplicate helper functions

### Phase 5: Pilot New Entity (Proof)

Deliverables:

1. Implement one pilot entity (`quest` recommended)
2. Use only new extension pathways:
   - schema
   - domain impl
   - registration
   - persistence/generation hooks
3. Record files touched and compare to legacy path

Acceptance:

- Pilot entity added with low invasive change count

---

## 6. Detailed Implementation Notes by Area

### 6.1 Commands

Current command roots should remain.

Near-term:

- keep `npc_commands.rs`, `location_commands.rs`, `faction_commands.rs`
- replace field mapping and repeated helper logic with shared schema/common helpers

Later:

- wrappers call generic domain executor utilities

### 6.2 Suggestions

Replace entity field suggestion hardcoded lists with:

- `settable_fields(kind)` for `<entity> set`
- `rerollable_fields(kind)` for `<entity> reroll`

Keep context-sensitive suggestions (like `npc travel to`) in domain-specific hooks.

### 6.3 Entity Admin and Resolution

Refactor wide optional structs over time:

- Start with existing `EntityDetails` for compatibility
- Add domain-specific details payload types behind trait methods
- Migrate `build_entity_card_doc` and `build_load_response` to domain-specific renderers

### 6.4 Persistence and Reroll Services

Do not attempt full generic DB model immediately.

First step:

- extract shared pipeline stages (validate -> normalize -> write vault -> upsert row -> upsert index)
- keep type-specific row creation in domain methods

### 6.5 Frontend

No immediate UI redesign required.

If new entities are added:

- extend `CommandClientEvent` variants in `runebound-models/src/events.rs`
- handle new events in `desktop/src/App.tsx`
- reuse `entity_card` block where possible

---

## 7. Agent Guardrails

1. Keep behavior parity at each phase; do not combine all phases in one PR
2. Add tests/checks for each phase before moving forward
3. Do not move command logic into `router.rs` or `main.rs`
4. Keep `command-specs` as command metadata source of truth
5. Prefer `runebound-models` contracts over ad-hoc duplicated types

---

## 8. Verification Checklist Per PR

- [ ] `make build` passes
- [ ] Existing command roots work (`npc`, `location`, `faction`)
- [ ] `set` field validation still correct
- [ ] `reroll` field validation still correct
- [ ] `save` and `cancel` behavior unchanged
- [ ] Autocomplete field suggestions still correct
- [ ] Clickable command refs still execute
- [ ] No new business logic added to `router.rs` or `main.rs`
- [ ] Documentation updated if extension points changed

---

## 9. Suggested PR Breakdown

1. `entities/schema` extraction and suggestion integration
2. `EditorSession` generic draft storage
3. `EntityDomain` trait + registry (adapters only)
4. Shared entity helper consolidation
5. Pilot entity (`quest`) using new framework

Each PR should include:

- problem statement
- files changed
- parity checks run
- follow-up items for next phase

---

## 10. Completion Criteria

Refactor is complete when all of the following are true:

- field definitions are schema-driven
- command modules are thin and reuse shared executor paths
- new entity onboarding is additive and registry-based
- pilot entity proves reduced blast radius
- docs (`docs/architecture.md`, `docs/feature-development.md`) reflect final pattern
