//! Portable fallback for systems with no dedicated platform file.
//!
//! Frames are copied through normal memory, which works with every GPU
//! backend. A Windows file can replace this with a D3D11 shared-texture
//! importer and a native menu.

use egui_wgpu::wgpu;

use super::{MenuCommand, VideoSupport};

pub struct NativeMenu;

pub fn video_support(_device: &wgpu::Device) -> VideoSupport {
    #[cfg(target_os = "windows")]
    let hw_decode = Some(crate::media::HwDecode {
        device_type: ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
        zero_copy_nv12: false,
        zero_copy_p010: false,
    });
    #[cfg(not(target_os = "windows"))]
    let hw_decode = None;
    VideoSupport { hw_decode, importer: None }
}

/// The system command that opens a file with its default app.
pub fn browser_command(file: &std::path::Path) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        // `start` is a command of `cmd`. The empty text is the window title.
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]).arg(file);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(file);
        command
    }
}

pub fn install_app_menu(_on_command: Box<dyn Fn(MenuCommand) + Send + Sync>) -> Option<NativeMenu> {
    None
}
