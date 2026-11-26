// Aerial perspective (the `aerial` setting; not in the original): a little of the fog's colour before the original's fog
// starts, growing with the distance, and the sun's light scattered in that air towards the sun. Applied to the lit colour
// before the fog, so the fog's own mix takes it away where it is whole: at the fog's end nothing of it is left, the
// terrain still meets the sky as in the original and nothing is seen farther than there. Level 3.
//
// Its strength rides in the fog's `directional_light_exponent` (`level::Atmosphere::distance_fog`): Bevy reads that only
// when the alpha of `directional_light_color` is above 0, which ours never is.
//
// What ends in Bevy's fog (what Bevy lights with its own material: vehicles, soldiers, weapons; and the trees, the baked
// statics and the billboards) takes the haze and the glow there (`level::gamma_fog` patches them in).
import bevy_pbr::render::mesh_view_bindings::{view, fog, lights};
import bevy_render::maths::PI;

// how much of the fog's colour the air holds out at the fog's end, at full strength (before the fog's own mix); it grows with
// the square of the distance, so what is near keeps its contrast (with 1 − e^(−3t) the dark water 50 m off came out milky)
const HAZE: f32 = 0.3;
// how bright the sun's light in the air is, at full strength, looking straight at it out at the fog's end
const GLOW: f32 = 0.22;
// how narrow its glow is round the sun
const GLOW_EXP: f32 = 6.0;

// The strength, 0–1 (0 without the setting).
fn aerial_strength() -> f32 {
    if (fog.directional_light_color.a > 0.0) {
        return 0.0;
    }
    return clamp(fog.directional_light_exponent, 0.0, 1.0);
}

// The colour `c` (linear) of what is `d` metres off, with the air's haze between it and the eye (no glow).
fn aerial_haze(c: vec3<f32>, d: f32) -> vec3<f32> {
    let k = aerial_strength();
    if (k <= 0.0) {
        return c;
    }
    let t = clamp(d / max(fog.be.y, 1.0), 0.0, 1.0);
    return mix(c, fog.base_color.rgb, k * HAZE * t * t);
}

// The colour `c` (linear) of what is at `p` with the sun's light scattered in the air between it and the eye, without the
// haze: for what Bevy's fog fogs (its material's `fog_enabled`), whose fog adds the haze after it (`level::gamma_fog`).
fn aerial_glow(c: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    let k = aerial_strength();
    if (k <= 0.0) {
        return c;
    }
    let v = p - view.world_position;
    let d = max(length(v), 1e-3);
    let t = clamp(d / max(fog.be.y, 1.0), 0.0, 1.0);
    // (the brightest directional light is the sun, or the night's fill with it; its colour over π is the level's diffuse)
    var sun = vec3(0.0);
    var to_sun = vec3(0.0, 1.0, 0.0);
    for (var i = 0u; i < lights.n_directional_lights; i = i + 1u) {
        let l = lights.directional_lights[i];
        if (dot(l.color.rgb, vec3(1.0)) > dot(sun, vec3(1.0))) {
            sun = l.color.rgb;
            to_sun = l.direction_to_light;
        }
    }
    let toward = pow(max(dot(v / d, to_sun), 0.0), GLOW_EXP);
    return c + min(sun / PI, vec3(1.0)) * (k * GLOW * toward * t * t);
}

// The colour `c` (linear) of what is at `p`, with the air between it and the eye: haze and glow, for what fogs itself
// (the terrain, the water).
fn aerial(c: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    return aerial_glow(aerial_haze(c, distance(p, view.world_position)), p);
}
