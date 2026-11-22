//! Portable fallback for systems with no dedicated platform file.
//!
//! Video is decoded on the CPU and frames are uploaded from normal memory,
//! which works with every GPU backend.

use egui_wgpu::wgpu;

use super::{MenuCommand, VideoSupport};

pub struct NativeMenu;

pub fn video_support(_device: &wgpu::Device) -> VideoSupport {
    VideoSupport { hw_decode: None, importer: None }
}

/// Opens a file with its default app.
pub fn open_in_browser(file: &std::path::Path) -> std::io::Result<()> {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(file);
    super::run_open_command(command)
}

/// The folder in which the apps of the user keep their settings.
pub fn config_dir() -> Option<std::path::PathBuf> {
    // An XDG path that is not absolute is not valid.
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).filter(|path| path.is_absolute());
    xdg.or_else(|| Some(std::env::home_dir()?.join(".config")))
}

pub fn install_app_menu(_on_command: Box<dyn Fn(MenuCommand) + Send + Sync>) -> Option<NativeMenu> {
    None
}
