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
// Progress reporting
// ─────────────────────────────────────────────────────────────────────────────

/// Progress event emitted to the caller during model loading.
///
/// The total step count is always **4**:
///
/// | Step | Event                                     | Meaning                          |
/// |------|-------------------------------------------|----------------------------------|
/// | 1/4  | `Fetching { file: "config.json", … }`     | Fetching / cache-checking config |
/// | 2/4  | `Fetching { file: "<model>.onnx", … }`    | Fetching / cache-checking model  |
/// | 3/4  | `Fetching { file: "<voices>.npz", … }`    | Fetching / cache-checking voices |
/// | 4/4  | `Loading`                                 | Building the ONNX session        |
#[derive(Debug, Clone)]
pub enum LoadProgress {
    /// About to fetch (or retrieve from cache) one of the three model files.
    Fetching { step: u32, total: u32, file: String },
    /// All files are available; building the ONNX runtime session.
    Loading,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Download (or reuse cached) model files and initialise a [`KittenTtsOnnx`],
/// calling `on_progress` before each step so callers can show a progress bar.
///
/// # Arguments
/// * `repo_id`     — HuggingFace repo, e.g. `"KittenML/kitten-tts-mini-0.8"`.
///                   Bare names are prefixed with `"KittenML/"`.
/// * `on_progress` — Called before each step; see [`LoadProgress`].
///
/// Files are cached in `~/.cache/huggingface/hub`; subsequent calls return
/// immediately from cache without a network round-trip.
///
/// # Example
/// ```no_run
/// let model = kittentts::download::load_from_hub_cb(
///     "KittenML/kitten-tts-mini-0.8",
///     |p| println!("{p:?}"),
/// ).unwrap();
/// ```
pub fn load_from_hub_cb<F>(repo_id: &str, mut on_progress: F) -> Result<KittenTtsOnnx>
where
    F: FnMut(LoadProgress),
{
    let repo_id = if repo_id.contains('/') {
        repo_id.to_string()
    } else {
        format!("KittenML/{}", repo_id)
    };

    let api = Api::new().context("Failed to initialise HuggingFace Hub client")?;

    // ── config.json ──────────────────────────────────────────────────────────
    on_progress(LoadProgress::Fetching {
        step: 1, total: 4, file: "config.json".into(),
    });
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
    on_progress(LoadProgress::Fetching {
        step: 2, total: 4, file: config.model_file.clone(),
    });
    let model_path = hf_download(&api, &repo_id, &config.model_file)?;

    // ── Voices NPZ ───────────────────────────────────────────────────────────
    on_progress(LoadProgress::Fetching {
        step: 3, total: 4, file: config.voices.clone(),
    });
    let voices_path = hf_download(&api, &repo_id, &config.voices)?;

    // ── Build ONNX session ───────────────────────────────────────────────────
    on_progress(LoadProgress::Loading);
    KittenTtsOnnx::load(
        &model_path,
        &voices_path,
        config.speed_priors,
        config.voice_aliases,
    )
}

/// Download and initialise a [`KittenTtsOnnx`] model from HuggingFace.
///
/// Convenience wrapper around [`load_from_hub_cb`] with a no-op progress
/// callback.  Use [`load_from_hub_cb`] if you need progress reporting.
pub fn load_from_hub(repo_id: &str) -> Result<KittenTtsOnnx> {
    load_from_hub_cb(repo_id, |_| {})
}

/// Convenience alias using the default nano model.
pub fn load_default() -> Result<KittenTtsOnnx> {
    load_from_hub("KittenML/kitten-tts-nano-0.8-int8")
}

/// Return the voice names bundled in a KittenTTS model **without** loading the
/// ONNX session.
///
/// Only `config.json` and the voices NPZ file are fetched (or retrieved from
/// the HuggingFace Hub cache).  This is significantly faster than
/// [`load_from_hub`] because the multi-hundred-millisecond ONNX session build
/// is skipped entirely.
///
/// The returned list contains:
/// * Every key in the voices NPZ (the primary voice identifiers).
/// * Every key in `config.voice_aliases` (friendly-name aliases), deduplicated.
///
/// The list is sorted alphabetically so callers get a stable order.
///
/// # Errors
/// Returns an error only if the Hub is unreachable on the first download AND
/// the files are not already in the local HuggingFace cache.  Once cached,
/// this function never makes a network request.
pub fn list_voices_from_hub(repo_id: &str) -> Result<Vec<String>> {
    let repo_id = if repo_id.contains('/') {
        repo_id.to_string()
    } else {
        format!("KittenML/{}", repo_id)
    };

    let api = Api::new().context("Failed to initialise HuggingFace Hub client")?;

    // ── config.json ──────────────────────────────────────────────────────────
    let config_path = hf_download(&api, &repo_id, "config.json")?;
    let config_bytes = std::fs::read(&config_path)
        .with_context(|| format!("Cannot read config: {}", config_path.display()))?;
    let config: ModelConfig = serde_json::from_slice(&config_bytes)
        .context("Failed to parse config.json")?;

    // ── Voices NPZ (keys only — data arrays not used) ────────────────────────
    let voices_path = hf_download(&api, &repo_id, &config.voices)?;
    let raw = crate::npz::load_npz(&voices_path)
        .with_context(|| format!("Cannot load voices NPZ: {}", voices_path.display()))?;

    // Collect NPZ keys (primary names) + alias friendly-names.
    let mut names: Vec<String> = raw.into_keys().collect();
    for alias_name in config.voice_aliases.keys() {
        if !names.contains(alias_name) {
            names.push(alias_name.clone());
        }
    }
    names.sort();
    Ok(names)
}
