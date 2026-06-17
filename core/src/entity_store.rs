use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use runebound_models::{
    DungeonFrontmatter, EventFrontmatter, FactionFrontmatter, GodFrontmatter, ItemFrontmatter,
    LocationFrontmatter, NpcFrontmatter,
};

use crate::config::config_paths;

const NPC_DIR: &str = "npcs";
const LOCATION_DIR: &str = "locations";
const FACTION_DIR: &str = "factions";
const ITEM_DIR: &str = "items";
const EVENT_DIR: &str = "events";
const GOD_DIR: &str = "gods";
const DUNGEON_DIR: &str = "dungeons";

/// Every per-kind subdirectory of the entity store, in kind order. The single
/// source the store ensures + writes into, so the kind set can't drift.
const ENTITY_DIRS: [&str; 7] = [
    NPC_DIR,
    LOCATION_DIR,
    FACTION_DIR,
    ITEM_DIR,
    EVENT_DIR,
    GOD_DIR,
    DUNGEON_DIR,
];

pub struct EntityStore {
    root: PathBuf,
}

impl EntityStore {
    pub fn new(workspace_root: &Path) -> Result<Self> {
        let paths = config_paths(workspace_root)?;
        let root = paths.entities;
        let store = Self { root };
        store.ensure_dirs()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_dirs(&self) -> Result<()> {
        for dir in ENTITY_DIRS {
            let path = self.root.join(dir);
            fs::create_dir_all(&path).with_context(|| {
                format!("failed to create {dir} directory at {}", path.display())
            })?;
        }
        Ok(())
    }

    fn path_for(&self, kind_dir: &str, slug: &str) -> PathBuf {
        self.root.join(kind_dir).join(format!("{slug}.toml"))
    }

    fn save_entity<T: serde::Serialize>(
        &self,
        kind_dir: &str,
        slug: &str,
        data: &T,
    ) -> Result<PathBuf> {
        let path = self.path_for(kind_dir, slug);
        let parent = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root.clone());
        fs::create_dir_all(&parent).with_context(|| {
            format!("failed to create entity directory at {}", parent.display())
        })?;
        let content = toml::to_string_pretty(data)?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write entity file {}", path.display()))?;
        Ok(path)
    }

    fn load_entity<T: serde::de::DeserializeOwned>(
        &self,
        kind_dir: &str,
        slug: &str,
    ) -> Result<Option<T>> {
        let path = self.path_for(kind_dir, slug);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read entity file {}", path.display()))?;
        let parsed = toml::from_str(&content)?;
        Ok(Some(parsed))
    }

