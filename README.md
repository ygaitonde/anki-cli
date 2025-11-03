# anki-cli

Command-line helper for creating Hindi sentence cards and English cloze cards in Anki using AnkiConnect and an LLM.

## Requirements

- Rust toolchain (1.80+ recommended) to build and run the binary (`cargo build`, `cargo run`).
- Anki desktop app with the [AnkiConnect add-on](https://foosoft.net/projects/anki-connect/) enabled and listening on `http://127.0.0.1:8765`.
- OpenAI-compatible API key (defaults to the official OpenAI endpoint). Any API that supports the `/chat/completions` interface and JSON response format should work.

## Installation

```bash
cargo build --release
# binary will be at target/release/anki-cli
```

You can also run directly with Cargo:

```bash
cargo run -- <command>
```

## Configuration

The CLI reads configuration from (highest priority first):

1. Command-line flags
2. Environment variables
3. Config file (`~/.config/anki-cli/config.toml` on macOS/Linux, or `--config <path>`) 
4. Built-in defaults

Supported environment variables / config keys:

```toml
openai_api_key   = "sk-..."      # required unless provided via CLI
openai_model     = "gpt-5"       # optional override (default when unset)
openai_base_url  = "https://api.openai.com/v1"  # optional, for custom endpoints
anki_connect_url = "http://127.0.0.1:8765"       # optional
hindi_deck       = "Hindi Sentence Practice"    # remembered automatically
english_deck     = "English Cloze Practice"     # remembered automatically
temperature      = 0.7                           # optional float
tags             = ["generated"]                # extra tags to apply to every note
```

Deck names are remembered automatically: after a successful (non `--dry-run`) run, the last-used deck for each language is saved back to the config file.

## Usage

Run `cargo run -- --help` for the full flag list. By default the CLI will show each generated card and prompt for approval before sending it to Anki; pass `--auto-approve` to skip the review step. Key commands are:

### Hindi sentence cards

```bash
cargo run -- hindi नमस्ते बारिश सपना

# or from a file (one word per line)
cargo run -- hindi --input words_hi.txt

# optional overrides
cargo run -- hindi --deck "My Hindi Deck" --dry-run नमस्ते
```

For each supplied word, two cards are added:

- Front: Hindi sentence (generated with the target word); Back: English translation.
- Front: English sentence; Back: Hindi sentence.

### English cloze cards

```bash
cargo run -- english "serendipity"

# batch from file
cargo run -- english --input words_en.txt
```

Each word yields a cloze card with `{{c1:: ... }}` syntax, an English explanation on the back, and an optional hint surfaced via Anki's built-in "Show Hint" link.

### Interactive mode

```bash
cargo run -- interactive

# optionally preselect language
cargo run -- interactive --language hindi
```

You’ll be prompted for words and asked whether to add more after each batch.

## Common Flags

- `--config <path>`: load/save configuration at a custom location.
- `--model <name>`: override the LLM model just for this run.
- `--anki-url <url>`: point to a different AnkiConnect instance.
- `--hindi-deck` / `--english-deck`: temporary overrides (also saved when successful).
- `--temperature <float>`: tweak the LLM creativity (0.0–2.0, default 0.7).
- `--tags tag1,tag2`: comma-separated extra tags applied to generated notes.
- `--dry-run`: preview generated content without calling AnkiConnect.
- `--auto-approve`: bypass the review prompt and send notes immediately (restores the legacy behaviour).
- `--verbose`: enable debug logging.

## Dry Run Preview

Use `--dry-run` to see generated sentences/clozes without creating notes. Helpful for checking prompt quality or when configuring decks.

## Development Notes

- `cargo fmt` keeps formatting consistent.
- `cargo check` ensures the code builds.
- Unit/integration tests can be added under `tests/` or via modules; mock the LLM/Anki clients for isolation if needed.

Contributions welcome—tweak prompts, add more languages, or extend configuration as desired.

