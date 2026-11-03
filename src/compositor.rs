//! Renders one output frame: background, device, the screen recording, camera zoom and touches.
//! The same code drives the live preview and the export, so what you see is what you get.

use crate::camera::CamState;
use crate::model::{FrameStyle, Style, Touch, TouchKind};
use rayon::prelude::*;

/// An RGBA8 image.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// Mip pyramid of a screen frame, so that heavy downscaling (a 2400 px tall recording drawn
/// 900 px tall) stays sharp instead of shimmering.
pub struct Mips {
    levels: Vec<Frame>,
}

impl Mips {
    pub fn build(base: Frame) -> Self {
        let mut levels = vec![base];
        loop {
            let last = levels.last().unwrap();
            if last.w < 128 || last.h < 128 {
                break;
            }
            let next = halve(last);
            levels.push(next);
        }
        Self { levels }
    }

    pub fn width(&self) -> usize {
        self.levels[0].w
    }

    fn level_for(&self, src_px_per_out_px: f32) -> &Frame {
        let lvl = if src_px_per_out_px > 1.0 { src_px_per_out_px.log2().floor() as usize } else { 0 };
        &self.levels[lvl.min(self.levels.len() - 1)]
    }
}

fn halve(f: &Frame) -> Frame {
    let (w, h) = (f.w / 2, f.h / 2);
    let mut data = vec![0u8; w * h * 4];
    data.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let r0 = &f.data[(2 * y) * f.w * 4..];
        let r1 = &f.data[(2 * y + 1) * f.w * 4..];
        for x in 0..w {
            for c in 0..4 {
                let i = 2 * x * 4 + c;
                let sum = r0[i] as u32 + r0[i + 4] as u32 + r1[i] as u32 + r1[i + 4] as u32;
                row[x * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    });
    Frame { w, h, data }
}

#[derive(Debug, Clone, Copy)]
pub struct Rect2 {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect2 {
    fn from_center(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self { x0: cx - w * 0.5, y0: cy - h * 0.5, x1: cx + w * 0.5, y1: cy + h * 0.5 }
    }
    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }
    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) * 0.5
    }
    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) * 0.5
    }
}

/// Where everything sits on the canvas before the camera is applied ("scene" coordinates,
/// which equal output pixels when the camera is not zoomed).
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub cw: f32,
    pub ch: f32,
    pub body: Rect2,
    pub body_r: f32,
    pub screen: Rect2,
    pub screen_r: f32,
    pub bezel: f32,
    pub framed: bool,
    pub portrait: bool,
}

impl Layout {
    pub fn new(cw: f32, ch: f32, style: &Style, video_w: f32, video_h: f32) -> Self {
        let min_dim = cw.min(ch);
        let pad = style.padding * min_dim;
        let avail_w = (cw - 2.0 * pad).max(1.0);
        let avail_h = (ch - 2.0 * pad).max(1.0);

        let aspect = (video_w / video_h.max(1.0)).max(0.05);
        let portrait = aspect < 1.0;
        let framed = style.frame != FrameStyle::None;
        let k = if framed { 0.045 } else { 0.0 };

        // Sizes in units of the screen's short side.
        let (sw, sh) = if portrait { (1.0, 1.0 / aspect) } else { (aspect, 1.0) };
        let unit = (avail_w / (sw + 2.0 * k)).min(avail_h / (sh + 2.0 * k));

        let (cx, cy) = (cw * 0.5, ch * 0.5);
        let screen = Rect2::from_center(cx, cy, sw * unit, sh * unit);
        let bezel = k * unit;
        let body = Rect2::from_center(cx, cy, screen.w() + 2.0 * bezel, screen.h() + 2.0 * bezel);
        let screen_r = unit
            * match style.frame {
                FrameStyle::Rounded => 0.105,
                FrameStyle::Flat => 0.04,
                FrameStyle::None => 0.06,
            };
        let body_r = if framed { screen_r + bezel } else { screen_r };

        Self { cw, ch, body, body_r, screen, screen_r, bezel, framed, portrait }
    }

