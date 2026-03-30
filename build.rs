//! Build script — minimal now that espeak-ng is a pure-Rust dependency.
//!
//! The C library linking logic has been removed.  The `espeak-ng` crate
//! (pure Rust) handles everything internally via bundled data files.

fn main() {
    // Nothing to do — espeak-ng is now a pure-Rust dependency with bundled data.
    // The build script is kept as a placeholder for any future platform-specific
    // setup (e.g. ORT linking hints).
}
