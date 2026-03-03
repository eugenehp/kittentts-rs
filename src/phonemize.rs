//! Phonemisation using the `libespeak-ng` C library.
//!
//! The full implementation (FFI bindings, initialisation, `phonemize()`) is
//! compiled only when the **`espeak`** Cargo feature is enabled.  Without it
//! every public function is still present but returns an informative error,
//! so downstream crates that only use the IPA-input APIs can compile and
//! publish to crates.io without a system libespeak-ng.
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
//! | Platform             | Requirement                                    |
//! |----------------------|------------------------------------------------|
//! | Alpine / Linux       | `apk add espeak-ng-dev` / `apt install libespeak-ng-dev` |
//! | macOS (Homebrew)     | `brew install espeak-ng`                       |
//! | iOS / Android        | Cross-compiled `libespeak-ng.{a,so}`; set `ESPEAK_LIB_DIR` at build time and [`set_data_path`] at runtime |
//!
//! ## Mobile setup (espeak feature)
//! 1. Cross-compile espeak-ng for the target ABI (see the project README).
//! 2. Bundle the `espeak-ng-data/` directory with the app.
//! 3. Before the first call to [`phonemize`], call [`set_data_path`] with the
//!    runtime path to that directory.

use std::path::{Path, PathBuf};

use anyhow::Result;
#[cfg(not(feature = "espeak"))]
use anyhow::anyhow;
use once_cell::sync::OnceCell;

// ─── Runtime data-path (always compiled) ─────────────────────────────────────

/// Optional runtime path to `espeak-ng-data/`.
/// Set by [`set_data_path`] before the first [`phonemize`] call.
static DATA_PATH: OnceCell<PathBuf> = OnceCell::new();

/// Set the path to the `espeak-ng-data` directory.
///
/// **Required on iOS and Android** when the `espeak` feature is enabled: bundle
/// `espeak-ng-data/` with the app and call this with its runtime path before any
/// call to [`phonemize`].
///
/// Optional on desktop — if not called the library searches its compiled-in
/// system path (e.g. `/usr/lib/x86_64-linux-gnu/espeak-ng-data` on Ubuntu,
/// `/usr/share/espeak-ng-data` on Alpine, or the Homebrew prefix on macOS).
///
/// Has no effect when the `espeak` feature is disabled (the path is stored but
/// never used).
///
/// Has no effect if called after [`phonemize`] has already initialised the
/// library.
pub fn set_data_path(path: &Path) {
    // Silently no-op if already set; the library is already (or about to be)
    // initialised with whatever path was set first.
    let _ = DATA_PATH.set(path.to_path_buf());
}

// ─── espeak feature: full FFI implementation ──────────────────────────────────

