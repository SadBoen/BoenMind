use serde::{Deserialize, Deserializer};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub data_dir: Option<PathBuf>,
    #[serde(deserialize_with = "deserialize_roots")]
    pub allowed_roots: Vec<PathBuf>,
    pub max_file_bytes: usize,
    pub max_files: usize,
    pub max_output_bytes: usize,
    pub default_timeout_ms: u64,
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub execution_enabled: bool,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub data_dir: PathBuf,
    pub allowed_roots: Vec<PathBuf>,
    pub max_file_bytes: usize,
    pub max_files: usize,
    pub max_output_bytes: usize,
    pub default_timeout_ms: u64,
    pub execution_enabled: bool,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            data_dir: None,
            allowed_roots: Vec::new(),
            max_file_bytes: 1_048_576,
            max_files: 5_000,
            max_output_bytes: 262_144,
            default_timeout_ms: 30_000,
            execution_enabled: false,
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let file = match path {
            Some(path) => {
                let raw = fs::read_to_string(path).unwrap_or_default();
                if raw.trim().is_empty() {
                    ConfigFile::default()
                } else {
                    serde_json::from_str::<ConfigFile>(&raw).map_err(|e| e.to_string())?
                }
            }
            None => ConfigFile::default(),
        };
        let base = file
            .data_dir
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| {
                path.and_then(Path::parent)
                    .and_then(Path::parent)
                    .map(|p| p.join("context-mode"))
                    .unwrap_or_else(|| PathBuf::from("context-mode"))
            });
        let roots = if file.allowed_roots.is_empty() {
            vec![base.clone()]
        } else {
            file.allowed_roots.iter().map(|p| normalize(p)).collect()
        };
        Ok(Self {
            data_dir: normalize(&base),
            allowed_roots: roots,
            max_file_bytes: file.max_file_bytes.clamp(1_024, 16_777_216),
            max_files: file.max_files.clamp(1, 5_000),
            max_output_bytes: file.max_output_bytes.clamp(1_024, 2_097_152),
            default_timeout_ms: file.default_timeout_ms.clamp(100, 600_000),
            execution_enabled: file.execution_enabled,
        })
    }

    #[cfg(test)]
    pub fn for_test(data_dir: PathBuf) -> Self {
        let data_dir = normalize(&data_dir);
        Self {
            allowed_roots: vec![data_dir.clone()],
            data_dir,
            max_file_bytes: 1_048_576,
            max_files: 5000,
            max_output_bytes: 262144,
            default_timeout_ms: 30_000,
            execution_enabled: false,
        }
    }

    pub fn allows(&self, path: &Path) -> bool {
        let path = normalize(path);
        self.allowed_roots
            .iter()
            .any(|root| path == *root || path.starts_with(root))
    }
}

fn deserialize_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Boolean {
        Bool(bool),
        Text(String),
    }
    match Boolean::deserialize(deserializer)? {
        Boolean::Bool(value) => Ok(value),
        Boolean::Text(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" | "" => Ok(false),
            _ => Err(serde::de::Error::custom(
                "execution_enabled 必须是 true/false",
            )),
        },
    }
}

fn deserialize_roots<'de, D>(deserializer: D) -> Result<Vec<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Roots {
        Many(Vec<PathBuf>),
        One(String),
    }
    match Roots::deserialize(deserializer)? {
        Roots::Many(paths) => Ok(paths),
        Roots::One(value) => Ok(value
            .split([',', '\n'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect()),
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;

fn normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
