//! Link settings for FFmpeg.
//!
//! The app links to the static FFmpeg that `scripts/build-ffmpeg.sh` builds
//! (see `.cargo/config.toml`). A static FFmpeg does not carry its own
//! dependencies, so this script adds the system libraries that it needs.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");

    let ffmpeg_dir = std::env::var("FFMPEG_DIR").unwrap_or_default();
    if !Path::new(&ffmpeg_dir).join("lib/libavcodec.a").exists() {
        panic!(
            "\n\nThe FFmpeg of this project is not built (FFMPEG_DIR = {ffmpeg_dir:?}).\n\
             Run this command one time, then build again:\n\n    ./scripts/build-ffmpeg.sh\n\n"
        );
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        for framework in ["VideoToolbox", "CoreMedia", "CoreVideo", "CoreFoundation", "CoreServices"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rustc-link-lib=z");
    }
}
