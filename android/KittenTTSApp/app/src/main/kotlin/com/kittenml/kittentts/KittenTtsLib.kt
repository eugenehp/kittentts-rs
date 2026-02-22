package com.kittenml.kittentts

/**
 * Thin Kotlin wrapper around the native JNI bridge (libkittentts_jni.so).
 *
 * Load order matters: ORT and espeak-ng are shared libraries that
 * libkittentts_jni.so depends on at link time.  Loading them first ensures
 * Android's dynamic linker can resolve their symbols.
 *
 * Callers use [TTSEngine], which manages the model handle lifecycle.
 * Direct use of this object is possible but requires careful handle management.
 *
 * ## Memory contract (mirrors kittentts.h)
 * | Call                    | Release with        |
 * |-------------------------|---------------------|
 * | [nativeModelLoad]       | [nativeModelFree]   |
 * | [nativeModelVoices]     | automatic (String)  |
 * | [nativeSynthesizeToFile]| automatic (String?) |
 */
object KittenTtsLib {

    /**
     * Non-null when any [System.loadLibrary] call failed at startup.
     * TTSEngine checks this before calling any native function.
     */
    val loadError: String?

    init {
        // Load shared dependencies in order so the linker can resolve symbols
        // when each subsequent library is opened.
        //
        // libc++_shared  — NDK C++ runtime; libonnxruntime.so is linked against it.
        // onnxruntime    — ONNX Runtime; libkittentts_jni.so calls into it via Rust.
        // espeak-ng      — phonemiser; libkittentts_jni.so calls into it via Rust.
        // kittentts_jni  — JNI bridge + Rust staticlib; depends on all of the above.
        loadError = try {
            System.loadLibrary("c++_shared")
            System.loadLibrary("onnxruntime")
            System.loadLibrary("espeak-ng")
            System.loadLibrary("kittentts_jni")
            null   // success
        } catch (e: UnsatisfiedLinkError) {
            "Native libraries missing — run android/build_rust_android.sh first.\n\n${e.message}"
        }
    }

    // ── C API wrappers ────────────────────────────────────────────────────────

    /**
     * Set the path to the extracted `espeak-ng-data/` directory.
     * Must be called once at startup before any synthesis.
     */
    external fun nativeSetEspeakDataPath(path: String)

    /**
     * Load a KittenTTS model.
     * @return opaque handle (> 0) on success, 0 on failure.
     */
    external fun nativeModelLoad(onnxPath: String, voicesPath: String): Long

    /** Free a model handle returned by [nativeModelLoad]. */
    external fun nativeModelFree(handle: Long)

    /**
     * Return the available voices as a compact JSON array string,
     * e.g. `["expr-voice-2-f","expr-voice-3-m"]`, or `null` on error.
     */
    external fun nativeModelVoices(handle: Long): String?

    /**
     * Synthesise [text] and write a 16-bit PCM WAV to [outputPath].
     * Blocks until inference is complete — call from a background thread.
     *
     * @return `null` on success, or a UTF-8 error message on failure.
     */
    external fun nativeSynthesizeToFile(
        handle: Long,
        text: String,
        voice: String,
        speed: Float,
        outputPath: String,
    ): String?
}
