// A tree as the original draws it:
// sprites: the vertex in view space plus its corner's offset × the object's scale, colour `c11`; trunks and branches: D3D's
// light by vertex, min(ambient + diffuse · N·L, 1) (the material's, not the vertex's: `DIFFUSEMATERIALSOURCE` 0 at these draws). MODULATE2X with the texture, in the screen's (gamma) space.
// Darkened where a soldier's or a vehicle's shadow falls by `shade.xyz`, the light it keeps there over its light (the
// enhanced profile's sun).
import bevy_pbr::render::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    mesh_view_bindings::{view, lights},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    pbr_types::{STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE},
    shadows::fetch_directional_shadow,
};
import bevy_pbr::render::{clustered_forward as clustering, mesh_view_bindings::clustered_lights, pbr_lighting::getDistanceAttenuation};
import bevy_render::maths::PI;
@if(VIEW_TRANSMISSION_TEXTURE)
import bevy_pbr::render::mesh_view_bindings::{view_transmission_texture, view_transmission_sampler};

struct TreeLight {
    shade: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    sun: vec4<f32>,
    // the tree's TEXTUREFACTOR (stage 1, MOD in the screen's space): how lit the terrain says it stands
    factor: vec4<f32>,
    // x 1: the alpha that makes the linear blending come out as D3D8's in the screen's space
    blend: vec4<f32>,
    // rgb: the sprites' colour, `c11`: the GeometryTemplate's `lightColor`
    sprite: vec4<f32>,
    // the fade towards the billboard: x `billboardDistance` (0: none), y the fade's
    // share of it, z 1 for the new function (cosine)
    fade: vec4<f32>,
};

// The alpha of a tree's mesh as it gives way to its billboard (`TreeFade` in level.rs; the client's tree renderer): 1 nearer
// than D − F; hidden past D (on the CPU, `fade_trees`). D grows with the tree's largest scale; the distance is on the
// ground plane, times the view's half vertical angle.
fn fade(world: mat4x4<f32>) -> f32 {
    if (light.fade.x <= 0.0) {
        return 1.0;
    }
    let far = light.fade.x * max(max(length(world[0].xyz), length(world[1].xyz)), length(world[2].xyz));
    let band = far * light.fade.y;
    let d = length(view.world_position.xz - world[3].xz) * atan(1.0 / view.clip_from_view[1][1]);
    let t = clamp((d - (far - band)) / max(band, 1e-4), 0.0, 1.0);
    return select(1.0 - t * t * t, cos(t * PI * 0.5), light.fade.z > 0.5);
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> light: TreeLight;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var at = view.view_from_world * mesh_functions::mesh_position_local_to_world(world, vec4(v.position, 1.0));
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, vertex_in.instance_index);
    out.uv = v.uv;
    // (only a sprite's corners have an offset)
    at = vec4(at.xy + v.uv_b * vec2(length(world[0].xyz), length(world[1].xyz)), at.zw);
    out.uv_b = v.uv_b;
    let diffuse = light.diffuse.rgb * max(dot(normalize(out.world_normal), light.sun.xyz), 0.0);
    // (alpha: the fade, the TEXTUREFACTOR's alpha in the original)
    out.color = vec4(select(min(light.ambient.rgb + diffuse, vec3(1.0)), light.sprite.rgb, any(v.uv_b != vec2(0.0))), fade(world));
    out.world_position = view.world_from_view * at;
    out.position = view.clip_from_view * at;
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
    // (the texture alone: the vertex's light goes on in the screen's space)
    var plain = in;
    plain.color = vec4(1.0);
    var pbr_input = pbr_input_from_standard_material(plain, is_front);
    var color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    let sprite = any(in.uv_b != vec2(0.0));
    color = vec4(from_screen(min(2.0 * to_screen(color.rgb) * in.color.rgb, vec3(1.0)) * light.factor.rgb), color.a);
    // (the stage MOD(TF, CURRENT) of the alpha: the fade, on the blended branches and leaves; a trunk does not blend)
    // (not on what is opaque, the trunks: an alpha below 1 there came out white over the sky)
    if ((pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) != STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE) {
        color.a *= light.factor.a * in.color.a;
    }
    // (a sprite takes no shadows: its position is not on the tree's surface and it came out black)
    if (lights.n_directional_lights > 0u && !sprite) {
        let s = sun_shadow(in.world_position, normalize(in.world_normal), in.position.xy);
        color = vec4(color.rgb * pow(mix(light.shade.xyz, vec3(1.0), s), vec3(2.2)), color.a);
    }
    let e = flash_light(in.world_position, normalize(in.world_normal), in.position.xy);
    if any(e > vec3(0.0)) {
        color = vec4(color.rgb * (1.0 + e), color.a);
    }
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    if (light.blend.x > 0.0) {
        out.color.a = gamma_alpha(out.color, in.position);
    }
    return out;
}

// D3D8 blends in the screen's space, Bevy in linear light: a dark twig half seen over the sand comes out lighter.
// With the world drawn so far under it (Bevy's copy for the transmissive pass, taken again after it: `backdrop.rs`), the alpha a' for which Bevy's
// a' S + (1 − a') D gives D3D's lin(a s + (1 − a) d) (least squares over rgb); as it is without the copy, or where they are alike.
// Exact over the opaque world; over another blended piece (the copy does not have it) nearly so.
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
