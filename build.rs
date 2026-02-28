//! Build script — locates and links `libespeak-ng` for every supported target.
//!
//! ## Resolution order
//!
//! 1. **`ESPEAK_LIB_DIR`** env var — explicit directory; required for mobile
//!    cross-compilation.  Set it to the directory that contains
//!    `libespeak-ng.{a,so,dylib}`.
//!
//! 2. **pkg-config** — on macOS the search is augmented with Homebrew's
//!    pkgconfig directories so that a plain `brew install espeak-ng` is
//!    sufficient.  The libdir reported by pkg-config is always added as an
//!    explicit `rustc-link-search` to handle Homebrew's non-standard prefix
//!    (`/opt/homebrew`, `/usr/local`) which is not on the linker's default
//!    search path.
//!
//! 3. **Platform path walk** — probes known directories in order:
//!    * macOS: output of `brew --prefix espeak-ng`, then the canonical
//!      Homebrew keg paths for arm64 (`/opt/homebrew`) and x86_64
//!      (`/usr/local`), then `/usr/local/lib`.
//!    * Linux: the Debian/Ubuntu multi-arch directory for the current target,
//!      then `/usr/lib64`, `/usr/lib`, `/usr/local/lib`.
//!
//! ## Static vs dynamic preference
//!
//! At every step the script first looks for `libespeak-ng.a` (static) and
//! only falls back to `libespeak-ng.{so,dylib}` (dynamic) when no static
//! archive is present.  When a static archive is linked the C++ standard
//! library is added explicitly because espeak-ng is a C++ project.
//!
//! ## Mobile cross-compilation
//!
//! ```text
//! ESPEAK_LIB_DIR=/path/to/sysroot/usr/lib cargo build --target aarch64-apple-ios
//! ```
//! iOS requires a static archive; the script emits `static=espeak-ng`
//! automatically when `ESPEAK_LIB_DIR` is set and `libespeak-ng.a` is found.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target_os   = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    println!("cargo:rerun-if-env-changed=ESPEAK_LIB_DIR");
    println!("cargo:rerun-if-env-changed=ESPEAK_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");

    // ── 1. Explicit override ──────────────────────────────────────────────────
    if let Ok(dir) = std::env::var("ESPEAK_LIB_DIR") {
        link_from_dir(&dir, &target_os);
        return;
    }

    // Mobile without explicit dir: fail early with a clear message.
    if matches!(&*target_os, "ios" | "android") {
        panic!(
            "\n\nESPEAK_LIB_DIR is not set.\n\
             Cross-compiling for {target_os} requires a pre-built libespeak-ng:\n\
             \n\
             \t1. Cross-compile espeak-ng for your target ABI.\n\
             \t2. Set ESPEAK_LIB_DIR to the directory that contains libespeak-ng.a\n\
             \t   (e.g. ESPEAK_LIB_DIR=/path/to/sysroot/usr/lib)\n"
        );
    }

    // ── 2. pkg-config ─────────────────────────────────────────────────────────
    // On macOS, pkg-config may not know about Homebrew's prefix unless
    // PKG_CONFIG_PATH is set.  We augment it here with the well-known
    // Homebrew pkgconfig directories before probing.
    if let Some(dir) = try_pkg_config(&target_os) {
        // Emit an explicit link-search so the linker finds the library even
        // when Homebrew's lib dir is not on the default search path.
        println!("cargo:rustc-link-search=native={dir}");
        // pkg-config crate already emitted rustc-link-lib; we're done.
        return;
    }

    // ── 3. Platform path walk ─────────────────────────────────────────────────
    let candidates = candidate_dirs(&target_os, &target_arch);

    // Prefer static archive.
    for dir in &candidates {
        let candidate = Path::new(dir).join("libespeak-ng.a");
        if candidate.exists() {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=static=espeak-ng");
            link_cxx(&target_os); // espeak-ng is C++ — pull in the stdlib
            return;
        }
    }

    // Fall back to dynamic library.
    let dylib = if target_os == "macos" { "libespeak-ng.dylib" } else { "libespeak-ng.so" };
    for dir in &candidates {
        if Path::new(dir).join(dylib).exists() {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=dylib=espeak-ng");
            return;
        }
    }

    // ── 4. Nothing found ──────────────────────────────────────────────────────
    panic!(
        "\n\n\
         kittentts: could not find libespeak-ng.\n\
         \n\
         Install it with:\n\
         \n\
         \t  macOS   :  brew install espeak-ng\n\
         \t  Ubuntu  :  sudo apt install libespeak-ng-dev\n\
         \t  Fedora  :  sudo dnf install espeak-ng-devel\n\
         \t  Alpine  :  apk add espeak-ng-dev\n\
         \t  Arch    :  sudo pacman -S espeak-ng\n\
         \n\
         Or point the build script directly at the library:\n\
         \n\
         \t  ESPEAK_LIB_DIR=/your/path/lib cargo build\n\n"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Emit link directives for the library found inside `dir`.
/// Prefers `libespeak-ng.a`; falls back to the shared library.
fn link_from_dir(dir: &str, target_os: &str) {
    println!("cargo:rustc-link-search=native={dir}");
    if Path::new(dir).join("libespeak-ng.a").exists() {
        println!("cargo:rustc-link-lib=static=espeak-ng");
        link_cxx(target_os);
    } else {
        println!("cargo:rustc-link-lib=dylib=espeak-ng");
    }
}

/// Emit the C++ standard-library link needed when statically linking espeak-ng.
fn link_cxx(target_os: &str) {
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}

/// Try pkg-config, augmenting `PKG_CONFIG_PATH` with Homebrew directories on
/// macOS.  Returns the libdir on success so the caller can emit
/// `rustc-link-search`.
fn try_pkg_config(target_os: &str) -> Option<String> {
    // Build an augmented PKG_CONFIG_PATH.
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
        // formula is keg-only and not linked into the standard Homebrew prefix.
        if let Some(keg) = brew_prefix("espeak-ng") {
            let p = format!("{keg}/lib/pkgconfig");
            if Path::new(&p).is_dir() { extra.insert(0, p); }
        }
    }

    // Prepend extras to the existing PKG_CONFIG_PATH.
    let existing = std::env::var("PKG_CONFIG_PATH").unwrap_or_default();
    if !existing.is_empty() { extra.push(existing); }

    let pkg_path = extra.join(":");

    // Call pkg-config directly so we can pass the augmented path as an
    // environment variable without mutating the process environment.
    let out = Command::new("pkg-config")
        .args(["--libs", "--cflags", "--static", "espeak-ng"])
        .env("PKG_CONFIG_PATH", &pkg_path)
        .output()
        .ok()?;

    if !out.status.success() {
        // Try again without --static (dynamic only).
        let out = Command::new("pkg-config")
            .args(["--libs", "--cflags", "espeak-ng"])
            .env("PKG_CONFIG_PATH", &pkg_path)
            .output()
            .ok()?;
        if !out.status.success() { return None; }
    }

    // Parse the pkg-config output and emit cargo directives.
    let flags = String::from_utf8(out.stdout).ok()?;
    emit_pkg_config_flags(&flags);

    // Also grab the libdir so the caller can emit rustc-link-search.
    let libdir = pkg_config_variable("espeak-ng", "libdir", &pkg_path)
        .unwrap_or_default();

    // Return the libdir (may be empty string if pkg-config didn't report one).
    Some(libdir)
}

/// Parse raw pkg-config `--libs --cflags` output and emit the appropriate
/// `cargo:` directives.
fn emit_pkg_config_flags(flags: &str) {
    for token in flags.split_whitespace() {
        if let Some(path) = token.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else if let Some(lib) = token.strip_prefix("-l") {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
        // -I, -D etc. (cflags) are ignored — phonemize.rs uses no includes.
    }
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

/// Ordered list of directories to probe for `libespeak-ng.{a,so,dylib}`.
fn candidate_dirs(target_os: &str, target_arch: &str) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();

    if target_os == "macos" {
        // `brew --prefix espeak-ng` → exact keg (handles keg-only formulae).
        if let Some(keg) = brew_prefix("espeak-ng") {
            dirs.push(format!("{keg}/lib"));
        }

        // Canonical Homebrew keg-only path for arm64 and x86_64.
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

    // Keep only directories that exist on this machine.
    dirs.into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
