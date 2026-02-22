/**
 * kittentts.h — C API for the KittenTTS Rust library.
 *
 * Import this header into your Swift project through the Objective-C
 * bridging header:
 *
 *   // KittenTTS-Bridging-Header.h
 *   #import "kittentts.h"
 *
 * Build the Rust library first with ios/build_rust_ios.sh, which produces
 *   ios/KittenTTS.xcframework
 * then add that XCFramework to your Xcode target's Frameworks phase.
 *
 * Memory rules
 * ────────────
 *  • KittenTtsHandle  — created by kittentts_model_load(), freed by kittentts_model_free().
 *  • Voice-list JSON  — returned by kittentts_model_voices(), freed by kittentts_free_string().
 *  • Error strings    — returned by kittentts_synthesize_to_file(), freed by kittentts_free_error().
 *    NULL return from synthesize means success (no string to free).
 */

#pragma once

#ifdef __cplusplus
extern "C" {
#endif

/** Opaque model handle.  Never dereference this from Swift/ObjC. */
typedef struct KittenTtsHandle KittenTtsHandle;

/**
 * Set the espeak-ng phoneme-data directory.
 *
 * MUST be called once at app launch before any synthesis call on iOS/Android.
 * Bundle `espeak-ng-data/` with your app and pass its runtime path here.
 * On desktop you may pass NULL to let espeak-ng locate data automatically.
 *
 *   kittentts_set_espeak_data_path(
 *       [[[NSBundle mainBundle] resourcePath]
 *            stringByAppendingPathComponent:@"espeak-ng-data"].UTF8String
 *   );
 */
void kittentts_set_espeak_data_path(const char *_Nonnull path);

/**
 * Load a KittenTTS model from an ONNX file and a voices NPZ file.
 *
 * @param onnx_path    Absolute path to `kitten_tts_mini_v0_8.onnx`.
 * @param voices_path  Absolute path to `voices.npz`.
 * @return             Opaque model handle, or NULL on failure (details to stderr).
 *                     Release with kittentts_model_free().
 */
KittenTtsHandle * _Nullable kittentts_model_load(
    const char * _Nonnull onnx_path,
    const char * _Nonnull voices_path
);

/**
 * Return the available voice names as a compact JSON array string.
 *
 * Example: `["expr-voice-2-f","expr-voice-3-m","expr-voice-4-f"]`
 *
 * @param model  Handle from kittentts_model_load().
 * @return       Heap-allocated UTF-8 string, or NULL on error.
 *               Release with kittentts_free_string().
 */
const char * _Nullable kittentts_model_voices(
    const KittenTtsHandle * _Nonnull model
);

/**
 * Synthesise text and write a 32-bit float WAV file.
 *
 * The call blocks until inference is complete.  Run it off the main thread.
 *
 * @param model        Handle from kittentts_model_load().
 * @param text         UTF-8 text to speak.
 * @param voice        One of the names returned by kittentts_model_voices().
 * @param speed        Speed multiplier — 1.0 is normal, 0.5 is slower, 2.0 faster.
 * @param output_path  Writable file path for the output WAV.
 * @return             NULL on success.
 *                     On failure: heap-allocated UTF-8 error message.
 *                     Release with kittentts_free_error().
 */
const char * _Nullable kittentts_synthesize_to_file(
    const KittenTtsHandle * _Nonnull model,
    const char * _Nonnull  text,
    const char * _Nonnull  voice,
    float                  speed,
    const char * _Nonnull  output_path
);

/** Free a string returned by kittentts_model_voices(). */
void kittentts_free_string(const char * _Nullable s);

/** Free an error string returned by kittentts_synthesize_to_file(). */
void kittentts_free_error(const char * _Nullable s);

/** Destroy a model handle and release all associated memory. */
void kittentts_model_free(KittenTtsHandle * _Nullable model);

#ifdef __cplusplus
}
#endif
