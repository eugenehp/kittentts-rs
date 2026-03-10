//! Build script — locates and links `libespeak-ng` for every supported target,
//! including cross-compilation scenarios.
//!
//! Static linking is preferred.  On Linux and Windows desktop a dynamic
//! fallback is accepted when no static archive is found.
//!
//! ## Resolution order (when the `espeak` feature is enabled)
//!
//! 1. **`ESPEAK_LIB_DIR`** env var — explicit directory containing the static
//!    archive.  Panics if the dir exists but no archive is present.
//!
//! 2. **pkg-config** — cross-aware: tries `<multiarch>-pkg-config` first when
//!    cross-compiling, then falls back to `pkg-config` with
//!    `PKG_CONFIG_ALLOW_CROSS=1`.  On macOS, Homebrew's pkgconfig directories
//!    are prepended automatically.
//!
//! 3. **Platform path walk** — well-known directories for each target OS,
//!    prefixed with `ESPEAK_SYSROOT` when set.
//!
//! If nothing is found the build panics with platform-specific instructions.
//!
//! ## Native builds (quick-start)
//!
//! ```text
//! macOS  :  brew install espeak-ng
//! Ubuntu :  sudo apt install libespeak-ng-dev
//! Alpine :  apk add espeak-ng-dev espeak-ng-static
//! Arch   :  sudo pacman -S espeak-ng
//! Windows:  see panic message for options
//! ```
//!
//! ## Cross-compilation
//!
//! ### Linux → Linux aarch64 (Debian/Ubuntu multiarch)
//! ```text
//! sudo dpkg --add-architecture arm64
//! sudo apt update
//! sudo apt install libespeak-ng-dev:arm64
//! cargo build --target aarch64-unknown-linux-gnu --features espeak
//! ```
//! pkg-config is discovered automatically via the `aarch64-linux-gnu-pkg-config`
//! binary (install `pkg-config-aarch64-linux-gnu` if needed) or via the
//! multiarch pkgconfig dir `/usr/lib/aarch64-linux-gnu/pkgconfig/`.
//!
//! ### Linux → Linux (musl / custom sysroot)
//! ```text
//! ESPEAK_SYSROOT=/path/to/musl-sysroot \
//!   cargo build --target x86_64-unknown-linux-musl --features espeak
//! ```
//! The build script prepends the sysroot to every Unix candidate path.
//!
//! ### Linux → Android (via NDK)
//! Build espeak-ng against the NDK sysroot first, then:
//! ```text
//! ESPEAK_LIB_DIR=/path/to/espeak-ng-android-arm64/lib \
//!   cargo build --target aarch64-linux-android --features espeak
//! ```
//! Or set `ANDROID_NDK_HOME` and place `libespeak-ng.a` under the NDK sysroot
//! lib directory.
//!
//! ### macOS → iOS
//! ```text
//! ESPEAK_LIB_DIR=/path/to/espeak-ng-ios/lib \
//!   cargo build --target aarch64-apple-ios --features espeak
//! ```
//!
//! ### Any host → any target (generic)
//! Point `ESPEAK_LIB_DIR` directly at the pre-built library directory:
//! ```text
//! ESPEAK_LIB_DIR=/sysroot/usr/lib/aarch64-linux-gnu \
//!   cargo build --target aarch64-unknown-linux-gnu --features espeak
//! ```
//!
//! Or set a sysroot and let the build script find the lib under it:
//! ```text
//! ESPEAK_SYSROOT=/path/to/target-sysroot \
//!   cargo build --target aarch64-unknown-linux-gnu --features espeak
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    // ── Cargo environment ─────────────────────────────────────────────────────
    let host   = std::env::var("HOST").unwrap_or_default();
    let target = std::env::var("TARGET").unwrap_or_default();

    let target_os   = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_env  = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let is_cross = host != target;
    let host_os  = os_from_triple(&host);

    // Derive the GNU multiarch tuple from target fields (used for Debian/Ubuntu
    // multiarch pkg-config and library paths, e.g. "aarch64-linux-gnu").
    let multiarch = target_to_multiarch(&target_arch, &target_os, &target_env);

    // ── Rerun triggers ────────────────────────────────────────────────────────
    for var in &[
        "ESPEAK_LIB_DIR",
        "ESPEAK_INCLUDE_DIR",
        "ESPEAK_SYSROOT",
        "ESPEAK_BUILD_SCRIPT",
        "PKG_CONFIG_PATH",
        "PKG_CONFIG_SYSROOT_DIR",
        "PKG_CONFIG_ALLOW_CROSS",
        "VCPKG_ROOT",
        "MSYS2_PATH",
        "ANDROID_NDK_HOME",
        "ANDROID_NDK_ROOT",
        "NDK_HOME",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    // ── Feature gate ──────────────────────────────────────────────────────────
    if std::env::var("CARGO_FEATURE_ESPEAK").is_err() {
        return;
    }

    // Sysroot used to prefix candidate lib dirs when cross-compiling.
    let sysroot: Option<String> = std::env::var("ESPEAK_SYSROOT").ok()
        .filter(|s| !s.is_empty());

    // ── 1. Explicit lib-dir override ──────────────────────────────────────────
    if let Ok(dir) = std::env::var("ESPEAK_LIB_DIR") {
        // Auto-build when the archive is missing and a build script is provided.
        if !static_lib_exists(&dir, &target_os, &target_env) {
            if let Ok(script) = std::env::var("ESPEAK_BUILD_SCRIPT") {
                run_espeak_build_script(
                    &script,
                    &target_os,
                    host_os,
                    &target,
                    sysroot.as_deref(),
                );
            }
        }
        link_static_from_dir(&dir, &target_os, &target_env);
        return;
    }

    // ── iOS / Android without ESPEAK_LIB_DIR: fail early ─────────────────────
    if matches!(target_os.as_str(), "ios" | "android") {
        let ndk_hint = if target_os == "android" {
            "\n\
             For Android, first cross-compile espeak-ng with the NDK, then:\n\
             \t  ESPEAK_LIB_DIR=/path/to/espeak-ng-android/lib \\\n\
             \t    cargo build --target <android-triple> --features espeak\n\
             \n\
             Hint: set ANDROID_NDK_HOME so the build script can print the\n\
             expected sysroot path."
        } else {
            ""
        };
        panic!(
            "\n\nESPEAK_LIB_DIR is not set.\n\
             Cross-compiling for {target_os} requires a pre-built static libespeak-ng:\n\
             \n\
             \t1. Cross-compile espeak-ng for your target ABI.\n\
             \t2. Set ESPEAK_LIB_DIR to the directory containing {lib}.\n\
             \t   e.g. ESPEAK_LIB_DIR=/path/to/sysroot/usr/lib\n\
             {ndk_hint}\n",
            lib = static_lib_name(&target_os, &target_env),
        );
    }

    // ── 2. pkg-config ─────────────────────────────────────────────────────────
    if let Some(dir) = pkg_config_libdir(
        &target_os,
        host_os,
        multiarch,
        is_cross,
        sysroot.as_deref(),
    ) {
        if static_lib_exists(&dir, &target_os, &target_env) {
            emit_static_link(&dir, &target_os, &target_env);
            return;
        }
        // Linux / Windows desktop: accept dynamic library as a fallback.
        if matches!(target_os.as_str(), "linux" | "windows") && has_dylib(&dir, &target_os) {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=espeak-ng");
            return;
        }
    }

    // ── 3. Platform path walk ─────────────────────────────────────────────────
    for dir in candidate_dirs(
        &target_os,
        &target_arch,
        &target_env,
        host_os,
        sysroot.as_deref(),
    ) {
        let dir_str = dir.to_string_lossy().into_owned();
        if static_lib_exists(&dir_str, &target_os, &target_env) {
            emit_static_link(&dir_str, &target_os, &target_env);
            return;
        }
        if matches!(target_os.as_str(), "linux" | "windows") && has_dylib(&dir_str, &target_os) {
            println!("cargo:rustc-link-search=native={dir_str}");
            println!("cargo:rustc-link-lib=espeak-ng");
            return;
        }
    }

    // ── 4. Nothing found ──────────────────────────────────────────────────────
    let lib = static_lib_name(&target_os, &target_env);
    let cross_hint = if is_cross {
        format!(
            "\nCross-compiling {host} → {target}.\n\
             The easiest fix is to point ESPEAK_LIB_DIR at a pre-built archive:\n\
             \n\
             \t  ESPEAK_LIB_DIR=/path/to/espeak-ng-{target_os}/lib \\\n\
             \t    cargo build --target {target} --features espeak\n\
             \n\
             Or set ESPEAK_SYSROOT to a sysroot that contains {lib}:\n\
             \n\
             \t  ESPEAK_SYSROOT=/path/to/target-sysroot \\\n\
             \t    cargo build --target {target} --features espeak\n",
        )
    } else {
        String::new()
    };

    panic!(
        "\n\n\
         kittentts: could not find libespeak-ng for target '{target_os}'.\n\
         {cross_hint}\n\
         Native install instructions:\n\
         \n\
         \t  Windows (MSVC) :  Install from https://github.com/espeak-ng/espeak-ng/releases\n\
         \t                     or:  vcpkg install espeak-ng:x64-windows-static\n\
         \t                     or:  pacman -S mingw-w64-x86_64-espeak-ng  (MSYS2)\n\
         \t                     Then: ESPEAK_LIB_DIR=C:\\path\\to\\espeak-ng\\lib\n\
         \t  macOS           :  brew install espeak-ng\n\
         \t  Ubuntu/Debian   :  sudo apt install libespeak-ng-dev\n\
         \t  Fedora          :  sudo dnf install espeak-ng-devel\n\
         \t  Alpine          :  apk add espeak-ng-dev espeak-ng-static\n\
         \t  Arch            :  sudo pacman -S espeak-ng\n\
         \n\
         Cross-compilation:\n\
         \n\
         \t  Linux → aarch64 (multiarch):\n\
         \t    sudo dpkg --add-architecture arm64\n\
         \t    sudo apt install libespeak-ng-dev:arm64\n\
         \t    cargo build --target aarch64-unknown-linux-gnu --features espeak\n\
         \n\
         \t  Any → any (generic sysroot):\n\
         \t    ESPEAK_SYSROOT=/path/to/sysroot \\\n\
         \t      cargo build --target <triple> --features espeak\n\
         \n\
         \t  Any → any (explicit path):\n\
         \t    ESPEAK_LIB_DIR=/path/to/lib \\\n\
         \t      cargo build --target <triple> --features espeak\n\n"
    );
}

