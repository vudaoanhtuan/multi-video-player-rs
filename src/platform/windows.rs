//! Windows: Direct3D 11 hardware decode, and frames that go from the decoder
//! to the renderer with no copy through normal memory.
//!
//! The renderer (wgpu) uses Direct3D 12. FFmpeg decodes with Direct3D 11 on
//! its own device, on the same GPU. The two devices cannot use the textures
//! of each other directly, and the decoder has only a small fixed number of
//! textures. So for each frame, the decode thread copies the picture on the
//! GPU to a shared texture. Direct3D 12 opens the same texture, and the
//! renderer reads it. The pixels never leave the GPU.
//!
//! The decoder has its own device on purpose. A stream that the GPU cannot
//! decode can make a device fail. With two devices, that stream goes to
//! software decode and the renderer continues.
//!
//! When the renderer does not use Direct3D 12, or the GPU cannot share
//! textures, FFmpeg copies each frame to normal memory.

use std::ffi::{c_void, CString};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use egui_wgpu::wgpu;
use ffmpeg_sys_next as ff;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HMODULE};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX,
    D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIResource1, DXGI_SHARED_RESOURCE_READ};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::LibraryLoader::{GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_PIN};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::{HwFrameImporter, ImportedSurface, MenuCommand, VideoSupport};
use crate::media::{HwDecode, HwSurface, SurfaceFormat};

/// Free shared textures of one size that stay in the pool. More are released.
const MAX_FREE_TEXTURES: usize = 48;
/// A free texture is not used again before this time. The renderer can still
/// be drawing the old picture, and the two devices do not wait for each other.
const REUSE_DELAY: Duration = Duration::from_millis(50);
/// Longest wait for the GPU to complete the copy of one picture.
const COPY_TIMEOUT: Duration = Duration::from_secs(1);
/// Time between two checks of the copy.
const COPY_POLL: Duration = Duration::from_micros(200);

static SHARING: OnceLock<Option<Sharing>> = OnceLock::new();

pub fn video_support(device: &wgpu::Device) -> VideoSupport {
    let sharing = SHARING.get_or_init(|| unsafe { Sharing::new(device) }).as_ref();
    let has_16_bit = device.features().contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);
    let zero_copy_nv12 = sharing.is_some_and(|sharing| sharing.shares_nv12);
    let zero_copy_p010 = sharing.is_some_and(|sharing| sharing.shares_p010) && has_16_bit;
    let zero_copy = zero_copy_nv12 || zero_copy_p010;
    VideoSupport {
        hw_decode: Some(HwDecode {
            device_type: ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            zero_copy_nv12,
            zero_copy_p010,
            export: zero_copy.then_some(export_frame as fn(&mut HwSurface) -> bool),
        }),
        importer: zero_copy.then(|| Box::new(SharedTextureImporter) as Box<dyn HwFrameImporter>),
    }
}

/// The Direct3D 11 device of the decoder, the Direct3D 12 device of the
/// renderer, and the textures that the two devices share.
struct Sharing {
    d3d11: ID3D11Device,
    /// The one device context of the decoder. Use it only inside the lock.
    context: ID3D11DeviceContext,
    /// The lock of FFmpeg for `context`. FFmpeg uses the context for the decode.
    lock: unsafe extern "C" fn(*mut c_void),
    unlock: unsafe extern "C" fn(*mut c_void),
    lock_ctx: *mut c_void,
    d3d12: ID3D12Device,
    shares_nv12: bool,
    shares_p010: bool,
    /// Shared textures that no frame uses, with the time when they became
    /// free. The oldest is at the front.
    free: Mutex<VecDeque<(Instant, SharedTexture)>>,
}

// SAFETY: Direct3D devices can be used from all threads. The device context
// is used only inside the lock of FFmpeg. `lock_ctx` belongs to that lock.
unsafe impl Send for Sharing {}
unsafe impl Sync for Sharing {}

/// One texture that the decoder device writes and the renderer device reads.
struct SharedTexture {
    d3d11: ID3D11Texture2D,
    d3d12: ID3D12Resource,
    /// Tells when the GPU has completed the copy to this texture.
    copy_done: ID3D11Query,
    width: u32,
    height: u32,
    format: SurfaceFormat,
}

