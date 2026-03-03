//! Build script — locates and links `libespeak-ng` **statically** for every
//! supported target.
//!
//! Static linking is unconditional: `libespeak-ng.a` must be present.
//! A dynamic `.so`/`.dylib` is never accepted.  This guarantees that the
//! final binary carries espeak-ng and its companion libraries (libucd,
//! libSpeechPlayer) without any runtime dylib dependency.
//!
//! ## Resolution order
//!
//! 1. **`ESPEAK_LIB_DIR`** env var — explicit directory that contains
//!    `libespeak-ng.a`.  Required for mobile cross-compilation.  Panics if the
//!    directory exists but the static archive is absent.
//!
//! 2. **pkg-config** — augmented with Homebrew's pkgconfig dirs on macOS.
//!    The `libdir` variable reported by pkg-config is probed for
//!    `libespeak-ng.a`; if absent pkg-config is skipped (no dynamic fallback).
//!
//! 3. **Platform path walk** — well-known directories searched in order:
//!    * macOS: `brew --prefix espeak-ng` keg, then `/opt/homebrew` and
//!      `/usr/local` canonical keg paths, then `/usr/local/lib`.
//!    * Linux: Debian/Ubuntu multi-arch dir for the current target,
//!      then `/usr/lib64`, `/usr/lib`, `/usr/local/lib`.
//!
//! If no `libespeak-ng.a` is found anywhere, the build panics with
//! actionable instructions.
//!
//! ## Mobile cross-compilation
//!
//! ```text
//! ESPEAK_LIB_DIR=/path/to/sysroot/usr/lib cargo build --target aarch64-apple-ios
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target_os   = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    println!("cargo:rerun-if-env-changed=ESPEAK_LIB_DIR");
    println!("cargo:rerun-if-env-changed=ESPEAK_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");

    println!("cargo:rerun-if-env-changed=ESPEAK_BUILD_SCRIPT");

    // ── Feature gate ──────────────────────────────────────────────────────────
    // When the `espeak` feature is not enabled the crate compiles without any
    // native system library.  All IPA-input APIs remain available; only the
    // text-to-phoneme (espeak-ng) path is absent.
    if std::env::var("CARGO_FEATURE_ESPEAK").is_err() {
        return;
    }

    // ── 1. Explicit override ──────────────────────────────────────────────────
    if let Ok(dir) = std::env::var("ESPEAK_LIB_DIR") {
        // Auto-build when the archive is missing and a build script is provided.
        if !Path::new(&dir).join("libespeak-ng.a").exists() {
            if let Ok(script) = std::env::var("ESPEAK_BUILD_SCRIPT") {
                run_espeak_build_script(&script, &target_os);
            }
        }
        link_static_from_dir(&dir, &target_os);
        return;
    }

    // Mobile without explicit dir: fail early with a clear message.
    if matches!(&*target_os, "ios" | "android") {
        panic!(
            "\n\nESPEAK_LIB_DIR is not set.\n\
             Cross-compiling for {target_os} requires a pre-built static libespeak-ng:\n\
             \n\
             \t1. Cross-compile espeak-ng for your target ABI.\n\
             \t2. Set ESPEAK_LIB_DIR to the directory containing libespeak-ng.a\n\
             \t   (e.g. ESPEAK_LIB_DIR=/path/to/sysroot/usr/lib)\n"
        );
    }

    // ── 2. pkg-config ─────────────────────────────────────────────────────────
    // Prefer the static archive; on Linux desktop fall back to the dynamic
    // library when only a .so is available (e.g. `apt install libespeak-ng-dev`).
    if let Some(dir) = pkg_config_libdir(&target_os) {
        if Path::new(&dir).join("libespeak-ng.a").exists() {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=static=espeak-ng");
            link_cxx(&target_os);
            return;
        }
        // Linux desktop: accept dynamic library when no static archive exists.
        if target_os == "linux" && has_dylib(&dir) {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=espeak-ng");
            return;
        }
    }

    // ── 3. Platform path walk ─────────────────────────────────────────────────
    // Prefer static; fall back to dynamic on Linux desktop.
    for dir in candidate_dirs(&target_os, &target_arch) {
        if dir.join("libespeak-ng.a").exists() {
            let dir_str = dir.to_string_lossy();
            println!("cargo:rustc-link-search=native={dir_str}");
            println!("cargo:rustc-link-lib=static=espeak-ng");
            link_cxx(&target_os);
            return;
        }
        if target_os == "linux" && has_dylib(dir.to_str().unwrap_or("")) {
            let dir_str = dir.to_string_lossy();
            println!("cargo:rustc-link-search=native={dir_str}");
            println!("cargo:rustc-link-lib=espeak-ng");
            return;
        }
    }

    // ── 4. Nothing found ──────────────────────────────────────────────────────
    panic!(
        "\n\n\
         kittentts: could not find libespeak-ng (static archive preferred, dynamic accepted on Linux).\n\
         \n\
         Install or build espeak-ng, then rebuild:\n\
         \n\
         \t  macOS   :  brew install espeak-ng               (dynamic; static: bash scripts/build-espeak-static.sh)\n\
         \t             ESPEAK_LIB_DIR=src-tauri/espeak-static/lib cargo build --features espeak\n\
         \t  Ubuntu  :  sudo apt install libespeak-ng-dev\n\
         \t  Fedora  :  sudo dnf install espeak-ng-devel\n\
         \t  Alpine  :  apk add espeak-ng-dev espeak-ng-static\n\
         \t  Arch    :  sudo pacman -S espeak-ng\n\
         \n\
         Or point the build script at an existing archive:\n\
         \n\
         \t  ESPEAK_LIB_DIR=/your/path/lib cargo build --features espeak\n\n"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Require `libespeak-ng.a` inside `dir` and emit static link directives.
/// Panics with a clear message if the archive is absent.
fn link_static_from_dir(dir: &str, target_os: &str) {
    let static_lib = Path::new(dir).join("libespeak-ng.a");
    if !static_lib.exists() {
        panic!(
            "\n\nESPEAK_LIB_DIR is set to {dir:?} but libespeak-ng.a was not found there.\n\
             \n\
             Run the preparation script first:\n\
             \n\
             \t  bash scripts/build-espeak-static.sh\n\
             \n\
             That script builds a self-contained static archive at:\n\
             \t  src-tauri/espeak-static/lib/libespeak-ng.a\n\n"
        );
    }
    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-lib=static=espeak-ng");
    link_cxx(target_os);
}

/// Returns `true` if `dir` contains a shared library for espeak-ng
/// (`libespeak-ng.so*` on Linux, `libespeak-ng.dylib` on macOS).
fn has_dylib(dir: &str) -> bool {
    let dir = Path::new(dir);
    // Accept libespeak-ng.so or any versioned variant (e.g. libespeak-ng.so.1).
    if dir.join("libespeak-ng.so").exists() { return true; }
    if dir.join("libespeak-ng.dylib").exists() { return true; }
    // Check for versioned .so.X
    std::fs::read_dir(dir).ok().map_or(false, |mut entries| {
        entries.any(|e| {
            e.ok().and_then(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                if s.starts_with("libespeak-ng.so.") { Some(()) } else { None }
            }).is_some()
        })
    })
}

