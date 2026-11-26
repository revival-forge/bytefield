// Heat haze (`heat.rs`): each pixel of the world's picture is taken from a little aside, by ripples that rise over each
// source (a segment on the screen with a radius) or a shock ring running out from a blast.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

const MAX: u32 = 24u;

struct HeatHaze {
    seg: array<vec4<f32>, 24>,
    shape: array<vec4<f32>, 24>,
    count: u32,
    time: f32,
    pad0: f32,
    pad1: f32,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> haze: HeatHaze;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // (in screen heights, so that a source is round)
    let dims = vec2<f32>(textureDimensions(screen_texture));
    let aspect = dims.x / dims.y;
    let p = vec2<f32>(in.uv.x * aspect, in.uv.y);
    var off = vec2<f32>(0.0);
    for (var i = 0u; i < min(haze.count, MAX); i++) {
        let s = haze.seg[i];
        let k = haze.shape[i];
        let a = vec2<f32>(s.x * aspect, s.y);
        let r = k.x;
        if (k.z > 0.5) {
            // the ring: pushed outwards on its edge
            let d = p - a;
            let x = (length(d) - r) / k.w;
            if (abs(x) < 3.0) {
                off += normalize(d + vec2<f32>(1e-6)) * k.y * exp(-x * x) * k.w * 0.35;
            }
            continue;
        }
        let b = vec2<f32>(s.z * aspect, s.w);
        let ab = b - a;
        let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-10), 0.0, 1.0);
        let d = length(p - (a + ab * t));
        // (wider along the way, thinner towards its end)
        let reach = r * (1.0 + t);
        if (d >= reach) {
            continue;
        }
        let w = k.y * (1.0 - smoothstep(0.2 * reach, reach, d)) * (1.0 - t * 0.8);
        // ripples in the source's own size, rising (the uv's y goes down)
        let q = (p - a) / max(r, 1e-4);
        let ph = haze.time * 7.0 + k.w;
        let n = vec2<f32>(sin(q.y * 7.0 + ph + 1.7 * sin(q.x * 3.0 + ph * 0.6)), cos(q.y * 5.0 + ph * 1.2 + 2.1 * sin(q.x * 4.0 - ph * 0.4)));
        off += n * w * min(r * 0.15, 0.006);
    }
    off = clamp(off, vec2<f32>(-0.012), vec2<f32>(0.012));
    off.x /= aspect;
    return textureSample(screen_texture, screen_sampler, in.uv + off);
}
