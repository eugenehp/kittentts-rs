//! Build script — links `libespeak-ng`.
//!
//! Resolution order:
//!   1. `ESPEAK_LIB_DIR` env var  — explicit path (mobile cross-compilation)
//!   2. pkg-config                — standard desktop discovery
//!   3. bare `-lespeak-ng`        — linker searches its default paths
//!
//! ## Mobile cross-compilation
//!
//! Set these env vars when building for iOS / Android:
//!
//! ```text
//! ESPEAK_LIB_DIR=/path/to/libespeak-ng.(a|so)  parent directory
//! ```
//!
//! iOS requires a static library (`libespeak-ng.a`); the build script emits
//! `rustc-link-lib=static=espeak-ng` automatically for that target.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    println!("cargo:rerun-if-env-changed=ESPEAK_LIB_DIR");

    // ── Explicit library directory (mobile cross-compilation) ─────────────────
    if let Ok(dir) = std::env::var("ESPEAK_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        let kind = if target_os == "ios" { "static" } else { "dylib" };
        println!("cargo:rustc-link-lib={kind}=espeak-ng");
        return;
    }

    // ── pkg-config (desktop Linux / macOS) ────────────────────────────────────
    if !matches!(target_os.as_str(), "ios" | "android") {
        if pkg_config::Config::new()
            .atleast_version("1.49")
            .probe("espeak-ng")
            .is_ok()
        {
            // pkg-config emits all necessary rustc-link-* lines itself.
            return;
        }
    }

    // ── Fallback: let the linker search its default paths ─────────────────────
    let kind = if target_os == "ios" { "static" } else { "dylib" };
    println!("cargo:rustc-link-lib={kind}=espeak-ng");
}
