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

pub fn install_app_menu(_on_command: Box<dyn Fn(MenuCommand) + Send + Sync>) -> Option<NativeMenu> {
    None
}