// SAFETY: the texture objects can be used from all threads. `copy_done` is
// used only inside the lock of the device context.
unsafe impl Send for SharedTexture {}
unsafe impl Sync for SharedTexture {}

/// A decoded picture in a shared texture. The texture goes back to the pool
/// when the last user of the frame drops it.
struct SharedPicture {
    texture: Option<SharedTexture>,
    picture_size: (u32, u32),
}

impl Drop for SharedPicture {
    fn drop(&mut self) {
        if let (Some(texture), Some(Some(sharing))) = (self.texture.take(), SHARING.get()) {
            sharing.give_back(texture);
        }
    }
}

impl Sharing {
    /// Makes the decoder device on the GPU of the renderer and tests if the
    /// two devices can share textures. Returns `None` when the renderer does
    /// not use Direct3D 12 or the decoder device cannot be made.
    unsafe fn new(device: &wgpu::Device) -> Option<Self> {
        let d3d12 = device.as_hal::<wgpu::hal::api::Dx12>()?.raw_device().clone();
        // wgpu loads the Direct3D 12 library and removes it from the process
        // when the renderer closes. The decode threads can still use shared
        // textures at that time. Keep the library until the process ends.
        let mut library = HMODULE::default();
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN, w!("d3d12.dll"), &mut library).ok()?;

        crate::media::ffmpeg::init();
        // FFmpeg selects the GPU by its position in the list of the system.
        let adapter = adapter_index(&d3d12)?;
        let adapter = CString::new(adapter.to_string()).ok()?;
        let mut context: *mut ff::AVBufferRef = ptr::null_mut();
        let device_type = ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA;
        let ret = ff::av_hwdevice_ctx_create(&mut context, device_type, adapter.as_ptr(), ptr::null_mut(), 0);
        let Some(context) = NonNull::new(context).filter(|_| ret >= 0) else {
            log::warn!("cannot make the Direct3D 11 device for hardware decode (error {ret})");
            return None;
        };
        let device_context = (*context.as_ptr()).data as *const ff::AVHWDeviceContext;
        let d3d11_context = &*((*device_context).hwctx as *const ff::AVD3D11VADeviceContext);
        let raw_device: *mut c_void = d3d11_context.device.cast();
        let raw_context: *mut c_void = d3d11_context.device_context.cast();
        let sharing = (|| {
            Some(Self {
                d3d11: ID3D11Device::from_raw_borrowed(&raw_device)?.clone(),
                context: ID3D11DeviceContext::from_raw_borrowed(&raw_context)?.clone(),
                lock: d3d11_context.lock?,
                unlock: d3d11_context.unlock?,
                lock_ctx: d3d11_context.lock_ctx,
                d3d12,
                shares_nv12: false,
                shares_p010: false,
                free: Mutex::new(VecDeque::new()),
            })
        })();
        // All decoders use this device, also when the sharing test fails.
        crate::media::ffmpeg::install_hw_device(device_type, context);

