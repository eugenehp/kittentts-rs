//! Phonemisation using the pure-Rust `espeak-ng` crate.
//!
//! The full implementation is compiled only when the **`espeak`** Cargo feature
//! is enabled.  Without it every public function is still present but returns
//! an informative error, so downstream crates that only use the IPA-input APIs
//! can compile and publish to crates.io without any espeak dependency.
//!
//! ## Enabling
//!
//! ```toml
//! # Cargo.toml of the consuming crate
//! kittentts = { version = "…", features = ["espeak"] }
//! ```
//!
//! ## Build requirements (when `espeak` feature is on)
//!
//! **None!** The `espeak-ng` crate is a pure-Rust port that bundles its own
//! data files via the `bundled-data-en` feature.  No system library, no
//! pkg-config, no cmake needed.
//!
//! ## Mobile setup (espeak feature)
//! The pure-Rust implementation works on all platforms out of the box.
//! If you need to override the data directory (e.g. for additional languages),
//! call [`set_data_path`] before the first [`phonemize`] call.

use std::path::{Path, PathBuf};

use anyhow::Result;
#[cfg(not(feature = "espeak"))]
use anyhow::anyhow;
use once_cell::sync::OnceCell;

// ─── Runtime data-path (always compiled) ─────────────────────────────────────

/// Optional runtime path to espeak-ng data.
/// Set by [`set_data_path`] before the first [`phonemize`] call.
static DATA_PATH: OnceCell<PathBuf> = OnceCell::new();

/// Set the path to the espeak-ng data directory.
///
/// With the pure-Rust `espeak-ng` crate and the `bundled-data-en` feature,
/// this is **optional** — bundled data is extracted to a temp directory
/// automatically.  Call this only if you need to override the data directory
/// (e.g. for additional languages or custom dictionaries).
///
/// Has no effect if called after [`phonemize`] has already initialised the
/// engine.
pub fn set_data_path(path: &Path) {
    let _ = DATA_PATH.set(path.to_path_buf());
}

// ─── espeak feature: pure-Rust implementation ─────────────────────────────────

#[cfg(feature = "espeak")]
mod inner {
    use std::path::PathBuf;

    use anyhow::{anyhow, Result};
    use once_cell::sync::OnceCell;

    use super::DATA_PATH;

    /// Lazily-initialised data directory for the bundled espeak-ng data.
    /// The bundled data files are extracted here once and reused.
    static BUNDLED_DATA_DIR: OnceCell<PathBuf> = OnceCell::new();

    /// Get or create the data directory with bundled espeak-ng data installed.
    fn get_data_dir() -> Result<&'static PathBuf> {
        // If the user explicitly set a data path, use that.
        if let Some(user_dir) = DATA_PATH.get() {
            // Return a static ref by storing it in BUNDLED_DATA_DIR too.
            return Ok(BUNDLED_DATA_DIR.get_or_init(|| user_dir.clone()));
        }

        BUNDLED_DATA_DIR.get_or_try_init(|| {
            // Use a deterministic cache directory to avoid re-extracting every time.
            let cache_dir = std::env::temp_dir().join("kittentts-espeak-ng-data");
            std::fs::create_dir_all(&cache_dir)
                .map_err(|e| anyhow!("Failed to create espeak-ng data dir: {}", e))?;

            // Install all bundled language data files.
            espeak_ng::install_bundled_data(&cache_dir)
                .map_err(|e| anyhow!("Failed to install bundled espeak-ng data: {}", e))?;

            Ok(cache_dir)
        })
    }

    fn create_engine() -> Result<espeak_ng::EspeakNg> {
        let data_dir = get_data_dir()?;
        espeak_ng::EspeakNg::with_data_dir("en", data_dir)
            .map_err(|e| anyhow!("espeak-ng init failed: {}", e))
    }

    pub(super) fn is_available() -> bool {
        create_engine().is_ok()
    }

    #[cfg(test)]
    pub(super) fn create_engine_for_lang(lang: &str) -> Result<espeak_ng::EspeakNg> {
        let data_dir = get_data_dir()?;
        espeak_ng::EspeakNg::with_data_dir(lang, data_dir)
            .map_err(|e| anyhow!("espeak-ng init for '{}' failed: {}", lang, e))
    }

    pub(super) fn run_phonemize(text: &str) -> Result<String> {
        if text.is_empty() {
            return Ok(String::new());
        }

        let engine = create_engine()?;
        let ipa = engine
            .text_to_phonemes(text)
            .map_err(|e| anyhow!("espeak-ng phonemise failed: {}", e))?;

        Ok(ipa.trim().to_owned())
    }

    #[cfg(test)]
    pub(super) fn run_phonemize_lang(lang: &str, text: &str) -> Result<String> {
        if text.is_empty() {
            return Ok(String::new());
        }

        let engine = create_engine_for_lang(lang)?;
        let ipa = engine
            .text_to_phonemes(text)
            .map_err(|e| anyhow!("espeak-ng phonemise ({}) failed: {}", lang, e))?;

        Ok(ipa.trim().to_owned())
    }
}

