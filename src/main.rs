mod anki;
mod config;
mod input;
mod llm;
mod workflows;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tracing::Level;

use crate::anki::AnkiConnectClient;
use crate::config::{Config, ConfigOverrides};
use crate::llm::OpenAiClient;
use crate::workflows::{RunContext, run_english_flow, run_hindi_flow};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "CLI to generate language flashcards in Anki via AnkiConnect"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Optional path to a configuration TOML file overriding defaults
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override the OpenAI model used for generation
    #[arg(long)]
    model: Option<String>,

    /// Override the AnkiConnect base URL
    #[arg(long = "anki-url")]
    anki_url: Option<String>,

    /// Override the Hindi deck name for this run
    #[arg(long = "hindi-deck")]
    hindi_deck: Option<String>,

    /// Override the English deck name for this run
    #[arg(long = "english-deck")]
    english_deck: Option<String>,

    /// Optional temperature override for the language model
    #[arg(long)]
    temperature: Option<f32>,

    /// Additional tags to attach to generated notes
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,

    /// Preview the generated notes without sending them to Anki
    #[arg(long)]
    dry_run: bool,

    /// Enable verbose logging
    #[arg(long)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate Hindi sentence cards from words provided via CLI arguments or file
    Hindi(LanguageArgs),
    /// Generate English cloze cards from words provided via CLI arguments or file
    English(LanguageArgs),
    /// Run an interactive session for adding cards
    Interactive(InteractiveArgs),
}

#[derive(Debug, Args)]
struct LanguageArgs {
    /// Optional path to a file containing words (one per line)
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Optional override for the deck name
    #[arg(long)]
    deck: Option<String>,

    /// Words supplied directly via CLI arguments
    #[arg(name = "WORD", required = false)]
    words: Vec<String>,
}

#[derive(Debug, Args)]
struct InteractiveArgs {
    /// Optional default language to preselect in the interactive prompt
    #[arg(long)]
    language: Option<Language>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Language {
    Hindi,
    English,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    init_tracing(cli.verbose)?;

    let overrides = ConfigOverrides {
        model: cli.model.clone(),
        anki_url: cli.anki_url.clone(),
        hindi_deck: cli.hindi_deck.clone(),
        english_deck: cli.english_deck.clone(),
        temperature: cli.temperature,
        extra_tags: if cli.tags.is_empty() {
            None
        } else {
            Some(cli.tags.clone())
        },
    };

    let config = Config::load(cli.config.clone(), overrides)?;
    let anki_client = AnkiConnectClient::new(config.anki_connect_url.clone());
    let llm_client = OpenAiClient::new(
        config.openai_api_key.clone(),
        config.openai_model.clone(),
        config.openai_base_url.clone(),
    )?;

    let run_ctx = RunContext {
        anki: &anki_client,
        llm: &llm_client,
        config: &config,
        dry_run: cli.dry_run,
    };

    match cli.command {
        Command::Hindi(args) => run_language(Language::Hindi, args, &run_ctx).await?,
        Command::English(args) => run_language(Language::English, args, &run_ctx).await?,
        Command::Interactive(args) => run_interactive(args, &run_ctx).await?,
    }

    Ok(())
}

fn init_tracing(verbose: bool) -> Result<()> {
    let level = if verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|err| anyhow::anyhow!("Failed to set tracing subscriber: {err}"))
}

async fn run_language(
    language: Language,
    args: LanguageArgs,
    run_ctx: &RunContext<'_>,
) -> Result<()> {
    let mut words = args.words;

    if let Some(path) = args.input {
        let mut from_file = input::read_words_from_file(&path)
            .with_context(|| format!("failed to read words from file {path:?}"))?;
        words.append(&mut from_file);
    }

    if words.is_empty() {
        anyhow::bail!("no words provided; specify words via CLI arguments or --input file");
    }

    let deck_override = args.deck;

    match language {
        Language::Hindi => run_hindi_flow(words, deck_override, run_ctx).await?,
        Language::English => run_english_flow(words, deck_override, run_ctx).await?,
    }

    Ok(())
}

async fn run_interactive(args: InteractiveArgs, run_ctx: &RunContext<'_>) -> Result<()> {
    workflows::run_interactive_session(args.language, run_ctx).await
}
