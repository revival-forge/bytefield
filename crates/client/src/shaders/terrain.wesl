// The terrain as the original draws it: stage 0 its colour tile,
// stage 1 ADDSIGNED its detail texture (c + d − 0.5), in the screen's (gamma) space, and the fog there too. Unlit: the
// tiles carry their light. In the enhanced profile the sun's shadow darkens it by
// `detail_scale.yzw`, the ambient over ambient + diffuse (the light's flag says whether it casts them). With the contact shadows
// theirs darkens it the same way, and Bevy's SSAO takes the ambient part of its light. In the original profile
// the original's shadows of soldiers and vehicles (`object_shadow`).
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    mesh_view_bindings::{view, fog, lights},
    mesh_view_types::{DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT, DIRECTIONAL_LIGHT_FLAGS_CONTACT_SHADOWS_ENABLED_BIT},
    shadows::fetch_directional_shadow,
};
import bevy_pbr::render::{clustered_forward as clustering, mesh_view_bindings::clustered_lights, pbr_lighting::getDistanceAttenuation};
import bevy_render::maths::PI;
@if(SCREEN_SPACE_AMBIENT_OCCLUSION) import bevy_pbr::render::mesh_view_bindings::screen_space_ambient_occlusion_texture;
@if(CONTACT_SHADOWS && DEPTH_PREPASS) import bevy_pbr::render::{pbr_functions::calculate_contact_shadow, mesh_view_bindings::contact_shadows_settings};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> detail_scale: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var detail_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var detail_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var shadow_rows: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var shadow_atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var shadow_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var<uniform> clip: vec4<f32>;

// Past the world's far plane (x, the view distance; in the view's depth, as D3D's projection clips), nothing: the sky
// shows there.
fn past_far(p: vec4<f32>, far: f32) -> bool {
    let z = dot(vec4(view.view_from_world[0].z, view.view_from_world[1].z, view.view_from_world[2].z, view.view_from_world[3].z), p);
    return -z > far;
}

// The original's shadows of soldiers and vehicles: each silhouette from the sun, one row of
// the atlas of 20, blended over the tile by its alpha × `params.y`, as the original draws the terrain again with each
// (and they add up). What is left of the colour at `p`.
fn object_shadow(params: vec4<f32>, p: vec3<f32>) -> f32 {
    var keep = 1.0;
    let n = u32(params.x);
    let texel = 0.5 / 128.0;
    for (var i = 0u; i < n; i = i + 1u) {
        let y = i32(i);
        let c = textureLoad(shadow_rows, vec2(2, y), 0);
        if (dot(c.xyz, p) + c.w > 0.0) {
            continue;
        }
        let r0 = textureLoad(shadow_rows, vec2(0, y), 0);
        let r1 = textureLoad(shadow_rows, vec2(1, y), 0);
        let uv = vec2(dot(r0.xyz, p) + r0.w, dot(r1.xyz, p) + r1.w);
        if (any(uv <= vec2(0.0)) || any(uv >= vec2(1.0))) {
            continue;
        }
        let a = textureSampleLevel(shadow_atlas, shadow_sampler, vec2(uv.x, (f32(i) + clamp(uv.y, texel, 1.0 - texel)) / 20.0), 0.0).r;
        keep *= 1.0 - params.y * a;
    }
    return keep;
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

// The sun among the directional lights: the one with light (the statics' shadow map has none: `level.rs`).
fn sun_index() -> u32 {
    for (var i = 0u; i < lights.n_directional_lights; i = i + 1u) {
        if (any(lights.directional_lights[i].color.rgb > vec3(0.0))) {
            return i;
        }
    }
    return 0u;
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

fn to_screen(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, 12.92 * c, c <= vec3(0.0031308));
}

fn from_screen(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if past_far(in.world_position, clip.x) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // The original's stage 1 carries MIPMAPLODBIAS -1.5: without it the sand near the camera comes out soft.
    // TODO: Bocage's grass, one patch grainier than the original (std dev 20.4 against 16.5), another equal: too much
    // bias on some detail textures?
    let d = textureSampleBias(detail_texture, detail_sampler, in.world_position.xz * detail_scale.x, view.mip_bias - 1.5).rgb;
    // (in the screen's space with the exact sRGB curve, as Bevy decodes the textures and encodes the screen: x^2.2 left the
    // dark ground darker, Cassino's)
    // TODO: near the camera the original takes a smaller mip of the colour tiles (blurred in depth, Omaha 268) and we keep
    // sharper 1–2 m patches its detail would hide: which LOD each takes.
    let base = to_screen(clamp(pbr_input.material.base_color.rgb, vec3(0.0), vec3(1.0)));
    var lit = clamp(base + to_screen(clamp(d, vec3(0.0), vec3(1.0))) - 0.5, vec3(0.0), vec3(1.0));
    var s = 1.0;
    if (any(detail_scale.yzw < vec3(1.0)) && lights.n_directional_lights > 0u) {
        s = sun_shadow(in.world_position, normalize(in.world_normal), in.position.xy);
@if(CONTACT_SHADOWS && DEPTH_PREPASS) {
        let sun = sun_index();
        if ((lights.directional_lights[sun].flags & DIRECTIONAL_LIGHT_FLAGS_CONTACT_SHADOWS_ENABLED_BIT) != 0u && contact_shadows_settings.linear_steps > 0u && s > 0.0) {
            s *= calculate_contact_shadow(in.world_position.xyz, in.position.xy, lights.directional_lights[sun].direction_to_light, contact_shadows_settings.linear_steps);
        }
}
    }
    // (the light the shadow leaves is the ambient: what the occlusion takes)
    var ao = 1.0;
@if(SCREEN_SPACE_AMBIENT_OCCLUSION) {
    // (Bevy's leaves open ground at some 0.8 and draws the mesh's creases: what is above that is open)
    ao = saturate(textureLoad(screen_space_ambient_occlusion_texture, vec2<i32>(in.position.xy - view.viewport.xy), 0i).r / 0.85);
}
    let unshadowed = lit;
    if (s < 1.0 || ao < 1.0) {
        lit = lit * (detail_scale.yzw * ao + (1.0 - detail_scale.yzw) * s);
    }
    // a flash's or an explosion's light, over the tile's own (its colour as the albedo)
    let e = flash_light(in.world_position, normalize(in.world_normal), in.position.xy);
    if any(e > vec3(0.0)) {
        lit = to_screen(clamp(from_screen(lit) + from_screen(unshadowed) * e, vec3(0.0), vec3(1.0)));
    }
    let shadow_params = textureLoad(shadow_rows, vec2(0, 20), 0);
    if (shadow_params.x > 0.0) {
        lit *= object_shadow(shadow_params, in.world_position.xyz);
    }
    // D3D's vertex fog (RANGEFOGENABLE: by the distance), mixed in the screen's space, not Bevy's linear one
    let f = clamp((distance(in.world_position.xyz, view.world_position) - fog.be.x) / (fog.be.y - fog.be.x), 0.0, 1.0);
    let fogged = mix(lit, to_screen(clamp(fog.base_color.rgb, vec3(0.0), vec3(1.0))), f);
    pbr_input.material.base_color = vec4(from_screen(fogged), pbr_input.material.base_color.a);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, pbr_input.material.base_color);
    return out;
}