// ── Library name / existence helpers ─────────────────────────────────────────

/// Platform-correct filename of the static espeak-ng library.
///
/// * MSVC (Windows):         `espeak-ng.lib`
/// * GNU / MinGW / all else: `libespeak-ng.a`
fn static_lib_name(target_os: &str, target_env: &str) -> &'static str {
    if target_os == "windows" && target_env == "msvc" {
        "espeak-ng.lib"
    } else {
        "libespeak-ng.a"
    }
}

/// Return `true` if the static archive for the target toolchain exists in `dir`.
fn static_lib_exists(dir: &str, target_os: &str, target_env: &str) -> bool {
    Path::new(dir).join(static_lib_name(target_os, target_env)).exists()
}

// ── Link emission helpers ─────────────────────────────────────────────────────

/// Emit `cargo:rustc-link-*` lines for a static link from `dir`.
fn emit_static_link(dir: &str, target_os: &str, target_env: &str) {
    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-lib=static=espeak-ng");
    link_cxx(target_os, target_env);
}

/// Require the static archive in `dir`; panic with clear instructions if absent.
fn link_static_from_dir(dir: &str, target_os: &str, target_env: &str) {
    if !static_lib_exists(dir, target_os, target_env) {
        let lib_name = static_lib_name(target_os, target_env);
        let script_hint = if target_os == "windows" {
            "\t  PowerShell:  .\\scripts\\build-espeak-static.ps1\n\
             \t  Or download: https://github.com/espeak-ng/espeak-ng/releases"
        } else {
            "\t  bash scripts/build-espeak-static.sh"
        };
        panic!(
            "\n\nESPEAK_LIB_DIR is set to {dir:?} but {lib_name} was not found there.\n\
             \n\
             Run the preparation script first:\n\
             \n\
             {script_hint}\n\
             \n\
             That script builds a self-contained static archive at:\n\
             \t  src-tauri/espeak-static/lib/{lib_name}\n\n"
        );
    }
    emit_static_link(dir, target_os, target_env);
}

