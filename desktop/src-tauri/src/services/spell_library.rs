//! Import + projection orchestration for the spell library.
//!
//! Mirrors [`crate::services::vault_sync`] for spells: the canonical TOML spell
//! store ([`CardStore<Spell>`]) is the source of truth, the SQLite `spells` table is
//! a rebuildable search projection. Import is a full replace (clear + repopulate both
//! layers); boot re-projects the store into the DB so a deleted `app.db` self-heals.

use std::path::Path;

use dnd_core::card_store::CardStore;
use dnd_core::db::SpellRow;
use dnd_core::npc::now_timestamp;
use dnd_core::spell_import::import_spells_from_dir;
use runebound_models::spells::Spell;

use crate::app_state::AppState;

pub struct SpellLibraryService;

impl SpellLibraryService {
    /// Import spells from `dir` (a 5etools repo root or its `data/spells` dir):
    /// parse + convert, replace the canonical TOML store, then project into the
    /// search DB. Returns the number of spells imported.
    pub async fn import_from_dir(&self, dir: &Path, state: &AppState) -> Result<usize, String> {
        let dir = dir.to_path_buf();
        // Parse + convert is blocking file IO + JSON; keep it off the async runtime.
        let spells = tokio::task::spawn_blocking(move || import_spells_from_dir(&dir))
            .await
            .map_err(|err| err.to_string())?
            .map_err(|err| format!("{err:#}"))?;

        // Replace the canonical TOML store (also blocking file IO).
        let store_spells = spells.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let store = CardStore::<Spell>::new().map_err(|err| err.to_string())?;
            store.clear().map_err(|err| err.to_string())?;
            for spell in &store_spells {
                store.save(spell).map_err(|err| err.to_string())?;
            }
            Ok(())
        })
        .await
        .map_err(|err| err.to_string())??;

        self.replace_db(&spells, state).await?;
        Ok(spells.len())
    }

    /// Re-project the canonical TOML store into the search DB. Called at boot so a
    /// rebuilt/empty `app.db` recovers the library from the store. No-op when the
    /// store is empty or the DB already matches it (the common case).
    pub async fn project_store_into_db(&self, state: &AppState) -> Result<(), String> {
        let spells = tokio::task::spawn_blocking(|| -> Result<Vec<Spell>, String> {
            let store = CardStore::<Spell>::new().map_err(|err| err.to_string())?;
            store.list().map_err(|err| err.to_string())
        })
        .await
        .map_err(|err| err.to_string())??;

        if spells.is_empty() {
            return Ok(());
        }
        let db_count = state.spell_repo().count(state.database().as_ref()).await?;
        if db_count as usize == spells.len() {
            return Ok(()); // already projected
        }
        self.replace_db(&spells, state).await
    }

    /// Replace every row in the `spells` table with `spells`, in one transaction.
    async fn replace_db(&self, spells: &[Spell], state: &AppState) -> Result<(), String> {
        let database = state.database();
        let repo = state.spell_repo();
        let timestamp = now_timestamp();
        let mut tx = database.begin().await.map_err(|err| err.to_string())?;
        repo.clear_tx(&mut tx).await?;
        for spell in spells {
            repo.upsert_tx(&mut tx, &spell_row(spell, &timestamp))
                .await?;
        }
        tx.commit().await.map_err(|err| err.to_string())?;
        Ok(())
    }
}

/// Project the searchable columns of a [`Spell`] into a [`SpellRow`]; the full card
/// stays in the TOML store.
fn spell_row(spell: &Spell, timestamp: &str) -> SpellRow {
    SpellRow {
        id: spell.slug.clone(),
        slug: spell.slug.clone(),
        name: spell.name.clone(),
        level: i64::from(spell.level),
        school: spell.school.clone(),
        source: spell.source.clone(),
        ritual: spell.ritual,
        concentration: spell.concentration,
        created_at: timestamp.to_string(),
        updated_at: timestamp.to_string(),
    }
}
