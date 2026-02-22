//! C FFI — bridges [`KittenTtsOnnx`] to iOS / Android callers.
//!
//! Functions are `#[no_mangle] extern "C"` so Swift / Kotlin can call them
//! through a thin bridging header without any Objective-C wrapper.
//!
//! ## Memory contract
//!
//! | Function                          | Caller frees with          |
//! |-----------------------------------|----------------------------|
//! | [`kittentts_model_load`]          | [`kittentts_model_free`]   |
//! | [`kittentts_model_voices`]        | [`kittentts_free_string`]  |
//! | [`kittentts_synthesize_to_file`]  | [`kittentts_free_error`]   |

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::path::Path;

use crate::model::KittenTtsOnnx;
use crate::phonemize;

// ─────────────────────────────────────────────────────────────────────────────

/// Opaque handle to a loaded KittenTTS model.
pub struct KittenTtsHandle {
    model: KittenTtsOnnx,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a non-null `*const c_char` to an owned `String`.
/// Returns `None` if `ptr` is null, `Err` if the bytes are not valid UTF-8.
unsafe fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
}

/// Heap-allocate an owned C string.  Returns null on interior nul bytes.
fn to_c_str(s: &str) -> *const c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null(),
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Set the `espeak-ng-data/` directory path.
///
/// **Must be called once at app startup on iOS / Android before any synthesis.**
/// On desktop, pass `NULL` to let espeak-ng locate its system data automatically.
///
/// ```c
/// kittentts_set_espeak_data_path("/var/mobile/Containers/Data/…/espeak-ng-data");
/// ```
#[no_mangle]
pub unsafe extern "C" fn kittentts_set_espeak_data_path(path: *const c_char) {
    if let Some(s) = unsafe { cstr_to_string(path) } {
        phonemize::set_data_path(Path::new(&s));
    }
}

/// Load a KittenTTS model from disk.
///
/// @param onnx_path    UTF-8 path to `kitten_tts_mini_v0_8.onnx`.
/// @param voices_path  UTF-8 path to `voices.npz`.
/// @return             Opaque model handle, or `NULL` on failure (details to stderr).
///                     Free with [`kittentts_model_free`].
#[no_mangle]
pub unsafe extern "C" fn kittentts_model_load(
    onnx_path: *const c_char,
    voices_path: *const c_char,
) -> *mut KittenTtsHandle {
    let (Some(onnx), Some(voices)) = (
        unsafe { cstr_to_string(onnx_path) },
        unsafe { cstr_to_string(voices_path) },
    ) else {
        eprintln!("[kittentts] kittentts_model_load: null argument");
        return std::ptr::null_mut();
    };

    match KittenTtsOnnx::load(
        Path::new(&onnx),
        Path::new(&voices),
        HashMap::new(), // speed_priors  — use model defaults
        HashMap::new(), // voice_aliases — no aliasing
    ) {
        Ok(model) => Box::into_raw(Box::new(KittenTtsHandle { model })),
        Err(e) => {
            eprintln!("[kittentts] load error: {e:#}");
            std::ptr::null_mut()
        }
    }
}

/// Return a JSON array of available voice names.
///
/// Example return value: `["expr-voice-2-f","expr-voice-3-m",…]`
///
/// @param model  Handle from [`kittentts_model_load`].
/// @return       Heap-allocated UTF-8 JSON string, or `NULL` on error.
///               Free with [`kittentts_free_string`].
#[no_mangle]
pub unsafe extern "C" fn kittentts_model_voices(
    model: *const KittenTtsHandle,
) -> *const c_char {
    if model.is_null() {
        return std::ptr::null();
    }
    let h = unsafe { &*model };
    let quoted: Vec<String> = h
        .model
        .available_voices
        .iter()
        .map(|v| format!("\"{}\"", v.replace('"', "\\\"")))
        .collect();
    let json = format!("[{}]", quoted.join(","));
    to_c_str(&json)
}

/// Synthesise `text` and write a 32-bit float WAV to `output_path`.
///
/// @param model        Handle from [`kittentts_model_load`].
/// @param text         UTF-8 text to speak.
/// @param voice        Voice name — must be one of the voices from
///                     [`kittentts_model_voices`].
/// @param speed        Speed multiplier (1.0 = normal, 0.5 = half, 2.0 = double).
/// @param output_path  Writable path for the output `.wav` file.
/// @return             `NULL` on success; on failure a heap-allocated UTF-8 error
///                     message that the caller must release with
///                     [`kittentts_free_error`].
#[no_mangle]
pub unsafe extern "C" fn kittentts_synthesize_to_file(
    model: *const KittenTtsHandle,
    text: *const c_char,
    voice: *const c_char,
    speed: f32,
    output_path: *const c_char,
) -> *const c_char {
    macro_rules! bail {
        ($msg:literal) => {
            return to_c_str($msg);
        };
        ($fmt:expr, $($arg:tt)*) => {
            return to_c_str(&format!($fmt, $($arg)*));
        };
    }

    if model.is_null() {
        bail!("null model handle");
    }
    let (Some(txt), Some(vox), Some(out)) = (
        unsafe { cstr_to_string(text) },
        unsafe { cstr_to_string(voice) },
        unsafe { cstr_to_string(output_path) },
    ) else {
        bail!("null argument (text, voice, or output_path)");
    };

    let h = unsafe { &*model };
    match h.model.generate_to_file(
        &txt,
        Path::new(&out),
        &vox,
        speed,
        /*clean_text=*/ true,
    ) {
        Ok(()) => std::ptr::null(),
        Err(e) => to_c_str(&format!("{e:#}")),
    }
}

/// Free a string returned by [`kittentts_model_voices`].
#[no_mangle]
pub unsafe extern "C" fn kittentts_free_string(s: *const c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s as *mut c_char) });
    }
}

/// Free an error string returned by [`kittentts_synthesize_to_file`].
#[no_mangle]
pub unsafe extern "C" fn kittentts_free_error(s: *const c_char) {
    unsafe { kittentts_free_string(s) };
}

/// Destroy a model handle and release all resources.
#[no_mangle]
pub unsafe extern "C" fn kittentts_model_free(model: *mut KittenTtsHandle) {
    if !model.is_null() {
        drop(unsafe { Box::from_raw(model) });
    }
}
