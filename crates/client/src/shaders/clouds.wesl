// The shadows of clouds drifting over the level (the `clouds` setting; not in the original): value noise drifting with
// the time, cast along the sun from a layer CLOUD_HEIGHT metres up. The terrain, the grass, the water, the trees, the
// baked statics and the billboards take it in their shaders; what Bevy lights (soldiers, vehicles, the arms, statics
// without a lightmap) in Bevy's own sun loop (`level::gamma_fog` patches it in). Level 3.
//
// Its strength rides in the red of the fog's `directional_light_color` (`level::Atmosphere::distance_fog`): Bevy reads that
// only when its alpha is above 0, which ours never is.
//
// TODO: `globals.time` wraps every hour and the clouds jump then (the grass's wind too): a longer
// `Time::<Virtual>::set_wrap_period`, minding the sway's precision.
import bevy_pbr::render::{
    mesh_view_bindings::{fog, lights, globals},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
};

const CLOUD_HEIGHT: f32 = 600.0;
// the noise's cell, metres
const CLOUD_CELL: f32 = 260.0;
// where the clouds drift, metres a second
const CLOUD_WIND: vec2<f32> = vec2(5.0, 2.0);

fn cloud_hash(i: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(i.x) * 0x8da6b343u) ^ (bitcast<u32>(i.y) * 0xd8163841u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777215.0;
}

fn cloud_noise(q: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(q));
    let f = q - floor(q);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(cloud_hash(i), cloud_hash(i + vec2(1, 0)), u.x);
    let b = mix(cloud_hash(i + vec2(0, 1)), cloud_hash(i + vec2(1, 1)), u.x);
    return mix(a, b, u.y);
}

// How much of the sun the clouds take at p, 0–1, with the sun towards `to_sun`, at `t` (`globals.time`).
fn cloud_cover(p: vec3<f32>, to_sun: vec3<f32>, t: f32) -> f32 {
    let q = (p.xz + to_sun.xz * ((CLOUD_HEIGHT - p.y) / max(to_sun.y, 0.2)) + CLOUD_WIND * t) / CLOUD_CELL;
    let n = 0.55 * cloud_noise(q) + 0.3 * cloud_noise(q * 2.07 + vec2(5.3, 1.7)) + 0.15 * cloud_noise(q * 4.13 + vec2(2.9, 7.1));
    return smoothstep(0.5, 0.66, n);
}

// Whether directional light i is the sun: the one with light and shadows (at night the fill has light and no
// shadows, the statics' own map shadows and no light).
fn is_sun(i: u32) -> bool {
    return any(lights.directional_lights[i].color.rgb > vec3(0.0)) && (lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u;
}

// The cloud over p, 0–1, towards the sun; 0 without it.
fn cloud_at(p: vec3<f32>) -> f32 {
    for (var i = 0u; i < lights.n_directional_lights; i = i + 1u) {
        if (is_sun(i)) {
            return cloud_cover(p, lights.directional_lights[i].direction_to_light, globals.time);
        }
    }
    return 0.0;
}

// The setting, 0–1 (0 without it or without the sun's shadows).
fn cloud_strength() -> f32 {
    if (fog.directional_light_color.a > 0.0) {
        return 0.0;
    }
    return clamp(fog.directional_light_color.r, 0.0, 1.0);
}

// The share of directional light i that reaches p under the clouds: for Bevy's own sun loop (`level::gamma_fog`).
fn cloud_light(i: u32, p: vec3<f32>) -> f32 {
    let k = cloud_strength();
    if (k <= 0.0 || !is_sun(i)) {
        return 1.0;
    }
    return 1.0 - k * cloud_cover(p, lights.directional_lights[i].direction_to_light, globals.time);
}