        let mut sharing = sharing?;
        sharing.shares_nv12 = sharing.create_texture(64, 64, SurfaceFormat::Nv12).is_some();
        sharing.shares_p010 = sharing.create_texture(64, 64, SurfaceFormat::P010).is_some();
        if !sharing.shares_nv12 {
            log::warn!("the GPU cannot share video textures with the renderer, frames are copied");
        }
        Some(sharing)
    }

    /// Makes a texture on the decoder device and opens it on the renderer device.
    unsafe fn create_texture(&self, width: u32, height: u32, format: SurfaceFormat) -> Option<SharedTexture> {
        let description = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: match format {
                SurfaceFormat::Nv12 => DXGI_FORMAT_NV12,
                SurfaceFormat::P010 => DXGI_FORMAT_P010,
            },
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32,
        };
        let mut d3d11 = None;
        self.d3d11.CreateTexture2D(&description, None, Some(&mut d3d11)).ok()?;
        let d3d11: ID3D11Texture2D = d3d11?;

        let handle = d3d11
            .cast::<IDXGIResource1>()
            .ok()?
            .CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0, PCWSTR::null())
            .ok()?;
        let mut d3d12: Option<ID3D12Resource> = None;
        let opened = self.d3d12.OpenSharedHandle(handle, &mut d3d12);
        // The two texture objects keep the texture alive. The handle is not necessary now.
        let _ = CloseHandle(handle);
        opened.ok()?;

        let query = D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 };
        let mut copy_done = None;
        self.d3d11.CreateQuery(&query, Some(&mut copy_done)).ok()?;
        Some(SharedTexture { d3d11, d3d12: d3d12?, copy_done: copy_done?, width, height, format })
    }

    /// Takes a free texture from the pool, or makes one.
    unsafe fn take_texture(&self, width: u32, height: u32, format: SurfaceFormat) -> Option<SharedTexture> {
        let free = {
            let mut free = self.free.lock().unwrap();
            let found = free
                .iter()
                .take_while(|(since, _)| since.elapsed() >= REUSE_DELAY)
                .position(|(_, t)| (t.width, t.height, t.format) == (width, height, format));
            found.and_then(|index| free.remove(index))
        };
        match free {
            Some((_, texture)) => Some(texture),
            None => self.create_texture(width, height, format),
        }
    }

    fn give_back(&self, texture: SharedTexture) {
        let mut free = self.free.lock().unwrap();
        let key = (texture.width, texture.height, texture.format);
        if free.iter().filter(|(_, t)| (t.width, t.height, t.format) == key).count() < MAX_FREE_TEXTURES {
            free.push_back((Instant::now(), texture));
        }
    }

    /// Copies the picture of a decoder frame to a shared texture and waits
    /// until the GPU has completed the copy.
    unsafe fn export(&self, surface: &HwSurface) -> Option<SharedPicture> {
        // The frame of FFmpeg: one texture that holds many pictures, and the
        // index of this picture.
        let raw_source = surface.data_pointer(0);
        let source = ID3D11Texture2D::from_raw_borrowed(&raw_source)?;
        let index = surface.data_pointer(1) as usize as u32;
        let mut source_description = D3D11_TEXTURE2D_DESC::default();
        source.GetDesc(&mut source_description);

        // The texture of the decoder is larger than the picture, for example
        // 1920 x 1088 for a picture of 1920 x 1080. Copy only the picture.
        // The two formats need an even size.
        let picture_size = surface.picture_size();
        let (width, height) = (picture_size.0.next_multiple_of(2), picture_size.1.next_multiple_of(2));
        if width == 0 || height == 0 || width > source_description.Width || height > source_description.Height {
            return None;
        }
        let texture = self.take_texture(width, height, surface.format)?;
        // From here, the texture goes back to the pool on each return.
        let picture = SharedPicture { texture: Some(texture), picture_size };
        let texture = picture.texture.as_ref()?;

        let region = D3D11_BOX { left: 0, top: 0, front: 0, right: width, bottom: height, back: 1 };
        (self.lock)(self.lock_ctx);
        self.context.CopySubresourceRegion(&texture.d3d11, 0, 0, 0, 0, source, index, Some(&region));
        self.context.End(&texture.copy_done);
        // Send the commands to the GPU now. The renderer is a different
        // device, so it does not wait for commands of this device.
        self.context.Flush();
        (self.unlock)(self.lock_ctx);

        // The decode is also a GPU command before the copy. So this wait is
        // the time that the GPU needs to decode the frame.
        let deadline = Instant::now() + COPY_TIMEOUT;
        loop {
            let mut done = 0i32;
            (self.lock)(self.lock_ctx);
            let result = self.context.GetData(
                &texture.copy_done,
                Some(ptr::from_mut(&mut done).cast()),
                size_of::<i32>() as u32,
                0,
            );
            (self.unlock)(self.lock_ctx);
            if result.is_err() || Instant::now() > deadline {
                return None;
            }
            if done != 0 {
                return Some(picture);
            }
            std::thread::sleep(COPY_POLL);
        }
    }
}

/// The position of the GPU of `device` in the GPU list of the system.
unsafe fn adapter_index(device: &ID3D12Device) -> Option<u32> {
    let wanted = device.GetAdapterLuid();
    let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
    // The list ends with an error.
    (0..).map_while(|index| factory.EnumAdapters1(index).ok().map(|adapter| (index, adapter))).find_map(
        |(index, adapter)| {
            let luid = adapter.GetDesc1().ok()?.AdapterLuid;
            (luid.LowPart == wanted.LowPart && luid.HighPart == wanted.HighPart).then_some(index)
        },
    )
}

