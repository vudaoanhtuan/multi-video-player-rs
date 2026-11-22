//! FFmpeg backend: reads the stored length of a file and decodes video.
//!
//! FFmpeg reads the MKV container and drives the hardware decoder of the
//! platform (VideoToolbox on macOS, Direct3D on Windows). Audio streams are
//! never read.

use std::ffi::CString;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::sync::{Mutex, Once};

use ffmpeg_sys_next as ff;

use super::{
    ColorMatrix, CpuImage, DecodeOptions, DecodedFrame, FrameData, FrameFormat, HwSurface, SurfaceFormat,
    VideoFrame,
};
use crate::library::MediaProbe;
use crate::time::Micros;

const EAGAIN: i32 = ff::AVERROR(libc::EAGAIN);
const MICROS: ff::AVRational = ff::AVRational { num: 1, den: 1_000_000 };
/// Threads for one software decoder. Hardware decode uses one thread.
const SOFTWARE_THREADS: i32 = 4;
/// Stop after this number of packets in a row that the decoder refuses.
const MAX_PACKET_ERRORS: u32 = 50;

/// Sets the log level of FFmpeg. Each entry point of this module calls it.
/// The platform layer calls it before it uses FFmpeg directly.
pub(crate) fn init() {
    static INIT: Once = Once::new();
    // FFmpeg prints to stderr. Problems are reported through `log` by this
    // app, so FFmpeg stays silent unless debug logging is on.
    let level = if log::log_enabled!(log::Level::Debug) { ff::AV_LOG_WARNING } else { ff::AV_LOG_FATAL };
    INIT.call_once(|| unsafe { ff::av_log_set_level(level) });
}

