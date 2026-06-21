//! Canonical TOML store for imported monster cards, mirroring [`crate::spell_store`].
//!
//! Each monster is one `monsters/<slug>.toml` file holding the full render-ready
//! [`Monster`] (the card payload). The SQLite `monsters` table is a rebuildable
//! projection of this store; a lookup searches the DB for the slug, then loads the
//! card from here. Re-import is a full replace ([`MonsterStore::clear`] then save).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use runebound_models::monsters::Monster;

use crate::config::config_paths;

pub struct MonsterStore {
    root: PathBuf,
}

impl MonsterStore {
    pub fn new() -> Result<Self> {
        let paths = config_paths()?;
        let store = Self::with_root(paths.monsters);
        store.ensure_dir()?;
        Ok(store)
    }

    /// Construct a store rooted at an explicit directory (tests; never the default
    /// config path).
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!("failed to create monster store at {}", self.root.display())
        })?;
        Ok(())
    }

    fn path_for(&self, slug: &str) -> PathBuf {
        self.root.join(format!("{slug}.toml"))
    }

    pub fn save_monster(&self, monster: &Monster) -> Result<PathBuf> {
        self.ensure_dir()?;
        let path = self.path_for(&monster.slug);
        let content = toml::to_string_pretty(monster)
            .with_context(|| format!("failed to encode monster {}", monster.slug))?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write monster file {}", path.display()))?;
        Ok(path)
    }

    pub fn load_monster(&self, slug: &str) -> Result<Option<Monster>> {
        let path = self.path_for(slug);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read monster file {}", path.display()))?;
        let monster = toml::from_str(&content)
            .with_context(|| format!("failed to parse monster file {}", path.display()))?;
        Ok(Some(monster))
    }

    pub fn list_monsters(&self) -> Result<Vec<Monster>> {
        let mut monsters = Vec::new();
        if !self.root.exists() {
            return Ok(monsters);
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read monster store {}", self.root.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read monster file {}", path.display()))?;
            let monster = toml::from_str(&content)
                .with_context(|| format!("failed to parse monster file {}", path.display()))?;
            monsters.push(monster);
        }
        Ok(monsters)
    }

    /// Remove every stored monster card — the first half of an idempotent re-import.
    pub fn clear(&self) -> Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read monster store {}", self.root.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runebound_models::monsters::{StatAbility, StatBlock, StatSection};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("monster_store_{}_{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn sample(slug: &str, name: &str) -> Monster {
        Monster {
            slug: slug.to_string(),
            name: name.to_string(),
            source: "XMM".to_string(),
            size: "Small".to_string(),
            creature_type: "Fey (Goblinoid)".to_string(),
            alignment: "Chaotic Neutral".to_string(),
            ac: "15".to_string(),
            hp: "10 (3d6)".to_string(),
            speed: "30 ft.".to_string(),
            abilities: [8, 15, 10, 10, 8, 8],
            saves: String::new(),
            skills: "Stealth +6".to_string(),
            damage_resistances: String::new(),
            damage_immunities: String::new(),
            damage_vulnerabilities: String::new(),
            condition_immunities: String::new(),
            senses: "Passive Perception 9".to_string(),
            languages: "Common, Goblin".to_string(),
            cr: "1/4 (XP 50; PB +2)".to_string(),
            gear: "scimitar".to_string(),
            sections: vec![StatSection {
                title: "Actions".to_string(),
                intro: Vec::new(),
                abilities: vec![StatAbility {
                    name: Some("Scimitar".to_string()),
                    body: vec![StatBlock::Text {
                        text: "Melee Attack Roll: +4.".to_string(),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn save_load_round_trips() {
        let store = MonsterStore::with_root(temp_root());
        store
            .save_monster(&sample("goblin-warrior", "Goblin Warrior"))
            .unwrap();
        let loaded = store
            .load_monster("goblin-warrior")
            .unwrap()
            .expect("present");
        assert_eq!(loaded.name, "Goblin Warrior");
        assert_eq!(loaded.abilities, [8, 15, 10, 10, 8, 8]);
        assert_eq!(
            loaded.sections[0].abilities[0].name.as_deref(),
            Some("Scimitar")
        );
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn missing_monster_is_none() {
        let store = MonsterStore::with_root(temp_root());
        store.ensure_dir().unwrap();
        assert!(store.load_monster("nope").unwrap().is_none());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn clear_then_list_is_empty() {
        let store = MonsterStore::with_root(temp_root());
        store
            .save_monster(&sample("goblin-warrior", "Goblin Warrior"))
            .unwrap();
        store.save_monster(&sample("lich", "Lich")).unwrap();
        assert_eq!(store.list_monsters().unwrap().len(), 2);
        store.clear().unwrap();
        assert_eq!(store.list_monsters().unwrap().len(), 0);
        let _ = fs::remove_dir_all(store.root());
    }
}