/// The `export` function of the decode setup. It runs on a decode thread.
fn export_frame(surface: &mut HwSurface) -> bool {
    static FAILED: AtomicBool = AtomicBool::new(false);
    let picture = match SHARING.get() {
        Some(Some(sharing)) => unsafe { sharing.export(surface) },
        _ => None,
    };
    match picture {
        Some(picture) => {
            surface.set_exported(Box::new(picture));
            true
        }
        None => {
            if !FAILED.swap(true, Ordering::Relaxed) {
                log::warn!("cannot copy a decoded frame to a shared texture, the frame is copied through memory");
            }
            false
        }
    }
}

/// A shared texture has two planes (NV12 or P010). wgpu gets two textures
/// that point at the two planes. The renderer then reads the shared texture
/// directly.
struct SharedTextureImporter;

impl HwFrameImporter for SharedTextureImporter {
    fn import(&self, device: &wgpu::Device, surface: &HwSurface) -> Option<ImportedSurface> {
        let picture = surface.exported::<SharedPicture>()?;
        let texture = picture.texture.as_ref()?;
        let formats = match texture.format {
            SurfaceFormat::Nv12 => [wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm],
            SurfaceFormat::P010 => [wgpu::TextureFormat::R16Unorm, wgpu::TextureFormat::Rg16Unorm],
        };
        unsafe {
            // The texture is open on the Direct3D 12 device of the renderer.
            device.as_hal::<wgpu::hal::api::Dx12>()?;

            let mut planes = Vec::with_capacity(2);
            for (plane, format) in formats.into_iter().enumerate() {
                // The chroma plane has half the size in each direction.
                let size = wgpu::Extent3d {
                    width: if plane == 0 { texture.width } else { texture.width / 2 },
                    height: if plane == 0 { texture.height } else { texture.height / 2 },
                    depth_or_array_layers: 1,
                };
                let hal_texture = wgpu::hal::dx12::Device::texture_from_raw(
                    texture.d3d12.clone(),
                    format,
                    wgpu::TextureDimension::D2,
                    size,
                    1,
                    1,
                )
                .with_plane_slice(plane as u32);
                planes.push(device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
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
                    // A shared texture is in the "common" state of Direct3D 12.
                    // A shader can read a texture in that state with no state
                    // change, and the texture goes back to "common" after the
                    // draw. So tell wgpu that the texture is ready to read:
                    // wgpu then changes no state. The two wgpu textures point
                    // at the same Direct3D 12 texture, and a state change
                    // through one of them would not be known to the other.
                    wgpu::TextureUses::RESOURCE,
                ));
            }
            let uv = planes.pop()?;
            let y = planes.pop()?;
            // A picture with an odd size is in a texture that is one pixel larger.
            let visible = [
                picture.picture_size.0 as f32 / texture.width as f32,
                picture.picture_size.1 as f32 / texture.height as f32,
            ];
            // The texture does not know the value range. The frame format has it.
            Some(ImportedSurface { y, uv, full_range: None, visible })
        }
    }
}

/// Opens a file with its default app.
pub fn open_in_browser(file: &Path) -> std::io::Result<()> {
    let file: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();
    let result = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(file.as_ptr()), None, None, SW_SHOWNORMAL) };
    // A value above 32 means success. Smaller values are error codes.
    let code = result.0 as usize;
    if code > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("Windows cannot open the file (code {code})")))
    }
}

/// The folder in which the apps of the user keep their settings.
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from).filter(|path| path.is_absolute())
}

/// A release build is a window program, which has no console. When a terminal
/// started the app, this connects the log output to that terminal.
pub fn attach_parent_console() {
    // This fails when the app has a console already, or when the parent has
    // none (a start from the Explorer). The two cases need no action.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Windows has no app menu. The menu button of the control bar and Ctrl+O
/// open a folder.
pub struct NativeMenu;

pub fn install_app_menu(_on_command: Box<dyn Fn(MenuCommand) + Send + Sync>) -> Option<NativeMenu> {
    None
}
