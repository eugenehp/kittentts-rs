import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    // kotlin.android is no longer needed — AGP 9.0 has built-in Kotlin support.
    // alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace   = "com.kittenml.kittentts"
    compileSdk  = 36

    defaultConfig {
        applicationId   = "com.kittenml.kittentts"
        minSdk          = 24
        targetSdk       = 36
        versionCode     = 1
        versionName     = "1.0"

        // Only build the arm64-v8a ABI — the one our build script targets.
        // Remove this filter if you add x86_64 / armeabi-v7a slices later.
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // New compilerOptions DSL — replaces the deprecated kotlinOptions block.
    kotlin {
        compilerOptions {
            jvmTarget = JvmTarget.JVM_17
        }
    }

    buildFeatures {
        compose = true
    }

    // jniLibs default is src/main/jniLibs — AGP 9 picks it up automatically.
    // assets default is src/main/assets — same.
}


dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.kotlinx.coroutines.android)

    debugImplementation(libs.compose.ui.tooling)
}