// ─── Public API (always compiled) ─────────────────────────────────────────────

/// Returns `true` if espeak-ng is available and initialises successfully.
///
/// Always returns `false` when the `espeak` Cargo feature is disabled.
pub fn is_espeak_available() -> bool {
    #[cfg(feature = "espeak")]
    {
        inner::is_available()
    }
    #[cfg(not(feature = "espeak"))]
    {
        false
    }
}

/// Convert `text` to IPA phonemes using the espeak-ng `en` voice.
///
/// Produces the same output as:
/// ```text
/// espeak-ng --ipa -q -v en --stdin
/// ```
///
/// **Requires the `espeak` Cargo feature.**  Returns an error when the feature
/// is disabled — use [`KittenTtsOnnx::generate_from_ipa`] as an alternative
/// that bypasses phonemisation entirely.
pub fn phonemize(text: &str) -> Result<String> {
    #[cfg(feature = "espeak")]
    {
        inner::run_phonemize(text)
    }
    #[cfg(not(feature = "espeak"))]
    {
        let _ = text;
        Err(anyhow!(
            "phonemize() requires the `espeak` Cargo feature.\n\
             Enable it with: kittentts = {{ features = [\"espeak\"] }}\n\
             Or use generate_from_ipa() to bypass phonemisation."
        ))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "espeak"))]
mod tests {
    use super::*;

    #[test]
    fn test_availability() {
        assert!(
            is_espeak_available(),
            "espeak-ng should be available with bundled data"
        );
    }

    #[test]
    fn test_phonemize_hello() {
        let ipa = phonemize("Hello world").expect("phonemize failed");
        assert!(!ipa.is_empty(), "IPA output should not be empty");
        assert!(
            ipa.contains('h') || ipa.contains('ɛ') || ipa.contains('l'),
            "unexpected IPA for 'Hello world': {ipa}"
        );
        println!("IPA: {ipa}");
    }

    #[test]
    fn test_phonemize_punctuation() {
        let ipa = phonemize("Hello, world.").expect("phonemize failed");
        assert!(!ipa.is_empty());
    }

    #[test]
    fn test_phonemize_empty() {
        let ipa = phonemize("").expect("phonemize failed");
        assert!(ipa.trim().is_empty(), "expected empty IPA for empty input, got: {ipa}");
    }

