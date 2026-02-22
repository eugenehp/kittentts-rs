/**
 * kittentts_jni.c — JNI bridge between Android/Kotlin and the Rust C API.
 *
 * Compiled by build_rust_android.sh into libkittentts_jni.so, which is placed
 * in app/src/main/jniLibs/arm64-v8a/ together with libespeak-ng.so and
 * libonnxruntime.so.
 *
 * Kotlin companion object loads them in dependency order:
 *   System.loadLibrary("onnxruntime")
 *   System.loadLibrary("espeak-ng")
 *   System.loadLibrary("kittentts_jni")
 *
 * Memory rules (mirrors kittentts.h):
 *   - model handle is stored as a jlong (uintptr_t); 0 means null.
 *   - Every GetStringUTFChars is paired with a ReleaseStringUTFChars.
 *   - Error strings and voice-list strings from Rust are freed immediately after
 *     being converted to a Java String.
 */

#include <jni.h>
#include <stdint.h>
#include <android/log.h>
#include "kittentts.h"

#define LOG_TAG "KittenTTS_JNI"
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO,  LOG_TAG, __VA_ARGS__)

/* ── helpers ──────────────────────────────────────────────────────────────── */

/** Convert a jlong cookie back to a typed pointer. */
static inline KittenTtsHandle *to_handle(jlong cookie) {
    return (KittenTtsHandle *)(uintptr_t)cookie;
}

/* ── JNI methods ──────────────────────────────────────────────────────────── */

/**
 * void KittenTtsLib.nativeSetEspeakDataPath(String path)
 *
 * Must be called once at startup with the path to the extracted
 * espeak-ng-data directory in the app's internal storage.
 */
JNIEXPORT void JNICALL
Java_com_kittenml_kittentts_KittenTtsLib_nativeSetEspeakDataPath(
        JNIEnv *env, jclass cls, jstring path)
{
    const char *p = (*env)->GetStringUTFChars(env, path, NULL);
    LOGI("set_espeak_data_path: %s", p ? p : "(null)");
    kittentts_set_espeak_data_path(p);
    (*env)->ReleaseStringUTFChars(env, path, p);
}

/**
 * long KittenTtsLib.nativeModelLoad(String onnxPath, String voicesPath)
 *
 * Returns a non-zero opaque handle on success, 0 on failure.
 * Free with nativeModelFree().
 */
JNIEXPORT jlong JNICALL
Java_com_kittenml_kittentts_KittenTtsLib_nativeModelLoad(
        JNIEnv *env, jclass cls, jstring onnxPath, jstring voicesPath)
{
    const char *onnx   = (*env)->GetStringUTFChars(env, onnxPath,   NULL);
    const char *voices = (*env)->GetStringUTFChars(env, voicesPath, NULL);

    LOGI("model_load: onnx=%s voices=%s", onnx ? onnx : "(null)",
                                           voices ? voices : "(null)");
    KittenTtsHandle *h = kittentts_model_load(onnx, voices);
    if (!h) {
        LOGE("model_load returned NULL");
    }

    (*env)->ReleaseStringUTFChars(env, onnxPath,   onnx);
    (*env)->ReleaseStringUTFChars(env, voicesPath, voices);

    return (jlong)(uintptr_t)h;
}

/**
 * void KittenTtsLib.nativeModelFree(long handle)
 */
JNIEXPORT void JNICALL
Java_com_kittenml_kittentts_KittenTtsLib_nativeModelFree(
        JNIEnv *env, jclass cls, jlong handle)
{
    kittentts_model_free(to_handle(handle));
}

/**
 * String? KittenTtsLib.nativeModelVoices(long handle)
 *
 * Returns a compact JSON array, e.g. ["expr-voice-2-f","expr-voice-3-m"],
 * or null on error.
 */
JNIEXPORT jstring JNICALL
Java_com_kittenml_kittentts_KittenTtsLib_nativeModelVoices(
        JNIEnv *env, jclass cls, jlong handle)
{
    if (!handle) return NULL;
    const char *json = kittentts_model_voices(to_handle(handle));
    if (!json) return NULL;
    jstring result = (*env)->NewStringUTF(env, json);
    kittentts_free_string(json);
    return result;
}

/**
 * String? KittenTtsLib.nativeSynthesizeToFile(
 *     long handle, String text, String voice, float speed, String outputPath)
 *
 * Returns null on success, or a UTF-8 error message on failure.
 */
JNIEXPORT jstring JNICALL
Java_com_kittenml_kittentts_KittenTtsLib_nativeSynthesizeToFile(
        JNIEnv *env, jclass cls,
        jlong   handle,
        jstring text,
        jstring voice,
        jfloat  speed,
        jstring outputPath)
{
    if (!handle) {
        return (*env)->NewStringUTF(env, "null model handle");
    }

    const char *txt = (*env)->GetStringUTFChars(env, text,       NULL);
    const char *vox = (*env)->GetStringUTFChars(env, voice,      NULL);
    const char *out = (*env)->GetStringUTFChars(env, outputPath, NULL);

    const char *err = kittentts_synthesize_to_file(
            to_handle(handle), txt, vox, (float)speed, out);

    (*env)->ReleaseStringUTFChars(env, text,       txt);
    (*env)->ReleaseStringUTFChars(env, voice,      vox);
    (*env)->ReleaseStringUTFChars(env, outputPath, out);

    if (!err) return NULL;   /* success */

    jstring result = (*env)->NewStringUTF(env, err);
    kittentts_free_error(err);
    return result;
}
