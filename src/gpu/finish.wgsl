// The finished frame for the encoder: the titles (a premultiplied overlay over `rect`) on top
// of the composed frame, converted to YUV 4:2:0, BT.709, TV range. Luma is one pass at full
// size; chroma another at half size, averaging each 2×2 block.

struct Finish {
    rect: vec4<f32>,      // overlay region: x0, y0, x1, y1 (empty when x1 <= x0)
    post: vec4<f32>,      // vignette, grain, fade, frame number
    size: vec4<f32>,      // frame width, height
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var overlay: texture_2d<f32>;
@group(0) @binding(2) var<uniform> fin: Finish;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// Grain noise, -0.5..0.5 (post::noise).
fn noise(x: u32, y: u32, frame: u32) -> f32 {
    var h = ((x + frame * 7919u) * 374761393u) ^ ((y + frame * 104729u) * 668265263u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32((h ^ (h >> 16u)) & 0xffffu) / 65535.0 - 0.5;
}

fn smooth01(t: f32) -> f32 {
    let x = clamp(t, 0.0, 1.0);
    return x * x * (3.0 - 2.0 * x);
}

// RGB 0..255 of a pixel: titles, then vignette, fade and grain (post::color).
fn color(p: vec2<u32>) -> vec3<f32> {
    var c = textureLoad(frame, p, 0).rgb * 255.0;
    let f = vec2<f32>(p);
    if f.x >= fin.rect.x && f.y >= fin.rect.y && f.x < fin.rect.z && f.y < fin.rect.w {
        let o = textureLoad(overlay, p, 0);
        c = o.rgb * 255.0 + c * (1.0 - o.a);
    }
    let d = (f + vec2<f32>(0.5) - fin.size.xy * 0.5) / (fin.size.xy * 0.5);
    let r = length(d) / sqrt(2.0);
    let k = (1.0 - fin.post.x * 0.75 * smooth01((r - 0.35) / 0.7)) * fin.post.z;
    let g = noise(p.x, p.y, u32(fin.post.w)) * fin.post.y * 36.0 * fin.post.z;
    return c * k + vec3<f32>(g);
}

fn luma_of(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn luma(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
    let y = 16.0 + luma_of(color(vec2<u32>(fp.xy))) * 219.0 / 255.0;
    return vec4<f32>(y / 255.0, 0.0, 0.0, 1.0);
}

struct Chroma {
    @location(0) u: vec4<f32>,
    @location(1) v: vec4<f32>,
}

@fragment
fn chroma(@builtin(position) fp: vec4<f32>) -> Chroma {
    let p = vec2<u32>(fp.xy) * 2u;
    let c = (color(p) + color(p + vec2<u32>(1u, 0u)) + color(p + vec2<u32>(0u, 1u)) + color(p + vec2<u32>(1u, 1u))) * 0.25;
    let y = luma_of(c);
    let cb = 128.0 + (c.b - y) / 1.8556 * 224.0 / 255.0;
    let cr = 128.0 + (c.r - y) / 1.5748 * 224.0 / 255.0;
    var out: Chroma;
    out.u = vec4<f32>(cb / 255.0, 0.0, 0.0, 1.0);
    out.v = vec4<f32>(cr / 255.0, 0.0, 0.0, 1.0);
    return out;
}

// The finished frame as RGBA, for the preview window.
@fragment
fn rgba(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(clamp(color(vec2<u32>(fp.xy)), vec3<f32>(0.0), vec3<f32>(255.0)) / 255.0, 1.0);
}
