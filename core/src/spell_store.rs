//! Canonical TOML store for imported spell cards, mirroring [`crate::entity_store`].
//!
//! Each spell is one `spells/<slug>.toml` file holding the full render-ready
//! [`Spell`] (the card payload). The SQLite `spells` table is a rebuildable
//! projection of this store; a lookup searches the DB for the slug, then loads the
//! card from here. Re-import is a full replace ([`SpellStore::clear`] then save).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use runebound_models::spells::Spell;

use crate::config::config_paths;

pub struct SpellStore {
    root: PathBuf,
}

impl SpellStore {
    pub fn new() -> Result<Self> {
        let paths = config_paths()?;
        let store = Self::with_root(paths.spells);
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
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create spell store at {}", self.root.display()))?;
        Ok(())
    }

    fn path_for(&self, slug: &str) -> PathBuf {
        self.root.join(format!("{slug}.toml"))
    }

    pub fn save_spell(&self, spell: &Spell) -> Result<PathBuf> {
        self.ensure_dir()?;
        let path = self.path_for(&spell.slug);
        let content = toml::to_string_pretty(spell)
            .with_context(|| format!("failed to encode spell {}", spell.slug))?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write spell file {}", path.display()))?;
        Ok(path)
    }

    pub fn load_spell(&self, slug: &str) -> Result<Option<Spell>> {
        let path = self.path_for(slug);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read spell file {}", path.display()))?;
        let spell = toml::from_str(&content)
            .with_context(|| format!("failed to parse spell file {}", path.display()))?;
        Ok(Some(spell))
    }

    pub fn list_spells(&self) -> Result<Vec<Spell>> {
        let mut spells = Vec::new();
        if !self.root.exists() {
            return Ok(spells);
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read spell store {}", self.root.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read spell file {}", path.display()))?;
            let spell = toml::from_str(&content)
                .with_context(|| format!("failed to parse spell file {}", path.display()))?;
            spells.push(spell);
        }
        Ok(spells)
    }

    /// Remove every stored spell card — the first half of an idempotent re-import.
    pub fn clear(&self) -> Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read spell store {}", self.root.display()))?
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
    use runebound_models::spells::SpellBlock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("spell_store_{}_{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn sample(slug: &str, name: &str) -> Spell {
        Spell {
            slug: slug.to_string(),
            name: name.to_string(),
            source: "XPHB".to_string(),
            level: 3,
            school: "Evocation".to_string(),
            casting_time: "1 Action".to_string(),
            range: "150 feet".to_string(),
            components: "V, S".to_string(),
            duration: "Instantaneous".to_string(),
            ritual: false,
            concentration: false,
            classes: Vec::new(),
            description: vec![SpellBlock::Text {
                text: "boom".to_string(),
            }],
            higher_levels: None,
        }
    }

    #[test]
    fn save_load_round_trips() {
        let store = SpellStore::with_root(temp_root());
        store.save_spell(&sample("fireball", "Fireball")).unwrap();
        let loaded = store.load_spell("fireball").unwrap().expect("present");
        assert_eq!(loaded.name, "Fireball");
        assert_eq!(loaded.level, 3);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn missing_spell_is_none() {
        let store = SpellStore::with_root(temp_root());
        store.ensure_dir().unwrap();
        assert!(store.load_spell("nope").unwrap().is_none());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn clear_then_list_is_empty() {
        let store = SpellStore::with_root(temp_root());
        store.save_spell(&sample("fireball", "Fireball")).unwrap();
        store.save_spell(&sample("light", "Light")).unwrap();
        assert_eq!(store.list_spells().unwrap().len(), 2);
        store.clear().unwrap();
        assert_eq!(store.list_spells().unwrap().len(), 0);
        let _ = fs::remove_dir_all(store.root());
    }
}
