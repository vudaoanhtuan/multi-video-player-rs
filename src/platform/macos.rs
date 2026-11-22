//! macOS: VideoToolbox decode, zero-copy frames through IOSurface, native menu.

use std::ffi::c_void;

use egui_wgpu::wgpu;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage};

use super::{HwFrameImporter, ImportedSurface, MenuCommand, VideoSupport};
use crate::media::{HwDecode, HwSurface, SurfaceFormat};

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVPixelBufferGetIOSurface(pixel_buffer: *mut c_void) -> *mut c_void;
    fn CVPixelBufferGetPixelFormatType(pixel_buffer: *mut c_void) -> u32;
    fn CVPixelBufferGetPlaneCount(pixel_buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetWidthOfPlane(pixel_buffer: *mut c_void, plane: usize) -> usize;
    fn CVPixelBufferGetHeightOfPlane(pixel_buffer: *mut c_void, plane: usize) -> usize;
}

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}
/// `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`
const NV12_FULL_RANGE: u32 = fourcc(b"420f");
/// `kCVPixelFormatType_420YpCbCr10BiPlanarFullRange`
const P010_FULL_RANGE: u32 = fourcc(b"xf20");
const NV12_VIDEO_RANGE: u32 = fourcc(b"420v");
const P010_VIDEO_RANGE: u32 = fourcc(b"x420");

pub fn video_support(device: &wgpu::Device) -> VideoSupport {
    let on_metal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.is_some();
    let has_16_bit = device.features().contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
    VideoSupport {
        hw_decode: Some(HwDecode {
            device_type: ffmpeg_sys_next::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            zero_copy_nv12: on_metal,
            zero_copy_p010: on_metal && has_16_bit,
            export: None,
        }),
        importer: on_metal.then(|| Box::new(IoSurfaceImporter) as Box<dyn HwFrameImporter>),
    }
}

/// VideoToolbox decodes into a `CVPixelBuffer` that is backed by an
/// IOSurface. Metal can make a texture that points at one plane of an
/// IOSurface. The GPU then reads the decoder output directly.
struct IoSurfaceImporter;

impl HwFrameImporter for IoSurfaceImporter {
    fn import(&self, device: &wgpu::Device, surface: &HwSurface) -> Option<ImportedSurface> {
        let pixel_buffer = surface.native_handle();
        if pixel_buffer.is_null() {
            return None;
        }
        unsafe {
            let io_surface = CVPixelBufferGetIOSurface(pixel_buffer) as *const IOSurfaceRef;
            if io_surface.is_null() || CVPixelBufferGetPlaneCount(pixel_buffer) < 2 {
                return None;
            }
            let io_surface = &*io_surface;
            let full_range = match CVPixelBufferGetPixelFormatType(pixel_buffer) {
                NV12_FULL_RANGE | P010_FULL_RANGE => Some(true),
                NV12_VIDEO_RANGE | P010_VIDEO_RANGE => Some(false),
                _ => None,
            };
            let formats = match surface.format {
                SurfaceFormat::Nv12 => [
                    (wgpu::TextureFormat::R8Unorm, MTLPixelFormat::R8Unorm),
                    (wgpu::TextureFormat::Rg8Unorm, MTLPixelFormat::RG8Unorm),
                ],
                SurfaceFormat::P010 => [
                    (wgpu::TextureFormat::R16Unorm, MTLPixelFormat::R16Unorm),
                    (wgpu::TextureFormat::Rg16Unorm, MTLPixelFormat::RG16Unorm),
                ],
            };
            let hal_device = device.as_hal::<wgpu::hal::api::Metal>()?;
            let raw_device = hal_device.raw_device();

            let mut planes = Vec::with_capacity(2);
            for (plane, (format, metal_format)) in formats.into_iter().enumerate() {
                let width = CVPixelBufferGetWidthOfPlane(pixel_buffer, plane);
                let height = CVPixelBufferGetHeightOfPlane(pixel_buffer, plane);
                let descriptor = MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                    metal_format,
                    width,
                    height,
                    false,
                );
                descriptor.setUsage(MTLTextureUsage::ShaderRead);
                let raw = raw_device.newTextureWithDescriptor_iosurface_plane(&descriptor, io_surface, plane)?;
                let size = wgpu::Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 };
                let hal_texture = wgpu::hal::metal::Device::texture_from_raw(
                    raw,
                    format,
                    MTLTextureType::Type2D,
                    1,
                    1,
                    wgpu::hal::CopyExtent { width: size.width, height: size.height, depth: 1 },
                    None,
                );
                planes.push(device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                    hal_texture,
                    &wgpu::TextureDescriptor {
                        label: Some("video plane"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::TextureUses::RESOURCE,
                ));
            }
            let uv = planes.pop()?;
            let y = planes.pop()?;
            Some(ImportedSurface { y, uv, full_range, visible: [1.0, 1.0] })
        }
    }
}

/// Opens a file with its default app.
pub fn open_in_browser(file: &std::path::Path) -> std::io::Result<()> {
    let mut command = std::process::Command::new("open");
    command.arg(file);
    super::run_open_command(command)
}

/// The folder in which the apps of the user keep their settings.
pub fn config_dir() -> Option<std::path::PathBuf> {
    Some(std::env::home_dir()?.join("Library").join("Application Support"))
}

pub struct NativeMenu {
    _menu: Menu,
}

const OPEN_FOLDER_ID: &str = "open-folder";

pub fn install_app_menu(on_command: Box<dyn Fn(MenuCommand) + Send + Sync>) -> Option<NativeMenu> {
    let open = MenuItem::with_id(
        OPEN_FOLDER_ID,
        "Open Folder…",
        true,
        Some(Accelerator::new(Modifiers::META, Code::KeyO)),
    );
    // On macOS the first menu is the app menu. The system shows the app name as its title.
    let app_menu = Submenu::with_items(
        crate::APP_NAME,
        true,
        &[
            &PredefinedMenuItem::about(Some(&format!("About {}", crate::APP_NAME)), None),
            &PredefinedMenuItem::separator(),
            &open,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    )
    .ok()?;
    let window_menu = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::fullscreen(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::close_window(None),
        ],
    )
    .ok()?;
    let menu = Menu::new();
    menu.append_items(&[&app_menu, &window_menu]).ok()?;
    menu.init_for_nsapp();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == OPEN_FOLDER_ID {
            on_command(MenuCommand::OpenFolder);
        }
    }));
    Some(NativeMenu { _menu: menu })
}