/// Emit the C++ standard-library link needed when statically linking espeak-ng.
/// espeak-ng is a C++ project; without this the linker cannot resolve C++ symbols.
fn link_cxx(target_os: &str) {
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}

/// Use pkg-config *only* to discover the library installation directory.
/// Returns the `libdir` variable if the package is known to pkg-config,
/// regardless of whether a static archive is present (caller checks that).
///
/// On macOS the search is augmented with Homebrew's pkgconfig directories.
fn pkg_config_libdir(target_os: &str) -> Option<String> {
    let mut extra: Vec<String> = Vec::new();

    if target_os == "macos" {
        // Homebrew arm64 (Apple Silicon) and x86_64 (Intel) well-known paths.
        for prefix in ["/opt/homebrew", "/usr/local"] {
            let p = format!("{prefix}/lib/pkgconfig");
            if Path::new(&p).is_dir() { extra.push(p); }
            let p = format!("{prefix}/share/pkgconfig");
            if Path::new(&p).is_dir() { extra.push(p); }
        }
        // `brew --prefix espeak-ng` gives the exact keg path even when the
        // formula is keg-only and not linked into the standard prefix.
        if let Some(keg) = brew_prefix("espeak-ng") {
            let p = format!("{keg}/lib/pkgconfig");
            if Path::new(&p).is_dir() { extra.insert(0, p); }
        }
    }

    let existing = std::env::var("PKG_CONFIG_PATH").unwrap_or_default();
    if !existing.is_empty() { extra.push(existing); }
    let pkg_path = extra.join(":");

    pkg_config_variable("espeak-ng", "libdir", &pkg_path)
}

