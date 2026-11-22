//! Everything that depends on the operating system.
//!
//! The rest of the app is portable: egui draws the UI, wgpu talks to the GPU
//! (Metal on macOS, Direct3D 12 on Windows), winit owns the window, and
//! FFmpeg decodes. This module holds the parts that have no portable form:
//!
//! | Part | macOS | Windows | Other systems (fallback) |
//! |---|---|---|---|
//! | Hardware decoder | VideoToolbox | Direct3D 11 | Software |
//! | Decoder frame to GPU texture | Zero copy through IOSurface | Copy on the GPU to a texture that the renderer shares | Copy through normal memory |
//! | App menu | Native menu bar | None (the UI has an open button and Ctrl+O) | None |
//! | Folder dialog | Native dialog | Native dialog | Native dialog (`rfd`) |
//! | Open a file in the web browser | `open` command | Windows shell | `xdg-open` command |
//! | Folder for the settings of apps | `~/Library/Application Support` | `%APPDATA%` | `$XDG_CONFIG_HOME` or `~/.config` |
//!
//! To add a system: add a file next to `macos.rs` with the same functions and
//! select it in the `cfg` block below. No other module needs a change.

use std::path::{Path, PathBuf};

use egui_wgpu::wgpu;

use crate::media::{HwDecode, HwSurface};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod fallback;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use fallback as imp;

/// GPU textures that show the planes of one hardware frame.
pub struct ImportedSurface {
    /// Plane 0: luma.
    pub y: wgpu::Texture,
    /// Plane 1: interleaved chroma at half size.
    pub uv: wgpu::Texture,
    /// The value range stored in the surface, when the surface knows it.
    pub full_range: Option<bool>,
    /// The part of the textures that holds the picture, as a fraction of the
    /// width and of the height. The picture starts at the top left corner.
    /// A decoder can use textures that are larger than the picture.
    pub visible: [f32; 2],
}

/// Turns a hardware decoder frame into GPU textures with no pixel copy.
pub trait HwFrameImporter {
    fn import(&self, device: &wgpu::Device, surface: &HwSurface) -> Option<ImportedSurface>;
}

/// What the platform can do for video on this GPU device.
pub struct VideoSupport {
    /// `None`: decode on the CPU.
    pub hw_decode: Option<HwDecode>,
    /// `None`: the decoder copies each frame to normal memory.
    pub importer: Option<Box<dyn HwFrameImporter>>,
}

pub fn video_support(device: &wgpu::Device) -> VideoSupport {
    imp::video_support(device)
}

/// The GPU interfaces that the app can use on this system. `None` lets wgpu
/// choose.
pub fn gpu_backends() -> Option<wgpu::Backends> {
    // Zero copy on Windows needs Direct3D 12.
    cfg!(target_os = "windows").then_some(wgpu::Backends::DX12)
}

/// Prepares the process. Call it first in `main`.
pub fn init_process() {
    #[cfg(target_os = "windows")]
    imp::attach_parent_console();
}

/// A command from the native app menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    OpenFolder,
}

/// Keeps the native menu alive. Drop it at app exit.
pub struct AppMenu(#[allow(dead_code)] imp::NativeMenu);

/// Makes the native app menu. `on_command` can be called from any thread.
/// Returns `None` on a platform with no native menu.
pub fn install_app_menu(on_command: impl Fn(MenuCommand) + Send + Sync + 'static) -> Option<AppMenu> {
    imp::install_app_menu(Box::new(on_command)).map(AppMenu)
}

/// Opens an HTML file in the web browser that the user set as the default.
pub fn open_in_browser(file: &Path) -> std::io::Result<()> {
    imp::open_in_browser(file)
}

/// The folder in which the apps of the user keep their settings. The app
/// makes its own folder in it. `None`: the system does not give the folder.
pub fn config_dir() -> Option<PathBuf> {
    imp::config_dir()
}

/// Runs a system command that opens a file with its default app.
#[cfg(not(target_os = "windows"))]
fn run_open_command(mut command: std::process::Command) -> std::io::Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("the system command ended with {status}")))
    }
}

/// Shows the native "choose folder" dialog. Call it on the UI thread.
pub fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Open a folder of camera recordings").pick_folder()
}
