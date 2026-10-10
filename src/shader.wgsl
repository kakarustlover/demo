// Two tiny pipelines: animated backgrounds (fullscreen) and rounded rect quads (panel + text pixels).
struct U {
    res: vec2<f32>,
    time: f32,
    kind: u32,
};
@group(0) @binding(0) var<uniform> u: U;

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: BgOut;
    o.pos = vec4<f32>(p[i], 0.0, 1.0);
    o.uv = vec2<f32>((p[i].x + 1.0) * 0.5, (1.0 - p[i].y) * 0.5);
    return o;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn blob(q: vec2<f32>, c: vec2<f32>, r: f32) -> f32 {
    let d = length(q - c) / r;
    return exp(-d * d * 2.0);
}

fn pal(x: f32) -> vec3<f32> {
    return 0.5 + 0.5 * cos(vec3<f32>(0.0, 2.0, 4.0) + x);
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    let a = u.res.x / u.res.y;
    let q = vec2<f32>(in.uv.x * a, in.uv.y);
    let t = u.time;
    var c = vec3<f32>(0.0);
    switch u.kind {
        case 0u: { // AURORA
            c = mix(vec3<f32>(0.10, 0.08, 0.40), vec3<f32>(1.0, 0.45, 0.6), in.uv.y);
            for (var i = 0; i < 5; i = i + 1) {
                let f = f32(i);
                let ctr = vec2<f32>(a * (0.5 + 0.35 * cos(t * 0.4 + f * 1.3)), 0.5 + 0.38 * sin(t * 0.3 + f * 2.1));
                c = c + pal(f * 1.2) * blob(q, ctr, 0.35) * 0.7;
            }
        }
        case 1u: { // OCEAN
            c = mix(vec3<f32>(0.05, 0.15, 0.30), vec3<f32>(0.40, 0.85, 0.95), in.uv.y);
            for (var i = 0; i < 4; i = i + 1) {
                let f = f32(i);
                let w = 0.30 + 0.17 * f + 0.03 * sin(q.x * (6.0 + f * 2.0) + t * (1.0 + f * 0.5));
                c = mix(c, vec3<f32>(1.0), 0.12 * smoothstep(0.0, 0.004, q.y - w));
            }
        }
        case 2u: { // SUNSET
            c = mix(vec3<f32>(0.17, 0.06, 0.33), vec3<f32>(1.0, 0.85, 0.35), in.uv.y);
            let sun = length(q - vec2<f32>(a * 0.5, 0.55));
            c = mix(c, vec3<f32>(1.0, 0.55, 0.0), 1.0 - smoothstep(0.26, 0.28, sun));
            let band = step(0.55, in.uv.y) * step(0.5, fract(in.uv.y * 18.0 + t * 0.05));
            c = mix(c, vec3<f32>(0.17, 0.06, 0.33), band * 0.85);
        }
        case 3u: { // NEON GRID
            let g = abs(fract(q * 8.0 + vec2<f32>(0.0, t * 0.3)) - vec2<f32>(0.5, 0.5));
            let line = smoothstep(0.46, 0.5, max(g.x, g.y));
            c = vec3<f32>(0.03, 0.02, 0.07) + line * mix(vec3<f32>(1.0, 0.0, 0.9), vec3<f32>(0.0, 0.9, 1.0), in.uv.y);
            let r1 = abs(length(q - vec2<f32>(a * 0.25, 0.3)) - 0.18);
            let r2 = abs(length(q - vec2<f32>(a * 0.75, 0.65)) - 0.22);
            c = c + vec3<f32>(1.0, 0.0, 0.9) * (1.0 - smoothstep(0.0, 0.012, r1));
            c = c + vec3<f32>(0.0, 0.9, 1.0) * (1.0 - smoothstep(0.0, 0.012, r2));
        }
        case 4u: { // STRIPES
            let s = (q.x * 0.94 + q.y * 0.34) * 9.0 + t * 0.5;
            c = pal(floor(s) * 1.3);
        }
        case 5u: { // DOTS
            let gq = q * 7.0;
            let id = floor(gq);
            var f = fract(gq) - vec2<f32>(0.5, 0.5);
            let h = hash(id);
            f.y = f.y + 0.2 * sin(t + h * 6.28);
            let d = length(f) - (0.2 + 0.2 * h);
            c = mix(vec3<f32>(0.08, 0.08, 0.16), pal(hash(id + vec2<f32>(1.0, 1.0)) * 6.28), 1.0 - smoothstep(0.0, 0.02, d));
        }
        default: { // LIGHT
            c = vec3<f32>(0.95, 0.95, 0.97);
            for (var i = 0; i < 4; i = i + 1) {
                let f = f32(i);
                let ctr = vec2<f32>(a * (0.5 + 0.3 * cos(t * 0.2 + f * 1.7)), 0.5 + 0.35 * sin(t * 0.25 + f * 2.3));
                c = mix(c, pal(f * 1.6 + 0.5), 1.0 - smoothstep(0.0, 0.01, length(q - ctr) - 0.2));
            }
        }
    }
    // Colours above are written as sRGB-looking values; the target is an sRGB view.
    return vec4<f32>(pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2)), 1.0);
}

struct QIn {
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius: vec4<f32>,
};
struct QOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) radius: f32,
};

@vertex
fn vs_quad(in: QIn) -> QOut {
    var corner = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let c = corner[in.vi];
    let px = in.rect.xy + c * in.rect.zw;
    var o: QOut;
    o.pos = vec4<f32>(px.x / u.res.x * 2.0 - 1.0, 1.0 - px.y / u.res.y * 2.0, 0.0, 1.0);
    o.local = c * in.rect.zw;
    o.size = in.rect.zw;
    o.color = in.color;
    o.radius = in.radius.x;
    return o;
}

@fragment
fn fs_quad(in: QOut) -> @location(0) vec4<f32> {
    let h = in.size * 0.5;
    let r = min(in.radius, min(h.x, h.y));
    let p = abs(in.local - h) - (h - vec2<f32>(r, r));
    let d = length(max(p, vec2<f32>(0.0, 0.0))) + min(max(p.x, p.y), 0.0) - r;
    let a = clamp(0.5 - d, 0.0, 1.0);
    return vec4<f32>(in.color.rgb, in.color.a * a);
}
