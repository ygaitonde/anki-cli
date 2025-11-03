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
        let system = "You create English cloze deletions for learners who want to improve their English vocabulary.".to_string();

        let user = format!(
            "Return STRICT JSON with keys word, cloze_sentence, translation, hint.\nRules:\n- Use Anki cloze syntax {{c1::...}} exactly once around the target word or phrase.\n- If a hint is provided, include it using the built-in format {{c1::answer::hint}} so Anki can show a hint link.\n- Sentence length 8-16 words.\n- For the translation field, provide a concise English paraphrase or definition that clarifies the meaning of the sentence.\n- Optional hint should help recall the word and can be null.\nTarget word: {word}"
        );

        let payload = self
            .chat_completion(system, user, temperature)
            .await
            .context("failed to fetch English cloze from OpenAI")?;

        let parsed: EnglishClozePayload = parse_json(&payload)?;

        let word_trimmed = parsed.word.trim().to_string();
        let hint = parsed
            .hint
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty());

        let cloze_sentence =
            build_cloze_sentence(parsed.cloze_sentence.trim(), &word_trimmed, hint.as_deref());

        Ok(EnglishClozeCard {
            word: word_trimmed,
            cloze_sentence,
            translation: parsed.translation.trim().to_string(),
            hint,
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

fn build_cloze_sentence(raw_sentence: &str, word: &str, hint: Option<&str>) -> String {
    let trimmed = raw_sentence.trim();
    let original = trimmed.to_string();

    let base_sentence =
        strip_existing_cloze_markup(trimmed, word).unwrap_or_else(|| original.clone());

    let mut cloze_sentence = match wrap_with_cloze(&base_sentence, word) {
        Some(wrapped) => wrapped,
        None => {
            tracing::warn!(
                "Failed to insert cloze markup for '{}' - reverting to model output",
                word
            );
            original
        }
    };

    if let Some(hint_value) = hint {
        cloze_sentence = inject_anki_hint(&cloze_sentence, hint_value);
    }

    cloze_sentence
}

fn strip_existing_cloze_markup(sentence: &str, replacement: &str) -> Option<String> {
    let mut result = String::with_capacity(sentence.len());
    let chars: Vec<char> = sentence.chars().collect();
    let mut index = 0;
    let mut replaced = false;

    while index < chars.len() {
        if chars[index] == '{' {
            let mut lookahead = index;
            while lookahead < chars.len() && chars[lookahead] == '{' {
                lookahead += 1;
            }

            if lookahead < chars.len() && matches!(chars[lookahead], 'c' | 'C') {
                let mut after_prefix = lookahead + 1;
                while after_prefix < chars.len() && chars[after_prefix].is_ascii_digit() {
                    after_prefix += 1;
                }

                if after_prefix + 1 < chars.len()
                    && chars[after_prefix] == ':'
                    && chars[after_prefix + 1] == ':'
                {
                    let mut depth = 0i32;
                    let mut cursor = index;

                    while cursor < chars.len() {
                        match chars[cursor] {
                            '{' => {
                                depth += 1;
                            }
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    cursor += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        cursor += 1;
                    }

                    if depth == 0 {
                        result.push_str(replacement);
                        index = cursor;
                        replaced = true;
                        continue;
                    } else {
                        return None;
                    }
                }
            }
        }

        result.push(chars[index]);
        index += 1;
    }

    if replaced { Some(result) } else { None }
}

fn wrap_with_cloze(sentence: &str, word: &str) -> Option<String> {
    if sentence.contains("{{c1::") {
        return Some(sentence.to_string());
    }

    if let Some(pos) = sentence.find(word) {
        let end = pos + word.len();
        let mut result = String::with_capacity(sentence.len() + word.len() + 8);
        result.push_str(&sentence[..pos]);
        result.push_str("{{c1::");
        result.push_str(&sentence[pos..end]);
        result.push_str("}}");
        result.push_str(&sentence[end..]);
        return Some(result);
    }

    let lower_sentence = sentence.to_lowercase();
    let lower_word = word.to_lowercase();
    if let Some(pos) = lower_sentence.find(&lower_word) {
        let end = advance_by_chars(sentence, pos, word.chars().count());
        let segment = &sentence[pos..end];
        let mut result = String::with_capacity(sentence.len() + segment.len() + 8);
        result.push_str(&sentence[..pos]);
        result.push_str("{{c1::");
        result.push_str(segment);
        result.push_str("}}");
        result.push_str(&sentence[end..]);
        return Some(result);
    }

    tracing::warn!(
        "Could not locate '{}' inside cloze sentence '{}'",
        word,
        sentence
    );
    None
}

fn advance_by_chars(text: &str, start: usize, char_count: usize) -> usize {
    let mut consumed = 0;
    for (offset, ch) in text[start..].char_indices() {
        consumed += 1;
        if consumed == char_count {
            return start + offset + ch.len_utf8();
        }
    }

    text.len()
}

fn inject_anki_hint(cloze_sentence: &str, hint: &str) -> String {
    let hint = hint.trim();
    if hint.is_empty() {
        return cloze_sentence.to_string();
    }

    if let Some(start) = cloze_sentence.find("{{c1::") {
        let prefix = &cloze_sentence[..start + 6];
        let rest = &cloze_sentence[start + 6..];
        if let Some(end_rel) = rest.find("}}") {
            let inside = &rest[..end_rel];
            if inside.contains("::") {
                return cloze_sentence.to_string();
            }
            let suffix = &rest[end_rel..];
            return format!("{}{}::{}{}", prefix, inside, hint, suffix);
        }
    }

    cloze_sentence.to_string()
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
