use crate::app::Paths;
use crate::error::{BfxError, Result};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct ConfigFile {
    #[serde(rename = "Models", default)]
    pub models: BTreeMap<String, Provider>,
    #[serde(rename = "Presets", default)]
    pub presets: BTreeMap<String, Preset>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Provider {
    #[serde(rename = "Model")]
    pub model: String,
    #[serde(rename = "URL")]
    pub url: String,
    #[serde(rename = "Key")]
    pub key: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Preset {
    #[serde(rename = "Pages")]
    pub pages: String,
    #[serde(rename = "Language")]
    pub language: String,
    #[serde(rename = "Format")]
    pub format: String,
    #[serde(rename = "Destination")]
    pub destination: String,
    #[serde(rename = "Watermark")]
    pub watermark: bool,
}

pub fn ensure_files(paths: &Paths) -> Result<()> {
    if !paths.config.exists() {
        let mut config = ConfigFile::default();
        let legacy_dirs = [
            paths.config_dir.clone(),
            paths.config_dir.join("data").join("config"),
            paths.config_dir.join("config"),
        ];
        let legacy_configs = [
            legacy_dirs[1].join("config.toml"),
            paths.config_dir.join("data").join("config.toml"),
            legacy_dirs[2].join("config.toml"),
        ];
        for legacy_config in legacy_configs {
            if legacy_config.is_file() {
                config = read_config(&legacy_config)?;
                break;
            }
        }
        for legacy_dir in legacy_dirs {
            if config.models.is_empty() {
                let providers = legacy_dir.join("providers.toml");
                if providers.is_file() {
                    config.models = read_file::<ConfigFile>(&providers, "providers.toml")?.models;
                }
            }
            if config.presets.is_empty() {
                let presets = legacy_dir.join("presets.toml");
                if presets.is_file() {
                    config.presets = read_file::<ConfigFile>(&presets, "presets.toml")?.presets;
                }
            }
        }
        if config.presets.is_empty() {
            config
                .presets
                .insert("Default".to_owned(), default_preset());
        }
        write_config(&paths.config, &config)?;
    }
    Ok(())
}

pub fn read_config(path: &Path) -> Result<ConfigFile> {
    read_file(path, "config.toml")
}

pub fn write_config(path: &Path, value: &ConfigFile) -> Result<()> {
    write_file(path, value, "config.toml")
}

pub fn find_provider(file: &ConfigFile, name: &str) -> Result<(String, Provider)> {
    file.models
        .iter()
        .find(|(saved, _)| saved.eq_ignore_ascii_case(name))
        .map(|(saved, provider)| (saved.clone(), provider.clone()))
        .ok_or_else(|| BfxError::config(format!("Model \"{name}\" is not configured")))
}

pub fn find_preset(file: &ConfigFile, name: &str) -> Result<(String, Preset)> {
    file.presets
        .iter()
        .find(|(saved, _)| saved.eq_ignore_ascii_case(name))
        .map(|(saved, preset)| (saved.clone(), preset.clone()))
        .ok_or_else(|| BfxError::config(format!("Preset \"{name}\" is not configured")))
}

pub fn read_key(value: &str) -> Result<String> {
    if let Some(name) = value.strip_prefix("env:") {
        return std::env::var(name).map_err(|_| {
            BfxError::config(format!("Environment variable \"{name}\" is not available"))
        });
    }
    Ok(value.to_owned())
}

pub fn openai_base_url(value: &str) -> Result<String> {
    let mut url = Url::parse(value.trim())
        .map_err(|error| BfxError::config(format!("Model URL is invalid ({error})")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(BfxError::config("Model URL must be an HTTP(S) URL"));
    }
    let path = url.path().trim_end_matches('/');
    if !path.split('/').any(|part| part == "v1") {
        let path = if path.is_empty() {
            "/v1".to_owned()
        } else {
            format!("{path}/v1")
        };
        url.set_path(&path);
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

pub fn default_preset() -> Preset {
    Preset {
        pages: "All".to_owned(),
        language: "en->zh".to_owned(),
        format: "Pair".to_owned(),
        destination: "Same".to_owned(),
        watermark: false,
    }
}

pub fn check_preset(name: &str, preset: &Preset) -> Result<()> {
    if preset.pages.trim().is_empty() {
        return Err(BfxError::config(format!(
            "Preset \"{name}\" has an empty Pages value"
        )));
    }
    let Some((source, target)) = preset.language.split_once("->") else {
        return Err(BfxError::config(format!(
            "Preset \"{name}\" has an invalid Language value"
        )));
    };
    if source.trim().is_empty() || target.trim().is_empty() {
        return Err(BfxError::config(format!(
            "Preset \"{name}\" has an invalid Language value"
        )));
    }
    if !matches!(
        preset.format.to_ascii_lowercase().as_str(),
        "pair" | "mono" | "both"
    ) {
        return Err(BfxError::config(format!(
            "Preset \"{name}\" has an invalid Format value"
        )));
    }
    if !preset.destination.eq_ignore_ascii_case("Same")
        && !std::path::Path::new(&preset.destination).is_absolute()
    {
        return Err(BfxError::config(format!(
            "Preset \"{name}\" has an invalid Destination value"
        )));
    }
    Ok(())
}

fn read_file<T>(path: &Path, label: &str) -> Result<T>
where
    T: for<'a> Deserialize<'a>,
{
    let text = std::fs::read_to_string(path)
        .map_err(|error| BfxError::config(format!("Cannot read {label} ({error})")))?;
    toml::from_str(&text)
        .map_err(|error| BfxError::config(format!("Cannot parse {label} ({error})")))
}

fn write_file<T>(path: &Path, value: &T, label: &str) -> Result<()>
where
    T: Serialize,
{
    let text = toml::to_string_pretty(value)
        .map_err(|error| BfxError::config(format!("Cannot write {label} ({error})")))?;
    let parent = path
        .parent()
        .ok_or_else(|| BfxError::config(format!("Cannot write {label}: no parent directory")))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| BfxError::config(format!("Cannot write {label}: invalid file name")))?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(parent.join(format!(".{file_name}.lock")))
        .map_err(|error| BfxError::config(format!("Cannot lock {label} ({error})")))?;
    lock.lock_exclusive()
        .map_err(|error| BfxError::config(format!("Cannot lock {label} ({error})")))?;
    let temporary = temporary_path(parent, file_name, label)?;
    let result = write_temporary(&temporary, &text, label).and_then(|()| {
        fs::rename(&temporary, path)
            .map_err(|error| BfxError::config(format!("Cannot replace {label} ({error})")))?;
        sync_directory(parent, label)
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(parent: &Path, file_name: &str, label: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| BfxError::config(format!("Cannot write {label} ({error})")))?
        .as_nanos();
    for attempt in 0..100 {
        let path = parent.join(format!(
            ".{file_name}.{}-{stamp}-{attempt}.tmp",
            std::process::id()
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(BfxError::config(format!(
        "Cannot allocate a temporary {label} file"
    )))
}

fn write_temporary(path: &Path, text: &str, label: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| BfxError::config(format!("Cannot write {label} ({error})")))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| BfxError::config(format!("Cannot write {label} ({error})")))
}

#[cfg(unix)]
fn sync_directory(path: &Path, label: &str) -> Result<()> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| BfxError::config(format!("Cannot finalize {label} ({error})")))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path, _label: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn creates_combined_config() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("bfx-config-{stamp}"));
        let config_dir = root.join("config");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        let paths = Paths {
            config: config_dir.join("config.toml"),
            config_dir,
            data_dir,
        };
        ensure_files(&paths).unwrap();
        assert!(read_config(&paths.config).unwrap().models.is_empty());
        assert!(
            read_config(&paths.config)
                .unwrap()
                .presets
                .contains_key("Default")
        );
        let mut config = read_config(&paths.config).unwrap();
        config.models.insert(
            "Model".to_owned(),
            Provider {
                model: "model".to_owned(),
                url: "https://example.test/v1".to_owned(),
                key: "test-key".to_owned(),
            },
        );
        write_config(&paths.config, &config).unwrap();
        assert_eq!(read_config(&paths.config).unwrap().models.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn adds_v1_to_openai_base_urls() {
        assert_eq!(
            openai_base_url("https://api.example.test").unwrap(),
            "https://api.example.test/v1"
        );
        assert_eq!(
            openai_base_url("https://api.example.test/openai/v1/").unwrap(),
            "https://api.example.test/openai/v1"
        );
    }

    #[test]
    fn migrates_legacy_config_files() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("bfx-config-migration-{stamp}"));
        let config_dir = root.join("config");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("providers.toml"),
            "[Models.Model]\nModel = \"model\"\nURL = \"https://example.test/v1\"\nKey = \"test-key\"\n",
        )
        .unwrap();
        std::fs::write(
            config_dir.join("presets.toml"),
            "[Presets.Custom]\nPages = \"All\"\nLanguage = \"EN->ZH\"\nFormat = \"Pair\"\nDestination = \"Same\"\nWatermark = false\n",
        )
        .unwrap();
        let paths = Paths {
            config: config_dir.join("config.toml"),
            config_dir,
            data_dir,
        };
        ensure_files(&paths).unwrap();
        let config = read_config(&paths.config).unwrap();
        assert!(config.models.contains_key("Model"));
        assert!(config.presets.contains_key("Custom"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_nested_config_file() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("bfx-config-nested-{stamp}"));
        let config_dir = root.clone();
        let data_dir = root.join("data");
        let nested = data_dir.join("config");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("config.toml"),
            "[Models.Model]\nModel = \"model\"\nURL = \"https://example.test/v1\"\nKey = \"test-key\"\n\n[Presets.Custom]\nPages = \"All\"\nLanguage = \"EN->ZH\"\nFormat = \"Pair\"\nDestination = \"Same\"\nWatermark = false\n",
        )
        .unwrap();
        let paths = Paths {
            config: config_dir.join("config.toml"),
            config_dir,
            data_dir,
        };
        ensure_files(&paths).unwrap();
        let config = read_config(&paths.config).unwrap();
        assert!(config.models.contains_key("Model"));
        assert!(config.presets.contains_key("Custom"));
        let _ = std::fs::remove_dir_all(root);
    }
}