#[cfg(feature = "espeak")]
mod inner {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int, c_void};
    use std::sync::Mutex;

    use anyhow::{anyhow, Result};
    use once_cell::sync::OnceCell;

    use super::DATA_PATH;

    // ── FFI bindings ──────────────────────────────────────────────────────────
    // Linking is handled by build.rs (pkg-config on desktop, ESPEAK_LIB_DIR on
    // mobile).  No #[link] attribute here so the same source compiles for every
    // target without change.

    extern "C" {
        /// Set the directory that contains `espeak-ng-data/`.
        /// Pass `NULL` to use the library's compiled-in default (works on desktop).
        fn espeak_ng_InitializePath(path: *const c_char);

        /// Initialise the phoneme tables.  Must be called after InitializePath.
        /// Returns ENS_OK (0) on success.
        fn espeak_ng_Initialize(context: *mut c_void) -> c_int;

        /// Select the voice used for phonemisation.
        /// Returns EE_OK (0) on success.
        fn espeak_ng_SetVoiceByName(name: *const c_char) -> c_int;

        /// Translate text to phonemes.
        ///
        /// `textptr` is an in/out pointer: on entry it points to the start of
        /// the text; on return it has advanced past the translated clause, or
        /// been set to `NULL` when the entire text has been consumed.
        ///
        /// Returns a pointer to an internal buffer holding the phonemes for the
        /// current clause, or `NULL` for an empty clause.  Copy the string
        /// before making any further espeak-ng calls (the buffer is overwritten).
        fn espeak_TextToPhonemes(
            textptr: *mut *const c_void,
            textmode: c_int,
            phonememode: c_int,
        ) -> *const c_char;
    }

    /// `textmode` value: input is UTF-8.
    const CHARS_UTF8: c_int = 1;

    /// `phonememode` value: output IPA (bit 1 set).
    const PHONEMES_IPA: c_int = 0x02;

    // ── Global state ──────────────────────────────────────────────────────────

    /// Serialises every call into the espeak-ng library.
    /// espeak-ng uses global state and is not thread-safe.
    pub(super) static LOCK: Mutex<()> = Mutex::new(());

    /// Cached result of the one-time initialisation.
    pub(super) static INIT: OnceCell<std::result::Result<(), String>> = OnceCell::new();

    // ── Initialisation ────────────────────────────────────────────────────────

    /// Called exactly once (inside LOCK) to initialise the espeak-ng library.
    pub(super) fn do_init() -> std::result::Result<(), String> {
        unsafe {
            // Build a CString for the data path, or use NULL for system default.
            let path_cstr: Option<CString> = DATA_PATH.get().map(|p| {
                CString::new(p.to_string_lossy().as_bytes())
                    .expect("espeak data path contains a null byte")
            });
            let path_ptr: *const c_char =
                path_cstr.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

            espeak_ng_InitializePath(path_ptr);

            // ENS_OK = 0
            let status = espeak_ng_Initialize(std::ptr::null_mut());
            if status != 0 {
                return Err(format!(
                    "espeak_ng_Initialize failed (status {:#010x})",
                    status
                ));
            }

            // Select en-us voice — the same one the model was trained on.
            let voice = CString::new("en-us").unwrap();
            let rc = espeak_ng_SetVoiceByName(voice.as_ptr());
            if rc != 0 {
                return Err(format!(
                    "espeak_ng_SetVoiceByName(\"en-us\") failed (rc {})",
                    rc
                ));
            }
        }
        Ok(())
    }

    // ── Public impl ───────────────────────────────────────────────────────────

    pub(super) fn is_available() -> bool {
        let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        INIT.get_or_init(do_init).is_ok()
    }

    pub(super) fn run_phonemize(text: &str) -> Result<String> {
        let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());

        INIT.get_or_init(do_init)
            .as_ref()
            .map_err(|e| anyhow!("espeak-ng: {}", e))?;

        let text_c = CString::new(text)
            .map_err(|_| anyhow!("phonemize: text contains a null byte"))?;

        let mut current: *const c_void = text_c.as_ptr() as *const c_void;
        let mut parts: Vec<String> = Vec::new();

        unsafe {
            while !current.is_null() {
                let phonemes_ptr =
                    espeak_TextToPhonemes(&mut current, CHARS_UTF8, PHONEMES_IPA);

                if phonemes_ptr.is_null() {
                    // Empty clause (e.g. leading whitespace) — keep looping.
                    continue;
                }

                // Copy out before the next call overwrites the internal buffer.
                let chunk = CStr::from_ptr(phonemes_ptr)
                    .to_str()
                    .map_err(|_| anyhow!("espeak-ng returned non-UTF-8 phonemes"))?
                    .trim()
                    .to_owned();

                if !chunk.is_empty() {
                    parts.push(chunk);
                }
            }
        }

        Ok(parts.join(" "))
    }
}

// ─── Public API (always compiled) ─────────────────────────────────────────────

/// Returns `true` if espeak-ng is available and initialises successfully.
///
/// Always returns `false` when the `espeak` Cargo feature is disabled.
///
/// On mobile this will return `false` until [`set_data_path`] has been called
/// with a valid path.
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

/// Convert `text` to IPA phonemes using the espeak-ng `en-us` voice.
///
/// Produces the same output as:
/// ```text
/// espeak-ng --ipa -q -v en-us --stdin
/// ```
///
/// **Requires the `espeak` Cargo feature.**  Returns an error when the feature
/// is disabled — use [`KittenTtsOnnx::generate_from_ipa`] as an alternative
/// that bypasses phonemisation entirely.
///
/// On **mobile** you must call [`set_data_path`] before the first call.
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
            "espeak-ng library initialised but is_espeak_available() returned false"
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
}
