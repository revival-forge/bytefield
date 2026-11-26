// An effect's sprite (`effects.rs`): Bevy's unlit path, and with soft particles its alpha fades
// where it nears what the depth prepass shows behind it, over half the sprite's size (`uv_b.x`); with no
// prepass on the view (the arms', the mirror's) or the setting off (`soft.x` 0), as before. `soft.y` 1: additive.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
};

// (with the setting `oit`: into the camera's list per pixel, sorted and blended after the
// transparent pass, as Bevy's `pbr.wesl` does; the pipeline then has no blending and draws nothing)
@if(MATERIAL_OIT_ENABLED)
import bevy_core_pipeline::oit::draw::oit_draw;

@if(DEPTH_PREPASS)
import bevy_pbr::{prepass::utils::prepass_depth, render::view_transformations::depth_ndc_to_view_z};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> soft: vec4<f32>;

@if(DEPTH_PREPASS && VERTEX_UVS_B)
fn fade(in: VertexOutput) -> f32 {
    if soft.x <= 0.0 {
        return 1.0;
    }
    // (view z is negative ahead: what is behind the sprite is the more negative)
    let gap = depth_ndc_to_view_z(in.position.z) - depth_ndc_to_view_z(prepass_depth(in.position, 0u));
    return clamp(gap / max(in.uv_b.x * soft.x, 0.05), 0.0, 1.0);
}
@else
fn fade(in: VertexOutput) -> f32 {
    return 1.0;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, pbr_input.material.base_color);
    out.color.a = out.color.a * fade(in);
    @if(MATERIAL_OIT_ENABLED) {
        oit_draw(in.position, vec4(out.color.rgb * out.color.a, out.color.a));
        discard;
    }
    // (an additive one, `soft.y` 1: premultiplied with no alpha, so it adds colour × alpha to what is behind; with OIT, blended)
    if soft.y > 0.5 {
        out.color = vec4(out.color.rgb * out.color.a, 0.0);
    }
    return out;
}
