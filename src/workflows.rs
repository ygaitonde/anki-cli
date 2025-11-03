use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Select};

use crate::Language;
use crate::anki::{AnkiConnectClient, Note, NoteOptions};
use crate::config::Config;
use crate::llm::{EnglishClozeCard, HindiCard, OpenAiClient};

pub struct RunContext<'a> {
    pub anki: &'a AnkiConnectClient,
    pub llm: &'a OpenAiClient,
    pub config: &'a Config,
    pub dry_run: bool,
    pub auto_approve: bool,
}

pub async fn run_hindi_flow(
    words: Vec<String>,
    deck_override: Option<String>,
    ctx: &RunContext<'_>,
) -> Result<()> {
    let deck = deck_override.unwrap_or_else(|| ctx.config.hindi_deck.clone());
    ctx.anki
        .ensure_deck_exists(&deck)
        .await
        .with_context(|| format!("failed to ensure Hindi deck {deck} exists"))?;

    let mut seen = HashSet::new();
    for word in normalize_words(words) {
        let key = word.to_lowercase();
        if !seen.insert(key) {
            tracing::debug!("Skipping duplicate word: {}", word);
            continue;
        }

        tracing::info!("Generating Hindi card for word: {}", word);
        let card = ctx
            .llm
            .generate_hindi_card(&word, ctx.config.temperature)
            .await
            .with_context(|| format!("failed to generate Hindi card for '{word}'"))?;

        if ctx.dry_run {
            print_hindi_card(&card, &deck, "DRY RUN");
            continue;
        }

        if !ctx.auto_approve {
            print_hindi_card(&card, &deck, "REVIEW");
            let approved = prompt_send_confirmation("Send these Hindi notes to Anki?")?;
            if !approved {
                tracing::info!("Skipping Hindi notes for '{}'", card.word);
                continue;
            }
        }

        let notes = build_hindi_notes(&card, &deck, &ctx.config.tags);
        let results = ctx
            .anki
            .add_notes(&notes)
            .await
            .with_context(|| format!("failed to add Hindi notes for '{word}'"))?;

        report_add_note_results(&card.word, &deck, results);
    }

    // Save the deck name for future use (skip in dry run)
    if !ctx.dry_run {
        if let Err(e) = ctx.config.save_hindi_deck(&deck) {
            tracing::warn!("Failed to save Hindi deck to config: {}", e);
        }
    }

    Ok(())
}

pub async fn run_english_flow(
    words: Vec<String>,
    deck_override: Option<String>,
    ctx: &RunContext<'_>,
) -> Result<()> {
    let deck = deck_override.unwrap_or_else(|| ctx.config.english_deck.clone());
    ctx.anki
        .ensure_deck_exists(&deck)
        .await
        .with_context(|| format!("failed to ensure English deck {deck} exists"))?;

    let mut seen = HashSet::new();
    for word in normalize_words(words) {
        let key = word.to_lowercase();
        if !seen.insert(key.clone()) {
            tracing::debug!("Skipping duplicate word: {}", word);
            continue;
        }

        tracing::info!("Generating English cloze for word: {}", word);
        let card = ctx
            .llm
            .generate_english_cloze(&word, ctx.config.temperature)
            .await
            .with_context(|| format!("failed to generate English cloze for '{word}'"))?;

        if ctx.dry_run {
            print_english_card(&card, &deck, "DRY RUN");
            continue;
        }

        if !ctx.auto_approve {
            print_english_card(&card, &deck, "REVIEW");
            let approved = prompt_send_confirmation("Send this English cloze to Anki?")?;
            if !approved {
                tracing::info!("Skipping English note for '{}'", card.word);
                continue;
            }
        }

        let note = build_english_note(&card, &deck, &ctx.config.tags);
        let results = ctx
            .anki
            .add_notes(&[note])
            .await
            .with_context(|| format!("failed to add English note for '{word}'"))?;

        report_add_note_results(&card.word, &deck, results);
    }

    // Save the deck name for future use (skip in dry run)
    if !ctx.dry_run {
        if let Err(e) = ctx.config.save_english_deck(&deck) {
            tracing::warn!("Failed to save English deck to config: {}", e);
        }
    }

    Ok(())
}

pub async fn run_interactive_session(
    default_language: Option<Language>,
    ctx: &RunContext<'_>,
) -> Result<()> {
    let mut keep_running = true;
    let mut preset_language = default_language;

    while keep_running {
        let language = match preset_language.take() {
            Some(lang) => lang,
            None => match prompt_language()? {
                Some(lang) => lang,
                None => {
                    tracing::info!("Exiting interactive session.");
                    break;
                }
            },
        };

        let input = Input::<String>::new()
            .with_prompt("Enter words (comma or newline separated). Leave empty to exit")
            .allow_empty(true)
            .interact_text()?;

        if input.trim().is_empty() {
            tracing::info!("No words provided. Exiting interactive mode.");
            break;
        }

        let words = split_input(&input);
        if words.is_empty() {
            tracing::warn!("No valid words parsed from input.");
        } else {
            match language {
                Language::Hindi => {
                    run_hindi_flow(words, None, ctx).await?;
                }
                Language::English => {
                    run_english_flow(words, None, ctx).await?;
                }
            }
        }

        keep_running = Confirm::new()
            .with_prompt("Add more cards?")
            .default(true)
            .interact()?;
    }

    Ok(())
}

