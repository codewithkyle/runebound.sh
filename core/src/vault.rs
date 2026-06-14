use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

const REQUIRED_TOP_LEVEL_DIRS: [&str; 7] = [
    "npcs",
    "locations",
    "items",
    "factions",
    ".trash/npcs",
    ".trash/locations",
    ".trash/factions",
];

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_root_exists(&self) -> Result<()> {
        if !self.root.exists() {
            bail!("vault path does not exist: {}", self.root.display());
        }
        if !self.root.is_dir() {
            bail!("vault path is not a directory: {}", self.root.display());
        }
        Ok(())
    }

    pub fn ensure_structure(&self) -> Result<()> {
        self.ensure_root_exists()?;

        for dir in REQUIRED_TOP_LEVEL_DIRS {
            let path = self.root.join(dir);
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create vault directory {}", path.display()))?;
        }

        Ok(())
    }

    pub fn read_relative(&self, relative: &Path) -> Result<String> {
        let full = self.resolve_relative(relative)?;
        fs::read_to_string(&full)
            .with_context(|| format!("failed to read vault file {}", full.display()))
    }

    pub fn write_relative(&self, relative: &Path, contents: &str) -> Result<()> {
        let full = self.resolve_relative(relative)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create parent directories for {}", full.display())
            })?;
        }

        fs::write(&full, contents)
            .with_context(|| format!("failed to write vault file {}", full.display()))
    }

    pub fn resolve_relative(&self, relative: &Path) -> Result<PathBuf> {
        if relative.is_absolute() {
            bail!("absolute paths are not allowed in vault operations");
        }

        for component in relative.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                Component::ParentDir => {
                    bail!("path traversal is not allowed: {}", relative.display())
                }
                Component::RootDir | Component::Prefix(_) => {
                    bail!("invalid path component in {}", relative.display())
                }
            }
        }

        Ok(self.root.join(relative))
    }
}

pub fn is_path_writable(dir: &Path) -> Result<()> {
    if !dir.exists() {
        bail!("path does not exist: {}", dir.display());
    }
    if !dir.is_dir() {
        bail!("path is not a directory: {}", dir.display());
    }

    let probe_path = dir.join(".dnd-assistant-write-check.tmp");
    fs::write(&probe_path, b"ok")
        .with_context(|| format!("path is not writable: {}", dir.display()))?;
    fs::remove_file(&probe_path).with_context(|| {
        format!(
            "failed to clean up write test file at {}",
            probe_path.display()
        )
    })?;

    Ok(())
}
