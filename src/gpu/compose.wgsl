// One output pixel, as `compose_scene` in compositor.rs computes it on the CPU: background,
// shadows, the imported model (rendered beforehand into `model_layer`) or the built-in device
// (side walls, back, front and screen), and the touch indicators, then the same dither.
// Colours are 0..255 throughout, like the CPU code.

struct Scene {
    size: vec4<f32>,      // cw, ch, ccx, ccy
    cam: vec4<f32>,       // tx, ty, inv_s, dev_scale
    dev: vec4<f32>,       // lcx, lcy, dx, dy
    pose: vec4<f32>,      // focal, depth, tilt, turn
    rot0: vec4<f32>,      // rotation rows
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    eye: vec4<f32>,       // xyz, face_light
    face_n: vec4<f32>,    // xyz, sheen_amount
    light: vec4<f32>,     // xyz, rim
    half_v: vec4<f32>,    // xyz, sc_short
    bg_a: vec4<f32>,      // rgb, half_extent
    bg_b: vec4<f32>,      // rgb, diag
    grad: vec4<f32>,      // gdx, gdy, body_top, body_h
    body: vec4<f32>,      // cx, cy, hx, hy
    screen: vec4<f32>,    // cx, cy, hx, hy
    radii: vec4<f32>,     // body r, screen r, shadow alpha, shadow blur
    shadow: vec4<f32>,    // shadow body cy, contact cy
    scr_rect: vec4<f32>,  // screen x0, y0, w, h
    metal: vec4<f32>,     // rgb
    flags: vec4<f32>,     // angled, mesh_free, framed, tablet
    flags2: vec4<f32>,    // portrait, curved, has_video, has_mesh
    misc: vec4<f32>,      // model supersampling, indicator count
    btn0: vec4<f32>,      // rounded rect: cx, cy, hx, hy
    btn0b: vec4<f32>,     // r, enabled
    btn1: vec4<f32>,
    btn1b: vec4<f32>,
    hole: vec4<f32>,      // x, y, r, enabled
    bar: vec4<f32>,
    barb: vec4<f32>,
    lens0: vec4<f32>,     // x, y, r, enabled
    lens1: vec4<f32>,
}

struct Indicator {
    a: vec4<f32>,         // x, y, r, alpha
    b: vec4<f32>,         // ring_r, ring_alpha, has_trail, trail_alpha
    c: vec4<f32>,         // trail start x, y
}

@group(0) @binding(0) var<uniform> sc: Scene;
@group(0) @binding(1) var<storage, read> inds: array<Indicator>;
@group(0) @binding(2) var video: texture_2d<f32>;
@group(0) @binding(3) var video_sampler: sampler;
@group(0) @binding(4) var model_layer: texture_2d<f32>;

const WHITE = vec3<f32>(255.0, 255.0, 255.0);

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn sd_rrect(r: vec4<f32>, rad: f32, p: vec2<f32>) -> f32 {
    let q = abs(p - r.xy) - r.zw + vec2<f32>(rad);
    let o = max(q, vec2<f32>(0.0));
    return length(o) + min(max(q.x, q.y), 0.0) - rad;
}

fn signum(x: f32) -> f32 {
    return select(-1.0, 1.0, x >= 0.0);
}

// Outward direction of the outline nearest to p.
fn normal_rrect(r: vec4<f32>, rad: f32, p: vec2<f32>) -> vec2<f32> {
    let d = p - r.xy;
    let q = abs(d) - r.zw + vec2<f32>(rad);
    if q.x > 0.0 && q.y > 0.0 {
        let l = max(length(q), 1e-6);
        return vec2<f32>(q.x / l * signum(d.x), q.y / l * signum(d.y));
    } else if q.x > q.y {
        return vec2<f32>(signum(d.x), 0.0);
    }
    return vec2<f32>(0.0, signum(d.y));
}

fn rot_apply(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(sc.rot0.xyz, v), dot(sc.rot1.xyz, v), dot(sc.rot2.xyz, v));
}

fn rot_apply_t(v: vec3<f32>) -> vec3<f32> {
    return sc.rot0.xyz * v.x + sc.rot1.xyz * v.y + sc.rot2.xyz * v.z;
}

