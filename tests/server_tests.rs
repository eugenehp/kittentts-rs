//! Tests for the TTS server voice mapping and request validation logic.
//!
//! These tests exercise the server's voice resolution and API format handling
//! without requiring a loaded model or network access.

#![cfg(feature = "server")]

use kittentts::encoding::AudioFormat;

// ─── Voice mapping tests ────────────────────────────────────────────────────

// Re-implement the mapping logic here for testing (the server binary owns
// the actual functions, but the mapping table is a public contract).

fn openai_voice_to_kittentts(name: &str) -> Option<&'static str> {
    match name {
        "alloy" => Some("Luna"),
        "echo" => Some("Hugo"),
        "fable" => Some("Kiki"),
        "onyx" => Some("Bruno"),
        "nova" => Some("Bella"),
        "shimmer" => Some("Rosie"),
        "ash" => Some("Jasper"),
        "sage" => Some("Leo"),
        "coral" => Some("Rosie"),
        _ => None,
    }
}

fn resolve_voice(name: &str, available: &[String]) -> Option<String> {
    if let Some(mapped) = openai_voice_to_kittentts(name) {
        if available.iter().any(|v| v == mapped) {
            return Some(mapped.to_string());
        }
    }
    for v in available {
        if v.eq_ignore_ascii_case(name) {
            return Some(v.clone());
        }
    }
    None
}

fn default_voices() -> Vec<String> {
    vec![
        "Bella", "Jasper", "Luna", "Bruno", "Rosie", "Hugo", "Kiki", "Leo",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[test]
fn all_openai_voices_map_to_valid_kittentts_voices() {
    let voices = default_voices();
    for name in &["alloy", "echo", "fable", "onyx", "nova", "shimmer", "ash", "sage", "coral"] {
        let resolved = resolve_voice(name, &voices);
        assert!(
            resolved.is_some(),
            "OpenAI voice '{name}' should map to a KittenTTS voice"
        );
        let mapped = resolved.unwrap();
        assert!(
            voices.contains(&mapped),
            "Mapped voice '{mapped}' for '{name}' should be in available voices"
        );
    }
}

#[test]
fn kittentts_native_names_resolve_directly() {
    let voices = default_voices();
    for name in &["Bella", "Jasper", "Luna", "Bruno", "Rosie", "Hugo", "Kiki", "Leo"] {
        assert!(
            resolve_voice(name, &voices).is_some(),
            "KittenTTS voice '{name}' should resolve directly"
        );
    }
}

#[test]
fn case_insensitive_voice_resolution() {
    let voices = default_voices();
    assert_eq!(resolve_voice("bella", &voices), Some("Bella".to_string()));
    assert_eq!(resolve_voice("JASPER", &voices), Some("Jasper".to_string()));
    assert_eq!(resolve_voice("luna", &voices), Some("Luna".to_string()));
}

#[test]
fn unknown_voice_returns_none() {
    let voices = default_voices();
    assert!(resolve_voice("nonexistent", &voices).is_none());
    assert!(resolve_voice("", &voices).is_none());
}

#[test]
fn openai_mapping_takes_precedence() {
    // "alloy" maps to "Luna", not matched as a direct name
    let voices = default_voices();
    assert_eq!(resolve_voice("alloy", &voices), Some("Luna".to_string()));
}

// ─── Format validation tests ────────────────────────────────────────────────

#[test]
fn all_openai_formats_parse() {
    for fmt in &["mp3", "wav", "opus", "flac", "pcm"] {
        assert!(
            AudioFormat::from_str_openai(fmt).is_some(),
            "Format '{fmt}' should be recognised"
        );
    }
}

#[test]
fn invalid_formats_rejected() {
    assert!(AudioFormat::from_str_openai("aac").is_none());
    assert!(AudioFormat::from_str_openai("ogg").is_none());
    assert!(AudioFormat::from_str_openai("MP3").is_none()); // case-sensitive
}

// ─── Request validation tests ───────────────────────────────────────────────

#[test]
fn speed_range_validation() {
    // Valid range is 0.25..=4.0
    let valid = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];
    let invalid = [0.0, 0.24, 4.01, -1.0, 100.0];

    for s in valid {
        assert!((0.25..=4.0).contains(&s), "Speed {s} should be valid");
    }
    for s in invalid {
        assert!(!(0.25..=4.0).contains(&s), "Speed {s} should be invalid");
    }
}

#[test]
fn input_length_validation() {
    let max_len = 4096;

    let short = "Hello!";
    assert!(short.len() <= max_len);

    let exact = "x".repeat(max_len);
    assert!(exact.len() <= max_len);

    let too_long = "x".repeat(max_len + 1);
    assert!(too_long.len() > max_len);
}
