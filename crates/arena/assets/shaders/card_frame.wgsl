// Battlefield card face: coloured frame, pale name bar, art-crop window, state rims.
// Extends StandardMaterial so scene lighting/tonemapping treat it like the full-card faces.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}

struct Params {
    frame_a: vec4<f32>, // rgb = primary frame colour
    frame_b: vec4<f32>, // rgb = secondary colour, a = how much of it (0 = solid A)
    state: vec4<f32>,   // x selectable, y highlighted, z weak-selectable, w hover
    misc: vec4<f32>,    // x tapped, y damage fraction, z kind (0 other, 1 land, 2 creature), w unused
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: Params;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var art_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var art_samp: sampler;

const CARD = vec2<f32>(0.63, 0.88);
// art window and name bar in uv space (0..1, y down)
const WIN_MIN = vec2<f32>(0.0714, 0.150);
const WIN_MAX = vec2<f32>(0.9286, 0.835);
const BAR_MIN = vec2<f32>(0.0714, 0.052);
const BAR_MAX = vec2<f32>(0.9286, 0.132);

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// signed distance (card units) to a rounded box given in uv-space min/max
fn sd_uv_box(uv: vec2<f32>, mn: vec2<f32>, mx: vec2<f32>, r: f32) -> f32 {
    let c = (mn + mx) * 0.5;
    let h = (mx - mn) * 0.5 * CARD;
    return sd_round_box((uv - c) * CARD, h, r);
}

fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let uv = in.uv;
    let t = globals.time;

    // ---- frame --------------------------------------------------------------------------
    let a = params.frame_a.rgb;
    let b = params.frame_b.rgb;
    var frame = mix(a, b, params.frame_b.a * smoothstep(0.15, 0.85, uv.x));
    // soft top-light gradient + inner vignette so the frame doesn't look flat
    frame = frame * (0.82 + 0.28 * (1.0 - uv.y)) ;
    let d_outer = sd_round_box((uv - 0.5) * CARD, CARD * 0.5, 0.034);
    // thin dark outline + bevel highlight just inside it
    let outline = smoothstep(-0.007, -0.004, d_outer);
    let bevel = smoothstep(-0.014, -0.008, d_outer) * (1.0 - smoothstep(-0.008, -0.006, d_outer));
    var col = frame + vec3<f32>(0.10) * bevel * (1.0 - uv.y);
    col = mix(col, vec3<f32>(0.02, 0.015, 0.012), outline);

    // ---- bottom stat strip (P/T plate is drawn by the HUD over this) ----------------------
    let strip = smoothstep(0.835, 0.86, uv.y) * (1.0 - smoothstep(0.95, 0.975, uv.y));
    col = mix(col, col * 0.72, strip * 0.8);
    // fine engraved line between art and strip
    let line_y = abs(uv.y - 0.845);
    col = mix(col, col * 0.55, (1.0 - smoothstep(0.0015, 0.003, line_y)) * 0.8);

    // ---- art window -----------------------------------------------------------------------
    let d_win = sd_uv_box(uv, WIN_MIN, WIN_MAX, 0.012);
    let win_uv = (uv - WIN_MIN) / (WIN_MAX - WIN_MIN);
    let dim = vec2<f32>(textureDimensions(art_tex));
    let art_aspect = dim.x / dim.y;
    let win_aspect = ((WIN_MAX.x - WIN_MIN.x) * CARD.x) / ((WIN_MAX.y - WIN_MIN.y) * CARD.y);
    var auv = win_uv;
    if (win_aspect < art_aspect) {
        auv.x = 0.5 + (win_uv.x - 0.5) * (win_aspect / art_aspect);
    } else {
        auv.y = 0.5 + (win_uv.y - 0.5) * (art_aspect / win_aspect);
    }
    var art = textureSample(art_tex, art_samp, clamp(auv, vec2<f32>(0.001), vec2<f32>(0.999))).rgb;
    // inner shadow along the window edge
    let edge_shadow = 1.0 - 0.38 * (1.0 - smoothstep(-0.028, -0.002, d_win));
    art = art * edge_shadow;
    // damage tint
    art = mix(art, art * vec3<f32>(1.0, 0.35, 0.3) + vec3<f32>(0.25, 0.0, 0.0), clamp(params.misc.y, 0.0, 1.0) * 0.6);
    let in_win = 1.0 - smoothstep(-0.003, 0.0, d_win);
    col = mix(col, art, in_win);
    // keyline around the window
    let key = (1.0 - smoothstep(0.0, 0.0035, abs(d_win + 0.002)));
    col = mix(col, vec3<f32>(0.03, 0.02, 0.015), key * 0.9);

    // ---- name bar ---------------------------------------------------------------------------
    let d_bar = sd_uv_box(uv, BAR_MIN, BAR_MAX, 0.016);
    let barlocal = (uv.y - BAR_MIN.y) / (BAR_MAX.y - BAR_MIN.y);
    var bar = mix(vec3<f32>(0.93, 0.91, 0.86), vec3<f32>(0.74, 0.72, 0.68), barlocal);
    bar = mix(bar, frame, 0.14);
    let in_bar = 1.0 - smoothstep(-0.003, 0.0, d_bar);
    col = mix(col, bar, in_bar);
    let bar_key = (1.0 - smoothstep(0.0, 0.0035, abs(d_bar + 0.002)));
    col = mix(col, vec3<f32>(0.03, 0.02, 0.015), bar_key * 0.85);

    // ---- states -----------------------------------------------------------------------------
    // tapped: drained and darkened (greyed out so tapped permanents read at a glance)
    let g = luma(col);
    col = mix(col, vec3<f32>(g) * 0.9, params.misc.x * 0.55);
    col = col * (1.0 - 0.32 * params.misc.x);
    // hover brighten: scale, never add — `col` is linear, so even +0.1 lifts the art's blacks to
    // a milky grey and the card reads as washed out
    col = col * (1.0 + 0.18 * params.state.w);

    // glowing rim in the frame: selectable (blue), highlighted (gold), weak (soft blue)
    let rim_w = 1.0 - smoothstep(-0.02, -0.004, d_outer);   // 1 at the very edge, 0 deeper in
    let rim = (smoothstep(-0.026, -0.006, d_outer) ) * (1.0 - smoothstep(-0.006, 0.0, d_outer));
    let pulse = 0.72 + 0.28 * sin(t * 3.2);
    let blue = vec3<f32>(0.20, 0.62, 1.0);
    let gold = vec3<f32>(1.0, 0.80, 0.30);
    var emissive = vec3<f32>(0.0);
    emissive += blue * rim * params.state.x * (1.4 * pulse + 0.4);
    emissive += gold * rim * params.state.y * 1.6;
    emissive += blue * rim * params.state.z * 0.5;
    col = mix(col, blue, 0.25 * rim * params.state.x);

    pbr_input.material.base_color = vec4<f32>(col, 1.0);
    pbr_input.material.emissive = vec4<f32>(emissive * 1.0, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
