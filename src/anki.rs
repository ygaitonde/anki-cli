use std::collections::BTreeMap;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct AnkiConnectClient {
    http: Client,
    base_url: String,
}

impl AnkiConnectClient {
    pub fn new(base_url: String) -> Self {
        Self {
            http: Client::new(),
            base_url,
        }
    }

    pub async fn ensure_deck_exists(&self, deck_name: &str) -> Result<()> {
        let request = AnkiRequest {
            action: "createDeck",
            version: 6,
            params: CreateDeckParams { deck: deck_name },
        };

        let response: AnkiResponse<Option<serde_json::Value>> = self
            .post(&request)
            .await
            .with_context(|| format!("failed to ensure deck {deck_name} exists"))?;

        if let Some(error) = response.error {
            if error.contains("exists") {
                tracing::debug!("deck {} already exists", deck_name);
                return Ok(());
            }
            anyhow::bail!("Anki returned error: {error}");
        }

        Ok(())
    }

    pub async fn add_notes(&self, notes: &[Note]) -> Result<Vec<Option<i64>>> {
        if notes.is_empty() {
            return Ok(vec![]);
        }

        let request = AnkiRequest {
            action: "addNotes",
            version: 6,
            params: AddNotesParams { notes },
        };

        let response: AnkiResponse<Vec<Option<i64>>> = self
            .post(&request)
            .await
            .context("failed to add notes via AnkiConnect")?;

        if let Some(error) = response.error {
            anyhow::bail!("Anki returned error: {error}");
        }

        response
            .result
            .context("missing result payload from AnkiConnect addNotes response")
    }

    async fn post<'a, T, R>(&self, payload: &'a AnkiRequest<'a, T>) -> Result<AnkiResponse<R>>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let url = format!("{}/", self.base_url.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .json(payload)
            .send()
            .await
            .context("failed to reach AnkiConnect")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("AnkiConnect HTTP error {status}: {body}");
        }

        let parsed = response
            .json::<AnkiResponse<R>>()
            .await
            .context("failed to parse AnkiConnect response body")?;

        Ok(parsed)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub deck_name: String,
    pub model_name: String,
    pub fields: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<NoteOptions>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NoteOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_duplicate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnkiRequest<'a, T> {
    action: &'a str,
    version: u8,
    params: T,
}

#[derive(Debug, Serialize)]
struct CreateDeckParams<'a> {
    deck: &'a str,
}

#[derive(Debug, Serialize)]
struct AddNotesParams<'a> {
    notes: &'a [Note],
}

#[derive(Debug, Deserialize)]
struct AnkiResponse<T> {
    result: Option<T>,
    error: Option<String>,
}
