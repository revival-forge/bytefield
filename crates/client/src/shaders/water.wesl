// The water: its two layers moving across each other (no ×2),
// tinted by `color` turning to `deepColor` with the depth (over
// `waterColorDepth` m, 5 by default; with its strength), its alpha from the vertex (`uv.y`: the
// original's 16 m grid, `assets::water_alpha`); plus its highlight (`normalMap`, `specularColor`); then the fog. In the screen's (gamma) space,
// like the original's fixed function. Unlit. With the setting `water_fx`
// it also refracts what is under it (Bevy's copy of the world drawn so far) and foams where the
// world under it is near (the depth prepass): at the shore, round a hull, a pier's legs.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    mesh_view_bindings::{view, fog, lights, globals},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
};
import bevy_pbr::render::{clustered_forward as clustering, mesh_view_bindings::clustered_lights, pbr_lighting::getDistanceAttenuation};
import bevy_render::maths::PI;
import bytefield::shaders::aerial::aerial;
import bytefield::shaders::clouds::cloud_at;

// (with the setting `oit`: into the camera's list per pixel, sorted and blended after the
// transparent pass, as Bevy's `pbr.wesl` does; the pipeline then has no blending and draws nothing)
@if(MATERIAL_OIT_ENABLED)
import bevy_core_pipeline::oit::draw::oit_draw;

@if(DEPTH_PREPASS)
import bevy_pbr::{prepass::utils::prepass_depth, render::view_transformations::depth_ndc_to_view_z};
@if(VIEW_TRANSMISSION_TEXTURE)
import bevy_pbr::render::mesh_view_bindings::{view_transmission_texture, view_transmission_sampler};

