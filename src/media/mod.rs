//! Video decode: frame types that the rest of the app uses, and the FFmpeg
//! backend that makes them.

pub mod ffmpeg;

use std::ffi::c_void;
use std::ptr::NonNull;

use ffmpeg_sys_next as ff;

use crate::time::Micros;

/// YUV to RGB matrix of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMatrix {
    Bt601,
    Bt709,
    Bt2020,
}

/// Layout of the pixels of a hardware frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFormat {
    /// 8 bit, plane 0 = Y, plane 1 = interleaved CbCr at half size.
    Nv12,
    /// 10 bit in the high bits of 16 bit values, same planes as NV12.
    P010,
}

/// How to show a frame. This does not depend on where the pixels are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameFormat {
    pub width: u32,
    pub height: u32,
    /// Width of one pixel divided by its height. 1.0 for square pixels.
    pub pixel_aspect: f32,
    pub matrix: ColorMatrix,
    pub full_range: bool,
    /// Bits for each color value in the stored planes: 8 or 10.
    pub bit_depth: u8,
}

impl FrameFormat {
    /// Shape of the picture on screen: width divided by height.
    pub fn display_aspect(&self) -> f32 {
        self.width as f32 * self.pixel_aspect / self.height.max(1) as f32
    }
}

/// An NV12 picture in normal memory.
pub struct CpuImage {
    pub y: Vec<u8>,
    pub y_stride: u32,
    pub uv: Vec<u8>,
    pub uv_stride: u32,
}

/// A decoded picture that is still in the memory of the hardware decoder.
///
/// The platform layer turns it into GPU textures with no copy. On macOS the
/// native handle is a `CVPixelBufferRef`.
pub struct HwSurface {
    frame: NonNull<ff::AVFrame>,
    pub format: SurfaceFormat,
}

// SAFETY: the surface owns one reference to a ref-counted, immutable AVFrame.
// FFmpeg buffer reference counts are atomic.
unsafe impl Send for HwSurface {}
unsafe impl Sync for HwSurface {}

impl HwSurface {
    /// Takes the ownership of `frame`.
    ///
    /// # Safety
    /// `frame` must be a valid hardware frame made by `av_frame_alloc`.
    pub(crate) unsafe fn from_raw(frame: NonNull<ff::AVFrame>, format: SurfaceFormat) -> Self {
        Self { frame, format }
    }

    /// The native surface handle. Its type depends on the hardware decoder:
    /// `CVPixelBufferRef` for VideoToolbox.
    pub fn native_handle(&self) -> *mut c_void {
        // FFmpeg stores the surface in `data[3]` for VideoToolbox, DXVA2, and VAAPI.
        unsafe { self.frame.as_ref().data[3].cast() }
    }
}

impl Drop for HwSurface {
    fn drop(&mut self) {
        let mut ptr = self.frame.as_ptr();
        unsafe { ff::av_frame_free(&mut ptr) };
    }
}

pub enum FrameData {
    Cpu(CpuImage),
    Hw(HwSurface),
}

/// One decoded picture.
pub struct VideoFrame {
    pub format: FrameFormat,
    pub data: FrameData,
}

/// A picture with its position in the chunk.
pub struct DecodedFrame {
    /// Time from the first frame of the file.
    pub pos: Micros,
    pub frame: VideoFrame,
}

/// Hardware decode setup. The platform layer chooses it.
#[derive(Clone, Copy, Debug)]
pub struct HwDecode {
    pub device_type: ff::AVHWDeviceType,
    /// The renderer can use NV12 hardware frames with no copy.
    pub zero_copy_nv12: bool,
    /// The renderer can use P010 hardware frames with no copy.
    pub zero_copy_p010: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DecodeOptions {
    /// `None` decodes on the CPU.
    pub hw: Option<HwDecode>,
}
