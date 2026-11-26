// A lit material with the original's light and/or `envmap true` as the original draws it (its rendering at high
// quality): stage 0 the lit texture, stage 1 BLENDCURRENTALPHA(CURRENT, TEXTURE) on the level's cube by the
// reflection vector. The texture matrix (as the original sets it) is diag(-1, 1, -1) with the view at identity: the world's
// reflection turned 180° about y, which puts the cube's sun where the level's is (El Alamein's, on the cube's +X face
// at (0.73, 0.55, -0.41); `sky.sunLightDirectionVec` -0.75/0.46/0.46). So the texture's alpha keeps the lit texture and
// the rest is the cube, unlit; blended in the screen's (gamma) space, before the fog.
// TODO: with the view off the identity, is it the inverse view × that turn (what this does) or a fixed turn in camera
// space? Only traced with the camera looking north.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_functions::get_tag,
    mesh_view_bindings::view,
};
@if(VIEW_TRANSMISSION_TEXTURE)
import bevy_pbr::render::mesh_view_bindings::{view_transmission_texture, view_transmission_sampler};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var cube_texture: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var cube_sampler: sampler;
struct FixedLight { ambient: vec4<f32>, diffuse: vec4<f32>, sun: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> light: FixedLight;

// (the exact sRGB curve, the one Bevy decodes the textures and encodes the screen with, not x^2.2)
fn to_screen(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3(0.0), vec3(1.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, 12.92 * x, x <= vec3(0.0031308));
}

fn from_screen(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3(0.0), vec3(1.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let keep = pbr_input.material.base_color.a;
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    // the original's fixed function: texture × 2 min(A + D N·L, 1) in gamma, no specular
    let base = pbr_input.material.base_color;
    // stage 1 × TEXTUREFACTOR: a vehicle's, a soldier's or a small static's in the terrain's baked shadow, its
    // mesh's tag as how far each channel is below 255 (`level::shade_tag`: none, 255)
    let tag = get_tag(in.instance_index);
    let tf = vec3(1.0) - vec3(f32(tag & 255u), f32((tag >> 8u) & 255u), f32((tag >> 16u) & 255u)) / 255.0;
    // (a skinned one, ambient.w 2: the skinning shader's sun `c7` is the level's diffuse × the same factor, its ambient `c8`
    // is not, Husky in shade)
    let sun_tf = select(vec3(1.0), tf, light.ambient.w > 1.5);
    let k = 2.0 * min(light.ambient.rgb + light.diffuse.rgb * sun_tf * max(dot(pbr_input.N, light.sun.xyz), 0.0), vec3(1.0));
    var lit = vec4(from_screen(min(to_screen(base.rgb) * k, vec3(1.0)) * tf), base.a);
    if light.ambient.w < -0.5 {
        lit = base;
    } else if light.ambient.w < 0.5 {
        lit = apply_pbr_lighting(pbr_input);
    }
    // (Bevy's frame to Refractor's mirrors z, the half turn mirrors x and z: x alone)
    let r = reflect(-pbr_input.V, pbr_input.N);
    let env = textureSample(cube_texture, cube_sampler, vec3(-r.x, r.y, r.z)).rgb;
    let g = mix(to_screen(env * light.sun.w), to_screen(lit.rgb), select(1.0, keep, light.sun.w > 0.5));
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(from_screen(g), lit.a));
    // (diffuse.w 1: a blended one, the glass)
    if light.diffuse.w > 0.5 {
        out.color.a = gamma_alpha(out.color, in.position);
    }
    return out;
}

// D3D8 blends in the screen's space and Bevy in linear light: the alpha for which Bevy's blending over the copy of the world
// gives D3D's.
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
