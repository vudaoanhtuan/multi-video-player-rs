//! Link settings for FFmpeg, and the resources of the Windows executable.
//!
//! The app links to the static FFmpeg that `scripts/build-ffmpeg.sh` builds
//! (`scripts/build-ffmpeg.ps1` on Windows, see `.cargo/config.toml`). A static FFmpeg does not carry its own
//! dependencies, so this script adds the system libraries that it needs.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let ffmpeg_dir = std::env::var("FFMPEG_DIR").unwrap_or_default();
    // The Microsoft compiler also gives the libraries of FFmpeg these names.
    if !Path::new(&ffmpeg_dir).join("lib/libavcodec.a").exists() {
        let command = if target_os == "windows" { r".\scripts\build-ffmpeg.ps1" } else { "./scripts/build-ffmpeg.sh" };
        panic!(
            "\n\nThe FFmpeg of this project is not built (FFMPEG_DIR = {ffmpeg_dir:?}).\n\
             Run this command one time, then build again:\n\n    {command}\n\n"
        );
    }

    if target_os == "macos" {
        for framework in ["VideoToolbox", "CoreMedia", "CoreVideo", "CoreFoundation", "CoreServices"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rustc-link-lib=z");
    }
    if target_os == "windows" {
        // The list is in the files ffmpeg-build/prefix/lib/pkgconfig/*.pc.
        for library in ["ole32", "user32", "bcrypt"] {
            println!("cargo:rustc-link-lib={library}");
        }
        #[cfg(windows)]
        windows_resources();
    }
}

/// Puts the app icon and the app name in the executable. The Explorer and
/// the task bar show them.
#[cfg(windows)]
fn windows_resources() {
    const ICON_SVG: &str = "assets/app-icon/app-icon.svg";
    println!("cargo:rerun-if-changed={ICON_SVG}");
    let svg = std::fs::read_to_string(ICON_SVG).expect("cannot read the app icon");
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let icon = Path::new(&out_dir).join("app-icon.ico");
    std::fs::write(&icon, ico_file(&svg)).expect("cannot write the icon file");

    let mut resources = winresource::WindowsResource::new();
    resources.set_icon(icon.to_str().expect("the build folder is not valid Unicode"));
    // The name of the app as the user sees it. The same text is `APP_NAME` in `src/lib.rs`.
    resources.set("ProductName", "Multi Video Player");
    resources.set("FileDescription", "Multi Video Player");
    resources.compile().expect("cannot compile the resources of the executable");
}

/// Draws the icon SVG at the sizes that Windows uses and packs the pictures
/// into the data of one `.ico` file.
#[cfg(windows)]
fn ico_file(svg: &str) -> Vec<u8> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let tree = Tree::from_str(svg, &Options::default()).expect("the app icon is not a valid SVG file");
    let pictures: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&size| {
            let mut pixmap = Pixmap::new(size, size).expect("the icon size is not zero");
            let scale = Transform::from_scale(size as f32 / tree.size().width(), size as f32 / tree.size().height());
            resvg::render(&tree, scale, &mut pixmap.as_mut());
            pixmap.encode_png().expect("cannot encode the icon")
        })
        .collect();

    // The file: a header, one directory entry of 16 bytes for each picture,
    // then the pictures as PNG data.
    let mut file = Vec::new();
    file.extend([0u16, 1, SIZES.len() as u16].iter().flat_map(|v| v.to_le_bytes()));
    let mut offset = 6 + 16 * SIZES.len();
    for (&size, picture) in SIZES.iter().zip(&pictures) {
        // A size of 256 is stored as 0.
        let side = if size >= 256 { 0 } else { size as u8 };
        file.extend([side, side, 0, 0]);
        // One color plane, 32 bits for each pixel.
        file.extend([1u16, 32].iter().flat_map(|v| v.to_le_bytes()));
        file.extend((picture.len() as u32).to_le_bytes());
        file.extend((offset as u32).to_le_bytes());
        offset += picture.len();
    }
    for picture in &pictures {
        file.extend(picture);
    }
    file
}