/// Emit the C++ runtime link required when statically linking espeak-ng.
///
/// * MSVC Windows:   no-op  — Rust's MSVC linker already includes the C++ runtime.
/// * macOS:          `libc++` (LLVM runtime shipped with Xcode / libc++-dev).
/// * Linux + MinGW:  `libstdc++` (GCC runtime).
fn link_cxx(target_os: &str, target_env: &str) {
    if target_os == "windows" && target_env == "msvc" {
        // MSVC links the C++ runtime automatically.
    } else if target_os == "macos" || target_os == "ios" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        // Linux (gnu + musl), Android, MinGW, FreeBSD, …
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}

/// Return `true` if `dir` contains a shared / import library for espeak-ng.
fn has_dylib(dir: &str, target_os: &str) -> bool {
    let d = Path::new(dir);
    match target_os {
        "windows" => {
            d.join("espeak-ng.dll").exists()
                || d.join("espeak-ng.dll.a").exists()
                || d.join("espeak-ng.lib").exists()
        }
        "macos" => d.join("libespeak-ng.dylib").exists(),
        _ => {
            if d.join("libespeak-ng.so").exists() {
                return true;
            }
            std::fs::read_dir(d).ok().map_or(false, |entries| {
                entries.flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("libespeak-ng.so.")
                })
            })
        }
    }
}