    /// Scale and translation the camera applies to the scene: `out = c + (q - c) * s + t`.
    pub fn camera(&self, cam: CamState) -> (f32, f32, f32) {
        let s = cam.scale.clamp(0.5, 5.0);
        let (cx, cy) = (self.cw * 0.5, self.ch * 0.5);
        let px = self.screen.x0 + cam.fx.clamp(0.0, 1.0) * self.screen.w();
        let py = self.screen.y0 + cam.fy.clamp(0.0, 1.0) * self.screen.h();

        // Move the focus towards the centre as the zoom grows; untouched at rest (s = 1).
        let pull = (2.0 * (s - 1.0)).min(s).max(0.0);
        let mut tx = -(px - cx) * pull;
        let mut ty = -(py - cy) * pull;

        // Keep the device inside the frame while it fits, and never show past its edges once it
        // is bigger than the frame.
        let clamp_axis = |t: f32, lo: f32, hi: f32, c: f32, size: f32| {
            let a = -(c + (lo - c) * s);
            let b = size - (c + (hi - c) * s);
            t.clamp(a.min(b), a.max(b))
        };
        // When the device overflows the frame both ways, push a little further so its rounded
        // corners never peek into a corner of the frame.
        let over = (self.body.w() * s - self.cw).min(self.body.h() * s - self.ch);
        let inset = self.body_r * 0.4 * smoothstep(0.0, 0.1 * self.cw.min(self.ch), over);
        tx = clamp_axis(tx, self.body.x0 + inset, self.body.x1 - inset, cx, self.cw);
        ty = clamp_axis(ty, self.body.y0 + inset, self.body.y1 - inset, cy, self.ch);
        (s, tx, ty)
    }

    /// Output pixel -> scene point.
    pub fn to_scene(&self, cam: (f32, f32, f32), x: f32, y: f32) -> (f32, f32) {
        let (s, tx, ty) = cam;
        let (cx, cy) = (self.cw * 0.5, self.ch * 0.5);
        ((x - cx - tx) / s + cx, (y - cy - ty) / s + cy)
    }
}

pub struct ComposeInput<'a> {
    pub style: &'a Style,
    pub screen: Option<&'a Mips>,
    pub video_w: f32,
    pub video_h: f32,
    pub cam: CamState,
    pub touches: &'a [Touch],
    pub time: f32,
}

struct Indicator {
    x: f32,
    y: f32,
    r: f32,
    alpha: f32,
    ring_r: f32,
    ring_alpha: f32,
    trail: Option<(f32, f32, f32)>, // start point and alpha
}

struct RRect {
    cx: f32,
    cy: f32,
    hx: f32,
    hy: f32,
    r: f32,
}

