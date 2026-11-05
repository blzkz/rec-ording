//! Finishing over the whole picture, after the titles: vignette, fade from and to black, and
//! film grain. finish.wgsl does exactly the same on the graphics card.

use crate::model::Style;

/// The finishing of one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Post {
    pub vignette: f32,
    pub grain: f32,
    /// How much of the picture shows (fades to black): 1 = all.
    pub fade: f32,
    /// Frame number, so the grain changes every frame.
    pub frame: u32,
}

impl Post {
    pub const NONE: Post = Post { vignette: 0.0, grain: 0.0, fade: 1.0, frame: 0 };

    pub fn is_none(&self) -> bool {
        self.vignette <= 0.0 && self.grain <= 0.0 && self.fade >= 1.0
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The finishing at timeline second `t` of a video lasting `total`, at `fps`.
pub fn at(style: &Style, t: f32, total: f32, fps: f32) -> Post {
    let f = &style.finish;
    let fade_in = if f.fade_in > 0.0 { smooth(t / f.fade_in) } else { 1.0 };
    let fade_out = if f.fade_out > 0.0 { smooth((total - t) / f.fade_out) } else { 1.0 };
    Post { vignette: f.vignette.clamp(0.0, 1.0), grain: f.grain.clamp(0.0, 1.0), fade: fade_in.min(fade_out), frame: (t * fps).round().max(0.0) as u32 }
}

/// Grain noise for a pixel and frame, -0.5..0.5 (the same hash on the graphics card).
#[inline(always)]
pub fn noise(x: u32, y: u32, frame: u32) -> f32 {
    let mut h = (x.wrapping_add(frame.wrapping_mul(7919))).wrapping_mul(374_761_393) ^ (y.wrapping_add(frame.wrapping_mul(104_729))).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0 - 0.5
}

/// Darkening of the vignette at a pixel (1 = none).
#[inline(always)]
pub fn vignette(x: f32, y: f32, w: f32, h: f32, amount: f32) -> f32 {
    let (dx, dy) = ((x - w * 0.5) / (w * 0.5), (y - h * 0.5) / (h * 0.5));
    let r = (dx * dx + dy * dy).sqrt() / std::f32::consts::SQRT_2;
    1.0 - amount * 0.75 * smooth((r - 0.35) / 0.7)
}

/// A finished colour (0..255) at pixel (x, y).
#[inline(always)]
pub fn color(c: [f32; 3], x: u32, y: u32, w: f32, h: f32, p: &Post) -> [f32; 3] {
    let k = vignette(x as f32 + 0.5, y as f32 + 0.5, w, h, p.vignette) * p.fade;
    let g = noise(x, y, p.frame) * p.grain * 36.0 * p.fade;
    c.map(|v| v * k + g)
}

/// Applies the finishing to an RGBA8 frame.
pub fn apply(out: &mut [u8], w: usize, h: usize, p: &Post) {
    if p.is_none() {
        return;
    }
    use rayon::prelude::*;
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let i = x * 4;
            let c = color([row[i] as f32, row[i + 1] as f32, row[i + 2] as f32], x as u32, y as u32, w as f32, h as f32, p);
            for k in 0..3 {
                row[i + k] = c[k].round().clamp(0.0, 255.0) as u8;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Finish;

    #[test]
    fn fades_darken_the_ends() {
        let style = Style { finish: Finish { fade_in: 1.0, fade_out: 2.0, ..Default::default() }, ..Style::default() };
        assert_eq!(at(&style, 0.0, 10.0, 30.0).fade, 0.0);
        assert_eq!(at(&style, 5.0, 10.0, 30.0).fade, 1.0);
        assert!(at(&style, 9.0, 10.0, 30.0).fade > 0.4 && at(&style, 9.0, 10.0, 30.0).fade < 0.6);
        assert!(at(&Style::default(), 0.0, 10.0, 30.0).is_none());
    }

    #[test]
    fn vignette_keeps_the_middle() {
        assert!((vignette(50.0, 50.0, 100.0, 100.0, 1.0) - 1.0).abs() < 1e-6);
        assert!(vignette(0.0, 0.0, 100.0, 100.0, 1.0) < 0.4);
    }
}
