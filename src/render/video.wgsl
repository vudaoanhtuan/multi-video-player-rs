// Draws one video frame from two planes: luma, and interleaved chroma at half size.
// The quad fills the viewport. The caller sets the viewport to the video area of the tile.

struct Params {
    // Maps (Y, Cb, Cr, 1) to RGB. Holds the value range and the color matrix.
    yuv_to_rgb: mat4x4<f32>,
    // x > 0.5: the render target is sRGB, so write linear values.
    options: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var luma_texture: texture_2d<f32>;
@group(0) @binding(2) var chroma_texture: texture_2d<f32>;
@group(0) @binding(3) var plane_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let x = f32(index & 1u);
    let y = f32(index >> 1u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn sample_yuv(uv: vec2<f32>) -> vec3<f32> {
    let luma = textureSampleLevel(luma_texture, plane_sampler, uv, 0.0).r;
    let chroma = textureSampleLevel(chroma_texture, plane_sampler, uv, 0.0).rg;
    return vec3<f32>(luma, chroma);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Screen pixel size in video pixels. Above 1 the video is drawn smaller than its size.
    let size = vec2<f32>(textureDimensions(luma_texture));
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let shrink = max(length(dx * size), length(dy * size));

    var yuv: vec3<f32>;
    if (shrink > 1.5) {
        // A small tile shows a large video. One sample for each screen pixel
        // skips video pixels and makes edges flicker. Use the mean of four
        // samples inside the screen pixel.
        yuv = (sample_yuv(in.uv - 0.25 * dx - 0.25 * dy)
            + sample_yuv(in.uv + 0.25 * dx - 0.25 * dy)
            + sample_yuv(in.uv - 0.25 * dx + 0.25 * dy)
            + sample_yuv(in.uv + 0.25 * dx + 0.25 * dy)) * 0.25;
    } else {
        yuv = sample_yuv(in.uv);
    }

    var rgb = (params.yuv_to_rgb * vec4<f32>(yuv, 1.0)).rgb;
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (params.options.x > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
