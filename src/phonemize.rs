//! Phonemisation using the `libespeak-ng` C library.
//!
//! Calls the espeak-ng C API directly instead of spawning a subprocess,
//! making it portable to platforms that forbid `fork`/`exec` (iOS) or that
//! lack the `espeak-ng` binary (Android).
//!
//! The output is identical to `espeak-ng --ipa -q -v en-us` because it drives
//! exactly the same translation engine.
//!
//! ## Build requirements
//! | Platform             | Requirement                                    |
//! |----------------------|------------------------------------------------|
//! | Alpine / Linux       | `apk add espeak-ng-dev` / `apt install libespeak-ng-dev` |
//! | macOS (Homebrew)     | `brew install espeak-ng`                       |
//! | iOS / Android        | Cross-compiled `libespeak-ng.{a,so}`; set `ESPEAK_LIB_DIR` at build time and [`set_data_path`] at runtime |
//!
//! ## Mobile setup
//! 1. Cross-compile espeak-ng for the target ABI (see the project README).
//! 2. Bundle the `espeak-ng-data/` directory with the app.
//! 3. Before the first call to [`phonemize`], call [`set_data_path`] with the
//!    runtime path to that directory.

use std::{
    ffi::{CStr, CString},
    os::raw::{c_char, c_int, c_void},
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;

// ─── FFI bindings ─────────────────────────────────────────────────────────────
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
    /// `textptr` is an in/out pointer: on entry it points to the start of the
    /// text; on return it has advanced past the translated clause, or been set
    /// to `NULL` when the entire text has been consumed.
    ///
    /// Returns a pointer to an internal buffer holding the phonemes for the
    /// current clause, or `NULL` for an empty clause.  Copy the string before
    /// making any further espeak-ng calls (the buffer is overwritten).
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

// ─── Global state ─────────────────────────────────────────────────────────────

/// Serialises every call into the espeak-ng library.
/// espeak-ng uses global state and is not thread-safe.
static LOCK: Mutex<()> = Mutex::new(());

/// Cached result of the one-time initialisation.
/// `Ok(())` → ready; `Err(msg)` → init failed (msg is returned to callers).
static INIT: OnceCell<std::result::Result<(), String>> = OnceCell::new();

/// Optional runtime path to `espeak-ng-data/`.
/// Set by [`set_data_path`] before the first [`phonemize`] call.
static DATA_PATH: OnceCell<PathBuf> = OnceCell::new();

// ─── Public configuration ─────────────────────────────────────────────────────

/// Set the path to the `espeak-ng-data` directory.
///
/// **Required on iOS and Android**: bundle `espeak-ng-data/` with the app and
/// call this with its runtime path before any call to [`phonemize`].
///
/// Optional on desktop — if not called the library searches its compiled-in
/// system path (e.g. `/usr/lib/x86_64-linux-gnu/espeak-ng-data` on Ubuntu,
/// `/usr/share/espeak-ng-data` on Alpine, or the Homebrew prefix on macOS).
///
/// Has no effect if called after [`phonemize`] has already initialised the
/// library.
pub fn set_data_path(path: &Path) {
    // Silently no-op if already set; the library is already (or about to be)
    // initialised with whatever path was set first.
    let _ = DATA_PATH.set(path.to_path_buf());
}

// ─── Initialisation ───────────────────────────────────────────────────────────

/// Called exactly once (inside LOCK) to initialise the espeak-ng library.
fn do_init() -> std::result::Result<(), String> {
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

// ─── Public API ───────────────────────────────────────────────────────────────

/// Returns `true` if espeak-ng initialises successfully.
///
/// On mobile this will return `false` until [`set_data_path`] has been called
/// with a valid path.
pub fn is_espeak_available() -> bool {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    INIT.get_or_init(do_init).is_ok()
}

/// Convert `text` to IPA phonemes using the espeak-ng `en-us` voice.
///
/// Produces the same output as:
/// ```text
/// espeak-ng --ipa -q -v en-us --stdin
/// ```
///
/// On **mobile** you must call [`set_data_path`] before the first call.
pub fn phonemize(text: &str) -> Result<String> {
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());

    // Initialise on the first call; cached on every subsequent call.
    INIT.get_or_init(do_init)
        .as_ref()
        .map_err(|e| anyhow!("espeak-ng: {}", e))?;

    let text_c =
        CString::new(text).map_err(|_| anyhow!("phonemize: text contains a null byte"))?;

    // `current` is the "cursor" that espeak_TextToPhonemes advances through the
    // input text one clause at a time.
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability() {
        // If the crate linked (build succeeded), the library is present.
        assert!(
            is_espeak_available(),
            "espeak-ng library initialised but is_espeak_available() returned false"
        );
    }

    #[test]
    fn test_phonemize_hello() {
        let ipa = phonemize("Hello world").expect("phonemize failed");
        assert!(!ipa.is_empty(), "IPA output should not be empty");
        // espeak-ng en-us: "Hello" → contains 'h' or 'ɛ', "world" → contains 'w'
        assert!(
            ipa.contains('h') || ipa.contains('ɛ') || ipa.contains('l'),
            "unexpected IPA for 'Hello world': {ipa}"
        );
        println!("IPA: {ipa}");
    }

    #[test]
    fn test_phonemize_punctuation() {
        // Punctuation should be preserved in IPA output (model was trained with it).
        let ipa = phonemize("Hello, world.").expect("phonemize failed");
        assert!(!ipa.is_empty());
    }

    #[test]
    fn test_phonemize_empty() {
        let ipa = phonemize("").expect("phonemize failed");
        // Empty input should produce empty or whitespace-only output.
        assert!(ipa.trim().is_empty(), "expected empty IPA for empty input, got: {ipa}");
    }
}