/// Call `pkg-config --variable=<var> <package>`.
fn pkg_config_variable(package: &str, var: &str, pkg_path: &str) -> Option<String> {
    let out = Command::new("pkg-config")
        .args([&format!("--variable={var}"), package])
        .env("PKG_CONFIG_PATH", pkg_path)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8(out.stdout).ok()?.trim().to_owned())
    } else {
        None
    }
}

/// Run `brew --prefix <formula>` and return the keg path on success.
fn brew_prefix(formula: &str) -> Option<String> {
    let out = Command::new("brew")
        .args(["--prefix", formula])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8(out.stdout).ok()?.trim().to_owned())
    } else {
        None
    }
}

/// Run `ESPEAK_BUILD_SCRIPT` to compile `libespeak-ng.a` from source.
/// Only invoked when `ESPEAK_LIB_DIR` is set but the archive is absent.
fn run_espeak_build_script(script: &str, target_os: &str) {
    if target_os != "macos" {
        return; // Linux: system package managers provide the lib; no auto-build.
    }

    // Canonicalize to resolve any `..` components injected by .cargo/config.toml.
    let script_path = std::fs::canonicalize(script)
        .unwrap_or_else(|_| Path::new(script).to_path_buf());

    if !script_path.exists() {
        eprintln!(
            "kittentts build.rs: ESPEAK_BUILD_SCRIPT={script:?} not found — skipping auto-build.\n\
             Run manually:  bash scripts/build-espeak-static.sh"
        );
        return;
    }

    eprintln!("kittentts build.rs: libespeak-ng.a not found — running {} …", script_path.display());

    // Augment PATH so cmake, libtool, nm, git are found even when Cargo
    // launched us with a minimal PATH (no Homebrew prefix).
    let current_path = std::env::var("PATH").unwrap_or_default();
    let full_path = format!(
        "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:{current_path}"
    );

    // `exec 1>&2` merges the script's stdout into stderr so cmake build errors
    // (which go to stdout) are visible in Cargo's `--- stderr` output.
    // Without this, Cargo silently drops all non-`cargo:` stdout lines.
    let shell_cmd = format!("exec 1>&2; bash '{}'", script_path.display());

    let status = Command::new("bash")
        .args(["-c", &shell_cmd])
        .env("PATH", &full_path)
        .status()
        .unwrap_or_else(|e| panic!("kittentts build.rs: failed to launch {script:?}: {e}"));

    if !status.success() {
        panic!(
            "\n\nkittentts build.rs: {script:?} failed ({status}).\n\
             See the script output above for the exact error.\n\n"
        );
    }
}

/// Ordered list of directories to probe for `libespeak-ng.a`.
/// Only directories that exist on this machine are returned.
fn candidate_dirs(target_os: &str, target_arch: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<String> = Vec::new();

    if target_os == "macos" {
        // `brew --prefix espeak-ng` → exact keg (handles keg-only formulae).
        if let Some(keg) = brew_prefix("espeak-ng") {
            dirs.push(format!("{keg}/lib"));
        }
        // Canonical Homebrew keg paths for arm64 and x86_64.
        for prefix in ["/opt/homebrew", "/usr/local"] {
            dirs.push(format!("{prefix}/opt/espeak-ng/lib"));
            dirs.push(format!("{prefix}/lib"));
        }
    } else {
        // Linux: Debian/Ubuntu multi-arch directory for the current target.
        let multiarch = match &*target_arch {
            "x86_64"      => "x86_64-linux-gnu",
            "aarch64"     => "aarch64-linux-gnu",
            "arm"         => "arm-linux-gnueabihf",
            "riscv64"     => "riscv64-linux-gnu",
            "s390x"       => "s390x-linux-gnu",
            "powerpc64le" => "powerpc64le-linux-gnu",
            _             => "",
        };
        if !multiarch.is_empty() {
            dirs.push(format!("/usr/lib/{multiarch}"));
        }
        dirs.extend(["/usr/lib64", "/usr/lib", "/usr/local/lib"].map(String::from));
    }

    dirs.into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}
