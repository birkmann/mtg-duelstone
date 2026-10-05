// Main-menu arcane portal: a swirling fbm vortex inside a bright rim and a counter-rotating ring of
// rune dashes. Unlit, with HDR values so bloom turns the rim into light. Premultiplied output:
// the vortex has alpha, so cards circling *behind* it disappear into the tunnel, while the rim
// and halo (alpha 0) only add light.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::globals}

struct Portal {
    color: vec4<f32>,   // rgb = rim colour, a = overall intensity
    params: vec4<f32>,  // x = ignition (0 → 1; above 1 = intro flash), y = swirl speed
};
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> portal: Portal;

const TAU: f32 = 6.2831853;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x);
    let b = mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x);
    return mix(a, b, u.y);
}

fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var amp = 0.5;
    var sum = 0.0;
    for (var i = 0; i < 5; i++) {
        sum += amp * noise(p);
        p = p * 2.03 + vec2<f32>(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ign = portal.params.x;
    let t = globals.time * portal.params.y;
    let p = (in.uv - 0.5) * 2.0;
    let r = length(p);
    let ang = atan2(p.y, p.x);
    // the rim opens outward from a point as the portal ignites
    let R = 0.6 * clamp(ign, 0.0, 1.0);
    if (R < 0.002) {
        discard;
    }

    // Swirl: angular offset grows toward the centre, so the noise spirals inward.
    let sw = ang + 1.8 / (r + 0.22) - t;
    let q = vec2<f32>(cos(sw), sin(sw)) * (r * 2.4 + 0.6);
    let n = fbm(q * 1.7 + vec2<f32>(0.0, t * 0.35));
    let n2 = fbm(q * 3.3 - vec2<f32>(t * 0.25, 0.0));
    let inside = 1.0 - smoothstep(R * 0.9, R, r);
    let depth = smoothstep(0.0, R, r);
    let deep = vec3<f32>(0.05, 0.012, 0.008);
    let hot = vec3<f32>(1.0, 0.45, 0.12);
    // contrast: mostly dark smoke with bright spiral arms
    let arms = smoothstep(0.42, 0.75, n);
    var col = mix(deep, hot, arms * (0.25 + 0.75 * depth)) * (0.4 + 1.5 * n2 * n2) * inside;
    // a black eye so it reads as a tunnel rather than a flat disc
    col *= mix(0.08, 1.0, smoothstep(0.0, R * 0.75, r));
    // bright streaks spiralling in
    let streak = pow(max(sin(sw * 5.0 + n * 6.0), 0.0), 12.0) * inside * depth;
    col += hot * streak * 1.6;

    let rim_col = portal.color.rgb;
    let rim = exp(-abs(r - R) * 60.0) * (0.75 + 0.25 * sin(ang * 7.0 + t * 3.0));
    col += rim_col * rim * 6.0;

    // rune ring: dashes of uneven length just outside the rim, turning against the swirl
    let rr = R * 1.17;
    let band = 1.0 - smoothstep(0.004, 0.016, abs(r - rr));
    let cell = (ang / TAU + 0.5) * 40.0 + t * 0.12;
    let dash = step(0.35 + 0.3 * hash(vec2<f32>(floor(cell), 3.0)), fract(cell));
    col += rim_col * band * dash * (0.9 + 0.6 * sin(ang * 3.0 - t * 2.0)) * 2.4;
    let band2 = 1.0 - smoothstep(0.0, 0.006, abs(r - R * 1.27));
    col += rim_col * band2 * 0.9;

    // halo outside the rim
    col += rim_col * exp(-max(r - R, 0.0) * 7.5) * (1.0 - inside) * 0.5;
    // intro flash: a burst of light while ignition overshoots 1
    col += rim_col * exp(-r * 2.5) * max(ign - 1.0, 0.0) * 9.0;
    col *= 1.0 - smoothstep(0.86, 1.0, r);

    let c = col * portal.color.a;
    let alpha = inside * portal.color.a;
    return vec4<f32>(c, alpha);
}
