//! Import + projection orchestration for the monster library.
//!
//! Mirrors [`crate::services::spell_library`] for monsters: the canonical TOML
//! monster store ([`CardStore<Monster>`]) is the source of truth, the SQLite
//! `monsters` table is a rebuildable search projection. Import is a full replace (clear +
//! repopulate both layers); boot re-projects the store into the DB so a deleted
//! `app.db` self-heals.

use std::path::Path;

use dnd_core::card_store::CardStore;
use dnd_core::db::MonsterRow;
use dnd_core::monster_import::{ImportSummary, cr_token_to_sort, import_monsters_from_dir};
use dnd_core::npc::now_timestamp;
use runebound_models::monsters::Monster;

use crate::app_state::AppState;

pub struct BestiaryLibraryService;

impl BestiaryLibraryService {
    /// Import monsters from `dir` (a 5etools repo root or its `data/bestiary` dir):
    /// parse + convert, replace the canonical TOML store, then project into the
    /// search DB. Returns the import summary (count + skipped `_copy` variants).
    pub async fn import_from_dir(
        &self,
        dir: &Path,
        state: &AppState,
    ) -> Result<ImportSummary, String> {
        let dir = dir.to_path_buf();
        // Parse + convert is blocking file IO + JSON; keep it off the async runtime.
        let summary = tokio::task::spawn_blocking(move || import_monsters_from_dir(&dir))
            .await
            .map_err(|err| err.to_string())?
            .map_err(|err| format!("{err:#}"))?;

        // Replace the canonical TOML store (also blocking file IO).
        let store_monsters = summary.monsters.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let store = CardStore::<Monster>::new().map_err(|err| err.to_string())?;
            store.clear().map_err(|err| err.to_string())?;
            for monster in &store_monsters {
                store.save(monster).map_err(|err| err.to_string())?;
            }
            Ok(())
        })
        .await
        .map_err(|err| err.to_string())??;

        self.replace_db(&summary.monsters, state).await?;
        Ok(summary)
    }

    /// Re-project the canonical TOML store into the search DB. Called at boot so a
    /// rebuilt/empty `app.db` recovers the library from the store. No-op when the
    /// store is empty or the DB already matches it (the common case).
    pub async fn project_store_into_db(&self, state: &AppState) -> Result<(), String> {
        let monsters = tokio::task::spawn_blocking(|| -> Result<Vec<Monster>, String> {
            let store = CardStore::<Monster>::new().map_err(|err| err.to_string())?;
            store.list().map_err(|err| err.to_string())
        })
        .await
        .map_err(|err| err.to_string())??;

        if monsters.is_empty() {
            return Ok(());
        }
        let db_count = state
            .monster_repo()
            .count(state.database().as_ref())
            .await?;
        if db_count as usize == monsters.len() {
            return Ok(()); // already projected
        }
        self.replace_db(&monsters, state).await
    }

    /// Replace every row in the `monsters` table with `monsters`, in one transaction.
    async fn replace_db(&self, monsters: &[Monster], state: &AppState) -> Result<(), String> {
        let database = state.database();
        let repo = state.monster_repo();
        let timestamp = now_timestamp();
        let mut tx = database.begin().await.map_err(|err| err.to_string())?;
        repo.clear_tx(&mut tx).await?;
        for monster in monsters {
            repo.upsert_tx(&mut tx, &monster_row(monster, &timestamp))
                .await?;
        }
        tx.commit().await.map_err(|err| err.to_string())?;
        Ok(())
    }
}

/// Project the searchable columns of a [`Monster`] into a [`MonsterRow`]; the full
/// card stays in the TOML store.
fn monster_row(monster: &Monster, timestamp: &str) -> MonsterRow {
    MonsterRow {
        id: monster.slug.clone(),
        slug: monster.slug.clone(),
        name: monster.name.clone(),
        cr: monster.cr.clone(),
        cr_sort: cr_sort(&monster.cr),
        creature_type: monster.creature_type.clone(),
        size: monster.size.clone(),
        source: monster.source.clone(),
        created_at: timestamp.to_string(),
        updated_at: timestamp.to_string(),
    }
}

/// Numeric CR for ordering, parsed from the leading token of the display string
/// ("1/4 (XP 50; PB +2)" → 0.25). The display `cr` always begins with the raw CR;
/// the fraction table itself lives once in [`cr_token_to_sort`].
fn cr_sort(display_cr: &str) -> f64 {
    cr_token_to_sort(display_cr.split_whitespace().next().unwrap_or("")).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::cr_sort;

    #[test]
    fn cr_sort_parses_the_leading_token() {
        assert_eq!(cr_sort("1/4 (XP 50; PB +2)"), 0.25);
        assert_eq!(cr_sort("17 (XP 18,000; PB +6)"), 17.0);
        assert_eq!(cr_sort("1/8"), 0.125);
        assert_eq!(cr_sort(""), 0.0);
    }
}
