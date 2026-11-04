// The imported model, as the CPU rasterizer in model3d.rs draws it: projected like
// `Scene::place`, depth-tested on 1/depth, lit by `shade`, with the recording on the screen
// triangles. Output is premultiplied (colour 0..255, coverage 0..1) and drawn at the
// supersampled size; compose.wgsl averages the samples of each pixel.

struct Frame {
    rot0: vec4<f32>,      // rotation rows; .w = mox, moy, unit
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    p0: vec4<f32>,        // focal, nearest, dev_scale, s
    p1: vec4<f32>,        // lcx + dx, lcy + dy, ccx, ccy
    p2: vec4<f32>,        // tx, ty, cw, ch
    light: vec4<f32>,
    half_v: vec4<f32>,
    scr: vec4<f32>,       // screen x0, y0, w, h
    vid: vec4<f32>,       // video x0, y0, w, h
    misc: vec4<f32>,      // has_video
}

struct Material {
    base: vec4<f32>,      // rgb (0..255), alpha
    pbr: vec4<f32>,       // metallic, roughness, cutoff (< 0: none), flags (1 texture, 2 blend, 4 screen)
    emissive: vec4<f32>,
}

@group(0) @binding(0) var<uniform> fr: Frame;
@group(0) @binding(1) var video: texture_2d<f32>;
@group(0) @binding(2) var video_sampler: sampler;
@group(1) @binding(0) var<uniform> mat: Material;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var tex_sampler: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) suv: vec2<f32>,
}

fn rot(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(fr.rot0.xyz, v), dot(fr.rot1.xyz, v), dot(fr.rot2.xyz, v));
}

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) suv: vec2<f32>) -> VOut {
    let unit = fr.rot2.w;
    let w = rot(vec3<f32>(fr.rot0.w + p.x * unit, fr.rot1.w + p.y * unit, p.z * unit));
    let focal = fr.p0.x;
    let f = focal / (focal + fr.p0.y - w.z);
    let x = fr.p1.x + w.x * f * fr.p0.z;
    let y = fr.p1.y + w.y * f * fr.p0.z;
    let ox = fr.p1.z + (x - fr.p1.z) * fr.p0.w + fr.p2.x;
    let oy = fr.p1.w + (y - fr.p1.w) * fr.p0.w + fr.p2.y;
    let ndc = vec2<f32>(ox / fr.p2.z * 2.0 - 1.0, 1.0 - oy / fr.p2.w * 2.0);
    // w = depth, so attributes interpolate perspective-correctly as on the CPU; nearer (larger
    // 1/depth) wins the depth test.
    let ww = 1.0 / f;
    var out: VOut;
    out.pos = vec4<f32>(ndc * ww, (1.0 - 0.5 * f) * ww, ww);
    out.n = rot(n);
    out.uv = uv;
    out.suv = suv;
    return out;
}

fn screen_color(u: f32, v: f32) -> vec3<f32> {
    let vu = (fr.scr.x + u * fr.scr.z - fr.vid.x) / fr.vid.z;
    let vv = (fr.scr.y + v * fr.scr.w - fr.vid.y) / fr.vid.w;
    if vu < -0.002 || vu > 1.002 || vv < -0.002 || vv > 1.002 {
        return vec3<f32>(0.0);
    }
    if fr.misc.x < 0.5 {
        return vec3<f32>(18.0, 18.0, 20.0);
    }
    return textureSampleLevel(video, video_sampler, clamp(vec2<f32>(vu, vv), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb * 255.0;
}

// Simple studio lighting: soft sky from above, a key light from the top-left and a highlight
// whose sharpness follows the roughness.
fn shade(base: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let diffuse = max(dot(n, fr.light.xyz), 0.0);
    let sky = 0.5 - 0.5 * n.y;
    let metal = clamp(mat.pbr.x, 0.0, 1.0);
    let rough = clamp(mat.pbr.y, 0.04, 1.0);
    let spec_pow = 6.0 + (1.0 - rough) * (1.0 - rough) * 160.0;
    let spec = pow(max(dot(n, fr.half_v.xyz), 0.0), spec_pow) * (0.06 + 0.9 * metal) * (1.15 - rough);
    let dif = base * (0.3 * sky + 0.8 * diffuse) * (1.0 - 0.8 * metal);
    let refl = base * metal * (0.25 + 0.55 * sky);
    let tint = select(vec3<f32>(255.0), base, metal > 0.5);
    return min(dif + refl + tint * spec + mat.emissive.rgb, vec3<f32>(255.0));
}

@fragment
fn fs(in: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let flags = u32(mat.pbr.w);
    if (flags & 4u) != 0u {
        return vec4<f32>(screen_color(in.suv.x, in.suv.y), 1.0);
    }
    var base = mat.base.rgb;
    var a = mat.base.a;
    if (flags & 1u) != 0u {
        let c = textureSampleLevel(tex, tex_sampler, in.uv, 0.0);
        base = base * c.rgb;
        a = a * c.a;
    }
    if mat.pbr.z >= 0.0 {
        if a < mat.pbr.z {
            discard;
        }
        a = 1.0;
    } else if (flags & 2u) == 0u {
        a = 1.0;
    }
    var n = normalize(in.n);
    if !front {
        n = -n;
    }
    return vec4<f32>(shade(base, n) * a, a);
}
