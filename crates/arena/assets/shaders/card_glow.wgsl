// Soft pulsing rim drawn on a slightly larger quad behind a card (hand cards, stack, anything
// Selectable/Highlighted). Additive and unlit so it reads as light, not geometry.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::globals}

struct Glow {
    color: vec4<f32>,  // rgb colour, a = intensity
    extra: vec4<f32>,  // x = pulse amount, y = pulse speed, z = quad margin (card units)
};
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> glow: Glow;

const CARD = vec2<f32>(0.63, 0.88);

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let m = glow.extra.z;
    let size = CARD + vec2<f32>(2.0 * m);
    let p = (in.uv - 0.5) * size;
    let d = sd_round_box(p, CARD * 0.5, 0.034);
    let pulse = 1.0 - glow.extra.x + glow.extra.x * (0.5 + 0.5 * sin(globals.time * glow.extra.y));
    // halo falling off outside the card edge, plus a crisp bright line hugging it
    let halo = exp(-max(d, 0.0) * 34.0) * smoothstep(-0.002, 0.004, d);
    let line = (1.0 - smoothstep(0.0, 0.007, abs(d - 0.006))) * smoothstep(-0.002, 0.003, d);
    let a = (halo * 0.75 + line * 0.9) * pulse * glow.color.a;
    return vec4<f32>(glow.color.rgb * a, a);
}