// ── GNU multiarch / target helpers ───────────────────────────────────────────

/// Derive the Debian/Ubuntu GNU multiarch tuple from Cargo's target fields.
///
/// Returns `None` for targets that don't use the Debian multiarch layout
/// (Windows, macOS, musl, Android, …).
fn target_to_multiarch<'a>(
    target_arch: &str,
    target_os: &str,
    target_env: &str,
) -> Option<&'a str> {
    if target_os != "linux" {
        return None;
    }
    // musl and Android don't use Debian multiarch paths.
    if target_env == "musl" || target_env.contains("android") {
        return None;
    }
    match (target_arch, target_env) {
        ("x86_64",      _)           => Some("x86_64-linux-gnu"),
        ("aarch64",     _)           => Some("aarch64-linux-gnu"),
        ("arm",         "gnueabihf") => Some("arm-linux-gnueabihf"),
        ("arm",         "gnueabi")   => Some("arm-linux-gnueabi"),
        ("arm",         _)           => Some("arm-linux-gnueabihf"), // conservative default
        ("i686",        _)           => Some("i386-linux-gnu"),
        ("riscv64",     _)           => Some("riscv64-linux-gnu"),
        ("s390x",       _)           => Some("s390x-linux-gnu"),
        ("powerpc64le", _)           => Some("powerpc64le-linux-gnu"),
        ("mips",        _)           => Some("mips-linux-gnu"),
        ("mips64",      _)           => Some("mips64-linux-gnuabi64"),
        ("loongarch64", _)           => Some("loongarch64-linux-gnu"),
        _                            => None,
    }
}

