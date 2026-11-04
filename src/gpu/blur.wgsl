// Motion blur: integer sums of the dithered subframes, then their rounded average, as on the CPU.

struct Blur {
    n: vec4<u32>,         // subframe count
}

@group(0) @binding(0) var sub_frame: texture_2d<f32>;
@group(0) @binding(1) var acc_in: texture_2d<u32>;
@group(0) @binding(2) var<uniform> blur: Blur;

@fragment
fn accumulate(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<u32>(fp.xy);
    let v = vec4<u32>(round(textureLoad(sub_frame, p, 0) * 255.0));
    return textureLoad(acc_in, p, 0) + v;
}

@fragment
fn resolve(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
    let n = blur.n.x;
    let a = textureLoad(acc_in, vec2<u32>(fp.xy), 0);
    return vec4<f32>((a + vec4<u32>(n / 2u)) / vec4<u32>(n)) / 255.0;
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