    #[test]
    fn test_all_bundled_languages() {
        let sample_texts: &[(&str, &str)] = &[
            ("af", "Hallo wêreld"),
            ("am", "ሰላም ዓለም"),
            ("an", "Hola mundo"),
            ("ar", "مرحبا بالعالم"),
            ("as", "নমস্কাৰ পৃথিৱী"),
            ("az", "Salam dünya"),
            ("ba", "Сәләм донъя"),
            ("be", "Прывітанне свет"),
            ("bg", "Здравей свят"),
            ("bn", "হ্যালো বিশ্ব"),
            ("bpy", "হ্যালো বিশ্ব"),
            ("bs", "Zdravo svijete"),
            ("ca", "Hola món"),
            ("chr", "ᎣᏏᏲ ᎡᎶᎯ"),
            ("cmn", "你好世界"),
            ("cs", "Ahoj světe"),
            ("cv", "Салам тĕнче"),
            ("cy", "Helo byd"),
            ("da", "Hej verden"),
            ("de", "Hallo Welt"),
            ("el", "Γεια σου κόσμε"),
            ("en", "Hello world"),
            ("eo", "Saluton mondo"),
            ("es", "Hola mundo"),
            ("et", "Tere maailm"),
            ("eu", "Kaixo mundua"),
            ("fa", "سلام دنیا"),
            ("fi", "Hei maailma"),
            ("fr", "Bonjour le monde"),
            ("ga", "Dia duit a dhomhan"),
            ("gd", "Halò a shaoghail"),
            ("gn", "Mba eichaporã"),
            ("grc", "Χαῖρε κόσμε"),
            ("gu", "હેલો વિશ્વ"),
            ("hak", "你好世界"),
            ("haw", "Aloha honua"),
            ("he", "שלום עולם"),
            ("hi", "नमस्ते दुनिया"),
            ("hr", "Pozdrav svijete"),
            ("ht", "Bonjou mond"),
            ("hu", "Helló világ"),
            ("hy", "Բարեdelays աշdelays"),
            ("ia", "Salute mundo"),
            ("id", "Halo dunia"),
            ("io", "Saluto mondo"),
            ("is", "Halló heimur"),
            ("it", "Ciao mondo"),
            ("ja", "こんにちは世界"),
            ("jbo", "coi rodo"),
            ("ka", "გამარჯობა მსოფლიო"),
            ("kk", "Сәлем әлем"),
            ("kl", "Aluu nunarsuaq"),
            ("kn", "ಹಲೋ ಪ್ರಪಂಚ"),
            ("ko", "안녕하세요 세계"),
            ("kok", "नमस्कार जग"),
            ("ku", "Silav cîhan"),
            ("ky", "Салам дүйнө"),
            ("la", "Salve munde"),
            ("lb", "Moien Welt"),
            ("lfn", "Bon dia mundo"),
            ("lt", "Sveikas pasauli"),
            ("lv", "Sveika pasaule"),
            ("mi", "Kia ora te ao"),
            ("mk", "Здраво свету"),
            ("ml", "ഹലോ ലോകം"),
            ("mr", "नमस्कार जग"),
            ("ms", "Helo dunia"),
            ("mt", "Bongu dinja"),
            ("mto", "Hola mundo"),
            ("my", "မင်္ဂလာပါ ကမ္ဘာ"),
            ("nci", "Niltze cemanahuac"),
            ("ne", "नमस्ते संसार"),
            ("nl", "Hallo wereld"),
            ("no", "Hei verden"),
            ("nog", "Салам дуныя"),
            ("om", "Akkam addunyaa"),
            ("or", "ନମସ୍କାର ବିଶ୍ୱ"),
            ("pa", "ਸਤ ਸ੍ਰੀ ਅਕਾਲ ਦੁਨੀਆ"),
            ("pap", "Bon dia mundo"),
            ("piqd", "nuqneH"),
            ("pl", "Witaj świecie"),
            ("pt", "Olá mundo"),
            ("py", "Hello world"),
            ("qdb", "Hello world"),
            ("qu", "Napaykullayki llaqta"),
            ("quc", "Saqarik uwachulew"),
            ("qya", "Aiya Arda"),
            ("ro", "Salut lume"),
            ("ru", "Привет мир"),
            ("sd", "هيلو دنيا"),
            ("shn", "မႂ်ႇသုင်ႇ လူၵ်ႈ"),
            ("si", "හෙලෝ ලෝකය"),
            ("sjn", "Mae govannen"),
            ("sk", "Ahoj svet"),
            ("sl", "Pozdravljen svet"),
            ("smj", "Buorre beaivi"),
            ("sq", "Përshëndetje botë"),
            ("sr", "Здраво свете"),
            ("sv", "Hej världen"),
            ("sw", "Habari dunia"),
            ("ta", "வணக்கம் உலகம்"),
            ("te", "హలో ప్రపంచం"),
            ("th", "สวัสดีชาวโลก"),
            ("ti", "ሰላም ዓለም"),
            ("tk", "Salam dünýä"),
            ("tn", "Dumela lefatshe"),
            ("tr", "Merhaba dünya"),
            ("tt", "Сәлам дөнья"),
            ("ug", "ياخشىمۇسىز دۇنيا"),
            ("uk", "Привіт світ"),
            ("ur", "ہیلو دنیا"),
            ("uz", "Salom dunyo"),
            ("vi", "Xin chào thế giới"),
            ("yue", "你好世界"),
        ];

        // Languages whose phoneme tables are missing in espeak-ng 0.1.0.
        // These are aliases in the C library (e.g. bs→hr) that the pure-Rust
        // port hasn't wired up yet.
        let known_missing: &[&str] = &["bs", "io", "lfn", "pap"];

        let mut passed = 0;
        let mut empty = Vec::new();
        let mut failed = Vec::new();
        let mut skipped = Vec::new();

        for &(lang, text) in sample_texts {
            match inner::run_phonemize_lang(lang, text) {
                Ok(ipa) if ipa.is_empty() => {
                    println!("  {lang:>5}: {text:30} → (empty)");
                    empty.push(lang);
                    passed += 1;
                }
                Ok(ipa) => {
                    println!("  {lang:>5}: {text:30} → {ipa}");
                    passed += 1;
                }
                Err(e) if known_missing.contains(&lang) => {
                    println!("  {lang:>5}: SKIPPED (known missing) — {e}");
                    skipped.push(lang);
                }
                Err(e) => {
                    eprintln!("  {lang:>5}: FAILED — {e}");
                    failed.push((lang, format!("{e}")));
                }
            }
        }

        let total = sample_texts.len();
        println!("\n{passed}/{total} languages succeeded, {} skipped (known missing)",
            skipped.len());
        if !empty.is_empty() {
            println!("{} languages returned empty IPA: {:?}", empty.len(), empty);
        }
        if !failed.is_empty() {
            println!("Unexpected failures:");
            for (lang, err) in &failed {
                println!("  {lang}: {err}");
            }
            panic!(
                "{} out of {total} languages had unexpected failures",
                failed.len(),
            );
        }
    }
}
