use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub fn read_words_from_file(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read input file at {}", path.display()))?;

    let mut words = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        for piece in trimmed.split(|c| c == ',' || c == ';') {
            let candidate = piece.trim();
            if !candidate.is_empty() {
                words.push(candidate.to_string());
            }
        }
    }

    Ok(words)
}
