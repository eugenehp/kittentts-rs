//! HuggingFace Hub model downloader — mirrors `get_model.py`.
//!
//! Downloads `config.json`, the ONNX model, and the voices NPZ file from a
//! HuggingFace repository, then constructs and returns a [`KittenTtsOnnx`].

use std::{collections::HashMap, path::PathBuf};

use anyhow::{bail, Context, Result};
use hf_hub::api::sync::Api;
use serde::Deserialize;

use crate::model::KittenTtsOnnx;

// ─────────────────────────────────────────────────────────────────────────────
// config.json schema
// ─────────────────────────────────────────────────────────────────────────────

/// Deserialised `config.json` from a KittenTTS HuggingFace repository.
#[derive(Debug, Deserialize)]
pub struct ModelConfig {
    /// Must be `"ONNX1"` or `"ONNX2"`.
    #[serde(rename = "type")]
    pub model_type: String,

    /// Filename of the ONNX model inside the repo (e.g. `"model.onnx"`).
    pub model_file: String,

    /// Filename of the voices NPZ file inside the repo (e.g. `"voices.npz"`).
    pub voices: String,

    /// Optional per-voice speed multipliers.
    #[serde(default)]
    pub speed_priors: HashMap<String, f32>,

    /// Optional friendly-name → NPZ-key aliases for voices.
    #[serde(default)]
    pub voice_aliases: HashMap<String, String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Download helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Download a single file from a HuggingFace repository.
fn hf_download(api: &Api, repo_id: &str, filename: &str) -> Result<PathBuf> {
    let repo = api.model(repo_id.to_string());
    repo.get(filename)
        .with_context(|| format!("Failed to download '{}' from '{}'", filename, repo_id))
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Download and initialise a [`KittenTtsOnnx`] model from HuggingFace.
///
/// Files are cached in the HuggingFace Hub cache directory
/// (`~/.cache/huggingface/hub` by default).
///
/// # Arguments
/// * `repo_id` — HuggingFace repository ID, e.g. `"KittenML/kitten-tts-mini-0.8"`.
///
/// # Example
/// ```no_run
/// let model = kittentts::download::load_from_hub("KittenML/kitten-tts-mini-0.8").unwrap();
/// let audio = model.generate("Hello world", "Jasper", 1.0, true).unwrap();
/// ```
pub fn load_from_hub(repo_id: &str) -> Result<KittenTtsOnnx> {
    // Expand bare model names (e.g. "kitten-tts-mini-0.8" → "KittenML/kitten-tts-mini-0.8")
    let repo_id = if repo_id.contains('/') {
        repo_id.to_string()
    } else {
        format!("KittenML/{}", repo_id)
    };

    println!("Downloading config from {}…", repo_id);
    let api = Api::new().context("Failed to initialise HuggingFace Hub client")?;

    // ── config.json ──────────────────────────────────────────────────────────
    let config_path = hf_download(&api, &repo_id, "config.json")?;
    let config_bytes = std::fs::read(&config_path)
        .with_context(|| format!("Cannot read config: {}", config_path.display()))?;
    let config: ModelConfig = serde_json::from_slice(&config_bytes)
        .context("Failed to parse config.json")?;

    if !matches!(config.model_type.as_str(), "ONNX1" | "ONNX2") {
        bail!(
            "Unsupported model type '{}' — expected ONNX1 or ONNX2",
            config.model_type
        );
    }

    // ── ONNX model ───────────────────────────────────────────────────────────
    println!("Downloading model file ({})…", config.model_file);
    let model_path = hf_download(&api, &repo_id, &config.model_file)?;

    // ── Voices NPZ ───────────────────────────────────────────────────────────
    println!("Downloading voices file ({})…", config.voices);
    let voices_path = hf_download(&api, &repo_id, &config.voices)?;

    // ── Build model ──────────────────────────────────────────────────────────
    println!("Loading model…");
    KittenTtsOnnx::load(
        &model_path,
        &voices_path,
        config.speed_priors,
        config.voice_aliases,
    )
}

/// Convenience alias using the default nano model.
pub fn load_default() -> Result<KittenTtsOnnx> {
    load_from_hub("KittenML/kitten-tts-nano-0.8-int8")
}
