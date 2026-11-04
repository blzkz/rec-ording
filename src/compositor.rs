//! Renders one output frame: background, device, the screen recording, camera zoom and touches.
//! The same code drives the live preview and the export, so what you see is what you get.

use crate::timeline::{CameraValue, PoseValue};
use crate::model::{FrameStyle, Style, Touch, TouchKind, VideoFit};
use crate::model3d::{Lighting, ModelMeta, PreparedModel};
use rayon::prelude::*;

/// 2D camera framing: zoom factor and the point of the recording it centres on (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamState {
    pub scale: f32,
    pub fx: f32,
    pub fy: f32,
}

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
    /// Where the recording sits: the screen itself, or larger/smaller when an imported model's
    /// screen has other proportions (see `VideoFit`).
    pub video: Rect2,
    pub bezel: f32,
    /// Thickness of the device, shown as a lit edge when it is turned.
    pub depth: f32,
    pub frame: FrameStyle,
    pub framed: bool,
    pub portrait: bool,
}

impl Layout {
    /// `model` gives the proportions of an imported model; without it a `Custom` frame falls
    /// back to the rounded phone.
    pub fn new(cw: f32, ch: f32, style: &Style, video_w: f32, video_h: f32, model: Option<&ModelMeta>) -> Self {
        let min_dim = cw.min(ch);
        let pad = style.padding * min_dim;
        let avail_w = (cw - 2.0 * pad).max(1.0);
        let avail_h = (ch - 2.0 * pad).max(1.0);

        if let (FrameStyle::Custom, Some(m)) = (style.frame, model) {
            // Fit the whole model; `unit` is the screen width in pixels.
            let (bw, bh) = ((m.x1 - m.x0).max(1e-3), (m.y1 - m.y0).max(1e-3));
            let unit = (avail_w / bw).min(avail_h / bh);
            let body = Rect2::from_center(cw * 0.5, ch * 0.5, bw * unit, bh * unit);
            let (scx, scy) = (body.cx() - (m.x0 + m.x1) * 0.5 * unit, body.cy() - (m.y0 + m.y1) * 0.5 * unit);
            let screen = Rect2::from_center(scx, scy, unit, unit / m.screen_aspect.max(0.05));
            let (vw, vh) = (video_w.max(1.0), video_h.max(1.0));
            let video = match style.video_fit {
                VideoFit::Stretch => screen,
                fit => {
                    let k = if fit == VideoFit::Fill {
                        (screen.w() / vw).max(screen.h() / vh)
                    } else {
                        (screen.w() / vw).min(screen.h() / vh)
                    };
                    let (w, h) = (vw * k, vh * k);
                    let pan = if fit == VideoFit::Fill { style.fit_pan.clamp(0.0, 1.0) } else { 0.5 };
                    let (x0, y0) = (screen.x0 - (w - screen.w()) * pan, screen.y0 - (h - screen.h()) * pan);
                    Rect2 { x0, y0, x1: x0 + w, y1: y0 + h }
                }
            };
            return Self {
                video,
                cw,
                ch,
                body,
                body_r: 0.12 * body.w().min(body.h()),
                screen,
                screen_r: 0.0,
                bezel: 0.0,
                depth: (m.z1 - m.z0) * unit,
                frame: FrameStyle::Custom,
                framed: false,
                portrait: m.screen_aspect < 1.0,
            };
        }

        let aspect = (video_w / video_h.max(1.0)).max(0.05);
        let portrait = aspect < 1.0;
        let framed = style.frame != FrameStyle::None;
        // Bezel, screen corner radius and thickness, in units of the screen's short side.
        let (k, radius_k, depth_k) = match style.frame {
            FrameStyle::Rounded | FrameStyle::Custom => (0.045, 0.105, 0.1),
            FrameStyle::Flat => (0.035, 0.035, 0.1),
            FrameStyle::Curved => (0.025, 0.07, 0.1),
            FrameStyle::Tablet => (0.07, 0.035, 0.045),
            FrameStyle::None => (0.0, 0.06, 0.0),
        };

        // Sizes in units of the screen's short side.
        let (sw, sh) = if portrait { (1.0, 1.0 / aspect) } else { (aspect, 1.0) };
        let unit = (avail_w / (sw + 2.0 * k)).min(avail_h / (sh + 2.0 * k));

        let (cx, cy) = (cw * 0.5, ch * 0.5);
        let screen = Rect2::from_center(cx, cy, sw * unit, sh * unit);
        let bezel = k * unit;
        let body = Rect2::from_center(cx, cy, screen.w() + 2.0 * bezel, screen.h() + 2.0 * bezel);
        let screen_r = unit * radius_k;
        let body_r = if framed { screen_r + bezel } else { screen_r };

        let frame = if style.frame == FrameStyle::Custom { FrameStyle::Rounded } else { style.frame };
        Self { cw, ch, body, body_r, screen, screen_r, video: screen, bezel, depth: depth_k * unit, frame, framed, portrait }
    }