/// Extract a simple OS string from a Cargo/Rust target triple.
fn os_from_triple(triple: &str) -> &'static str {
    if triple.contains("windows") { "windows" }
    else if triple.contains("darwin") || triple.contains("apple") { "macos" }
    else if triple.contains("android") { "android" }
    else if triple.contains("linux") { "linux" }
    else if triple.contains("freebsd") { "freebsd" }
    else if triple.contains("openbsd") { "openbsd" }
    else if triple.contains("netbsd") { "netbsd" }
    else { "unknown" }
}

// ── pkg-config ────────────────────────────────────────────────────────────────

/// Locate the espeak-ng library directory via pkg-config.
///
/// Cross-compilation strategy:
/// 1. Try `<multiarch>-pkg-config` (Debian: `apt install pkg-config-aarch64-linux-gnu`).
/// 2. Try standard `pkg-config` with `PKG_CONFIG_ALLOW_CROSS=1` and a
///    multiarch-specific `PKG_CONFIG_PATH`.
///
/// The sysroot (if any) is applied via `PKG_CONFIG_SYSROOT_DIR` and by
/// prepending it to the multiarch pkgconfig directory.
fn pkg_config_libdir(
    target_os: &str,
    host_os: &'static str,
    multiarch: Option<&str>,
    is_cross: bool,
    sysroot: Option<&str>,
) -> Option<String> {
    let mut extra_paths: Vec<String> = Vec::new();

    // ── macOS: Homebrew directories (only useful when the host is macOS) ──────
    if target_os == "macos" && host_os == "macos" {
        for prefix in ["/opt/homebrew", "/usr/local"] {
            for sub in ["lib/pkgconfig", "share/pkgconfig"] {
                let p = format!("{prefix}/{sub}");
                if Path::new(&p).is_dir() { extra_paths.push(p); }
            }
        }
        if let Some(keg) = brew_prefix("espeak-ng") {
            let p = format!("{keg}/lib/pkgconfig");
            if Path::new(&p).is_dir() { extra_paths.insert(0, p); }
        }
    }

    // ── Cross-compile: multiarch pkgconfig directories ────────────────────────
    if let Some(ma) = multiarch {
        // Sysroot-prefixed multiarch pkgconfig (e.g. for custom sysroots).
        if let Some(sr) = sysroot {
            let p = format!("{sr}/usr/lib/{ma}/pkgconfig");
            if Path::new(&p).is_dir() { extra_paths.push(p); }
            let p = format!("{sr}/usr/share/pkgconfig");
            if Path::new(&p).is_dir() { extra_paths.push(p); }
        }
        // Standard Debian multiarch pkgconfig (installed by `libespeak-ng-dev:<arch>`).
        let p = format!("/usr/lib/{ma}/pkgconfig");
        if Path::new(&p).is_dir() { extra_paths.push(p); }
    }

    // Append any user-supplied PKG_CONFIG_PATH.
    if let Ok(user_path) = std::env::var("PKG_CONFIG_PATH") {
        if !user_path.is_empty() { extra_paths.push(user_path); }
    }

    // Path separator is `;` on Windows, `:` elsewhere.
    let sep = if target_os == "windows" { ";" } else { ":" };
    let pkg_path = extra_paths.join(sep);

    // ── Try cross-specific pkg-config binary first ────────────────────────────
    if is_cross {
        if let Some(ma) = multiarch {
            let cross_bin = format!("{ma}-pkg-config");
            if let Some(dir) = run_pkg_config_variable(
                &cross_bin,
                "espeak-ng",
                "libdir",
                &pkg_path,
                None,   // The cross binary handles sysroot internally.
                false,  // allow_cross flag is implicit for cross binaries.
            ) {
                return Some(dir);
            }
        }
    }

    // ── Standard pkg-config, optionally with allow-cross + sysroot ───────────
    run_pkg_config_variable(
        "pkg-config",
        "espeak-ng",
        "libdir",
        &pkg_path,
        sysroot,
        is_cross,
    )
}

