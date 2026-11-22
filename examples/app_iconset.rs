//! Writes the app icon as a macOS iconset folder.
//!
//! `scripts/bundle-macos.sh` runs this, then `iconutil` turns the folder into
//! the `.icns` file of the app bundle.
//!
//! Usage: `cargo run --example app_iconset -- <folder>.iconset`

use std::path::PathBuf;

fn main() -> Result<(), String> {
    let folder = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: app_iconset <folder>.iconset")?);
    std::fs::create_dir_all(&folder).map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
    // The names and sizes that `iconutil` expects.
    for points in [16u32, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let name = format!("icon_{points}x{points}{suffix}.png");
            let png = multi_video_player::ui::app_icon::png(points * scale).ok_or("cannot draw the icon")?;
            std::fs::write(folder.join(&name), png).map_err(|e| format!("cannot write {name}: {e}"))?;
        }
    }
    Ok(())
}
