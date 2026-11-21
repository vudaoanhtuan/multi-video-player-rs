//! Draws video frames inside the egui render pass with a small wgpu pipeline.
//!
//! Hardware frames are used with no copy when the platform has an importer.
//! Frames in normal memory are uploaded to textures that each tile keeps.

use std::sync::Arc;

use egui_wgpu::wgpu;

use crate::media::{ColorMatrix, CpuImage, FrameData, FrameFormat, VideoFrame};
use crate::platform::HwFrameImporter;

/// Uniform data of the shader. See `video.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    yuv_to_rgb: [[f32; 4]; 4],
    options: [f32; 4],
}

/// Builds the matrix that maps normalized (Y, Cb, Cr, 1) plane values to RGB.
fn yuv_to_rgb(matrix: ColorMatrix, full_range: bool, bit_depth: u8) -> [[f32; 4]; 4] {
    let (kr, kb) = match matrix {
        ColorMatrix::Bt601 => (0.299, 0.114),
        ColorMatrix::Bt709 => (0.2126, 0.0722),
        ColorMatrix::Bt2020 => (0.2627, 0.0593),
    };
    let kg = 1.0 - kr - kb;

    // Plane values are normalized to 0..1 by the texture format. A 10 bit
    // value is stored in the high bits of 16 bits.
    let (max_code, shift) = if bit_depth > 8 { (65535.0, 64.0) } else { (255.0, 1.0) };
    let levels = if bit_depth > 8 { 4.0 } else { 1.0 }; // 10 bit levels are 4 times the 8 bit levels
    let code = |level: f64| level * levels * shift / max_code;
    let (y_offset, y_scale, c_scale) = if full_range {
        let top = if bit_depth > 8 { 1023.0 / 4.0 } else { 255.0 };
        (0.0, 1.0 / code(top), 1.0 / code(top))
    } else {
        (code(16.0), 1.0 / code(219.0), 1.0 / code(224.0))
    };
    let c_mid = code(128.0);

    let r_cr = 2.0 * (1.0 - kr) * c_scale;
    let b_cb = 2.0 * (1.0 - kb) * c_scale;
    let g_cb = -2.0 * (1.0 - kb) * kb / kg * c_scale;
    let g_cr = -2.0 * (1.0 - kr) * kr / kg * c_scale;
    let y_term = -y_offset * y_scale;

    // Columns of the matrix: Y, Cb, Cr, constant.
    let columns = [
        [y_scale, y_scale, y_scale, 0.0],
        [0.0, g_cb, b_cb, 0.0],
        [r_cr, g_cr, 0.0, 0.0],
        [y_term - r_cr * c_mid, y_term - (g_cb + g_cr) * c_mid, y_term - b_cb * c_mid, 1.0],
    ];
    columns.map(|column| column.map(|v| v as f32))
}

/// Textures of one tile for frames that are in normal memory.
struct CpuTextures {
    size: (u32, u32),
    y: wgpu::Texture,
    uv: wgpu::Texture,
    bind_group: Arc<wgpu::BindGroup>,
}

/// The frame that a tile shows, with the GPU objects that draw it.
struct Shown {
    /// Keeps a hardware frame alive while the GPU reads its memory.
    frame: Arc<VideoFrame>,
    bind_group: Arc<wgpu::BindGroup>,
    display_aspect: f32,
}

/// GPU state of one tile.
pub struct TileSurface {
    uniform: wgpu::Buffer,
    params: Option<Params>,
    cpu_textures: Option<CpuTextures>,
    shown: Option<Shown>,
    /// The frame before `shown`. It stays alive for one more screen refresh,
    /// because the GPU can still be drawing it.
    retired: Option<Shown>,
}

/// A frame that is ready to draw.
pub struct TileDraw {
    bind_group: Arc<wgpu::BindGroup>,
    /// Width divided by height of the picture.
    pub display_aspect: f32,
}

pub struct VideoRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: Arc<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    importer: Option<Box<dyn HwFrameImporter>>,
    srgb_target: bool,
    import_failed: std::cell::Cell<bool>,
}

impl VideoRenderer {
    pub fn new(state: &egui_wgpu::RenderState, importer: Option<Box<dyn HwFrameImporter>>) -> Self {
        let device = state.device.clone();
        let shader = device.create_shader_module(wgpu::include_wgsl!("video.wgsl"));

        let plane = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video tile"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                plane(1),
                plane(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video tile"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video tile"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            // These must match the egui render pass: no depth buffer, one sample.
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: state.target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video tile"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            device,
            queue: state.queue.clone(),
            pipeline: Arc::new(pipeline),
            layout,
            sampler,
            importer,
            srgb_target: state.target_format.is_srgb(),
            import_failed: std::cell::Cell::new(false),
        }
    }

    pub fn new_tile(&self) -> TileSurface {
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video tile params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        TileSurface { uniform, params: None, cpu_textures: None, shown: None, retired: None }
    }

    /// Makes `frame` the picture of the tile. Call it once for each tile on
    /// each screen refresh. `None` means black, and the result is then `None`.
    pub fn prepare(&self, tile: &mut TileSurface, frame: Option<&Arc<VideoFrame>>) -> Option<TileDraw> {
        tile.retired = None;
        let Some(frame) = frame else {
            tile.retired = tile.shown.take();
            return None;
        };
        if !tile.shown.as_ref().is_some_and(|shown| Arc::ptr_eq(&shown.frame, frame)) {
            tile.retired = tile.shown.take();
            tile.shown = self.build(tile, frame);
        }
        let shown = tile.shown.as_ref()?;
        Some(TileDraw { bind_group: shown.bind_group.clone(), display_aspect: shown.display_aspect })
    }

