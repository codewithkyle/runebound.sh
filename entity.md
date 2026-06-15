# Entity Draft Session API

Phase 2 of the entity refactor replaces the legacy `EditorMode` + per-entity draft
slots with a generic, kind-aware editor session. This document summarizes the new
API so future phases (domains/registry/pilot entities) can build on the shared
concepts without re-reading the implementation.

## Terminology

- **EntityKind** – Enum (`Npc`, `Location`, `Faction`) in `entities/kind.rs` used to
  reference entity families in a type-safe way.
- **DraftEnvelope** – Enum defined in `app_state.rs` wrapping each concrete draft
  type (`NpcDraftSession`, etc.). Provides typed accessors (`as_npc`,
  `as_location_mut`, ... ) and reports its `EntityKind`.
- **EditorSession** – Struct stored behind `AppState::editor_session` mutex.
  Tracks a `HashMap<EntityKind, DraftEnvelope>` plus `active_kind: Option<EntityKind>`
  to preserve the previous “active editor” behavior.

## EditorSession Surface Area

```rust
pub struct EditorSession { /* fields hidden */ }

impl EditorSession {
    pub fn active_kind(&self) -> Option<EntityKind>;
    pub fn set_active_draft(&mut self, draft: DraftEnvelope);
    pub fn activate(&mut self, kind: EntityKind);
    pub fn clear_kind(&mut self, kind: EntityKind) -> Option<DraftEnvelope>;
    pub fn clear_all(&mut self);

    // Typed helpers for each entity
    pub fn get_npc(&self) -> Option<&NpcDraftSession>;
    pub fn get_npc_mut(&mut self) -> Option<&mut NpcDraftSession>;
    pub fn set_npc(&mut self, draft: NpcDraftSession);
    pub fn take_npc(&mut self) -> Option<NpcDraftSession>;
    // Location + faction variants follow the same pattern.
}
```

Key semantics:

1. **Single source of truth** – All draft access goes through the helpers above.
   No module reaches into fields like `editor.npc_draft` directly.
2. **Active kind** – `active_kind` mirrors the old `EditorMode`. Whenever a draft is
   set via `set_<entity>`, it becomes active. Clearing the active kind promotes the
   next available draft following deterministic priority (NPC → Location → Faction)
   so cancel/save flows stay consistent with the previous UX.
3. **Typed mutation** – `get_<entity>_mut` returns the concrete draft type, so
   command handlers (`npc set`, `location set`, etc.) can mutate in place without
   cloning until they need to emit events/responses.
4. **Bulk operations** – `clear_all()` wipes every draft and resets the active kind,
   which `save`/`delete` flows can call after writing to persistence.

## Usage Patterns

- **Creating drafts** (`create_commands.rs`, `entity_commands.rs::build_load_response`)
  ```rust
  let mut editor = state.editor_session.lock().await;
  editor.set_npc(new_draft.clone());
  editor.clear_kind(EntityKind::Location); // maintain legacy exclusivity
  ```

- **Mutating drafts** (`npc_set`, `location_set`, `faction_set`)
  ```rust
  let mut editor = state.editor_session.lock().await;
  let draft = editor
      .get_npc_mut()
      .ok_or_else(|| "no active npc draft ...".to_string())?;
  draft.name = value.to_string();
  let snapshot = draft.clone(); // for summary/event output
  editor.activate(EntityKind::Npc);
  ```

- **Clearing drafts** (`handle_cancel`, `create_*` when switching kinds)
  ```rust
  editor.take_location();           // removes and adjusts active_kind automatically
  editor.clear_kind(EntityKind::Npc);
  editor.clear_all();               // wipe everything (used after save)
  ```

- **Global commands** (`system_commands.rs`)
  ```rust
  match editor.active_kind() {
      Some(EntityKind::Npc) => npc_save(...),
      Some(EntityKind::Location) => location_save(...),
      Some(EntityKind::Faction) => faction_save(...),
      None => { /* no active draft */ }
  }
  ```

## Migration Notes

- Previous `EditorMode` enum and fields (`npc_draft`, etc.) have been removed. Any
  lingering references should route through the helpers above.
- `SuggestionService` now checks `editor.active_kind()` to decide which root
  commands to surface/omit and when to expose `npc travel` suggestions.
- When adding new entity kinds in later phases, extend `DraftEnvelope`, add typed
  helpers, and include the kind in `next_active_after`. No other module should need
  to change.

## Entity Domains & Registry

Phase 3 introduces a `EntityDomain` trait plus per-entity adapters housed under
`entities/domains/`. Each adapter encapsulates the logic that previously lived
inside the command modules (rename/set/reroll/save/cancel/show/help) and exposes
shared helpers such as `npc_summary_text` and `npc_event_from_draft`.

```rust
#[async_trait]
pub trait EntityDomain: Send + Sync {
    fn kind(&self) -> EntityKind;
    fn schema(&self) -> &'static EntitySchema;
    fn help_text(&self) -> String;

    async fn show_draft(&self, state: &AppState) -> DomainResult;
    async fn rename(&self, value: &str, state: &AppState) -> DomainResult;
    async fn set_field(&self, field: &str, value: &str, state: &AppState) -> DomainResult;
    async fn reroll_field(
        &self,
        field: &str,
        prompt: Option<String>,
        state: &AppState,
    ) -> DomainResult;
    async fn save(&self, state: &AppState) -> DomainResult;
    async fn cancel(&self, state: &AppState) -> DomainResult;
}
```

- `entities/registry.rs` provides `EntityDomainRegistry`, a `HashMap` keyed by
  `EntityKind`. `build_default_registry()` registers `NpcDomain`, `LocationDomain`,
  and `FactionDomain` at startup.
- `AppState` stores `Arc<EntityDomainRegistry>`; call `state.domains().domain(kind)`
  to clone the desired adapter.
- Command modules now only parse CLI-specific syntax and delegate to the domain:

  ```rust
  let domain = state.domains().domain(EntityKind::Npc).expect("npc domain");
  domain.set_field(field, value, state).await?;
  ```

- `system_commands::{handle_save, handle_cancel}` dispatch through the registry based
  on `editor.active_kind()`, ensuring persistence/cleanup logic lives in the adapters.
- `entities/domains/mod.rs` re-exports summary/event helpers so other modules (create,
  system, entity) continue to reuse them without reaching into command-specific code.
- Special commands that fall outside the trait (e.g., `npc travel`, global
  `reroll`) remain local for now, but they also consume the shared helpers defined in
  the adapters.

With the domain layer in place, adding a new entity means implementing its schema,
writing an adapter in `entities/domains/`, and registering it. Command routers and
system flows stay untouched aside from parsing a new root keyword.
