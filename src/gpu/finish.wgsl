// The finished frame for the encoder: the titles (a premultiplied overlay over `rect`) on top
// of the composed frame, converted to YUV 4:2:0, BT.709, TV range. Luma is one pass at full
// size; chroma another at half size, averaging each 2×2 block.

struct Finish {
    rect: vec4<f32>,      // overlay region: x0, y0, x1, y1 (empty when x1 <= x0)
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var overlay: texture_2d<f32>;
@group(0) @binding(2) var<uniform> fin: Finish;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// RGB 0..255 of a pixel, titles included.
fn color(p: vec2<u32>) -> vec3<f32> {
    var c = textureLoad(frame, p, 0).rgb * 255.0;
    let f = vec2<f32>(p);
    if f.x >= fin.rect.x && f.y >= fin.rect.y && f.x < fin.rect.z && f.y < fin.rect.w {
        let o = textureLoad(overlay, p, 0);
        c = o.rgb * 255.0 + c * (1.0 - o.a);
    }
    return c;
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