    fn delete_entity(&self, kind_dir: &str, slug: &str) -> Result<()> {
        let path = self.path_for(kind_dir, slug);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove entity file {}", path.display()))?;
        }
        Ok(())
    }

    fn list_entities<T: serde::de::DeserializeOwned>(&self, kind_dir: &str) -> Result<Vec<T>> {
        let dir = self.root.join(kind_dir);
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read entity file {}", path.display()))?;
            let parsed = toml::from_str(&content)?;
            out.push(parsed);
        }
        Ok(out)
    }

    pub fn save_npc(&self, data: &NpcFrontmatter) -> Result<PathBuf> {
        self.save_entity(NPC_DIR, &data.slug, data)
    }

    pub fn load_npc(&self, slug: &str) -> Result<Option<NpcFrontmatter>> {
        self.load_entity(NPC_DIR, slug)
    }

    pub fn delete_npc(&self, slug: &str) -> Result<()> {
        self.delete_entity(NPC_DIR, slug)
    }

    pub fn list_npcs(&self) -> Result<Vec<NpcFrontmatter>> {
        self.list_entities(NPC_DIR)
    }

    pub fn save_location(&self, data: &LocationFrontmatter) -> Result<PathBuf> {
        self.save_entity(LOCATION_DIR, &data.slug, data)
    }

    pub fn load_location(&self, slug: &str) -> Result<Option<LocationFrontmatter>> {
        self.load_entity(LOCATION_DIR, slug)
    }

    pub fn delete_location(&self, slug: &str) -> Result<()> {
        self.delete_entity(LOCATION_DIR, slug)
    }

    pub fn list_locations(&self) -> Result<Vec<LocationFrontmatter>> {
        self.list_entities(LOCATION_DIR)
    }

    pub fn save_faction(&self, data: &FactionFrontmatter) -> Result<PathBuf> {
        self.save_entity(FACTION_DIR, &data.slug, data)
    }

    pub fn load_faction(&self, slug: &str) -> Result<Option<FactionFrontmatter>> {
        self.load_entity(FACTION_DIR, slug)
    }

    pub fn delete_faction(&self, slug: &str) -> Result<()> {
        self.delete_entity(FACTION_DIR, slug)
    }

    pub fn list_factions(&self) -> Result<Vec<FactionFrontmatter>> {
        self.list_entities(FACTION_DIR)
    }

    pub fn save_item(&self, data: &ItemFrontmatter) -> Result<PathBuf> {
        self.save_entity(ITEM_DIR, &data.slug, data)
    }

    pub fn load_item(&self, slug: &str) -> Result<Option<ItemFrontmatter>> {
        self.load_entity(ITEM_DIR, slug)
    }

    pub fn delete_item(&self, slug: &str) -> Result<()> {
        self.delete_entity(ITEM_DIR, slug)
    }

    pub fn list_items(&self) -> Result<Vec<ItemFrontmatter>> {
        self.list_entities(ITEM_DIR)
    }

    pub fn save_event(&self, data: &EventFrontmatter) -> Result<PathBuf> {
        self.save_entity(EVENT_DIR, &data.slug, data)
    }

    pub fn load_event(&self, slug: &str) -> Result<Option<EventFrontmatter>> {
        self.load_entity(EVENT_DIR, slug)
    }

    pub fn delete_event(&self, slug: &str) -> Result<()> {
        self.delete_entity(EVENT_DIR, slug)
    }

    pub fn list_events(&self) -> Result<Vec<EventFrontmatter>> {
        self.list_entities(EVENT_DIR)
    }

    pub fn save_god(&self, data: &GodFrontmatter) -> Result<PathBuf> {
        self.save_entity(GOD_DIR, &data.slug, data)
    }

    pub fn load_god(&self, slug: &str) -> Result<Option<GodFrontmatter>> {
        self.load_entity(GOD_DIR, slug)
    }

    pub fn delete_god(&self, slug: &str) -> Result<()> {
        self.delete_entity(GOD_DIR, slug)
    }

    pub fn list_gods(&self) -> Result<Vec<GodFrontmatter>> {
        self.list_entities(GOD_DIR)
    }

    pub fn save_dungeon(&self, data: &DungeonFrontmatter) -> Result<PathBuf> {
        self.save_entity(DUNGEON_DIR, &data.slug, data)
    }

    pub fn load_dungeon(&self, slug: &str) -> Result<Option<DungeonFrontmatter>> {
        self.load_entity(DUNGEON_DIR, slug)
    }

    pub fn delete_dungeon(&self, slug: &str) -> Result<()> {
        self.delete_entity(DUNGEON_DIR, slug)
    }

    pub fn list_dungeons(&self) -> Result<Vec<DungeonFrontmatter>> {
        self.list_entities(DUNGEON_DIR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_root() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("dnd_entity_store_{}_{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn ensure_dirs_creates_every_kind_directory() {
        let root = temp_root();
        let store = EntityStore { root: root.clone() };
        store.ensure_dirs().expect("ensure dirs");

        assert_eq!(ENTITY_DIRS.len(), 7, "every entity kind must have a dir");
        for dir in ENTITY_DIRS {
            assert!(
                root.join(dir).is_dir(),
                "expected entity store subdirectory `{dir}` to be created"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }
}
