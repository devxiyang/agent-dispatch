use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{DispatchError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchConfig {
    pub default: String,
    #[serde(default)]
    pub backends: BTreeMap<String, BackendConfig>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
    #[serde(default)]
    pub aliases: BTreeMap<String, AliasConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub backend: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasConfig {
    pub model: String,
    pub prompt: Option<String>,
}

impl DispatchConfig {
    pub fn dispatch_home() -> PathBuf {
        home_dir().join(".dispatch")
    }

    pub fn config_path() -> PathBuf {
        Self::dispatch_home().join("config.yaml")
    }

    pub fn runtime_dir() -> PathBuf {
        Self::dispatch_home().join("runtime")
    }

    pub fn session_storage_root() -> PathBuf {
        Self::runtime_dir().join("sessions")
    }

    pub fn load() -> Result<Self> {
        Self::load_from_path(Self::config_path())
    }

    pub fn load_from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let raw = fs::read_to_string(&path).map_err(|source| DispatchError::Io {
            path: path.clone(),
            source,
        })?;
        serde_yaml::from_str(&raw).map_err(DispatchError::SerdeYaml)
    }

    pub fn load_if_exists() -> Result<Option<Self>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }
        Self::load_from_path(path).map(Some)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(Self::config_path())
    }

    pub fn save_to_path(&self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| DispatchError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let raw = serde_yaml::to_string(self).map_err(DispatchError::SerdeYaml)?;
        fs::write(&path, raw).map_err(|source| DispatchError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(())
    }

    pub fn set_default(&mut self, model_or_alias: String) {
        self.default = model_or_alias;
    }

    pub fn upsert_backend(&mut self, name: String, executable: String, args: Vec<String>) {
        self.backends
            .insert(name, BackendConfig { executable, args });
    }

    pub fn remove_backend(&mut self, name: &str) -> Option<BackendConfig> {
        self.backends.remove(name)
    }

    pub fn upsert_model(&mut self, name: String, backend: String, model: Option<String>) {
        self.models.insert(name, ModelConfig { backend, model });
    }

    pub fn remove_model(&mut self, name: &str) -> Option<ModelConfig> {
        self.models.remove(name)
    }

    pub fn upsert_alias(&mut self, name: String, model: String, prompt: Option<String>) {
        self.aliases.insert(name, AliasConfig { model, prompt });
    }

    pub fn remove_alias(&mut self, name: &str) -> Option<AliasConfig> {
        self.aliases.remove(name)
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(".").to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::DispatchConfig;

    #[test]
    fn roundtrips_config_and_supports_mutation_helpers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.yaml");

        let mut config = DispatchConfig {
            default: "pi-default".into(),
            backends: Default::default(),
            models: Default::default(),
            aliases: Default::default(),
        };
        config.upsert_backend(
            "pi".into(),
            "pi".into(),
            vec!["--session".into(), "{session_file}".into()],
        );
        config.upsert_model("pi-default".into(), "pi".into(), None);
        config.upsert_alias(
            "reviewer".into(),
            "pi-default".into(),
            Some("find risks".into()),
        );
        config.set_default("reviewer".into());
        config.save_to_path(&path).unwrap();

        let loaded = DispatchConfig::load_from_path(&path).unwrap();
        assert_eq!(loaded.default, "reviewer");
        assert_eq!(loaded.backends["pi"].executable, "pi");
        assert_eq!(loaded.models["pi-default"].backend, "pi");
        assert_eq!(
            loaded.aliases["reviewer"].prompt.as_deref(),
            Some("find risks")
        );
    }

    #[test]
    fn removes_config_entries() {
        let mut config = DispatchConfig {
            default: "pi-default".into(),
            backends: Default::default(),
            models: Default::default(),
            aliases: Default::default(),
        };
        config.upsert_backend("pi".into(), "pi".into(), vec![]);
        config.upsert_model("pi-default".into(), "pi".into(), None);
        config.upsert_alias("reviewer".into(), "pi-default".into(), None);

        assert!(config.remove_backend("pi").is_some());
        assert!(config.remove_model("pi-default").is_some());
        assert!(config.remove_alias("reviewer").is_some());
    }
}
