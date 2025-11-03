use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Config {
    pub openai_api_key: String,
    pub openai_model: String,
    pub openai_base_url: String,
    pub anki_connect_url: String,
    pub hindi_deck: String,
    pub english_deck: String,
    pub temperature: f32,
    pub tags: Vec<String>,
    config_path: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct FileConfig {
    openai_api_key: Option<String>,
    openai_model: Option<String>,
    openai_base_url: Option<String>,
    anki_connect_url: Option<String>,
    hindi_deck: Option<String>,
    english_deck: Option<String>,
    temperature: Option<f32>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone)]
pub struct ConfigOverrides {
    pub model: Option<String>,
    pub anki_url: Option<String>,
    pub hindi_deck: Option<String>,
    pub english_deck: Option<String>,
    pub temperature: Option<f32>,
    pub extra_tags: Option<Vec<String>>,
}

impl Config {
    pub fn load(config_path: Option<PathBuf>, overrides: ConfigOverrides) -> Result<Self> {
        let file_config = load_file_config(config_path.as_ref())?;

        let openai_api_key = file_config
            .openai_api_key
            .clone()
            .or_else(|| env::var("OPENAI_API_KEY").ok())
            .context("missing OpenAI API key; set OPENAI_API_KEY or add to config")?;

        let openai_model = overrides
            .model
            .clone()
            .or(file_config.openai_model.clone())
            .or_else(|| env::var("OPENAI_MODEL").ok())
            .unwrap_or_else(|| "gpt-4o".to_string());

        let openai_base_url = file_config
            .openai_base_url
            .clone()
            .or_else(|| env::var("OPENAI_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let anki_connect_url = overrides
            .anki_url
            .clone()
            .or(file_config.anki_connect_url.clone())
            .or_else(|| env::var("ANKI_CONNECT_URL").ok())
            .unwrap_or_else(|| "http://127.0.0.1:8765".to_string());

        let hindi_deck = overrides
            .hindi_deck
            .clone()
            .or(file_config.hindi_deck.clone())
            .unwrap_or_else(|| "Hindi Sentence Practice".to_string());

        let english_deck = overrides
            .english_deck
            .clone()
            .or(file_config.english_deck.clone())
            .unwrap_or_else(|| "English Cloze Practice".to_string());

        let temperature = overrides
            .temperature
            .or(file_config.temperature)
            .or_else(|| {
                env::var("OPENAI_TEMPERATURE")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(0.7);

        let mut tags: Vec<String> = file_config
            .tags
            .unwrap_or_else(|| vec!["generated".to_string()])
            .into_iter()
            .filter_map(|tag| {
                let cleaned = tag.trim();
                if cleaned.is_empty() {
                    None
                } else {
                    Some(cleaned.to_string())
                }
            })
            .collect();

        if tags.is_empty() {
            tags.push("generated".to_string());
        }
        if let Some(extra) = overrides.extra_tags {
            for tag in extra {
                let cleaned = tag.trim();
                if cleaned.is_empty() {
                    continue;
                }
                if !tags
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(cleaned))
                {
                    tags.push(cleaned.to_string());
                }
            }
        }

        // Determine which config path to use for saving
        let config_path = if let Some(ref path) = config_path {
            Some(path.clone())
        } else {
            default_config_path()
        };

        Ok(Self {
            openai_api_key,
            openai_model,
            openai_base_url,
            anki_connect_url,
            hindi_deck,
            english_deck,
            temperature,
            tags,
            config_path,
        })
    }

    /// Save the Hindi deck name to the config file for future use
    pub fn save_hindi_deck(&self, deck_name: &str) -> Result<()> {
        self.save_deck_field("hindi_deck", deck_name)
    }

    /// Save the English deck name to the config file for future use
    pub fn save_english_deck(&self, deck_name: &str) -> Result<()> {
        self.save_deck_field("english_deck", deck_name)
    }

    fn save_deck_field(&self, field: &str, value: &str) -> Result<()> {
        let config_path = match &self.config_path {
            Some(path) => path.clone(),
            None => default_config_path()
                .context("could not determine config file path to save deck")?,
        };

        // Load existing config or create new one
        let mut file_config = if config_path.exists() {
            read_config_from_path(&config_path)?
        } else {
            FileConfig::default()
        };

        // Update the deck field
        match field {
            "hindi_deck" => file_config.hindi_deck = Some(value.to_string()),
            "english_deck" => file_config.english_deck = Some(value.to_string()),
            _ => anyhow::bail!("unknown deck field: {}", field),
        }

        // Ensure the config directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory at {}", parent.display())
            })?;
        }

        // Serialize and write the config
        let toml_string =
            toml::to_string_pretty(&file_config).context("failed to serialize config to TOML")?;
        fs::write(&config_path, toml_string)
            .with_context(|| format!("failed to write config file to {}", config_path.display()))?;

        tracing::debug!("Saved {} to config file: {}", field, value);
        Ok(())
    }
}

fn load_file_config(path: Option<&PathBuf>) -> Result<FileConfig> {
    if let Some(path) = path {
        if path.exists() {
            return read_config_from_path(path);
        }
        anyhow::bail!("config path {:?} does not exist", path);
    }

    if let Some(default_path) = default_config_path() {
        if default_path.exists() {
            return read_config_from_path(&default_path);
        }
    }

    Ok(FileConfig::default())
}

fn read_config_from_path(path: &Path) -> Result<FileConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    toml::from_str(&raw)
        .with_context(|| format!("failed to parse config file at {}", path.display()))
}

fn default_config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "language-cli", "anki-cli")
        .map(|dirs| dirs.config_dir().join("config.toml"))
}
