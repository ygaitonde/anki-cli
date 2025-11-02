use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct OpenAiClient {
    http: Client,
    api_key: String,
    model: String,
    base_url: String,
}

#[derive(Debug, Clone)]
pub struct HindiCard {
    pub word: String,
    pub hindi_sentence: String,
    pub english_sentence: String,
}

#[derive(Debug, Clone)]
pub struct EnglishClozeCard {
    pub word: String,
    pub cloze_sentence: String,
    pub translation: String,
    pub hint: Option<String>,
}

impl OpenAiClient {
    pub fn new(api_key: String, model: String, base_url: String) -> Result<Self> {
        if api_key.trim().is_empty() {
            anyhow::bail!("OpenAI API key cannot be empty");
        }

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client for OpenAI")?;

        Ok(Self {
            http,
            api_key,
            model,
            base_url,
        })
    }

    pub async fn generate_hindi_card(&self, word: &str, temperature: f32) -> Result<HindiCard> {
        let prompt = format!(
            "You are creating language learning flashcards. Generate a natural, short Hindi sentence that uses the target word exactly once and is easy for learners to understand. Provide a natural-sounding English translation. Target word: {word}"
        );

        let user = format!(
            "Return STRICT JSON with keys word, hindi_sentence, english_sentence. Requirements:\n- sentence length 5-12 words\n- include the word exactly once, unmodified unless grammatical inflection is required\n- keep language learner-friendly\n- use Devanagari for Hindi.\nTarget word: {word}"
        );

        let payload = self
            .chat_completion(prompt, user, temperature)
            .await
            .context("failed to fetch Hindi card from OpenAI")?;

        let parsed: HindiCardPayload = parse_json(&payload)?;

        if !parsed.hindi_sentence.contains(parsed.word.trim()) {
            tracing::warn!(
                "Hindi sentence may not contain original word: {}",
                parsed.word
            );
        }

        Ok(HindiCard {
            word: parsed.word.trim().to_string(),
            hindi_sentence: parsed.hindi_sentence.trim().to_string(),
            english_sentence: parsed.english_sentence.trim().to_string(),
        })
    }

    pub async fn generate_english_cloze(
        &self,
        word: &str,
        temperature: f32,
    ) -> Result<EnglishClozeCard> {
        let system = format!(
            "You create English cloze deletions for language learners. Generate a short, conversational sentence that uses the target word naturally."
        );

        let user = format!(
            "Return STRICT JSON with keys word, cloze_sentence, translation, hint.\nRules:\n- Use Anki cloze syntax {{c1::...}} exactly once around the target word or phrase.\n- Sentence length 8-16 words.\n- Provide a brief translation into Hindi for the translation field.\n- Optional hint should help recall the word and can be null.\nTarget word: {word}"
        );

        let payload = self
            .chat_completion(system, user, temperature)
            .await
            .context("failed to fetch English cloze from OpenAI")?;

        let mut parsed: EnglishClozePayload = parse_json(&payload)?;
        if !parsed.cloze_sentence.contains("{{c1::") {
            tracing::warn!("Cloze sentence missing cloze markup for word: {}", word);
            parsed.cloze_sentence = parsed
                .cloze_sentence
                .replace(&parsed.word, &format!("{{{{c1::{}}}}}", parsed.word));
        }

        Ok(EnglishClozeCard {
            word: parsed.word.trim().to_string(),
            cloze_sentence: parsed.cloze_sentence.trim().to_string(),
            translation: parsed.translation.trim().to_string(),
            hint: parsed
                .hint
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty()),
        })
    }

    async fn chat_completion(
        &self,
        system: String,
        user: String,
        temperature: f32,
    ) -> Result<String> {
        let temperature = temperature.clamp(0.0, 2.0);

        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: system,
                },
                Message {
                    role: "user".to_string(),
                    content: user,
                },
            ],
            temperature,
            response_format: Some(ResponseFormat {
                kind: "json_object".to_string(),
            }),
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .context("failed to call OpenAI chat completion endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI HTTP error {status}: {body}");
        }

        let parsed: ChatCompletionResponse = response
            .json()
            .await
            .context("failed to parse OpenAI response JSON")?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("OpenAI returned no choices"))?;

        Ok(choice.message.content)
    }
}

fn parse_json<T>(raw: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = raw.trim();
    let json = if trimmed.starts_with("```") {
        extract_json_block(trimmed).unwrap_or_else(|| trimmed.to_string())
    } else {
        trimmed.to_string()
    };

    serde_json::from_str(&json).with_context(|| format!("failed to parse JSON payload: {json}"))
}

fn extract_json_block(raw: &str) -> Option<String> {
    let mut lines = raw.lines();
    let first = lines.next()?;
    if !first.starts_with("```") {
        return None;
    }

    let mut content: Vec<&str> = lines.collect();
    if content.is_empty() {
        return None;
    }

    if let Some(last) = content.last() {
        if last.trim().starts_with("```") {
            content.pop();
        }
    }

    Some(content.join("\n"))
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct HindiCardPayload {
    word: String,
    hindi_sentence: String,
    english_sentence: String,
}

#[derive(Debug, Deserialize)]
struct EnglishClozePayload {
    word: String,
    cloze_sentence: String,
    translation: String,
    #[serde(default)]
    hint: Option<String>,
}