fn hash(x: u32, y: u32) -> f32 {
    var h = (x * 374761393u) ^ (y * 668265263u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32((h ^ (h >> 16u)) & 0xffffu) / 65535.0 - 0.5;
}

fn finish(col: vec3<f32>, px: u32, py: u32) -> vec4<f32> {
    let n = hash(px, py);
    let v = floor(clamp(col + vec3<f32>(0.5 + n), vec3<f32>(0.0), vec3<f32>(255.0)));
    return vec4<f32>(v / 255.0, 1.0);
}

// The model layer (premultiplied) averaged over the pixel's samples, over `col`.
fn with_model(col: vec3<f32>, px: u32, py: u32) -> vec3<f32> {
    let ss = u32(sc.misc.x);
    var c = vec4<f32>(0.0);
    for (var j = 0u; j < ss; j++) {
        for (var i = 0u; i < ss; i++) {
            c += textureLoad(model_layer, vec2<u32>(px * ss + i, py * ss + j), 0);
        }
    }
    c /= f32(ss * ss);
    if c.a > 0.0 {
        return col * (1.0 - c.a) + c.rgb;
    }
    return col;
}

fn sample_video(u: f32, v: f32) -> vec3<f32> {
    if sc.flags2.z < 0.5 {
        return vec3<f32>(18.0, 18.0, 20.0);
    }
    return textureSampleLevel(video, video_sampler, vec2<f32>(u, v), 0.0).rgb * 255.0;
}

@fragment
fn fs(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
    let xf = fp.x;
    let yf = fp.y;
    let px = u32(fp.x);
    let py = u32(fp.y);
    let ccx = sc.size.z;
    let ccy = sc.size.w;
    let tx = sc.cam.x;
    let ty = sc.cam.y;
    let dev_scale = sc.cam.w;
    let lcx = sc.dev.x;
    let lcy = sc.dev.y;
    let focal = sc.pose.x;
    let depth = sc.pose.y;
    let angled = sc.flags.x > 0.5;
    let mesh_free = sc.flags.y > 0.5;
    let framed = sc.flags.z > 0.5;
    let tablet = sc.flags.w > 0.5;
    let portrait = sc.flags2.x > 0.5;
    let curved = sc.flags2.y > 0.5;
    let has_mesh = sc.flags2.w > 0.5;

    // 1. Background: gradient with a soft light from the top-left.
    let g = clamp(((xf - ccx) * sc.grad.x + (yf - ccy) * sc.grad.y) / sc.bg_a.w * 0.5 + 0.5, 0.0, 1.0);
    var col = mix(sc.bg_a.rgb, sc.bg_b.rgb, smoothstep(0.0, 1.0, g));
    let glow = max(1.0 - length(vec2<f32>(xf, yf)) / sc.bg_b.w, 0.0) * 0.07;
    col = mix(col, WHITE, glow);

    // 2. Point of the device's front face under this pixel.
    let sx = lcx + ((xf - ccx - tx) * sc.cam.z + ccx - lcx - sc.dev.z) / dev_scale;
    let sy = lcy + ((yf - ccy - ty) * sc.cam.z + ccy - lcy - sc.dev.w) / dev_scale;
    var inv_s = sc.cam.z / dev_scale;
    var q = vec2<f32>(sx, sy);
    var d = vec3<f32>(0.0);
    var has_ray = false;
    var back = false;
    if angled {
        let dir = vec3<f32>(sx - lcx, sy - lcy, -focal);
        d = rot_apply_t(dir);
        back = d.z > 0.0;
        let plane = select(0.0, -depth, back && mesh_free);
        let t = (plane - sc.eye.z) / d.z;
        if abs(d.z) < 1e-4 || t <= 0.0 {
            if has_mesh {
                col = with_model(col, px, py);
            }
            return finish(col, px, py);
        }
        let dlen = length(dir);
        let facing = max(abs(dot(sc.face_n.xyz, dir) / dlen), 0.25);
        inv_s = inv_s * t * dlen / focal / facing;
        q = vec2<f32>(lcx + sc.eye.x + t * d.x, lcy + sc.eye.y + t * d.y);
        has_ray = true;
    }

    // 3. Shadows
    let shadow_alpha = sc.radii.z;
    if shadow_alpha > 0.0 {
        let blur = sc.radii.w;
        let sd1 = sd_rrect(vec4<f32>(sc.body.x, sc.shadow.x, sc.body.zw), sc.radii.x, q);
        let a = shadow_alpha * (1.0 - smoothstep(-blur, blur, sd1));
        let sd2 = sd_rrect(vec4<f32>(sc.body.x, sc.shadow.y, sc.body.zw), sc.radii.x, q);
        let a2 = shadow_alpha * 0.5 * (1.0 - smoothstep(-blur * 0.12, blur * 0.12, sd2));
        col = col * (1.0 - (a + a2 - a * a2));
    }

    // 4. Imported model, or the built-in device's side walls.
    if has_mesh {
        col = with_model(col, px, py);
    }
    let d_body = sd_rrect(sc.body, sc.radii.x, q);
    if has_ray && !has_mesh {
        if depth > 0.0 && d_body > -inv_s && d_body < depth * 2.0 + 2.0 * inv_s {
            // Walk the ray from the face it enters to the opposite one; where it is deepest
            // inside the outline is the side wall it hits.
            let tb = (select(-depth, 0.0, back) - sc.eye.z) / d.z;
            let b = vec2<f32>(lcx + sc.eye.x + tb * d.x, lcy + sc.eye.y + tb * d.y);
            var best = 3.4e38;
            var bp = q;
            for (var k = 1; k <= 8; k++) {
                let p = q + (b - q) * (f32(k) / 8.0);
                let sdv = sd_rrect(sc.body, sc.radii.x, p);
                if sdv < best {
                    best = sdv;
                    bp = p;
                }
            }
            let c = clamp(0.5 - best / inv_s, 0.0, 1.0);
            if c > 0.0 {
                let n2 = normal_rrect(sc.body, sc.radii.x, bp);
                let n = rot_apply(vec3<f32>(n2, 0.0));
                let diffuse = max(dot(n, sc.light.xyz), 0.0);
                let spec = pow(max(dot(n, sc.half_v.xyz), 0.0), 30.0) * 140.0;
                let k = 0.3 + 0.75 * diffuse;
                col = mix(col, sc.metal.rgb * k + vec3<f32>(spec), c);
            }
        }
    }

    if back {
        if !has_mesh && d_body < inv_s {
            let c = clamp(0.5 - d_body / inv_s, 0.0, 1.0);
            let k = 0.45 + 0.55 * max(dot(-sc.face_n.xyz, sc.light.xyz), 0.0);
            var back_col = sc.metal.rgb * k;
            if sc.barb.y > 0.5 {
                let cb = clamp(0.5 - sd_rrect(sc.bar, sc.barb.x, q) / inv_s, 0.0, 1.0);
                if cb > 0.0 {
                    back_col = mix(back_col, vec3<f32>(22.0, 22.0, 26.0), cb);
                    for (var i = 0; i < 2; i++) {
                        let l = select(sc.lens1, sc.lens0, i == 0);
                        if l.w > 0.5 {
                            let dl = length(q - l.xy) - l.z;
                            back_col = mix(back_col, vec3<f32>(6.0, 8.0, 14.0), clamp(0.5 - dl / inv_s, 0.0, 1.0));
                            let ring = abs(dl + l.z * 0.35) - l.z * 0.06;
                            back_col = mix(back_col, vec3<f32>(70.0, 80.0, 100.0), clamp(0.5 - ring / inv_s, 0.0, 1.0) * 0.6);
                        }
                    }
                }
            }
            col = mix(col, back_col, c);
        }
        return finish(col, px, py);
    }

    // 5. Device front
    if framed && !has_mesh {
        let shade = (1.12 - 0.3 * clamp((q.y - sc.grad.z) / sc.grad.w, 0.0, 1.0)) * sc.eye.w;
        let metal_px = sc.metal.rgb * shade;
        for (var i = 0; i < 2; i++) {
            let r = select(sc.btn1, sc.btn0, i == 0);
            let rb = select(sc.btn1b, sc.btn0b, i == 0);
            if rb.y > 0.5 {
                let c = clamp(0.5 - sd_rrect(r, rb.x, q) / inv_s, 0.0, 1.0);
                if c > 0.0 {
                    col = mix(col, mix(metal_px, vec3<f32>(0.0), 0.12), c);
                }
            }
        }
        let c = clamp(0.5 - d_body / inv_s, 0.0, 1.0);
        if c > 0.0 {
            let inset = -d_body;
            let rim = sc.light.w;
            // Polished edge highlight, then the metal band, then the black glass bezel.
            let hl = clamp(1.0 - inset / (rim * 0.4), 0.0, 1.0) * 0.28;
            let band = mix(metal_px, WHITE, hl);
            let inner = smoothstep(rim - inv_s, rim + inv_s, inset);
            var front = mix(band, vec3<f32>(9.0, 9.0, 11.0), inner);
            if tablet && sc.hole.w > 0.5 {
                let dh = length(q - sc.hole.xy) - sc.hole.z;
                front = mix(front, vec3<f32>(28.0, 30.0, 40.0), clamp(0.5 - dh / inv_s, 0.0, 1.0));
            }
            col = mix(col, front, c);
        }
    }

    // 6. Screen
    if !has_mesh {
        let dsc = sd_rrect(sc.screen, sc.radii.y, q);
        let c = clamp(0.5 - dsc / inv_s, 0.0, 1.0);
        if c > 0.0 {
            let u = (q.x - sc.scr_rect.x) / sc.scr_rect.z;
            let v = (q.y - sc.scr_rect.y) / sc.scr_rect.w;
            var scol = sample_video(u, v);
            let sc_short = sc.half_v.w;
            if curved {
                // Glass curving away along the long edges.
                var e = select(min(v, 1.0 - v) * sc.scr_rect.w, min(u, 1.0 - u) * sc.scr_rect.z, portrait);
                e = e / (sc_short * 0.045);
                let dark = (1.0 - smoothstep(0.0, 1.0, e)) * 0.45;
                scol = scol * (1.0 - dark);
                let gl = (e - 0.35) / 0.12;
                scol = mix(scol, WHITE, exp(-(gl * gl)) * 0.18);
            }
            if !tablet && sc.hole.w > 0.5 {
                let dh = length(q - sc.hole.xy) - sc.hole.z;
                let chole = clamp(0.5 - dh / inv_s, 0.0, 1.0);
                if chole > 0.0 {
                    scol = mix(scol, vec3<f32>(4.0, 4.0, 6.0), chole);
                }
            }
            let sheen = sc.face_n.w;
            if sheen > 0.0 {
                let gs = u * 0.6 + v * 0.4 - (0.55 - sc.pose.w / 120.0 + sc.pose.z / 160.0);
                let gq = gs / 0.12;
                scol = mix(scol, WHITE, sheen * exp(-(gq * gq)));
            }
            col = mix(col, scol, c);
        }
    }

    // 7. Touch indicators
    let n_inds = u32(sc.misc.y);
    for (var i = 0u; i < n_inds; i++) {
        let ind = inds[i];
        let ip = ind.a.xy;
        let r = ind.a.z;
        if ind.b.z > 0.5 {
            // Distance to the segment from the trail start to the finger.
            let s0 = ind.c.xy;
            let vv = ip - s0;
            let len2 = max(dot(vv, vv), 1e-6);
            let h = clamp(dot(q - s0, vv) / len2, 0.0, 1.0);
            let dd = length(q - s0 - vv * h);
            let a = ind.b.w * h * clamp(0.5 - (dd - r * 0.7) / inv_s, 0.0, 1.0);
            if a > 0.0 {
                col = mix(col, WHITE, a);
            }
        }
        let dist = length(q - ip);
        if ind.a.w > 0.0 {
            let dd = dist - r;
            let fill = clamp(0.5 - dd / inv_s, 0.0, 1.0);
            if fill > 0.0 {
                // Grey body with a white rim reads well on both light and dark apps.
                let rimv = smoothstep(-r * 0.16 - inv_s, -r * 0.16 + inv_s, dd);
                let a = ind.a.w * fill * min(1.0 + 0.6 * rimv, 1.6);
                col = mix(col, mix(vec3<f32>(118.0, 118.0, 126.0), WHITE, rimv), min(a, 1.0));
            }
        }
        if ind.b.y > 0.0 {
            let dd = abs(dist - ind.b.x) - r * 0.06;
            let a = clamp(0.5 - dd / inv_s, 0.0, 1.0) * ind.b.y;
            if a > 0.0 {
                col = mix(col, WHITE, a);
            }
        }
    }
    return finish(col, px, py);
}