    /// Scale and translation the camera applies to the scene: `out = c + (q - c) * s + t`.
    pub fn camera(&self, cam: CamState) -> (f32, f32, f32) {
        let s = cam.scale.clamp(0.5, 5.0);
        let (cx, cy) = (self.cw * 0.5, self.ch * 0.5);
        let px = self.video.x0 + cam.fx.clamp(0.0, 1.0) * self.video.w();
        let py = self.video.y0 + cam.fy.clamp(0.0, 1.0) * self.video.h();

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

#[derive(Clone, Copy)]
pub struct ComposeInput<'a> {
    pub style: &'a Style,
    pub screen: Option<&'a Mips>,
    pub video_w: f32,
    pub video_h: f32,
    /// Camera at this instant (zoom, focus and angles added to the device's).
    pub cam: CameraValue,
    /// Device motion at this instant (offset, scale and angles).
    pub pose: PoseValue,
    pub touches: &'a [Touch],
    pub time: f32,
    /// Seconds since the start of the (trimmed) video, for the intro and idle motion.
    pub since_start: f32,
    /// Imported model, drawn instead of the built-in phone when the style asks for it.
    pub model: Option<&'a PreparedModel>,
    /// Supersampling of the model's edges (1 = off, 2 = 4 samples per pixel).
    pub supersample: usize,
    /// Titles on screen, drawn over everything.
    pub texts: &'a [crate::text::Shown<'a>],
    /// Instants averaged for motion blur (see `timeline::shutter`); empty for none.
    pub blur: &'a [(f32, CameraValue, PoseValue)],
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

