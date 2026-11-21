//! Link settings for the `static-ffmpeg` feature.
//!
//! A static FFmpeg does not carry its own dependencies. This script adds the
//! system libraries that the FFmpeg build of `scripts/build-ffmpeg.sh` needs.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let static_ffmpeg = std::env::var_os("CARGO_FEATURE_STATIC_FFMPEG").is_some();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if static_ffmpeg && target_os == "macos" {
        for framework in ["VideoToolbox", "CoreMedia", "CoreVideo", "CoreFoundation", "CoreServices"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rustc-link-lib=z");
    }
}