fn build_hindi_notes(card: &HindiCard, deck: &str, base_tags: &[String]) -> Vec<Note> {
    let tags = collect_tags(base_tags, &card.word, "hindi");

    let mut forward_fields = BTreeMap::new();
    forward_fields.insert("Front".to_string(), card.hindi_sentence.clone());
    forward_fields.insert("Back".to_string(), card.english_sentence.clone());

    let mut reverse_fields = BTreeMap::new();
    reverse_fields.insert("Front".to_string(), card.english_sentence.clone());
    reverse_fields.insert("Back".to_string(), card.hindi_sentence.clone());

    let note_options = NoteOptions {
        allow_duplicate: Some(false),
        duplicate_scope: Some("deck".to_string()),
    };

    vec![
        Note {
            deck_name: deck.to_string(),
            model_name: "Basic".to_string(),
            fields: forward_fields,
            tags: tags.clone(),
            options: Some(note_options.clone()),
        },
        Note {
            deck_name: deck.to_string(),
            model_name: "Basic".to_string(),
            fields: reverse_fields,
            tags,
            options: Some(note_options),
        },
    ]
}

fn build_english_note(card: &EnglishClozeCard, deck: &str, base_tags: &[String]) -> Note {
    let mut fields = BTreeMap::new();
    fields.insert("Text".to_string(), card.cloze_sentence.clone());

    let mut back_extra = format!("Explanation: {}", card.translation.trim());
    if let Some(hint) = &card.hint {
        if !hint.trim().is_empty() {
            back_extra.push_str("\nHint: ");
            back_extra.push_str(hint.trim());
        }
    }

    fields.insert("Back Extra".to_string(), back_extra);

    let tags = collect_tags(base_tags, &card.word, "english");

    Note {
        deck_name: deck.to_string(),
        model_name: "Cloze".to_string(),
        fields,
        tags,
        options: Some(NoteOptions {
            allow_duplicate: Some(false),
            duplicate_scope: Some("deck".to_string()),
        }),
    }
}

fn collect_tags(base: &[String], word: &str, language_tag: &str) -> Vec<String> {
    let mut tags = base.to_vec();
    if !tags
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(language_tag))
    {
        tags.push(language_tag.to_string());
    }

    let word_tag = format!("word_{}", sanitize_tag(word));
    if !tags
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&word_tag))
    {
        tags.push(word_tag);
    }

    tags
}

fn sanitize_tag(input: &str) -> String {
    input
        .trim()
        .chars()
        .map(|c| match c {
            c if c.is_whitespace() => '_',
            ':' | ';' | ',' => '_',
            _ => c,
        })
        .collect()
}

fn normalize_words(words: Vec<String>) -> Vec<String> {
    words
        .into_iter()
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn report_add_note_results(word: &str, deck: &str, results: Vec<Option<i64>>) {
    for (idx, outcome) in results.into_iter().enumerate() {
        match outcome {
            Some(note_id) => {
                tracing::info!("Added note {} for '{}' to deck '{}'", note_id, word, deck)
            }
            None => tracing::warn!(
                "Anki reported a duplicate for '{}' (card #{}).",
                word,
                idx + 1
            ),
        }
    }
}

fn print_hindi_card(card: &HindiCard, deck: &str, label: &str) {
    println!("[{}][{}] {}", label, deck, card.word);
    println!("  Hindi : {}", card.hindi_sentence);
    println!("  English: {}", card.english_sentence);
}

fn print_english_card(card: &EnglishClozeCard, deck: &str, label: &str) {
    println!("[{}][{}] {}", label, deck, card.word);
    println!("  Cloze       : {}", card.cloze_sentence);
    println!("  Explanation : {}", card.translation);
    if let Some(hint) = &card.hint {
        if !hint.trim().is_empty() {
            println!("  Hint        : {}", hint);
        }
    }
}

fn prompt_send_confirmation(prompt: &str) -> Result<bool> {
    Confirm::new()
        .with_prompt(prompt)
        .default(true)
        .interact()
        .context("failed to read approval input")
}

fn prompt_language() -> Result<Option<Language>> {
    let selections = vec!["Hindi sentence cards", "English cloze cards", "Exit"];
    let choice = Select::new()
        .with_prompt("Choose a language workflow")
        .items(&selections)
        .default(0)
        .interact()?;

    match choice {
        0 => Ok(Some(Language::Hindi)),
        1 => Ok(Some(Language::English)),
        _ => Ok(None),
    }
}

fn split_input(input: &str) -> Vec<String> {
    input
        .split(|c| c == ',' || c == ';' || c == '\n' || c == '\r')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
