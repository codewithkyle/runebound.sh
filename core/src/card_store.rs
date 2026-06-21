//! Canonical TOML store for an imported reference library, mirroring
//! [`crate::entity_store`]'s generic helpers.
//!
//! Each card is one `<root>/<slug>.toml` file holding the full render-ready payload
//! (a [`Spell`], a [`Monster`], …). The matching SQLite search table is a rebuildable
//! projection of this store; a lookup searches the DB for the slug, then loads the
//! card from here. Re-import is a full replace ([`CardStore::clear`] then save).
//!
//! [`Spell`]: runebound_models::spells::Spell
//! [`Monster`]: runebound_models::monsters::Monster

use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::{ConfigPaths, config_paths};

/// A record stored as one `<root>/<slug>.toml` file in a reference library.
/// `core` owns the trait, so it may impl it for the `runebound-models` payload
/// types (the trait is local — the orphan rule is satisfied).
pub trait Card: Serialize + DeserializeOwned + Sized {
    /// Human noun for error context ("spell", "monster").
    const NOUN: &'static str;
    /// Stable kebab-case primary key.
    fn slug(&self) -> &str;
    /// Where this card kind's files live under the config dir.
    fn store_root(paths: &ConfigPaths) -> PathBuf;
}

/// Canonical TOML store for an imported reference library. The SQLite search table
/// is a rebuildable projection of this store (see the `*LibraryService`s).
pub struct CardStore<T: Card> {
    root: PathBuf,
    _marker: PhantomData<T>,
}

impl<T: Card> CardStore<T> {
    pub fn new() -> Result<Self> {
        let store = Self::with_root(T::store_root(&config_paths()?));
        store.ensure_dir()?;
        Ok(store)
    }

    /// Construct a store rooted at an explicit directory (tests; never the default
    /// config path).
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            _marker: PhantomData,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create {} store at {}",
                T::NOUN,
                self.root.display()
            )
        })?;
        Ok(())
    }

    fn path_for(&self, slug: &str) -> PathBuf {
        self.root.join(format!("{slug}.toml"))
    }

    pub fn save(&self, card: &T) -> Result<PathBuf> {
        self.ensure_dir()?;
        let path = self.path_for(card.slug());
        let content = toml::to_string_pretty(card)
            .with_context(|| format!("failed to encode {} {}", T::NOUN, card.slug()))?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write {} file {}", T::NOUN, path.display()))?;
        Ok(path)
    }

    pub fn load(&self, slug: &str) -> Result<Option<T>> {
        let path = self.path_for(slug);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {} file {}", T::NOUN, path.display()))?;
        let card = toml::from_str(&content)
            .with_context(|| format!("failed to parse {} file {}", T::NOUN, path.display()))?;
        Ok(Some(card))
    }

    pub fn list(&self) -> Result<Vec<T>> {
        let mut cards = Vec::new();
        if !self.root.exists() {
            return Ok(cards);
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read {} store {}", T::NOUN, self.root.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {} file {}", T::NOUN, path.display()))?;
            let card = toml::from_str(&content)
                .with_context(|| format!("failed to parse {} file {}", T::NOUN, path.display()))?;
            cards.push(card);
        }
        Ok(cards)
    }

    /// Remove every stored card — the first half of an idempotent re-import.
    pub fn clear(&self) -> Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read {} store {}", T::NOUN, self.root.display()))?
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

impl Card for runebound_models::spells::Spell {
    const NOUN: &'static str = "spell";
    fn slug(&self) -> &str {
        &self.slug
    }
    fn store_root(paths: &ConfigPaths) -> PathBuf {
        paths.spells.clone()
    }
}

impl Card for runebound_models::monsters::Monster {
    const NOUN: &'static str = "monster";
    fn slug(&self) -> &str {
        &self.slug
    }
    fn store_root(paths: &ConfigPaths) -> PathBuf {
        paths.monsters.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runebound_models::monsters::{Monster, Span, StatAbility, StatBlock, StatSection};
    use runebound_models::spells::{Spell, SpellBlock};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("card_store_{label}_{}_{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn spell(slug: &str, name: &str) -> Spell {
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
                spans: vec![Span::Text {
                    text: "boom".to_string(),
                }],
            }],
            higher_levels: None,
        }
    }

    fn monster(slug: &str, name: &str) -> Monster {
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
                        spans: vec![Span::Text {
                            text: "Melee Attack Roll: +4.".to_string(),
                        }],
                    }],
                }],
            }],
            lore: Vec::new(),
        }
    }

    #[test]
    fn save_load_round_trips() {
        let store = CardStore::<Spell>::with_root(temp_root("spell"));
        store.save(&spell("fireball", "Fireball")).unwrap();
        let loaded = store.load("fireball").unwrap().expect("present");
        assert_eq!(loaded.name, "Fireball");
        assert_eq!(loaded.level, 3);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn missing_card_is_none() {
        let store = CardStore::<Spell>::with_root(temp_root("spell"));
        store.ensure_dir().unwrap();
        assert!(store.load("nope").unwrap().is_none());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn clear_then_list_is_empty() {
        let store = CardStore::<Spell>::with_root(temp_root("spell"));
        store.save(&spell("fireball", "Fireball")).unwrap();
        store.save(&spell("light", "Light")).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
        store.clear().unwrap();
        assert_eq!(store.list().unwrap().len(), 0);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn monster_round_trips_through_the_same_generic() {
        // Proves the trait wiring works for a second card kind (nested spans + abilities).
        let store = CardStore::<Monster>::with_root(temp_root("monster"));
        store
            .save(&monster("goblin-warrior", "Goblin Warrior"))
            .unwrap();
        let loaded = store.load("goblin-warrior").unwrap().expect("present");
        assert_eq!(loaded.name, "Goblin Warrior");
        assert_eq!(loaded.abilities, [8, 15, 10, 10, 8, 8]);
        assert_eq!(
            loaded.sections[0].abilities[0].name.as_deref(),
            Some("Scimitar")
        );
        let _ = fs::remove_dir_all(store.root());
    }
}