/// Call `<binary> --variable=<var> <package>` and return the trimmed output.
///
/// Sets `PKG_CONFIG_ALLOW_CROSS=1` when `allow_cross` is true.
/// Sets `PKG_CONFIG_SYSROOT_DIR` when `sysroot` is provided.
fn run_pkg_config_variable(
    binary: &str,
    package: &str,
    var: &str,
    pkg_path: &str,
    sysroot: Option<&str>,
    allow_cross: bool,
) -> Option<String> {
    let mut cmd = Command::new(binary);
    cmd.args([&format!("--variable={var}"), package]);
    cmd.env("PKG_CONFIG_PATH", pkg_path);
    if allow_cross {
        cmd.env("PKG_CONFIG_ALLOW_CROSS", "1");
    }
    if let Some(sr) = sysroot {
        cmd.env("PKG_CONFIG_SYSROOT_DIR", sr);
    }
    let out = cmd.output().ok()?;
    if out.status.success() {
        Some(String::from_utf8(out.stdout).ok()?.trim().to_owned())
    } else {
        None
    }
}

// ── Candidate directory walk ──────────────────────────────────────────────────

/// Return an ordered list of existing directories to probe for the espeak-ng
/// library.  Respects `sysroot` (prepended to Unix paths) and host-conditional
/// paths (brew, MSYS2) that are only useful when the host OS matches.
fn candidate_dirs(
    target_os: &str,
    target_arch: &str,
    target_env: &str,
    host_os: &str,
    sysroot: Option<&str>,
) -> Vec<PathBuf> {
    let mut dirs: Vec<String> = Vec::new();

    /// Helper: prepend `sysroot` (if any) to an absolute Unix path.
    fn with_sysroot(sysroot: Option<&str>, path: &str) -> String {
        match sysroot {
            Some(sr) => format!("{sr}{path}"),
            None     => path.to_owned(),
        }
    }

    match target_os {
        // ── macOS ─────────────────────────────────────────────────────────────
        "macos" => {
            // brew is only available when the host is macOS.
            if host_os == "macos" {
                if let Some(keg) = brew_prefix("espeak-ng") {
                    dirs.push(format!("{keg}/lib"));
                }
                for prefix in ["/opt/homebrew", "/usr/local"] {
                    dirs.push(format!("{prefix}/opt/espeak-ng/lib"));
                    dirs.push(format!("{prefix}/lib"));
                }
            }
        }

        // ── iOS ───────────────────────────────────────────────────────────────
        "ios" => {
            // iOS is always cross-compiled from macOS; ESPEAK_LIB_DIR should have
            // been set by the caller.  Provide Homebrew paths as a last resort for
            // simulators built on a macOS host.
            if host_os == "macos" {
                for prefix in ["/opt/homebrew", "/usr/local"] {
                    dirs.push(format!("{prefix}/opt/espeak-ng/lib"));
                    dirs.push(format!("{prefix}/lib"));
                }
            }
        }

        // ── Android ───────────────────────────────────────────────────────────
        "android" => {
            // espeak-ng is not part of the NDK sysroot; users must build it
            // separately.  We search under the NDK sysroot as a courtesy in case
            // someone placed it there manually.
            let ndk_root = std::env::var("ANDROID_NDK_HOME")
                .or_else(|_| std::env::var("ANDROID_NDK_ROOT"))
                .or_else(|_| std::env::var("NDK_HOME"))
                .ok();

            if let Some(ndk) = &ndk_root {
                // Derive host-prebuilt sub-directory from the build host.
                let host_prebuilt = match host_os {
                    "windows" => "windows-x86_64",
                    "macos"   => "darwin-x86_64",
                    _         => "linux-x86_64",
                };
                let ndk_sysroot =
                    format!("{ndk}/toolchains/llvm/prebuilt/{host_prebuilt}/sysroot");

                // NDK sysroot lib directories for the target ABI.
                for api in ["35", "34", "33", "32", "31", "30", "29", "28", "24", "21"] {
                    let abi = android_abi(target_arch);
                    dirs.push(format!("{ndk_sysroot}/usr/lib/{abi}/{api}"));
                }
                dirs.push(format!("{ndk_sysroot}/usr/lib/{}", android_abi(target_arch)));
            }
        }

        // ── Windows ───────────────────────────────────────────────────────────
        "windows" => {
            // Official eSpeak NG installer default paths.
            for pf_var in ["PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432"] {
                if let Ok(base) = std::env::var(pf_var) {
                    dirs.push(format!("{base}\\eSpeak NG\\lib"));
                    dirs.push(format!("{base}\\eSpeak NG"));
                }
            }

            // vcpkg (arch-aware, static triplet preferred).
            if let Ok(vcpkg_root) = std::env::var("VCPKG_ROOT") {
                let arch = if target_arch == "aarch64" { "arm64" } else { "x64" };
                for triplet in [
                    format!("{arch}-windows-static"),
                    format!("{arch}-windows"),
                ] {
                    dirs.push(format!("{vcpkg_root}\\installed\\{triplet}\\lib"));
                }
            }
            // vcpkg default install inside the repo (relative path).
            dirs.push("vcpkg\\installed\\x64-windows-static\\lib".to_owned());
            dirs.push("vcpkg\\installed\\x64-windows\\lib".to_owned());

            // MSYS2 / MinGW (only useful on a Windows host).
            if host_os == "windows" {
                let msys2_root = std::env::var("MSYS2_PATH")
                    .unwrap_or_else(|_| "C:\\msys64".to_owned());
                let mingw_sub = match target_arch.as_ref() {
                    "aarch64" => "clangarm64",
                    "x86"     => "mingw32",
                    _         => "mingw64",
                };
                dirs.push(format!("{msys2_root}\\{mingw_sub}\\lib"));
                dirs.push(format!("C:\\msys2\\{mingw_sub}\\lib"));

                // Chocolatey.
                if let Ok(choco) = std::env::var("ChocolateyInstall") {
                    dirs.push(format!("{choco}\\lib\\espeak-ng\\lib"));
                }
                dirs.push("C:\\ProgramData\\chocolatey\\lib\\espeak-ng\\lib".to_owned());
            }
        }

        // ── Linux (GNU and musl) ──────────────────────────────────────────────
        "linux" => {
            let multiarch = target_to_multiarch(target_arch, target_os, target_env);

            if target_env == "musl" {
                // musl sysroots have a flat /usr/lib layout.
                dirs.push(with_sysroot(sysroot, "/usr/lib"));
                dirs.push(with_sysroot(sysroot, "/usr/local/lib"));
            } else {
                // GNU: Debian/Ubuntu multiarch first.
                if let Some(ma) = multiarch {
                    dirs.push(with_sysroot(sysroot, &format!("/usr/lib/{ma}")));
                }
                dirs.push(with_sysroot(sysroot, "/usr/lib64"));
                dirs.push(with_sysroot(sysroot, "/usr/lib"));
                dirs.push(with_sysroot(sysroot, "/usr/local/lib"));
            }
        }

        // ── FreeBSD / OpenBSD / NetBSD / other Unix ───────────────────────────
        _ => {
            dirs.push(with_sysroot(sysroot, "/usr/local/lib"));
            dirs.push(with_sysroot(sysroot, "/usr/lib"));
        }
    }

    dirs.into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

// ── Platform-specific helpers ─────────────────────────────────────────────────

/// Map a Cargo `target_arch` to an Android ABI directory name.
fn android_abi(target_arch: &str) -> &'static str {
    match target_arch {
        "aarch64"     => "aarch64-linux-android",
        "arm"         => "arm-linux-androideabi",
        "x86_64"      => "x86_64-linux-android",
        "x86"         => "i686-linux-android",
        "riscv64"     => "riscv64-linux-android",
        _             => "aarch64-linux-android",
    }
}

