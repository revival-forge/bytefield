// A species' tree billboards (`BillboardMaterial` in level.rs; the client's draw and the game's
// `Shaders/Tree/Billboard.pso`): a quad per tree on its foot, its width along the eye's right on the ground plane, its
// height along the tree's up. The texture holds the tree's N views side by side; the eye's angle round the tree picks
// two neighbours and the pixel shader blends them (`lrp r1, v1.a, t0, t1`), times the colour (the tree's TEXTUREFACTOR ×
// `billboardLightScale`) and its alpha (the fade, sin(t·π/2)), all in the screen's (gamma) space as D3D8.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_bindings::{base_color_texture, base_color_sampler},
    mesh_view_bindings::view,
};
import bevy_render::maths::PI;
@if(VIEW_TRANSMISSION_TEXTURE)
import bevy_pbr::render::mesh_view_bindings::{view_transmission_texture, view_transmission_sampler};

struct Billboard {
    // x the views, y `billboardFadeScale`, z 1 for `useNewBillboardFadeFunc`, w 1 to blend in the screen's space
    params: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> bb: Billboard;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // (the attributes: `billboard_mesh` in level.rs)
    let foot = vertex_in.position;
    let size = vertex_in.normal;
    let corner = vertex_in.uv;
    let axes = vertex_in.tangent;
    let up = vec3(vertex_in.uv_b.x, sqrt(max(1.0 - dot(vertex_in.uv_b, vertex_in.uv_b), 0.0)), vertex_in.uv_b.y);
    let eye = view.world_position;
    // the fade (as `TreeFade`): nothing nearer than D − F
    let far = size.z;
    let band = far * bb.params.y;
    let d = length(eye.xz - foot.xz) * atan(1.0 / view.clip_from_view[1][1]);
    var alpha = 1.0;
    if (far > 0.0) {
        let t = clamp((d - (far - band)) / max(band, 1e-4), 0.0, 1.0);
        alpha = select(t, sin(t * PI * 0.5), bb.params.z > 0.5);
        if (d < far - band) {
            alpha = 0.0;
        }
    }
    // the view: in the original's axes (z = −Bevy's), the eye seen from the tree against
    // its forward, 0..1 round it, linear in the cosine
    let to_eye = normalize(vec2(eye.x - foot.x, foot.z - eye.z) + vec2(1e-6, 0.0));
    var a = 0.25 - dot(to_eye, axes.xy) * 0.249975;
    if (dot(to_eye, axes.zw) < 0.0) {
        a = 1.0 - a;
    }
    // the views' centres at i + ½: the one before (t0) and the next (t1), blended by how near the first
    let n = bb.params.x;
    let x = a * n - 0.5;
    let i0 = floor(x);
    let blend = 1.0 - (x - i0);
    let r = normalize(view.world_from_view[0].xz + vec2(1e-6, 0.0));
    let pos = foot + vec3(r.x, 0.0, r.y) * corner.x * size.x + up * corner.y * size.y;
    let u = (i0 + select(0.0, 1.0, corner.x > 0.0)) / n;
    out.uv = vec2(u, 1.0 - corner.y);
    out.uv_b = vec2(u + 1.0 / n, 1.0 - corner.y);
    out.color = vec4(vertex_in.color.rgb, alpha);
    out.world_tangent = vec4(blend, 0.0, 0.0, 0.0);
    out.world_normal = vec3(0.0, 1.0, 0.0);
    out.world_position = vec4(pos, 1.0);
    out.position = view.clip_from_world * vec4(pos, 1.0);
    if (alpha <= 0.0) {
        // (out of the view's volume)
        out.position = vec4(2.0, 2.0, 2.0, 1.0);
    }
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}

fn to_screen(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3(0.0031308));
}

fn from_screen(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var plain = in;
    plain.color = vec4(1.0);
    let pbr_input = pbr_input_from_standard_material(plain, is_front);
    let t0 = textureSample(base_color_texture, base_color_sampler, in.uv);
    let t1 = textureSample(base_color_texture, base_color_sampler, in.uv_b);
    let a = in.world_tangent.x;
    let r1 = mix(vec4(to_screen(t1.rgb), t1.a), vec4(to_screen(t0.rgb), t0.a), a);
    let color = vec4(from_screen(clamp(r1.rgb * in.color.rgb, vec3(0.0), vec3(1.0))), r1.a * in.color.a);
    // (the alpha test the trees leave on: ALPHAREF 0, GREATER)
    if (color.a <= 0.0) {
        discard;
    }
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    if (bb.params.w > 0.5) {
        out.color.a = gamma_alpha(out.color, in.position);
    }
    return out;
}

// Blended in the screen's space as D3D8, over Bevy's copy of the world drawn so far: `tree.wesl`'s, the same.
@if(VIEW_TRANSMISSION_TEXTURE)
fn gamma_alpha(c: vec4<f32>, pos: vec4<f32>) -> f32 {
    let d = textureSampleLevel(view_transmission_texture, view_transmission_sampler, pos.xy / view.viewport.zw, 0.0).rgb;
    let s = clamp(c.rgb, vec3(0.0), vec3(1.0));
    let g = from_screen(mix(to_screen(clamp(d, vec3(0.0), vec3(1.0))), to_screen(s), c.a));
    let k = dot(s - d, s - d);
    if (k < 1e-5) {
        return c.a;
    }
    return clamp(dot(g - d, s - d) / k, 0.0, 1.0);
}
@else
fn gamma_alpha(c: vec4<f32>, pos: vec4<f32>) -> f32 {
    return c.a;
}