impl RRect {
    #[inline(always)]
    fn sd(&self, x: f32, y: f32) -> f32 {
        let qx = (x - self.cx).abs() - self.hx + self.r;
        let qy = (y - self.cy).abs() - self.hy + self.r;
        let ox = qx.max(0.0);
        let oy = qy.max(0.0);
        (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - self.r
    }

    fn of(rect: &Rect2, r: f32) -> Self {
        Self { cx: rect.cx(), cy: rect.cy(), hx: rect.w() * 0.5, hy: rect.h() * 0.5, r }
    }
}

#[inline(always)]
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

#[inline(always)]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline(always)]
fn rgbf(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

#[inline(always)]
fn hash(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0 - 0.5
}

#[inline(always)]
fn sample(f: &Frame, u: f32, v: f32) -> [f32; 3] {
    let x = (u * f.w as f32 - 0.5).clamp(0.0, (f.w - 1) as f32);
    let y = (v * f.h as f32 - 0.5).clamp(0.0, (f.h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(f.w - 1), (y0 + 1).min(f.h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |xx: usize, yy: usize| {
        let i = (yy * f.w + xx) * 4;
        [f.data[i] as f32, f.data[i + 1] as f32, f.data[i + 2] as f32]
    };
    let top = mix(px(x0, y0), px(x1, y0), fx);
    let bottom = mix(px(x0, y1), px(x1, y1), fx);
    mix(top, bottom, fy)
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn indicators(layout: &Layout, style: &Style, touches: &[Touch], time: f32) -> Vec<Indicator> {
    if !style.show_touches {
        return Vec::new();
    }
    let sc = &layout.screen;
    let to_scene = |u: f32, v: f32| (sc.x0 + u * sc.w(), sc.y0 + v * sc.h());
    let r = sc.w().min(sc.h()) * 0.055 * style.touch_size;
    const FADE: f32 = 0.4;

    let mut out = Vec::new();
    for t in touches {
        let age = time - t.time;
        if age < -0.05 || age > t.duration + FADE {
            continue;
        }
        let appear = smoothstep(-0.05, 0.06, age);
        let release = ((age - t.duration) / FADE).clamp(0.0, 1.0);
        match t.kind {
            TouchKind::Tap => {
                let (x, y) = to_scene(t.x, t.y);
                out.push(Indicator {
                    x,
                    y,
                    r: r * (0.8 + 0.2 * appear) * (1.0 + 0.35 * release),
                    alpha: 0.5 * appear * (1.0 - release).powi(2),
                    ring_r: r * (1.0 + 1.1 * release),
                    ring_alpha: if release > 0.0 { 0.45 * (1.0 - release) } else { 0.0 },
                    trail: None,
                });
            }
            TouchKind::Swipe => {
                let p = ease_out(age / t.duration.max(0.05));
                let (sx, sy) = to_scene(t.x, t.y);
                let (ex, ey) = to_scene(t.end_x, t.end_y);
                let (x, y) = (sx + (ex - sx) * p, sy + (ey - sy) * p);
                let tail = (p - 0.45).max(0.0);
                let (tx, ty) = (sx + (ex - sx) * tail, sy + (ey - sy) * tail);
                let fade = 1.0 - release;
                out.push(Indicator {
                    x,
                    y,
                    r: r * (0.8 + 0.2 * appear),
                    alpha: 0.5 * appear * fade * fade,
                    ring_r: 0.0,
                    ring_alpha: 0.0,
                    trail: Some((tx, ty, 0.22 * appear * fade)),
                });
            }
        }
    }
    out
}

/// Renders the frame into `out` (RGBA8, `ow * oh * 4` bytes).
pub fn compose(out: &mut [u8], ow: usize, oh: usize, inp: &ComposeInput) {
    if out.len() != ow * oh * 4 || ow == 0 || oh == 0 {
        return;
    }
    let style = inp.style;
    let (cw, ch) = (ow as f32, oh as f32);
    let layout = Layout::new(cw, ch, style, inp.video_w, inp.video_h);
    let cam = layout.camera(inp.cam);
    let (s, tx, ty) = cam;
    let inv_s = 1.0 / s;
    let (ccx, ccy) = (cw * 0.5, ch * 0.5);
    let min_dim = cw.min(ch);

    // Background gradient
    let bg_a = rgbf(style.background.a);
    let bg_b = rgbf(style.background.b);
    let ang = style.background.angle.to_radians();
    let (gdx, gdy) = (ang.cos(), ang.sin());
    let half_extent = ((cw * gdx.abs() + ch * gdy.abs()) * 0.5).max(1.0);
    let diag = (cw * cw + ch * ch).sqrt();

    // Device shapes
    let body = RRect::of(&layout.body, layout.body_r);
    let screen = RRect::of(&layout.screen, layout.screen_r);
    let shadow_alpha = style.shadow.clamp(0.0, 1.0) * 0.55;
    let shadow_blur = 0.06 * min_dim;
    let shadow_body = RRect { cy: body.cy + 0.025 * min_dim, ..RRect::of(&layout.body, layout.body_r) };
    let contact = RRect { cy: body.cy + 0.006 * min_dim, ..RRect::of(&layout.body, layout.body_r) };

    let metal = rgbf(style.frame_color.rgb());
    let rim = layout.bezel * 0.34;
    let glass = [9.0, 9.0, 11.0];

    let buttons: Vec<RRect> = if layout.framed {
        let b = &layout.body;
        let depth = (layout.bezel * 0.16).max(1.0);
        let rr = depth;
        if layout.portrait {
            let x = b.x1 + depth * 0.4;
            vec![
                RRect { cx: x, cy: b.y0 + b.h() * 0.255, hx: depth, hy: b.h() * 0.035, r: rr },
                RRect { cx: x, cy: b.y0 + b.h() * 0.40, hx: depth, hy: b.h() * 0.07, r: rr },
            ]
        } else {
            let y = b.y0 - depth * 0.4;
            vec![
                RRect { cx: b.x0 + b.w() * 0.255, cy: y, hx: b.w() * 0.035, hy: depth, r: rr },
                RRect { cx: b.x0 + b.w() * 0.40, cy: y, hx: b.w() * 0.07, hy: depth, r: rr },
            ]
        }
    } else {
        Vec::new()
    };

    let hole = if layout.framed {
        let sc = &layout.screen;
        let r = sc.w().min(sc.h()) * 0.024;
        let off = sc.w().min(sc.h()) * 0.05;
        Some(if layout.portrait { (sc.cx(), sc.y0 + off, r) } else { (sc.x0 + off, sc.cy(), r) })
    } else {
        None
    };

    let mip = inp.screen.map(|m| {
        let ratio = m.width() as f32 / (layout.screen.w() * s).max(1.0);
        m.level_for(ratio)
    });

    let inds = indicators(&layout, style, inp.touches, inp.time);
    let white = [255.0, 255.0, 255.0];
    let body_top = layout.body.y0;
    let body_h = layout.body.h().max(1.0);

    out.par_chunks_mut(ow * 4).enumerate().for_each(|(py, row)| {
        let yf = py as f32 + 0.5;
        for px in 0..ow {
            let xf = px as f32 + 0.5;

            // 1. Background: gradient with a soft light from the top-left, dithered to avoid banding.
            let g = (((xf - ccx) * gdx + (yf - ccy) * gdy) / half_extent * 0.5 + 0.5).clamp(0.0, 1.0);
            let mut col = mix(bg_a, bg_b, smoothstep(0.0, 1.0, g));
            let glow = (1.0 - ((xf * xf + yf * yf).sqrt() / diag)).max(0.0) * 0.07;
            col = mix(col, white, glow);

            // 2. Scene point under this pixel.
            let qx = (xf - ccx - tx) * inv_s + ccx;
            let qy = (yf - ccy - ty) * inv_s + ccy;
            let cov = |d: f32| (0.5 - d * s).clamp(0.0, 1.0);

            // 3. Shadows
            if shadow_alpha > 0.0 {
                let d = shadow_body.sd(qx, qy);
                let a = shadow_alpha * (1.0 - smoothstep(-shadow_blur, shadow_blur, d));
                let d2 = contact.sd(qx, qy);
                let a2 = shadow_alpha * 0.5 * (1.0 - smoothstep(-shadow_blur * 0.12, shadow_blur * 0.12, d2));
                let k = 1.0 - (a + a2 - a * a2);
                col = [col[0] * k, col[1] * k, col[2] * k];
            }

            // 4. Device body
            if layout.framed {
                let shade = 1.12 - 0.3 * ((qy - body_top) / body_h).clamp(0.0, 1.0);
                let metal_px = [metal[0] * shade, metal[1] * shade, metal[2] * shade];
                for b in &buttons {
                    let c = cov(b.sd(qx, qy));
                    if c > 0.0 {
                        col = mix(col, mix(metal_px, [0.0; 3], 0.12), c);
                    }
                }
                let d = body.sd(qx, qy);
                let c = cov(d);
                if c > 0.0 {
                    let depth = -d;
                    // Polished edge highlight, then the metal band, then the black glass bezel.
                    let hl = (1.0 - depth / (rim * 0.4)).clamp(0.0, 1.0) * 0.28;
                    let band = mix(metal_px, white, hl);
                    let inner = smoothstep(rim - inv_s, rim + inv_s, depth);
                    col = mix(col, mix(band, glass, inner), c);
                }
            }

            // 5. Screen
            let d = screen.sd(qx, qy);
            let c = cov(d);
            if c > 0.0 {
                let u = (qx - layout.screen.x0) / layout.screen.w();
                let v = (qy - layout.screen.y0) / layout.screen.h();
                let mut sc = match mip {
                    Some(f) => sample(f, u, v),
                    None => [18.0, 18.0, 20.0],
                };
                if let Some((hx, hy, hr)) = hole {
                    let dh = ((qx - hx).powi(2) + (qy - hy).powi(2)).sqrt() - hr;
                    let ch = cov(dh);
                    if ch > 0.0 {
                        sc = mix(sc, [4.0, 4.0, 6.0], ch);
                    }
                }
                col = mix(col, sc, c);
            }

            // 6. Touch indicators
            for ind in &inds {
                if let Some((sx, sy, ta)) = ind.trail {
                    // Distance to the segment from the trail start to the finger.
                    let (vx, vy) = (ind.x - sx, ind.y - sy);
                    let len2 = (vx * vx + vy * vy).max(1e-6);
                    let h = (((qx - sx) * vx + (qy - sy) * vy) / len2).clamp(0.0, 1.0);
                    let dd = ((qx - sx - vx * h).powi(2) + (qy - sy - vy * h).powi(2)).sqrt();
                    let a = ta * h * cov(dd - ind.r * 0.7);
                    if a > 0.0 {
                        col = mix(col, white, a);
                    }
                }
                let dist = ((qx - ind.x).powi(2) + (qy - ind.y).powi(2)).sqrt();
                if ind.alpha > 0.0 {
                    let dd = dist - ind.r;
                    let fill = cov(dd);
                    if fill > 0.0 {
                        // Grey body with a white rim reads well on both light and dark apps.
                        let rim = smoothstep(-ind.r * 0.16 - inv_s, -ind.r * 0.16 + inv_s, dd);
                        let body_col = [118.0, 118.0, 126.0];
                        let a = ind.alpha * fill * (1.0 + 0.6 * rim).min(1.6);
                        col = mix(col, mix(body_col, white, rim), a.min(1.0));
                    }
                }
                if ind.ring_alpha > 0.0 {
                    let dd = (dist - ind.ring_r).abs() - ind.r * 0.06;
                    let a = cov(dd) * ind.ring_alpha;
                    if a > 0.0 {
                        col = mix(col, white, a);
                    }
                }
            }

            let n = hash(px, py);
            let i = px * 4;
            row[i] = (col[0] + 0.5 + n).clamp(0.0, 255.0) as u8;
            row[i + 1] = (col[1] + 0.5 + n).clamp(0.0, 255.0) as u8;
            row[i + 2] = (col[2] + 0.5 + n).clamp(0.0, 255.0) as u8;
            row[i + 3] = 255;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Style;

    #[test]
    fn draws_background_device_and_screen() {
        let (w, h) = (320usize, 180usize);
        let screen = Frame { w: 108, h: 240, data: [255u8, 0, 0, 255].repeat(108 * 240) };
        let mips = Mips::build(screen);
        let style = Style::default();
        let mut out = vec![0u8; w * h * 4];
        let inp = ComposeInput {
            style: &style,
            screen: Some(&mips),
            video_w: 1080.0,
            video_h: 2400.0,
            cam: CamState::IDENTITY,
            touches: &[],
            time: 0.0,
        };
        compose(&mut out, w, h, &inp);
        let px = |x: usize, y: usize| &out[(y * w + x) * 4..(y * w + x) * 4 + 4];
        let center = px(w / 2, h / 2);
        assert!(center[0] > 200 && center[1] < 40, "screen shows the recording: {center:?}");
        let corner = px(2, 2);
        assert!(corner[0] != 255 || corner[1] != 0, "background, not the screen: {corner:?}");
        assert!(out.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn zoom_keeps_device_covering_frame() {
        let style = Style::default();
        let layout = Layout::new(1920.0, 1080.0, &style, 1080.0, 2400.0);
        let cam = layout.camera(CamState { scale: 2.5, fx: 0.5, fy: 0.05 });
        // Top edge of the zoomed device must not come down into the frame.
        let (_, top) = layout.to_scene(cam, 960.0, 0.0);
        assert!(top >= layout.body.y0 - 0.5, "{top} vs {}", layout.body.y0);
    }
}
