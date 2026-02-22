//! Basic KittenTTS example — downloads the model and synthesises speech.
//!
//! Usage:
//!   cargo run --example basic
//!   cargo run --example basic -- --voice Jasper --text "Hello from Rust!"
//!
//! Requirements:
//!   - espeak-ng on $PATH (apk add espeak-ng / apt install espeak-ng)
//!   - Internet access for the first run (model is cached afterwards)

use std::path::Path;

fn main() -> anyhow::Result<()> {
    // ── Parse simple CLI arguments ───────────────────────────────────────────
    let mut args = std::env::args().skip(1).peekable();

    let mut model_id = "KittenML/kitten-tts-mini-0.8".to_string();
    let mut voice    = "Jasper".to_string();
    let mut text     = "This high quality TTS model works without a GPU.".to_string();
    let mut output   = "output.wav".to_string();
    let mut speed    = 1.0f32;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model"  => { if let Some(v) = args.next() { model_id = v; } }
            "--voice"  => { if let Some(v) = args.next() { voice    = v; } }
            "--text"   => { if let Some(v) = args.next() { text     = v; } }
            "--output" => { if let Some(v) = args.next() { output   = v; } }
            "--speed"  => { if let Some(v) = args.next() { speed    = v.parse().unwrap_or(1.0); } }
            "--help"   => {
                println!(
                    "Usage: basic [--model REPO_ID] [--voice NAME] \
                     [--text TEXT] [--output FILE] [--speed FLOAT]"
                );
                return Ok(());
            }
            _ => {}
        }
    }

    // ── Check espeak-ng ──────────────────────────────────────────────────────
    if !kittentts::phonemize::is_espeak_available() {
        eprintln!(
            "WARNING: espeak-ng not found on $PATH.\n\
             Install with:  apk add espeak-ng  (Alpine)\n\
             Or:            apt install espeak-ng  (Debian/Ubuntu)\n\
             Or:            brew install espeak-ng  (macOS)"
        );
    }

    // ── Download / load model ────────────────────────────────────────────────
    println!("Model  : {}", model_id);
    println!("Voice  : {}", voice);
    println!("Text   : {:?}", text);
    println!("Speed  : {}", speed);
    println!("Output : {}", output);
    println!();

    let tts = kittentts::download::load_from_hub(&model_id)?;

    println!("Available voices: {:?}", tts.available_voices);

    // ── Generate audio ───────────────────────────────────────────────────────
    println!("\nSynthesising speech…");
    tts.generate_to_file(&text, Path::new(&output), &voice, speed, true)?;

    println!("Done!");
    Ok(())
}
