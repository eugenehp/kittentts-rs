// Top-level build file.
// Kotlin plugins are applied directly in app/build.gradle.kts via the
// version catalog — no need to re-declare them here with apply false.
plugins {
    alias(libs.plugins.android.application) apply false
}