    /// Outward direction of the outline nearest to (x, y).
    #[inline(always)]
    fn normal(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let qx = dx.abs() - self.hx + self.r;
        let qy = dy.abs() - self.hy + self.r;
        if qx > 0.0 && qy > 0.0 {
            let l = (qx * qx + qy * qy).sqrt().max(1e-6);
            (qx / l * dx.signum(), qy / l * dy.signum())
        } else if qx > qy {
            (dx.signum(), 0.0)
        } else {
            (0.0, dy.signum())
        }
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
    let sc = &layout.video;
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

type Mat3 = [[f32; 3]; 3];

/// world = Rz(roll) · Rx(tilt) · Ry(turn) · local
fn rotation(tilt: f32, turn: f32, roll: f32) -> Mat3 {
    let (sx, cx) = tilt.to_radians().sin_cos();
    let (sy, cy) = turn.to_radians().sin_cos();
    let (sz, cz) = roll.to_radians().sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    matmul(&rz, &matmul(&rx, &ry))
}

fn matmul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

#[inline(always)]
fn apply(m: &Mat3, v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

#[inline(always)]
fn apply_t(m: &Mat3, v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

#[inline(always)]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where the device is at an instant: angles in degrees, offset in pixels, scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevicePose {
    pub tilt: f32,
    pub turn: f32,
    pub roll: f32,
    pub dx: f32,
    pub dy: f32,
    pub scale: f32,
}

/// The device at this instant: the style's angle (eased flat while zoomed in), plus the camera
/// and device keyframes, the optional intro swing and idle sway.
pub fn device_pose(style: &Style, cam: &CameraValue, pose: &PoseValue, since_start: f32, min_dim: f32) -> DevicePose {
    let (mut tilt, mut turn, mut roll) = (style.tilt, style.turn, style.roll);
    if style.straighten_on_zoom {
        let k = 1.0 - smoothstep(1.0, 1.4, cam.zoom);
        tilt *= k;
        turn *= k;
        roll *= k;
    }
    tilt += cam.tilt + pose.tilt;
    turn += cam.turn + pose.turn;
    roll += cam.roll + pose.roll;
    let t = since_start.max(0.0);
    let mut dy = pose.dy * min_dim;
    if style.float_motion {
        tilt += 1.8 * (t * 0.8).sin();
        turn += 2.6 * (t * 0.55 + 1.0).sin();
        dy += 0.008 * min_dim * (t * 1.1).sin();
    }
    if style.intro {
        let rest = 1.0 - ease_out(t / 1.4);
        // Swing in from further along the chosen turn, so the two never cancel out.
        let side = if style.turn > 0.0 { 1.0 } else { -1.0 };
        turn += 40.0 * side * rest;
        tilt += 18.0 * rest;
        dy += 0.6 * min_dim * rest * rest;
    }
    DevicePose { tilt, turn, roll, dx: pose.dx * min_dim, dy, scale: pose.scale.max(0.05) }
}

/// Renders the frame into `out` (RGBA8, `ow * oh * 4` bytes): the scene (averaged over the
/// shutter when it moves fast), then the titles.
pub fn compose(out: &mut [u8], ow: usize, oh: usize, inp: &ComposeInput) {
    if out.len() != ow * oh * 4 || ow == 0 || oh == 0 {
        return;
    }
    if inp.blur.len() < 2 {
        compose_scene(out, ow, oh, inp);
    } else {
        let mut acc = vec![0u32; out.len()];
        for &(time, cam, pose) in inp.blur {
            compose_scene(out, ow, oh, &ComposeInput { cam, pose, time, since_start: time, ..*inp });
            for (a, &v) in acc.iter_mut().zip(out.iter()) {
                *a += v as u32;
            }
        }
        let n = inp.blur.len() as u32;
        for (o, a) in out.iter_mut().zip(acc) {
            *o = ((a + n / 2) / n) as u8;
        }
    }
    crate::text::draw(out, ow, oh, inp.style, inp.texts);
}

fn compose_scene(out: &mut [u8], ow: usize, oh: usize, inp: &ComposeInput) {
    let style = inp.style;
    let (cw, ch) = (ow as f32, oh as f32);
    let model = inp.model.filter(|_| style.frame == FrameStyle::Custom);
    let layout = Layout::new(cw, ch, style, inp.video_w, inp.video_h, model.map(|m| &m.meta));
    let cam = layout.camera(CamState { scale: inp.cam.zoom, fx: inp.cam.x, fy: inp.cam.y });
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

    // 3D pose: the device is a slab whose front face sits at z = 0, seen in perspective from
    // the front. With no angle this reduces exactly to the flat 2D layout.
    let DevicePose { tilt, turn, roll, dx, dy, scale: dev_scale } = device_pose(style, &inp.cam, &inp.pose, inp.since_start, min_dim);
    let angled = tilt.abs() > 0.01 || turn.abs() > 0.01 || roll.abs() > 0.01 || model.is_some();
    let rot = rotation(tilt, turn, roll);
    let focal = 1.15 * cw.max(ch);
    let (lcx, lcy) = (layout.body.cx(), layout.body.cy());
    // Push the device back so its nearest corner stays at the flat layout's size: turning it
    // never makes it grow past the margins.
    let (hx, hy) = (layout.body.w() * 0.5, layout.body.h() * 0.5);
    let unit = layout.screen.w();
    // Model points (screen-width units, origin at the screen centre) relative to the device centre.
    let (mox, moy) = (layout.screen.cx() - lcx, layout.screen.cy() - lcy);
    let nearest = match model {
        Some(m) => {
            let mm = &m.meta;
            let mut z = f32::MIN;
            for x in [mm.x0, mm.x1] {
                for y in [mm.y0, mm.y1] {
                    for zz in [mm.z0, mm.z1] {
                        z = z.max(apply(&rot, [mox + x * unit, moy + y * unit, zz * unit])[2]);
                    }
                }
            }
            z.max(0.0)
        }
        None => [(-hx, -hy), (hx, -hy), (-hx, hy), (hx, hy)]
            .iter()
            .map(|&(x, y)| apply(&rot, [x, y, 0.0])[2])
            .fold(0.0f32, f32::max),
    };
    let eye = apply_t(&rot, [0.0, 0.0, focal + nearest]);
    let face_n = apply(&rot, [0.0, 0.0, 1.0]);
    let light = {
        let l = [-0.45f32, -0.65, 0.6];
        let n = dot(l, l).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    let half_v = {
        let h = [light[0], light[1], light[2] + 1.0];
        let n = dot(h, h).sqrt();
        [h[0] / n, h[1] / n, h[2] / n]
    };
    let face_light = if angled { 0.86 + 0.24 * dot(face_n, light) } else { 1.0 };
    let sheen_amount = ((tilt.abs() + turn.abs()) / 30.0).min(1.0) * 0.07;
    let depth = layout.depth;

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
    let tablet = layout.frame == FrameStyle::Tablet;

    let buttons: Vec<RRect> = if layout.framed {
        let b = &layout.body;
        let bd = (layout.bezel * if tablet { 0.1 } else { 0.16 }).max(1.0);
        if tablet {
            let y = b.y0 - bd * 0.4;
            vec![
                RRect { cx: b.x0 + b.w() * 0.8, cy: y, hx: b.w() * 0.045, hy: bd, r: bd },
                RRect { cx: b.x0 + b.w() * 0.12, cy: y, hx: b.w() * 0.03, hy: bd, r: bd },
            ]
        } else if layout.portrait {
            let x = b.x1 + bd * 0.4;
            vec![
                RRect { cx: x, cy: b.y0 + b.h() * 0.255, hx: bd, hy: b.h() * 0.035, r: bd },
                RRect { cx: x, cy: b.y0 + b.h() * 0.40, hx: bd, hy: b.h() * 0.07, r: bd },
            ]
        } else {
            let y = b.y0 - bd * 0.4;
            vec![
                RRect { cx: b.x0 + b.w() * 0.255, cy: y, hx: b.w() * 0.035, hy: bd, r: bd },
                RRect { cx: b.x0 + b.w() * 0.40, cy: y, hx: b.w() * 0.07, hy: bd, r: bd },
            ]
        }
    } else {
        Vec::new()
    };

    // Front camera: a punch hole in the screen, or a dot in the bezel on tablets.
    let sc_short = layout.screen.w().min(layout.screen.h());
    let hole = if !layout.framed {
        None
    } else if tablet {
        Some((layout.body.cx(), layout.body.y0 + layout.bezel * 0.5, layout.bezel * 0.14))
    } else {
        let sc = &layout.screen;
        let off = sc_short * 0.05;
        Some(if layout.portrait { (sc.cx(), sc.y0 + off, sc_short * 0.024) } else { (sc.x0 + off, sc.cy(), sc_short * 0.024) })
    };
    let curved = layout.frame == FrameStyle::Curved;

    let mip = inp.screen.map(|m| {
        let ratio = m.width() as f32 / (layout.video.w() * s).max(1.0);
        m.level_for(ratio)
    });

    let inds = indicators(&layout, style, inp.touches, inp.time);
    let white = [255.0, 255.0, 255.0];
    let body_top = layout.body.y0;
    let body_h = layout.body.h().max(1.0);

    // Back of the built-in device: a camera bar across the top with two lenses.
    let mesh_free = model.is_none();
    let (camera_bar, lenses) = if layout.framed {
        let b = &layout.body;
        if layout.portrait {
            let (cy, h) = (b.y0 + b.h() * 0.13, b.h() * 0.065);
            let bar = RRect { cx: b.cx(), cy, hx: b.w() * 0.42, hy: h * 0.5, r: h * 0.5 };
            (Some(bar), vec![(b.cx() - b.w() * 0.25, cy, h * 0.32), (b.cx() - b.w() * 0.08, cy, h * 0.32)])
        } else {
            let (cx, w) = (b.x0 + b.w() * 0.13, b.w() * 0.065);
            let bar = RRect { cx, cy: b.cy(), hx: w * 0.5, hy: b.h() * 0.42, r: w * 0.5 };
            (Some(bar), vec![(cx, b.cy() - b.h() * 0.25, w * 0.32), (cx, b.cy() - b.h() * 0.08, w * 0.32)])
        }
    } else {
        (None, Vec::new())
    };

    const BAND: usize = 16;
    let lighting = Lighting { light, half: half_v };
    // Animated models: pose for this instant.
    let pose = model.and_then(|m| {
        let cm = style.model.as_ref()?;
        let anim = cm.animation?;
        let d = m.animation_duration(anim)?.max(1e-3);
        let t = (inp.time - cm.animation_start).max(0.0);
        m.pose(anim, if cm.loop_animation { t.rem_euclid(d) } else { t.min(d) })
    });
    let projected = model.map(|m| {
        m.project(
            pose.as_ref(),
            |p| {
                let w = apply(&rot, [mox + p[0] * unit, moy + p[1] * unit, p[2] * unit]);
                let f = focal / (focal + nearest - w[2]);
                let (x, y) = (lcx + dx + w[0] * f * dev_scale, lcy + dy + w[1] * f * dev_scale);
                (ccx + (x - ccx) * s + tx, ccy + (y - ccy) * s + ty, f)
            },
            |n| apply(&rot, n),
            ow,
            oh,
            inp.supersample.clamp(1, 3),
            BAND,
        )
    });
    // Point on the model's screen -> the recording (black outside it, for `VideoFit::Fit`).
    let (scr, vid) = (layout.screen, layout.video);
    let screen_color = |u: f32, v: f32| {
        let vu = (scr.x0 + u * scr.w() - vid.x0) / vid.w();
        let vv = (scr.y0 + v * scr.h() - vid.y0) / vid.h();
        if !(-0.002..=1.002).contains(&vu) || !(-0.002..=1.002).contains(&vv) {
            return [0.0, 0.0, 0.0];
        }
        match mip {
            Some(f) => sample(f, vu.clamp(0.0, 1.0), vv.clamp(0.0, 1.0)),
            None => [18.0, 18.0, 20.0],
        }
    };

    out.par_chunks_mut(ow * 4 * BAND).enumerate().for_each(|(band_idx, chunk)| {
        let rows = chunk.len() / (ow * 4);
        let mesh = projected.as_ref().map(|p| p.raster(band_idx, rows, &screen_color, &lighting));
        for (ri, row) in chunk.chunks_mut(ow * 4).enumerate() {
            let py = band_idx * BAND + ri;
            let yf = py as f32 + 0.5;
            for px in 0..ow {
                let xf = px as f32 + 0.5;

                // 1. Background: gradient with a soft light from the top-left, dithered to avoid banding.
                let g = (((xf - ccx) * gdx + (yf - ccy) * gdy) / half_extent * 0.5 + 0.5).clamp(0.0, 1.0);
                let mut col = mix(bg_a, bg_b, smoothstep(0.0, 1.0, g));
                let glow = (1.0 - ((xf * xf + yf * yf).sqrt() / diag)).max(0.0) * 0.07;
                col = mix(col, white, glow);

                // 2. Point of the device's front face under this pixel (camera zoom, then 3D pose).
                // Undo the device's own offset and scale around its centre.
                let sx = lcx + ((xf - ccx - tx) * inv_s + ccx - lcx - dx) / dev_scale;
                let sy = lcy + ((yf - ccy - ty) * inv_s + ccy - lcy - dy) / dev_scale;
                let inv_s = inv_s / dev_scale;
                // Seen from behind (a full turn), the built-in device shows its back face.
                let (qx, qy, inv_s, ray, back) = if angled {
                    let dir = [sx - lcx, sy - lcy, -focal];
                    let d = apply_t(&rot, dir);
                    let back = d[2] > 0.0;
                    let plane = if back && mesh_free { -depth } else { 0.0 };
                    let t = (plane - eye[2]) / d[2];
                    if d[2].abs() < 1e-4 || t <= 0.0 {
                        write_px(row, px, col, py);
                        continue;
                    }
                    let dlen = dot(dir, dir).sqrt();
                    let facing = (dot(face_n, dir) / dlen).abs().max(0.25);
                    let size = inv_s * t * dlen / focal / facing;
                    (lcx + eye[0] + t * d[0], lcy + eye[1] + t * d[1], size, Some(d), back)
                } else {
                    (sx, sy, inv_s, None, false)
                };
                let cov = |d: f32| (0.5 - d / inv_s).clamp(0.0, 1.0);

                // 3. Shadows
                if shadow_alpha > 0.0 {
                    let d = shadow_body.sd(qx, qy);
                    let a = shadow_alpha * (1.0 - smoothstep(-shadow_blur, shadow_blur, d));
                    let d2 = contact.sd(qx, qy);
                    let a2 = shadow_alpha * 0.5 * (1.0 - smoothstep(-shadow_blur * 0.12, shadow_blur * 0.12, d2));
                    let k = 1.0 - (a + a2 - a * a2);
                    col = [col[0] * k, col[1] * k, col[2] * k];
                }

                // 4. Imported model, or the built-in device (edge, front, screen)
                if let Some(m) = &mesh {
                    let (c, a) = m.pixel(px, ri);
                    if a > 0.0 {
                        col = [col[0] * (1.0 - a) + c[0], col[1] * (1.0 - a) + c[1], col[2] * (1.0 - a) + c[2]];
                    }
                }
                let d_body = body.sd(qx, qy);
                if let (Some(d), None) = (ray, &mesh) {
                    if depth > 0.0 && d_body > -inv_s && d_body < depth * 2.0 + 2.0 * inv_s {
                        // Walk the ray from the face it enters to the opposite one and find where
                        // it is deepest inside the outline: that is the side wall it hits.
                        let tb = ((if back { 0.0 } else { -depth }) - eye[2]) / d[2];
                        let (bx, by) = (lcx + eye[0] + tb * d[0], lcy + eye[1] + tb * d[1]);
                        let (mut best, mut bp) = (f32::MAX, (qx, qy));
                        for k in 1..=8 {
                            let f = k as f32 / 8.0;
                            let (x, y) = (qx + (bx - qx) * f, qy + (by - qy) * f);
                            let sd = body.sd(x, y);
                            if sd < best {
                                best = sd;
                                bp = (x, y);
                            }
                        }
                        let c = cov(best);
                        if c > 0.0 {
                            let (nx, ny) = body.normal(bp.0, bp.1);
                            let n = apply(&rot, [nx, ny, 0.0]);
                            let diffuse = dot(n, light).max(0.0);
                            let spec = dot(n, half_v).max(0.0).powi(30) * 140.0;
                            let k = 0.3 + 0.75 * diffuse;
                            let side = [metal[0] * k + spec, metal[1] * k + spec, metal[2] * k + spec];
                            col = mix(col, side, c);
                        }
                    }
                }

                if back {
                    if mesh.is_none() && d_body < inv_s {
                        let c = cov(d_body);
                        let n = [-face_n[0], -face_n[1], -face_n[2]];
                        let k = 0.45 + 0.55 * dot(n, light).max(0.0);
                        let mut back_col = [metal[0] * k, metal[1] * k, metal[2] * k];
                        if let Some(bar) = &camera_bar {
                            let cb = cov(bar.sd(qx, qy));
                            if cb > 0.0 {
                                back_col = mix(back_col, [22.0, 22.0, 26.0], cb);
                                for (lx, ly, lr) in &lenses {
                                    let dl = ((qx - lx).powi(2) + (qy - ly).powi(2)).sqrt() - lr;
                                    back_col = mix(back_col, [6.0, 8.0, 14.0], cov(dl));
                                    back_col = mix(back_col, [70.0, 80.0, 100.0], cov((dl + lr * 0.35).abs() - lr * 0.06) * 0.6);
                                }
                            }
                        }
                        col = mix(col, back_col, c);
                    }
                    write_px(row, px, col, py);
                    continue;
                }

                // 5. Device front
                if layout.framed && mesh.is_none() {
                    let shade = (1.12 - 0.3 * ((qy - body_top) / body_h).clamp(0.0, 1.0)) * face_light;
                    let metal_px = [metal[0] * shade, metal[1] * shade, metal[2] * shade];
                    for b in &buttons {
                        let c = cov(b.sd(qx, qy));
                        if c > 0.0 {
                            col = mix(col, mix(metal_px, [0.0; 3], 0.12), c);
                        }
                    }
                    let c = cov(d_body);
                    if c > 0.0 {
                        let inset = -d_body;
                        // Polished edge highlight, then the metal band, then the black glass bezel.
                        let hl = (1.0 - inset / (rim * 0.4)).clamp(0.0, 1.0) * 0.28;
                        let band = mix(metal_px, white, hl);
                        let inner = smoothstep(rim - inv_s, rim + inv_s, inset);
                        let mut front = mix(band, glass, inner);
                        if tablet {
                            if let Some((hx, hy, hr)) = hole {
                                let dh = ((qx - hx).powi(2) + (qy - hy).powi(2)).sqrt() - hr;
                                front = mix(front, [28.0, 30.0, 40.0], cov(dh));
                            }
                        }
                        col = mix(col, front, c);
                    }
                }

                // 6. Screen
                let d = screen.sd(qx, qy);
                let c = if mesh.is_some() { 0.0 } else { cov(d) };
                if c > 0.0 {
                    let u = (qx - layout.screen.x0) / layout.screen.w();
                    let v = (qy - layout.screen.y0) / layout.screen.h();
                    let mut sc = match mip {
                        Some(f) => sample(f, u, v),
                        None => [18.0, 18.0, 20.0],
                    };
                    if curved {
                        // Glass curving away along the long edges.
                        let e = if layout.portrait { u.min(1.0 - u) * layout.screen.w() } else { v.min(1.0 - v) * layout.screen.h() };
                        let e = e / (sc_short * 0.045);
                        let dark = (1.0 - smoothstep(0.0, 1.0, e)) * 0.45;
                        sc = [sc[0] * (1.0 - dark), sc[1] * (1.0 - dark), sc[2] * (1.0 - dark)];
                        sc = mix(sc, white, (-((e - 0.35) / 0.12).powi(2)).exp() * 0.18);
                    }
                    if !tablet {
                        if let Some((hx, hy, hr)) = hole {
                            let dh = ((qx - hx).powi(2) + (qy - hy).powi(2)).sqrt() - hr;
                            let ch = cov(dh);
                            if ch > 0.0 {
                                sc = mix(sc, [4.0, 4.0, 6.0], ch);
                            }
                        }
                    }
                    if sheen_amount > 0.0 {
                        let g = u * 0.6 + v * 0.4 - (0.55 - turn / 120.0 + tilt / 160.0);
                        sc = mix(sc, white, sheen_amount * (-(g / 0.12).powi(2)).exp());
                    }
                    col = mix(col, sc, c);
                }

                // 7. Touch indicators
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

                write_px(row, px, col, py);
            }
        }
    });
}

#[inline(always)]
fn write_px(row: &mut [u8], px: usize, col: [f32; 3], py: usize) {
    let n = hash(px, py);
    let i = px * 4;
    row[i] = (col[0] + 0.5 + n).clamp(0.0, 255.0) as u8;
    row[i + 1] = (col[1] + 0.5 + n).clamp(0.0, 255.0) as u8;
    row[i + 2] = (col[2] + 0.5 + n).clamp(0.0, 255.0) as u8;
    row[i + 3] = 255;
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
            cam: CameraValue::REST,
            pose: PoseValue::REST,
            touches: &[],
            time: 0.0,
            since_start: 0.0,
            model: None,
            supersample: 1,
            texts: &[],
            blur: &[],
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
        let layout = Layout::new(1920.0, 1080.0, &style, 1080.0, 2400.0, None);
        let cam = layout.camera(CamState { scale: 2.5, fx: 0.5, fy: 0.05 });
        // Top edge of the zoomed device must not come down into the frame.
        let (_, top) = layout.to_scene(cam, 960.0, 0.0);
        assert!(top >= layout.body.y0 - 0.5, "{top} vs {}", layout.body.y0);
    }
}


#[cfg(test)]
mod fit_tests {
    use super::*;
    use crate::model::{Style, VideoFit};

    #[test]
    fn video_fill_fit_and_stretch() {
        // A model screen wider (0.6) than the recording (0.45).
        let meta = ModelMeta { screen_aspect: 0.6, x0: -0.55, x1: 0.55, y0: -0.95, y1: 0.95, z0: -0.1, z1: 0.0 };
        let layout = |fit: VideoFit, pan: f32| {
            let style = Style { frame: FrameStyle::Custom, video_fit: fit, fit_pan: pan, ..Style::default() };
            Layout::new(1920.0, 1080.0, &style, 1080.0, 2400.0, Some(&meta))
        };
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;

        let l = layout(VideoFit::Fill, 0.5);
        assert!(close(l.video.w(), l.screen.w()) && l.video.h() > l.screen.h(), "fill covers, cropping top and bottom");
        assert!(close(l.video.cy(), l.screen.cy()));
        let top = layout(VideoFit::Fill, 0.0);
        assert!(close(top.video.y0, top.screen.y0), "pan 0 keeps the top of the recording");

        let l = layout(VideoFit::Fit, 0.5);
        assert!(close(l.video.h(), l.screen.h()) && l.video.w() < l.screen.w(), "fit shows everything, with side bars");
        assert!(close(l.video.w() / l.video.h(), 0.45));

        let l = layout(VideoFit::Stretch, 0.5);
        assert!(close(l.video.w(), l.screen.w()) && close(l.video.h(), l.screen.h()));
    }
}

#[cfg(test)]
mod pose_tests {
    use super::*;
    use crate::model::Style;

    #[test]
    fn straightens_when_zoomed_and_intro_settles() {
        let style = Style { turn: -28.0, tilt: 6.0, intro: true, ..Style::default() };
        let zoomed = device_pose(&style, &CameraValue { zoom: 1.8, ..CameraValue::REST }, &PoseValue::REST, 5.0, 1080.0);
        assert!(zoomed.tilt.abs() < 1e-3 && zoomed.turn.abs() < 1e-3, "{zoomed:?}");
        let start = device_pose(&style, &CameraValue::REST, &PoseValue::REST, 0.0, 1080.0);
        assert!(start.turn < -60.0 && start.dy > 500.0, "swings in from below, turned further: {start:?}");
        let settled = device_pose(&style, &CameraValue::REST, &PoseValue::REST, 3.0, 1080.0);
        assert!((settled.turn + 28.0).abs() < 1e-3 && settled.dy.abs() < 1e-3, "{settled:?}");
    }

    #[test]
    fn angled_device_stays_inside_margins() {
        let style = Style { tilt: 38.0, ..Style::default() };
        let (w, h) = (320usize, 180usize);
        let mut out = vec![0u8; w * h * 4];
        let inp = ComposeInput { style: &style, screen: None, video_w: 1080.0, video_h: 2400.0, cam: CameraValue::REST, pose: PoseValue::REST, touches: &[], time: 0.0, since_start: 5.0, model: None, supersample: 1, texts: &[], blur: &[] };
        compose(&mut out, w, h, &inp);
        // Top and bottom rows are background (no dark device pixels).
        for y in [0, h - 1] {
            assert!(out[y * w * 4..(y + 1) * w * 4].chunks(4).all(|p| p[0] > 60), "row {y} touches the device");
        }
    }
}
