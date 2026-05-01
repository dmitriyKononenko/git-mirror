// config.rs — Persistent configuration via TOML file

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Config {
    pub source_repo: Option<String>,
    pub destinations: Vec<Destination>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Destination {
    pub label: String,
    pub path: String,
}

impl Config {
    pub fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("git-mirror").join("config.toml")
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path())
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    /// Load from an explicit path — used directly in tests so we never touch
    /// the real OS config directory during a test run.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let contents = std::fs::read_to_string(path).context("Failed to read config file")?;
        toml::from_str(&contents).context("Failed to parse config file")
    }

    /// Save to an explicit path — mirrors `load_from` for the same reason.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(path, contents).context("Failed to write config file")
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_config() -> Config {
        Config {
            source_repo: Some("/repo/my-app".to_string()),
            destinations: vec![
                Destination { label: "Server 1".into(), path: "/deploy/s1".into() },
                Destination { label: "Server 2".into(), path: "/deploy/s2".into() },
            ],
        }
    }

    // Serialization must round-trip losslessly — if this fails, saved configs
    // would silently drop data on the next load.
    #[test]
    fn test_config_serialization_roundtrip() {
        let config = sample_config();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, loaded);
    }

    // A missing file must not error — it should silently return defaults.
    // This is the "first run" case.
    #[test]
    fn test_load_from_missing_file_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.toml");
        let config = Config::load_from(&path).unwrap();
        assert_eq!(config, Config::default());
        assert!(config.source_repo.is_none());
        assert!(config.destinations.is_empty());
    }

    // Save then load must produce an identical struct.
    #[test]
    fn test_save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let original = sample_config();
        original.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(original, loaded);
    }

    // Save must create intermediate directories automatically.
    #[test]
    fn test_save_creates_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let deep_path = tmp.path().join("a").join("b").join("c").join("config.toml");
        let config = sample_config();
        assert!(config.save_to(&deep_path).is_ok());
        assert!(deep_path.exists());
    }

    #[test]
    fn test_default_config_is_empty() {
        let c = Config::default();
        assert!(c.source_repo.is_none());
        assert!(c.destinations.is_empty());
    }
}