    fn build(&self, tile: &mut TileSurface, frame: &Arc<VideoFrame>) -> Option<Shown> {
        let mut format = frame.format;
        let bind_group = match &frame.data {
            FrameData::Hw(surface) => {
                let imported = self.importer.as_ref().and_then(|i| i.import(&self.device, surface));
                let Some(imported) = imported else {
                    if !self.import_failed.replace(true) {
                        log::error!("cannot use a hardware frame as a GPU texture");
                    }
                    return None;
                };
                if let Some(full_range) = imported.full_range {
                    format.full_range = full_range;
                }
                Arc::new(self.bind_group(&tile.uniform, &imported.y, &imported.uv))
            }
            FrameData::Cpu(image) => self.upload(tile, &format, image),
        };

        let params = Params {
            yuv_to_rgb: yuv_to_rgb(format.matrix, format.full_range, format.bit_depth),
            options: [self.srgb_target as u8 as f32, 0.0, 0.0, 0.0],
        };
        if tile.params != Some(params) {
            self.queue.write_buffer(&tile.uniform, 0, bytemuck::bytes_of(&params));
            tile.params = Some(params);
        }
        Some(Shown { frame: frame.clone(), bind_group, display_aspect: format.display_aspect() })
    }

    /// Copies an image from normal memory into the textures of the tile.
    fn upload(&self, tile: &mut TileSurface, format: &FrameFormat, image: &CpuImage) -> Arc<wgpu::BindGroup> {
        let size = (format.width, format.height);
        let chroma_size = (format.width.div_ceil(2), format.height.div_ceil(2));
        if tile.cpu_textures.as_ref().is_none_or(|t| t.size != size) {
            let texture = |(width, height): (u32, u32), format| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("video plane"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            let y = texture(size, wgpu::TextureFormat::R8Unorm);
            let uv = texture(chroma_size, wgpu::TextureFormat::Rg8Unorm);
            let bind_group = Arc::new(self.bind_group(&tile.uniform, &y, &uv));
            tile.cpu_textures = Some(CpuTextures { size, y, uv, bind_group });
        }
        let textures = tile.cpu_textures.as_ref().unwrap();
        let write = |texture: &wgpu::Texture, data: &[u8], stride: u32, (width, height): (u32, u32)| {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(height) },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
        };
        write(&textures.y, &image.y, image.y_stride, size);
        write(&textures.uv, &image.uv, image.uv_stride, chroma_size);
        textures.bind_group.clone()
    }

    fn bind_group(&self, uniform: &wgpu::Buffer, y: &wgpu::Texture, uv: &wgpu::Texture) -> wgpu::BindGroup {
        let y_view = y.create_view(&wgpu::TextureViewDescriptor::default());
        let uv_view = uv.create_view(&wgpu::TextureViewDescriptor::default());
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video tile"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&y_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&uv_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// An egui shape that draws the frame over `rect`.
    pub fn paint_callback(&self, rect: egui::Rect, draw: &TileDraw) -> egui::PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            TilePaint { pipeline: self.pipeline.clone(), bind_group: draw.bind_group.clone() },
        )
    }
}

struct TilePaint {
    pipeline: Arc<wgpu::RenderPipeline>,
    bind_group: Arc<wgpu::BindGroup>,
}

impl egui_wgpu::CallbackTrait for TilePaint {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        // egui has set the viewport to the rectangle of the callback.
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &*self.bind_group, &[]);
        render_pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(m: [[f32; 4]; 4], yuv: [f32; 3]) -> [f32; 3] {
        let v = [yuv[0], yuv[1], yuv[2], 1.0];
        [0, 1, 2].map(|row| (0..4).map(|col| m[col][row] * v[col]).sum())
    }

    fn assert_rgb(actual: [f32; 3], expected: [f32; 3]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() < 0.004, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn limited_range_black_white_and_red() {
        let m = yuv_to_rgb(ColorMatrix::Bt709, false, 8);
        assert_rgb(convert(m, [16.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0]), [0.0, 0.0, 0.0]);
        assert_rgb(convert(m, [235.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0]), [1.0, 1.0, 1.0]);
        // BT.709 limited range red is (63, 102, 240).
        assert_rgb(convert(m, [63.0 / 255.0, 102.0 / 255.0, 240.0 / 255.0]), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn full_range_and_bt601() {
        let m = yuv_to_rgb(ColorMatrix::Bt601, true, 8);
        assert_rgb(convert(m, [1.0, 128.0 / 255.0, 128.0 / 255.0]), [1.0, 1.0, 1.0]);
        // BT.601 full range blue is (29, 255, 107).
        assert_rgb(convert(m, [29.0 / 255.0, 1.0, 107.0 / 255.0]), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn ten_bit_limited_range() {
        let m = yuv_to_rgb(ColorMatrix::Bt709, false, 10);
        let code = |v: f32| v * 64.0 / 65535.0;
        assert_rgb(convert(m, [code(64.0), code(512.0), code(512.0)]), [0.0, 0.0, 0.0]);
        assert_rgb(convert(m, [code(940.0), code(512.0), code(512.0)]), [1.0, 1.0, 1.0]);
    }
}
