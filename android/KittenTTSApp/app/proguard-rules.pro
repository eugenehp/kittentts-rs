# KittenTTS ProGuard rules
# The JNI methods are called from native code — keep their Kotlin names.
-keep class com.kittenml.kittentts.KittenTtsLib { *; }
