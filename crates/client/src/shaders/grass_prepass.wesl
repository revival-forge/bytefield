// The grass in the depth prepass (`grass.wesl`): the same place, and where it was a frame before for the motion vectors.
import bevy_pbr::prepass::io::{Vertex, VertexOutput, decompress_vertex};
import bevy_render::globals::Globals;
import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(107) var<uniform> wind: vec4<f32>;
// (the clock: in the prepass's view bindings, Bevy's `globals` is the second)
@group(0) @binding(1) var<uniform> globals: Globals;

// A blade's vertex where the wind and the distance put it. `p` the vertex as made (Bevy's axes), `root` its tuft's
// root (`uv_b` its x and z) and `up` its height over it (the colour's alpha). The tufts thin out with the distance (a
// share of them, by a hash of the root, shrinks into the ground) and the rest shrink from 0.6 of the reach to it; the
// wind bends each blade by the square of its height, a gust that travels across the field plus a flutter per tuft.
fn grass_at(p: vec3<f32>, root: vec3<f32>, up: f32, t: f32, eye: vec3<f32>) -> vec3<f32> {
    let reach = wind.z;
    let d = distance(root, eye);
    // (a hash without sin: the roots are hundreds of metres from 0)
    var q = fract(root.xz * vec2(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    let rank = fract((q.x + q.y) * q.x);
    let keep = mix(1.0, 0.35, smoothstep(0.25 * reach, reach, d));
    let s = (1.0 - smoothstep(0.6 * reach, reach, d)) * clamp((keep - rank) * 8.0, 0.0, 1.0);
    let dir = normalize(vec2(1.0, 0.35));
    let gust = 0.6 + 0.4 * sin(dot(root.xz, dir) * 0.35 - t * 1.7);
    let flutter = 0.3 * sin(t * 3.3 + rank * 6.2832);
    let bend = dir * (wind.w * (gust + flutter) * up * up);
    return root + (p - root) * s + vec3(bend.x, 0.0, bend.y) * s;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let p = mesh_functions::mesh_position_local_to_world(world, vec4(v.position, 1.0)).xyz;
    let root = vec3(v.uv_b.x, p.y - v.color.a, v.uv_b.y);
    out.world_position = vec4(grass_at(p, root, v.color.a, globals.time, view.world_position), 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
    @if(VERTEX_UVS_A)
    out.uv = v.uv;
    @if(VERTEX_UVS_B)
    out.uv_b = v.uv_b;
    @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, vertex_in.instance_index);
    @if(VERTEX_COLORS)
    out.color = vec4(v.color.rgb, 1.0);
    @if(MOTION_VECTOR_PREPASS) {
    out.previous_world_position = vec4(grass_at(p, root, v.color.a, globals.time - globals.delta_time, view.world_position), 1.0);
    }
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}