fn error_text(code: i32) -> String {
    let mut buf = [0 as std::ffi::c_char; 128];
    unsafe {
        ff::av_strerror(code, buf.as_mut_ptr(), buf.len());
        std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

fn path_to_cstring(path: &Path) -> Result<CString, String> {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path.to_str().ok_or("file path is not valid Unicode")?.as_bytes().to_vec();
    CString::new(bytes).map_err(|_| "file path contains a NUL byte".to_owned())
}

/// Opens the container and returns it with the index of the video stream.
/// All other streams are set to "discard", so the demuxer never returns audio.
unsafe fn open_input(path: &Path) -> Result<(*mut ff::AVFormatContext, usize), String> {
    init();
    let cpath = path_to_cstring(path)?;
    let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
    let ret = ff::avformat_open_input(&mut fmt, cpath.as_ptr(), ptr::null(), ptr::null_mut());
    if ret < 0 {
        return Err(format!("cannot open file: {}", error_text(ret)));
    }
    let streams = std::slice::from_raw_parts((*fmt).streams, (*fmt).nb_streams as usize);
    let mut video = None;
    for (i, &stream) in streams.iter().enumerate() {
        let par = (*stream).codecpar;
        let is_video = (*par).codec_type == ff::AVMediaType::AVMEDIA_TYPE_VIDEO
            && (*stream).disposition & ff::AV_DISPOSITION_ATTACHED_PIC == 0;
        if is_video && video.is_none() {
            video = Some(i);
        } else {
            (*stream).discard = ff::AVDiscard::AVDISCARD_ALL;
        }
    }
    match video {
        Some(index) => Ok((fmt, index)),
        None => {
            ff::avformat_close_input(&mut fmt);
            Err("no video track".to_owned())
        }
    }
}

/// Reads the stored video length from the file header. Decodes nothing.
pub struct FfmpegProbe;

impl MediaProbe for FfmpegProbe {
    fn video_length(&self, path: &Path) -> Result<Option<Micros>, String> {
        unsafe {
            let (mut fmt, _) = open_input(path)?;
            let duration = (*fmt).duration;
            ff::avformat_close_input(&mut fmt);
            Ok((duration != ff::AV_NOPTS_VALUE && duration > 0).then_some(duration))
        }
    }
}

struct DevicePtr(*mut ff::AVBufferRef);
// SAFETY: an AVBufferRef to a hardware device context is safe to share. Each
// decoder takes its own reference with `av_buffer_ref`.
unsafe impl Send for DevicePtr {}

/// The hardware device of the app, with its type. The pointer is null after a
/// failed creation.
static DEVICE: Mutex<Option<(ff::AVHWDeviceType, DevicePtr)>> = Mutex::new(None);

/// Makes `device` the hardware device of the app for `device_type`. The
/// platform layer uses this when the decoder must work on the GPU device of
/// the renderer. Without it, FFmpeg makes its own device at the first use.
///
/// # Safety
/// `device` must be an initialized hardware device context of this type. The
/// function takes the ownership of the reference.
pub unsafe fn install_hw_device(device_type: ff::AVHWDeviceType, device: NonNull<ff::AVBufferRef>) {
    let old = DEVICE.lock().unwrap().replace((device_type, DevicePtr(device.as_ptr())));
    if let Some((_, mut old)) = old {
        ff::av_buffer_unref(&mut old.0);
    }
}

/// One hardware device for the whole app. `None` after a failed creation.
fn hw_device(device_type: ff::AVHWDeviceType) -> Option<*mut ff::AVBufferRef> {
    let mut guard = DEVICE.lock().unwrap();
    if let Some((existing, device)) = guard.as_ref() {
        if *existing == device_type {
            return (!device.0.is_null()).then_some(device.0);
        }
    }
    let mut device: *mut ff::AVBufferRef = ptr::null_mut();
    let ret = unsafe { ff::av_hwdevice_ctx_create(&mut device, device_type, ptr::null(), ptr::null_mut(), 0) };
    if ret < 0 {
        log::warn!("hardware decode is not available: {}", error_text(ret));
        device = ptr::null_mut();
    }
    *guard = Some((device_type, DevicePtr(device)));
    (!device.is_null()).then_some(device)
}

/// Decodes the video stream of one chunk.
pub struct Decoder {
    opts: DecodeOptions,
    fmt: *mut ff::AVFormatContext,
    codec: *const ff::AVCodec,
    ctx: *mut ff::AVCodecContext,
    stream_index: i32,
    time_base: ff::AVRational,
    /// Timestamp of the first frame, in stream time base units.
    start_pts: i64,
    packet: *mut ff::AVPacket,
    frame: *mut ff::AVFrame,
    /// Target for frames copied out of hardware memory.
    sw_frame: *mut ff::AVFrame,
    sws: *mut ff::SwsContext,
    sws_key: (i32, i32, i32),
    /// Pixel format of hardware frames, when hardware decode is in use.
    hw_format: Option<i32>,
    hw_checked: bool,
    last_seek: Micros,
    /// A frame decoded ahead of time by [`Decoder::prime`].
    stash: Option<DecodedFrame>,
    draining: bool,
    finished: bool,
    /// Drop packets until the next key frame.
    need_key: bool,
    /// The last packet was a key frame sent in key-frame-only mode.
    expect_frame: bool,
    packet_errors: u32,
    last_key_pos: Option<Micros>,
    key_interval: Option<Micros>,
    last_pos: Micros,
    /// Position of the latest video packet that was read, decoded or not.
    read_pos: Option<Micros>,
}

// SAFETY: a decoder is used by one thread at a time. It can move between threads.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Opens the file and the decoder. The position is the first frame.
    pub fn open(path: &Path, opts: DecodeOptions) -> Result<Self, String> {
        unsafe {
            let (fmt, index) = open_input(path)?;
            let stream = *(*fmt).streams.add(index);
            let mut decoder = Self {
                opts,
                fmt,
                codec: ptr::null(),
                ctx: ptr::null_mut(),
                stream_index: index as i32,
                time_base: (*stream).time_base,
                start_pts: 0,
                packet: ff::av_packet_alloc(),
                frame: ff::av_frame_alloc(),
                sw_frame: ff::av_frame_alloc(),
                sws: ptr::null_mut(),
                sws_key: (0, 0, -1),
                hw_format: None,
                hw_checked: false,
                last_seek: 0,
                stash: None,
                draining: false,
                finished: false,
                need_key: false,
                expect_frame: false,
                packet_errors: 0,
                last_key_pos: None,
                key_interval: None,
                last_pos: 0,
                read_pos: None,
            };
            // From here, `Drop` frees all FFmpeg objects on an error return.
            decoder.codec = ff::avcodec_find_decoder((*(*stream).codecpar).codec_id);
            if decoder.codec.is_null() {
                return Err("no decoder for this video format".to_owned());
            }
            decoder.open_codec(opts.hw.is_some())?;
            decoder.start_pts = decoder.read_first_timestamp();
            decoder.seek(0)?;
            Ok(decoder)
        }
    }

    /// Makes the decoder context. Replaces an existing context.
    unsafe fn open_codec(&mut self, use_hw: bool) -> Result<(), String> {
        ff::avcodec_free_context(&mut self.ctx);
        self.hw_format = None;
        self.hw_checked = false;

        let ctx = ff::avcodec_alloc_context3(self.codec);
        if ctx.is_null() {
            return Err("out of memory".to_owned());
        }
        self.ctx = ctx;
        let stream = *(*self.fmt).streams.add(self.stream_index as usize);
        let ret = ff::avcodec_parameters_to_context(ctx, (*stream).codecpar);
        if ret < 0 {
            return Err(format!("bad stream parameters: {}", error_text(ret)));
        }
        (*ctx).pkt_timebase = self.time_base;

        if let (true, Some(hw)) = (use_hw, self.opts.hw) {
            if let Some(format) = self.hw_pixel_format(hw.device_type) {
                if let Some(device) = hw_device(hw.device_type) {
                    (*ctx).hw_device_ctx = ff::av_buffer_ref(device);
                    self.hw_format = Some(format);
                }
            }
        }
        if self.hw_format.is_some() {
            // The hardware does the work. More threads only add delay.
            (*ctx).thread_count = 1;
        } else {
            (*ctx).thread_count = SOFTWARE_THREADS;
            (*ctx).thread_type = ff::FF_THREAD_FRAME | ff::FF_THREAD_SLICE;
        }

        let ret = ff::avcodec_open2(ctx, self.codec, ptr::null_mut());
        if ret < 0 {
            return Err(format!("cannot open decoder: {}", error_text(ret)));
        }
        Ok(())
    }

    /// The hardware pixel format of this codec for a device type, if the codec supports it.
    unsafe fn hw_pixel_format(&self, device_type: ff::AVHWDeviceType) -> Option<i32> {
        for i in 0.. {
            let config = ff::avcodec_get_hw_config(self.codec, i);
            if config.is_null() {
                return None;
            }
            let by_device = (*config).methods & ff::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32 != 0;
            if by_device && (*config).device_type == device_type {
                return Some((*config).pix_fmt as i32);
            }
        }
        None
    }

    /// Reads the timestamp of the first video packet. The file name gives the
    /// time of the first frame, so all positions are relative to this value.
    unsafe fn read_first_timestamp(&mut self) -> i64 {
        loop {
            if ff::av_read_frame(self.fmt, self.packet) < 0 {
                return 0;
            }
            let packet = &*self.packet;
            let is_video = packet.stream_index == self.stream_index;
            let ts = if packet.pts != ff::AV_NOPTS_VALUE { packet.pts } else { packet.dts };
            ff::av_packet_unref(self.packet);
            if is_video {
                return if ts == ff::AV_NOPTS_VALUE { 0 } else { ts };
            }
        }
    }

    fn pos_of(&self, ts: i64) -> Micros {
        unsafe { ff::av_rescale_q(ts - self.start_pts, self.time_base, MICROS) }
    }

    /// Moves to the key frame at or before `pos`. The caller then reads
    /// frames until it has the exact frame that it needs.
    pub fn seek(&mut self, pos: Micros) -> Result<(), String> {
        unsafe {
            let ts = self.start_pts + ff::av_rescale_q(pos.max(0), MICROS, self.time_base);
            let mut ret = ff::av_seek_frame(self.fmt, self.stream_index, ts, ff::AVSEEK_FLAG_BACKWARD);
            if ret < 0 {
                // Some files have no index. Start again from the first frame.
                ret = ff::av_seek_frame(self.fmt, self.stream_index, self.start_pts, ff::AVSEEK_FLAG_BACKWARD);
            }
            if ret < 0 {
                return Err(format!("cannot seek: {}", error_text(ret)));
            }
            ff::avcodec_flush_buffers(self.ctx);
        }
        self.stash = None;
        self.draining = false;
        self.finished = false;
        self.need_key = false;
        self.expect_frame = false;
        self.packet_errors = 0;
        self.last_key_pos = None;
        self.read_pos = None;
        self.last_seek = pos;
        Ok(())
    }

    /// Decodes the next frame now and keeps it for the next call of
    /// [`Decoder::next_frame`]. This also starts the hardware decoder, which
    /// is the slow part of opening a chunk.
    pub fn prime(&mut self) -> Result<(), String> {
        if self.stash.is_none() {
            self.stash = self.next_frame(false)?;
        }
        Ok(())
    }

    /// Position of the latest frame that was read from the file since the
    /// last seek. In key-frame-only mode this is later than the last decoded frame.
    pub fn read_pos(&self) -> Option<Micros> {
        self.read_pos
    }

    /// Time between key frames, when the decoder has seen two of them.
    pub fn key_interval(&self) -> Option<Micros> {
        self.key_interval
    }

    /// Returns the next frame in time order, or `None` at the end of the file.
    ///
    /// With `key_only`, the decoder skips all frames that are not key frames.
    /// This is the cheap way to keep up with the clock at a high speed.
    pub fn next_frame(&mut self, key_only: bool) -> Result<Option<DecodedFrame>, String> {
        if let Some(frame) = self.stash.take() {
            return Ok(Some(frame));
        }
        unsafe {
            loop {
                if self.finished {
                    return Ok(None);
                }
                let ret = ff::avcodec_receive_frame(self.ctx, self.frame);
                if ret == 0 {
                    self.expect_frame = false;
                    if self.fell_back_to_software()? {
                        continue;
                    }
                    return self.take_frame().map(Some);
                }
                if ret == ff::AVERROR_EOF {
                    self.finished = true;
                    return Ok(None);
                }
                if ret != EAGAIN {
                    return Err(format!("decode error: {}", error_text(ret)));
                }
                if self.draining {
                    self.finished = true;
                    return Ok(None);
                }
                if self.expect_frame {
                    // The decoder holds the key frame back. Force it out. This
                    // empties the decoder, so decode starts again at a key frame.
                    self.expect_frame = false;
                    ff::avcodec_send_packet(self.ctx, ptr::null());
                    let ret = ff::avcodec_receive_frame(self.ctx, self.frame);
                    let frame = if ret == 0 { Some(self.take_frame()) } else { None };
                    ff::avcodec_flush_buffers(self.ctx);
                    self.need_key = true;
                    match frame {
                        Some(frame) => return frame.map(Some),
                        None => continue,
                    }
                }
                self.feed_packet(key_only);
            }
        }
    }

    /// Reads packets until one goes into the decoder, or the file ends.
    unsafe fn feed_packet(&mut self, key_only: bool) {
        loop {
            if ff::av_read_frame(self.fmt, self.packet) < 0 {
                // End of file, or a read error: get the frames still inside the decoder.
                ff::avcodec_send_packet(self.ctx, ptr::null());
                self.draining = true;
                return;
            }
            let packet = &*self.packet;
            if packet.stream_index != self.stream_index {
                ff::av_packet_unref(self.packet);
                continue;
            }
            let is_key = packet.flags & ff::AV_PKT_FLAG_KEY != 0;
            let ts = if packet.pts != ff::AV_NOPTS_VALUE { packet.pts } else { packet.dts };
            if ts != ff::AV_NOPTS_VALUE {
                let pos = self.pos_of(ts);
                self.read_pos = Some(self.read_pos.map_or(pos, |p| p.max(pos)));
                if is_key {
                    if let Some(last) = self.last_key_pos.filter(|last| pos > *last) {
                        self.key_interval = Some(pos - last);
                    }
                    self.last_key_pos = Some(pos);
                }
            }
            if (key_only || self.need_key) && !is_key {
                ff::av_packet_unref(self.packet);
                continue;
            }
            self.need_key = false;
            let ret = ff::avcodec_send_packet(self.ctx, self.packet);
            ff::av_packet_unref(self.packet);
            if ret < 0 && ret != EAGAIN {
                // A damaged packet. Continue at the next key frame.
                self.packet_errors += 1;
                if self.packet_errors > MAX_PACKET_ERRORS {
                    self.draining = true;
                    return;
                }
                self.need_key = true;
                continue;
            }
            self.packet_errors = 0;
            self.expect_frame = key_only;
            return;
        }
    }

    /// Hardware decode can fail for one stream (for example an unusual
    /// profile). FFmpeg then gives software frames from one thread. Open the
    /// decoder again as a software decoder with threads.
    unsafe fn fell_back_to_software(&mut self) -> Result<bool, String> {
        let Some(hw_format) = self.hw_format else { return Ok(false) };
        if self.hw_checked {
            return Ok(false);
        }
        self.hw_checked = true;
        if (*self.frame).format == hw_format {
            return Ok(false);
        }
        log::warn!("hardware decode failed for this stream, using software decode");
        ff::av_frame_unref(self.frame);
        self.open_codec(false)?;
        self.seek(self.last_seek)?;
        Ok(true)
    }

    /// Turns `self.frame` into a [`DecodedFrame`] and clears `self.frame`.
    unsafe fn take_frame(&mut self) -> Result<DecodedFrame, String> {
        let frame = &*self.frame;
        let ts = if frame.best_effort_timestamp != ff::AV_NOPTS_VALUE {
            frame.best_effort_timestamp
        } else {
            frame.pts
        };
        // A frame with no timestamp is shown directly after the frame before it.
        let pos = if ts != ff::AV_NOPTS_VALUE { self.pos_of(ts) } else { self.last_pos + 1 };
        self.last_pos = pos;

        let mut format = frame_format(frame);
        let data = if Some(frame.format) == self.hw_format {
            let surface_format = hw_surface_format(frame);
            let zero_copy = match (self.opts.hw, surface_format) {
                (Some(hw), Some(SurfaceFormat::Nv12)) => hw.zero_copy_nv12,
                (Some(hw), Some(SurfaceFormat::P010)) => hw.zero_copy_p010,
                _ => false,
            };
            if let (true, Some(surface_format)) = (zero_copy, surface_format) {
                // Move the frame out. The picture stays in hardware memory,
                // and the renderer reads it there.
                let owned = NonNull::new(ff::av_frame_alloc()).ok_or("out of memory")?;
                ff::av_frame_move_ref(owned.as_ptr(), self.frame);
                let mut surface = HwSurface::from_raw(owned, surface_format);
                let export = self.opts.hw.and_then(|hw| hw.export);
                if export.is_none_or(|export| export(&mut surface)) {
                    format.bit_depth = if surface_format == SurfaceFormat::P010 { 10 } else { 8 };
                    FrameData::Hw(surface)
                } else {
                    // The platform layer cannot give this frame to the renderer.
                    FrameData::Cpu(self.copy_hw_frame(owned.as_ptr(), format.full_range)?)
                }
            } else {
                // Copy the picture to normal memory. Slower, but works with all renderers.
                let image = self.copy_hw_frame(self.frame, format.full_range);
                ff::av_frame_unref(self.frame);
                FrameData::Cpu(image?)
            }
        } else {
            let image = self.convert_to_nv12(self.frame, format.full_range);
            ff::av_frame_unref(self.frame);
            FrameData::Cpu(image?)
        };
        Ok(DecodedFrame { pos, frame: VideoFrame { format, data } })
    }

    /// Copies a frame from hardware memory to normal memory as 8 bit NV12.
    unsafe fn copy_hw_frame(&mut self, frame: *const ff::AVFrame, full_range: bool) -> Result<CpuImage, String> {
        let ret = ff::av_hwframe_transfer_data(self.sw_frame, frame, 0);
        if ret < 0 {
            return Err(format!("cannot read hardware frame: {}", error_text(ret)));
        }
        let image = self.convert_to_nv12(self.sw_frame, full_range);
        ff::av_frame_unref(self.sw_frame);
        image
    }

    /// Copies or converts a software frame to 8 bit NV12.
    unsafe fn convert_to_nv12(&mut self, frame: *const ff::AVFrame, full_range: bool) -> Result<CpuImage, String> {
        let f = &*frame;
        let (w, h) = (f.width, f.height);
        if w <= 0 || h <= 0 {
            return Err("frame has no size".to_owned());
        }
        let (chroma_w, chroma_h) = ((w + 1) / 2, (h + 1) / 2);
        // swscale can write some bytes after the end of a row. Use padded rows.
        let y_stride = (w + 31) & !31;
        let uv_stride = (chroma_w * 2 + 31) & !31;
        let mut y = vec![0u8; (y_stride * h) as usize + 64];
        let mut uv = vec![0u8; (uv_stride * chroma_h) as usize + 64];

        if f.format == ff::AVPixelFormat::AV_PIX_FMT_NV12 as i32 {
            copy_plane(f.data[0], f.linesize[0], &mut y, y_stride, w, h);
            copy_plane(f.data[1], f.linesize[1], &mut uv, uv_stride, chroma_w * 2, chroma_h);
        } else {
            if self.sws.is_null() || self.sws_key != (w, h, f.format) {
                ff::sws_freeContext(self.sws);
                let src_format: ff::AVPixelFormat = std::mem::transmute(f.format);
                self.sws = ff::sws_getContext(
                    w,
                    h,
                    src_format,
                    w,
                    h,
                    ff::AVPixelFormat::AV_PIX_FMT_NV12,
                    ff::SWS_POINT,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                );
                if self.sws.is_null() {
                    return Err("cannot convert this pixel format".to_owned());
                }
                // Keep the value range of the source. The shader does the range conversion.
                let table = ff::sws_getCoefficients(ff::SWS_CS_ITU601);
                let range = full_range as i32;
                ff::sws_setColorspaceDetails(self.sws, table, range, table, range, 0, 1 << 16, 1 << 16);
                self.sws_key = (w, h, f.format);
            }
            let dst = [y.as_mut_ptr(), uv.as_mut_ptr(), ptr::null_mut(), ptr::null_mut()];
            let dst_stride = [y_stride, uv_stride, 0, 0];
            let ret = ff::sws_scale(
                self.sws,
                f.data.as_ptr().cast(),
                f.linesize.as_ptr(),
                0,
                h,
                dst.as_ptr(),
                dst_stride.as_ptr(),
            );
            if ret < 0 {
                return Err(format!("pixel format conversion failed: {}", error_text(ret)));
            }
        }
        Ok(CpuImage { y, y_stride: y_stride as u32, uv, uv_stride: uv_stride as u32 })
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // Hardware surfaces in `stash` must go before the decoder context.
        self.stash = None;
        unsafe {
            ff::sws_freeContext(self.sws);
            ff::av_frame_free(&mut self.frame);
            ff::av_frame_free(&mut self.sw_frame);
            ff::av_packet_free(&mut self.packet);
            ff::avcodec_free_context(&mut self.ctx);
            ff::avformat_close_input(&mut self.fmt);
        }
    }
}

unsafe fn copy_plane(src: *const u8, src_stride: i32, dst: &mut [u8], dst_stride: i32, row_bytes: i32, rows: i32) {
    for row in 0..rows as isize {
        let from = src.offset(row * src_stride as isize);
        let to = dst.as_mut_ptr().offset(row * dst_stride as isize);
        ptr::copy_nonoverlapping(from, to, row_bytes as usize);
    }
}

unsafe fn hw_surface_format(frame: &ff::AVFrame) -> Option<SurfaceFormat> {
    if frame.hw_frames_ctx.is_null() {
        return None;
    }
    let frames = (*frame.hw_frames_ctx).data as *const ff::AVHWFramesContext;
    match (*frames).sw_format {
        ff::AVPixelFormat::AV_PIX_FMT_NV12 => Some(SurfaceFormat::Nv12),
        ff::AVPixelFormat::AV_PIX_FMT_P010LE => Some(SurfaceFormat::P010),
        _ => None,
    }
}

fn frame_format(frame: &ff::AVFrame) -> FrameFormat {
    // Read the color fields as numbers. A damaged file can hold a value that
    // the Rust enum does not have.
    let colorspace = unsafe { ptr::read(ptr::addr_of!(frame.colorspace).cast::<u32>()) };
    let color_range = unsafe { ptr::read(ptr::addr_of!(frame.color_range).cast::<u32>()) };
    const BT709: u32 = ff::AVColorSpace::AVCOL_SPC_BT709 as u32;
    const BT470BG: u32 = ff::AVColorSpace::AVCOL_SPC_BT470BG as u32;
    const SMPTE170M: u32 = ff::AVColorSpace::AVCOL_SPC_SMPTE170M as u32;
    const BT2020_NCL: u32 = ff::AVColorSpace::AVCOL_SPC_BT2020_NCL as u32;
    const BT2020_CL: u32 = ff::AVColorSpace::AVCOL_SPC_BT2020_CL as u32;
    let matrix = match colorspace {
        BT709 => ColorMatrix::Bt709,
        BT470BG | SMPTE170M => ColorMatrix::Bt601,
        BT2020_NCL | BT2020_CL => ColorMatrix::Bt2020,
        // Not stored in the file. Use the normal rule: HD is BT.709, SD is BT.601.
        _ if frame.height >= 720 => ColorMatrix::Bt709,
        _ => ColorMatrix::Bt601,
    };
    let full_format = [
        ff::AVPixelFormat::AV_PIX_FMT_YUVJ420P,
        ff::AVPixelFormat::AV_PIX_FMT_YUVJ422P,
        ff::AVPixelFormat::AV_PIX_FMT_YUVJ444P,
    ]
    .iter()
    .any(|f| *f as i32 == frame.format);
    let full_range = full_format || color_range == ff::AVColorRange::AVCOL_RANGE_JPEG as u32;
    let sar = frame.sample_aspect_ratio;
    let pixel_aspect = if sar.num > 0 && sar.den > 0 { sar.num as f32 / sar.den as f32 } else { 1.0 };
    FrameFormat {
        width: frame.width.max(0) as u32,
        height: frame.height.max(0) as u32,
        pixel_aspect,
        matrix,
        full_range,
        bit_depth: 8,
    }
}