/// Run `brew --prefix <formula>` and return the keg path (macOS only).
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

// ── Auto-build script ─────────────────────────────────────────────────────────

/// Run `ESPEAK_BUILD_SCRIPT` to compile the espeak-ng static library from source.
///
/// Called only when `ESPEAK_LIB_DIR` points at a directory where the expected
/// static archive is absent.
///
/// The following environment variables are forwarded to the script so it can
/// set up the correct cross-compilation toolchain:
///
/// | Variable              | Value                                             |
/// |-----------------------|---------------------------------------------------|
/// | `ESPEAK_TARGET`       | Cargo target triple (e.g. `aarch64-linux-android`) |
/// | `ESPEAK_TARGET_OS`    | `android`, `linux`, `macos`, `windows`, …        |
/// | `ESPEAK_TARGET_ARCH`  | `aarch64`, `x86_64`, …                           |
/// | `ESPEAK_SYSROOT`      | Forwarded if set                                  |
/// | `ANDROID_NDK_HOME`    | Forwarded if set                                  |
fn run_espeak_build_script(
    script: &str,
    target_os: &str,
    host_os: &str,
    target_triple: &str,
    sysroot: Option<&str>,
) {
    match host_os {
        "linux" | "macos" | "windows" => {}
        _ => return, // Unknown host — skip.
    }

    let script_path = std::fs::canonicalize(script)
        .unwrap_or_else(|_| Path::new(script).to_path_buf());

    if !script_path.exists() {
        let manual_cmd = match host_os {
            "windows" => "powershell -ExecutionPolicy Bypass -File scripts\\build-espeak-static.ps1",
            _         => "bash scripts/build-espeak-static.sh",
        };
        eprintln!(
            "kittentts build.rs: ESPEAK_BUILD_SCRIPT={script:?} not found — \
             skipping auto-build.\nRun manually:  {manual_cmd}"
        );
        return;
    }

    eprintln!(
        "kittentts build.rs: static library not found for {target_triple} — \
         running {} …",
        script_path.display()
    );

    // ── Build the command based on host OS and script extension ──────────────
    let mut cmd = if host_os == "windows" {
        let ext = script_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "ps1" {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script_path.to_str().unwrap_or(script),
            ]);
            c
        } else {
            // .bat / .cmd / bare name
            let mut c = Command::new("cmd");
            c.args(["/C", script_path.to_str().unwrap_or(script)]);
            c
        }
    } else {
        // macOS / Linux
        let current_path = std::env::var("PATH").unwrap_or_default();
        // Augment PATH so cmake, make, git etc. are found on macOS (Homebrew
        // is not on the PATH when Cargo invokes the build script).
        let full_path = format!(
            "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:\
             /usr/bin:/bin:/usr/sbin:/sbin:{current_path}"
        );
        // Redirect stdout → stderr so cmake output is visible in `cargo build`
        // output (Cargo discards non-`cargo:` stdout from build scripts).
        let shell_cmd = format!("exec 1>&2; bash '{}'", script_path.display());
        let mut c = Command::new("bash");
        c.args(["-c", &shell_cmd]);
        c.env("PATH", full_path);
        c
    };

    // ── Forward cross-compilation context to the script ──────────────────────
    cmd.env("ESPEAK_TARGET",       target_triple);
    cmd.env("ESPEAK_TARGET_OS",    target_os);

    // target_arch is not easily available here; derive it from the triple.
    let target_arch = target_triple.split('-').next().unwrap_or("");
    cmd.env("ESPEAK_TARGET_ARCH",  target_arch);

    if let Some(sr) = sysroot {
        cmd.env("ESPEAK_SYSROOT", sr);
    }
    for ndk_var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_HOME"] {
        if let Ok(val) = std::env::var(ndk_var) {
            cmd.env(ndk_var, val);
            break; // forward only the first one found
        }
    }

    let status = cmd.status()
        .unwrap_or_else(|e| {
            panic!("kittentts build.rs: failed to launch build script {script:?}: {e}")
        });

    if !status.success() {
        panic!(
            "\n\nkittentts build.rs: build script {script:?} failed ({status}).\n\
             See the output above for the exact error.\n\n"
        );
    }
}