struct WaterParams {
    color: vec4<f32>,
    deep: vec4<f32>,
    layer1: vec4<f32>,
    layer2: vec4<f32>,
    normal: vec4<f32>,
    specular: vec4<f32>,
    reflect: vec4<f32>,
    shade: vec4<f32>,
    fx: vec4<f32>,
    sun: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var tex1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var tex1_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var tex2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var tex2_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var normal_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var reflection: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var reflection_sampler: sampler;

// how far the normal map moves the reflection, in screens
const REFLECTION_WAVES: f32 = 0.015;

// the side of a cell of the original's water, in metres
const WATER_GRID: f32 = 16.0;

// `water_fx`: how far past the mesh's shore (its depth, by vertex) it is drawn, so that the foam reaches
// where the ground really meets it; how deep the foam goes, in metres; how far the refraction moves what
// is under it, in the screen's heights, and over how many metres of depth it grows to that, and from how
// far it shrinks with the distance
const FOAM_REACH: f32 = 4.0;
const FOAM_DEPTH: f32 = 1.2;
const FOAM_LINE: f32 = 0.05;
const FOAM_SPREAD: f32 = 1.5;
const FOAM_BAND: f32 = 8.0;
const FOAM_ALPHA: f32 = 0.85;
const FOAM_RGB: vec3<f32> = vec3(0.92, 0.94, 0.93);
const REFRACTION: f32 = 0.012;
const REFRACTION_DEPTH: f32 = 1.5;
const REFRACTION_NEAR: f32 = 25.0;

// the world under the water here (what the depth prepass shows along the same ray); far down without it
@if(DEPTH_PREPASS)
fn under(pos: vec4<f32>, p: vec3<f32>) -> vec3<f32> {
    let k = depth_ndc_to_view_z(prepass_depth(pos, 0u)) / depth_ndc_to_view_z(pos.z);
    return view.world_position + (p - view.world_position) * k;
}
@else
fn under(pos: vec4<f32>, p: vec3<f32>) -> vec3<f32> {
    return p - vec3(0.0, 1e4, 0.0);
}

// the world under the water (gamma), `shift` pixels away; only what is under it moves: where the shifted
// pixel shows something before the water (a hull), the one straight behind. Alpha 0 without the copy.
@if(VIEW_TRANSMISSION_TEXTURE && DEPTH_PREPASS)
fn behind(pos: vec4<f32>, shift: vec2<f32>) -> vec4<f32> {
    var px = clamp(pos.xy + shift, vec2(0.5), view.viewport.zw - 0.5);
    // (reverse z: the greater is the nearer)
    if prepass_depth(vec4(px, 0.0, 0.0), 0u) > pos.z {
        px = pos.xy;
    }
    let c = textureSampleLevel(view_transmission_texture, view_transmission_sampler, px / view.viewport.zw, 0.0).rgb;
    return vec4(gamma(c), 1.0);
}
@else
fn behind(pos: vec4<f32>, shift: vec2<f32>) -> vec4<f32> {
    return vec4(0.0);
}

// D3D's CAMERASPACEREFLECTIONVECTOR is per vertex, on the water's grid of 16 m, and interpolated across: near the camera,
// far from what each pixel would see
fn node_reflection(x: f32, y: f32, z: f32) -> vec3<f32> {
    return reflect(normalize(vec3(x, y, z) * WATER_GRID * vec3(1.0, 0.0, 1.0) + vec3(0.0, y, 0.0) - view.world_position), vec3(0.0, 1.0, 0.0));
}

// TODO: only checked with the camera looking north (+Z, the view at identity): does the glint turn with the camera
// like this? Needs a capture or a reference run with the player turned.
fn vertex_reflection(p: vec3<f32>) -> vec3<f32> {
    let g = p.xz / WATER_GRID;
    let c = floor(g);
    let f = g - c;
    // (the quad's diagonal: from (0,0) to (1,1) of the grid)
    if f.x > f.y {
        return node_reflection(c.x, p.y, c.y) * (1.0 - f.x) + node_reflection(c.x + 1.0, p.y, c.y) * (f.x - f.y) + node_reflection(c.x + 1.0, p.y, c.y + 1.0) * f.y;
    }
    return node_reflection(c.x, p.y, c.y) * (1.0 - f.y) + node_reflection(c.x, p.y, c.y + 1.0) * (f.y - f.x) + node_reflection(c.x + 1.0, p.y, c.y + 1.0) * f.x;
}

// the world under the water (gamma), straight behind; alpha 0 without the copy (setting `water_gamma`)
@if(VIEW_TRANSMISSION_TEXTURE)
fn behind_here(pos: vec4<f32>) -> vec4<f32> {
    let c = textureSampleLevel(view_transmission_texture, view_transmission_sampler, pos.xy / view.viewport.zw, 0.0).rgb;
    return vec4(gamma(c), 1.0);
}
@else
fn behind_here(pos: vec4<f32>) -> vec4<f32> {
    return vec4(0.0);
}

// D3D's vertex fog with RANGEFOGENABLE: linear in the distance, mixed in the screen's space
fn gamma_fog(p: vec3<f32>) -> f32 {
    return clamp((distance(p, view.world_position) - fog.be.x) / (fog.be.y - fog.be.x), 0.0, 1.0);
}

// (the exact sRGB curve, the one Bevy decodes the textures and encodes the screen with, not x^2.2)
fn to_screen(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3(0.0), vec3(1.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, 12.92 * x, x <= vec3(0.0031308));
}

fn from_screen(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3(0.0), vec3(1.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

fn gamma(c: vec3<f32>) -> vec3<f32> {
    return to_screen(c);
}

// The point lights of the flashes and explosions on a surface at `p` facing `n`:
// what they add over its own light (Lambert, at the view's exposure; zero without them).
fn flash_light(p: vec4<f32>, n: vec3<f32>, frag: vec2<f32>) -> vec3<f32> {
    let view_z = dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z, view.view_from_world[2].z, view.view_from_world[3].z), p);
    let r = clustering::unpack_clusterable_object_index_ranges(clustering::view_fragment_cluster_index(frag, view_z, false));
    var e = vec3(0.0);
    for (var i = r.first_point_light_index_offset; i < r.first_spot_light_index_offset; i = i + 1u) {
        let l = &clustered_lights.data[clustering::get_clusterable_object_id(i)];
        let d = (*l).position_radius.xyz - p.xyz;
        let d2 = max(dot(d, d), 1e-4);
        e += (*l).color_inverse_square_range.rgb * getDistanceAttenuation(d2, (*l).color_inverse_square_range.w) * max(dot(n, d * inverseSqrt(d2)), 0.0);
    }
    return e * view.exposure / PI;
}

// The sun's shadow on a surface that carries the statics' in its own light: the sun's map (soldiers,
// vehicles and the statics) darkens only where the statics' own map leaves it lit. 1 without the sun's shadows.
fn sun_shadow(p: vec4<f32>, n: vec3<f32>, frag: vec2<f32>) -> f32 {
    let view_z = dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z, view.view_from_world[2].z, view.view_from_world[3].z), p);
    var s = 1.0;
    var lit = 1.0;
    for (var i = 0u; i < lights.n_directional_lights; i = i + 1u) {
        if ((lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u) {
            continue;
        }
        let f = fetch_directional_shadow(i, p, n, view_z, frag);
        if (any(lights.directional_lights[i].color.rgb > vec3(0.0))) {
            s = f;
        } else {
            lit = f;
        }
    }
    return 1.0 - (1.0 - s) * lit;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // past the world's far plane (sun.w, the view distance; in the view's depth, as D3D's projection clips): the sky
    // TODO: Midway's sea ends nearer in the original (~330 m against our 500: its horizon 24 px under the centre, ours 16);
    // it draws the water in 256 m patches (a reference run in Midway): which, and up to where?
    let view_z = dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z, view.view_from_world[2].z, view.view_from_world[3].z), in.world_position);
    if water.sun.w > 0.0 && -view_z > water.sun.w {
        discard;
    }
    let depth = in.uv.x;
    let fx = water.fx.x > 0.0;
    if depth <= select(0.0, -FOAM_REACH, fx) {
        discard;
    }
    // the time is in layer1.w; Refractor's ground plane
    let t = water.layer1.w;
    let xz = vec2(in.world_position.x, -in.world_position.z);
    let a = textureSample(tex1, tex1_sampler, xz * water.layer1.z + water.layer1.xy * t);
    let b = textureSample(tex2, tex2_sampler, xz * water.layer2.z + water.layer2.xy * t);
    let k = select(0.0, clamp(depth / water.layer2.w, 0.0, 1.0), water.layer2.w > 0.0);
    let tint = mix(water.color.rgb, water.deep.rgb, k);
    // D3D's first pass: stage 0 and 1 MODULATE, times the vertex colour
    var rgb0 = gamma(a.rgb) * gamma(b.rgb) * tint;
    // the second, added (SRCALPHA, ONE; its alpha, the vertex's): DOT3 of the normal map with a cube map, times
    // `specularColor`. The cube holds, for a direction d,
    // normalize(d + sun); its coordinates are the reflection vector turned 180° about y, as in envmap.wesl
    let r = vertex_reflection(in.world_position.xyz);
    let n = gamma(textureSample(normal_map, normal_sampler, xz * water.normal.z + water.normal.xy * t).rgb) * 2.0 - 1.0;
    let lit = clamp(dot(n, normalize(normalize(vec3(-r.x, r.y, r.z)) + water.sun.xyz)), 0.0, 1.0);
    var spec = lit * water.specular.rgb * water.specular.w;
    // the enhanced profile's sun shadows, like on the terrain; no highlight there. The water's own colour darkens with the
    // depth (`k`): in the shallows the shadow is on the bottom (the terrain), and a second one on the surface, apart from it
    // with the sun's slant, came out as a grey-violet band (Iwo_Jima's camera)
    if (any(water.shade.xyz < vec3(1.0)) && lights.n_directional_lights > 0u) {
        let s = sun_shadow(in.world_position, vec3(0.0, 1.0, 0.0), in.position.xy) * (1.0 - water.shade.w * cloud_at(in.world_position.xyz));
        rgb0 = rgb0 * mix(vec3(1.0), mix(water.shade.xyz, vec3(1.0), s), k);
        spec = spec * s;
    }
    let e = flash_light(in.world_position, vec3(0.0, 1.0, 0.0), in.position.xy);
    if any(e > vec3(0.0)) {
        rgb0 = gamma(from_screen(rgb0) * (1.0 + e));
    }
    // (the air's haze, before the fog: `aerial.wesl`)
    rgb0 = gamma(aerial(from_screen(rgb0), in.world_position.xyz));
    let alpha = a.a * b.a * in.uv.y;
    // each pass fogged to half the level's fog (FOGCOLOR #5a5e64 in Wake's #b5bcc9): added, the fog in full
    let f = gamma_fog(in.world_position.xyz);
    let half_fog = gamma(fog.base_color.rgb) * 0.5;
    // (blended by `alpha`: what the second pass adds, over it)
    var fogged = clamp(mix(rgb0, half_fog, f) + mix(spec, half_fog, f) * in.uv.y / max(alpha, 1e-3), vec3(0.0), vec3(1.0));
    var out_alpha = alpha;
    // the enhanced profile's reflection: the world mirrored on the plane (the
    // mirrored camera's image, already fogged along its path), wavy with the normal map, by Schlick's
    // Fresnel for water (times `reflect.x`) over what the original draws
    if water.reflect.x > 0.0 {
        let to_eye = normalize(view.world_position - in.world_position.xyz);
        // less where it is shallow (the original's alpha by the depth): the sand keeps showing
        let fresnel = (0.02 + 0.98 * pow(1.0 - clamp(to_eye.y, 0.0, 1.0), 5.0)) * water.reflect.x * mix(0.3, 1.0, in.uv.y);
        let uv = clamp(in.position.xy / view.viewport.zw + n.xy * REFLECTION_WAVES, vec2(0.0), vec2(1.0));
        let mirrored = gamma(textureSample(reflection, reflection_sampler, uv).rgb);
        out_alpha = fresnel + (1.0 - fresnel) * alpha;
        fogged = (fresnel * mirrored + (1.0 - fresnel) * alpha * fogged) / max(out_alpha, 1e-4);
    }
    // D3D blends in the screen's space, Bevy's blending in linear: half-seen water comes out too bright (Omaha's
    // shore). With the world's copy, the blending by hand
    if water.fx.y > 0.0 && !fx {
        let bg = behind_here(in.position);
        if bg.a > 0.0 {
            fogged = clamp(fogged * out_alpha + bg.rgb * (1.0 - out_alpha), vec3(0.0), vec3(1.0));
            out_alpha = 1.0;
        }
    }
    if fx {
        let to_eye = view.world_position - in.world_position.xyz;
        let u = under(in.position, in.world_position.xyz);
        // how deep it is, straight down, and whether it faces up (the bottom) or not (a hull, a leg)
        let d = in.world_position.y - u.y;
        let up = abs(normalize(cross(dpdx(u), dpdy(u))).y);
        // what is under it, moved by the normal map: less where it is shallow (the shore stays put) and
        // far away; the water over it with its alpha, as the blending would
        let shift = n.xy * REFRACTION * view.viewport.w * clamp(d / REFRACTION_DEPTH, 0.0, 1.0) * clamp(REFRACTION_NEAR / length(to_eye), 0.0, 1.0);
        let bg = behind(in.position, shift);
        if bg.a > 0.0 {
            fogged = fogged * out_alpha + bg.rgb * (1.0 - out_alpha);
            out_alpha = 1.0;
        }
        // the foam: a line where it meets the world and bands over the shallows, broken by the normal map
        // at three scales
        let n2 = gamma(textureSample(normal_map, normal_sampler, xz * water.normal.z * 0.37 - water.normal.yx * t * 1.3).rgb) * 2.0 - 1.0;
        // the line: within FOAM_LINE m of depth and FOAM_SPREAD m (across) of where the bottom comes out,
        // so that a flat beach does not foam for metres; it comes and goes, its edge and its streaks
        // broken by the normal maps
        let n3 = gamma(textureSample(normal_map, normal_sampler, xz * water.normal.z * 2.3 + water.normal.yx * t).rgb) * 2.0 - 1.0;
        let wob = clamp(0.5 + 0.6 * (n3.x + n2.y), 0.0, 1.0);
        let across = d * up / max(sqrt(1.0 - up * up), 0.01);
        let near = max(d / FOAM_LINE, across / FOAM_SPREAD);
        let swash = 0.5 * sin(t * 0.9 + 0.08 * (xz.x + xz.y)) + 0.8 * (wob - 0.5);
        let line = (1.0 - smoothstep(0.0, 1.0, near - swash)) * smoothstep(0.2, 0.7, wob);
        // the bands: thin lines of foam across, every FOAM_BAND m, running in to the shore over the
        // shallows (the bottom only, not a hull)
        let wave = pow(0.5 + 0.5 * sin(6.2832 * across / FOAM_BAND + t * 1.2 + 2.5 * n2.x), 6.0);
        let bands = 0.6 * wave * smoothstep(0.3, 0.7, wob) * smoothstep(0.7, 0.9, up) * (1.0 - smoothstep(0.3 * FOAM_DEPTH, FOAM_DEPTH, d));
        let foam = FOAM_ALPHA * max(line, bands);
        let foam_rgb = mix(FOAM_RGB, gamma(fog.base_color.rgb), f);
        let a = foam + (1.0 - foam) * out_alpha;
        fogged = (foam * foam_rgb + (1.0 - foam) * out_alpha * fogged) / max(a, 1e-4);
        out_alpha = a;
    }
    pbr_input.material.base_color = vec4(from_screen(fogged), out_alpha);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, pbr_input.material.base_color);
    @if(MATERIAL_OIT_ENABLED) {
        oit_draw(in.position, vec4(out.color.rgb * out.color.a, out.color.a));
        discard;
    }
    return out;
}
