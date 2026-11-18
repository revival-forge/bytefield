// A static with its own lightmap as the original draws it (its D3D
// reference run with lightmaps): no lighting, stage 0 the texture, stage 1 MODULATE2X the lightmap (the second
// UVs), in the screen's (gamma) space. The lightmap comes as the level's palette's colours (`Palette.pal`), not sRGB.
// In the enhanced profile a soldier's or a vehicle's shadow and a flash's light fall on it too:
// the lightmap already holds the statics' own shadows, so the sun's map darkens only where the statics' map leaves it lit.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    mesh_view_bindings::{view, lights},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
};
import bevy_pbr::render::{clustered_forward as clustering, mesh_view_bindings::clustered_lights, pbr_lighting::getDistanceAttenuation};
import bevy_render::maths::PI;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var lightmap_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var lightmap_sampler: sampler;
// what the surface keeps of its light in a shadow (rgb), in the screen's space; 1 without the sun's shadows. w: the far
// distance f of a `textureFade` piece (else 0)
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> shade: vec4<f32>;

// (as in tree.wesl)
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
            // (a face turned from the sun has no sun to lose: its lightmap already has it dark, and its own shadow, of a
            // static whose shadow the tiles lack and so not in the statics' map, fell on it twice)
            s = select(1.0, f, dot(n, lights.directional_lights[i].direction_to_light) > 0.0);
        } else {
            lit = f;
        }
    }
    return 1.0 - (1.0 - s) * lit;
}

fn to_screen(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3(0.0031308));
}

fn from_screen(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    // (a mesh without the second UVs never takes this material: the light of the palette's middle)
    var light = vec3(0.5);
@if(VERTEX_UVS_B) {
    light = textureSample(lightmap_texture, lightmap_sampler, in.uv_b).rgb;
}
    // (in the screen's space with the exact sRGB curve, the one Bevy decodes the textures and encodes the screen with: with
    // x^2.2 the dark lightmaps came out darker, Cassino's walls in shadow 24 for the original's 30)
    let lit = min(to_screen(clamp(color.rgb, vec3(0.0), vec3(1.0))) * light * 2.0, vec3(1.0));
    var linear = from_screen(lit);
    let n = normalize(in.world_normal);
    if (any(shade.xyz < vec3(1.0)) && lights.n_directional_lights > 0u) {
        linear = linear * pow(mix(shade.xyz, vec3(1.0), sun_shadow(in.world_position, n, in.position.xy)), vec3(2.2));
    }
    linear = linear * (1.0 + flash_light(in.world_position, n, in.position.xy));
    // `textureFade`: the original's TEXTUREFACTOR alpha for Husky's
    // `italianhouse08_m1` (in Husky, f 200) is (d² − (f/10)²) / (f² − (f/10)²) of the distance d: clear up close, opaque at f.
    // Level 2: the original by the object's distance, here the fragment's. TODO: the f/10 is a guess with f 200 only.
    var alpha = color.a;
    if (shade.w > 0.0) {
        let d = in.world_position.xyz - view.world_position;
        let n2 = shade.w * shade.w * 0.01;
        alpha = alpha * clamp((dot(d, d) - n2) / (shade.w * shade.w - n2), 0.0, 1.0);
    }
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(linear, alpha));
    return out;
}
