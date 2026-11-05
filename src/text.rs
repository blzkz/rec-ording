//! Titles over the video, drawn into the frame like everything else, so the preview and the
//! export match: Apple-style headlines in Inter Display that fade in, build up word by word or
//! line by line, and push the device aside while they are on screen.

use crate::model::{Style, TextClip, TextColor, TextPlace, TextReveal, TextSize};
use ab_glyph::{point, Font, FontRef, GlyphId, PxScale, ScaleFont};
use std::sync::OnceLock;

struct Fonts {
    semibold: FontRef<'static>,
    bold: FontRef<'static>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| Fonts {
        semibold: FontRef::try_from_slice(include_bytes!("../assets/fonts/InterDisplay-SemiBold.ttf")).expect("bundled font"),
        bold: FontRef::try_from_slice(include_bytes!("../assets/fonts/InterDisplay-Bold.ttf")).expect("bundled font"),
    })
}

/// A small white label on a dark box with its top left corner at (x, y), for contact sheets.
pub fn label(out: &mut [u8], w: usize, h: usize, x: usize, y: usize, text: &str, size: f32) {
    let font = &fonts().semibold;
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let pad = size * 0.45;
    let mut caret = pad;
    let mut glyphs = Vec::new();
    for ch in text.chars() {
        let id = sf.glyph_id(ch);
        glyphs.push(id.with_scale_and_position(scale, point(caret, pad + sf.ascent())));
        caret += sf.h_advance(id);
    }
    let (bw, bh) = ((caret + pad).ceil() as usize, (sf.ascent() - sf.descent() + pad * 2.0).ceil() as usize);
    for yy in y..(y + bh).min(h) {
        for xx in x..(x + bw).min(w) {
            let i = (yy * w + xx) * 4;
            for k in 0..3 {
                out[i + k] = (out[i + k] as f32 * 0.3) as u8;
            }
        }
    }
    for g in glyphs {
        let Some(o) = font.outline_glyph(g) else { continue };
        let b = o.px_bounds();
        o.draw(|gx, gy, c| {
            let (px, py) = (x as i64 + b.min.x as i64 + gx as i64, y as i64 + b.min.y as i64 + gy as i64);
            if px < 0 || py < 0 || px as usize >= w || py as usize >= h {
                return;
            }
            let i = (py as usize * w + px as usize) * 4;
            for k in 0..3 {
                out[i + k] = (out[i + k] as f32 * (1.0 - c) + 255.0 * c).round() as u8;
            }
        });
    }
}

/// A title on screen, `local` seconds into its clip.
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    pub clip: &'a TextClip,
    pub local: f32,
}

/// Time a word or line takes to come in, and the whole title to fade out.
const IN: f32 = 0.55;
const OUT: f32 = 0.45;

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn is_blank(c: &TextClip) -> bool {
    c.title.trim().is_empty() && c.subtitle.trim().is_empty()
}


/// Where the device goes, `v` of the way (0..1), to make room for a title at `place`: offset in
/// fractions of the frame's short side and a scale factor, for a frame `ratio` = width / height.
/// Degrees the device turns towards a title with `face_text`.
pub const FACE_DEGREES: f32 = 14.0;

/// Turn and tilt (degrees) towards a title at `place`, `v` of the way.
pub fn face(place: TextPlace, v: f32) -> (f32, f32) {
    let (turn, tilt) = match place {
        TextPlace::Left => (-FACE_DEGREES, 0.0),
        TextPlace::Right => (FACE_DEGREES, 0.0),
        TextPlace::Top => (0.0, FACE_DEGREES * 0.7),
        TextPlace::Bottom => (0.0, -FACE_DEGREES * 0.7),
        TextPlace::Center => (0.0, 0.0),
    };
    (turn * v, tilt * v)
}

pub fn room(place: TextPlace, v: f32, ratio: f32) -> (f32, f32, f32) {
    let wide = ratio.max(1.0);
    let tall = (1.0 / ratio).max(1.0);
    // On a narrow frame there is no room beside the device: it also gets smaller.
    let side_scale = if ratio < 1.2 { 0.8 } else { 1.0 };
    let (dx, dy, s) = match place {
        TextPlace::Left => (0.2 * wide, 0.0, side_scale),
        TextPlace::Right => (-0.2 * wide, 0.0, side_scale),
        TextPlace::Top => (0.0, 0.1 * tall, 0.84),
        TextPlace::Bottom => (0.0, -0.1 * tall, 0.84),
        TextPlace::Center => (0.0, 0.0, 1.0),
    };
    (dx * v, dy * v, 1.0 + (s - 1.0) * v)
}

/// Font sizes of the headline and the smaller line, as fractions of the frame's short side.
fn sizes(size: TextSize) -> (f32, f32) {
    match size {
        TextSize::Small => (0.05, 0.034),
        TextSize::Medium => (0.068, 0.04),
        TextSize::Large => (0.15, 0.048),
    }
}

fn color(style: &Style, c: TextColor) -> [f32; 3] {
    const LIGHT: [f32; 3] = [255.0, 255.0, 255.0];
    const DARK: [f32; 3] = [29.0, 29.0, 31.0];
    match c {
        TextColor::Light => LIGHT,
        TextColor::Dark => DARK,
        TextColor::Auto => {
            let lum = |c: [u8; 3]| (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0;
            if (lum(style.background.a) + lum(style.background.b)) * 0.5 > 0.6 {
                DARK
            } else {
                LIGHT
            }
        }
    }
}

/// One run of text at a fixed size: a font, its size and the spacing between letters.
#[derive(Clone, Copy)]
struct Face {
    font: &'static FontRef<'static>,
    size: f32,
    tracking: f32,
}

impl Face {
    fn new(font: &'static FontRef<'static>, size: f32) -> Self {
        // Big type is set tighter, as in Apple's headlines.
        let tracking = if size > 60.0 { -0.025 } else { -0.012 } * size;
        Face { font, size, tracking }
    }

    fn width(&self, s: &str) -> f32 {
        let sf = self.font.as_scaled(PxScale::from(self.size));
        let mut w = 0.0;
        let mut prev: Option<GlyphId> = None;
        for ch in s.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                w += sf.kern(p, id);
            }
            w += sf.h_advance(id) + self.tracking;
            prev = Some(id);
        }
        (w - self.tracking).max(0.0)
    }

    fn ascent(&self) -> f32 {
        self.font.as_scaled(PxScale::from(self.size)).ascent()
    }
}

/// A word placed in the frame, with what drives its animation.
struct Word {
    text: String,
    face: Face,
    x: f32,
    baseline: f32,
    /// Order among all words (headline first), for word-by-word.
    index: usize,
    /// Headline line it comes from (before wrapping), for line-by-line; `None` for the smaller line.
    group: Option<usize>,
}

struct Line {
    words: Vec<(String, f32)>,
    width: f32,
    group: usize,
}

/// Splits `text` into lines no wider than `max_w`, keeping its own line breaks.
fn wrap(face: &Face, text: &str, max_w: f32) -> Vec<Line> {
    let space = face.width("a a") - face.width("aa");
    let mut lines = Vec::new();
    for (group, src) in text.lines().map(str::trim).filter(|l| !l.is_empty()).enumerate() {
        let mut line = Line { words: Vec::new(), width: 0.0, group };
        for word in src.split_whitespace() {
            let w = face.width(word);
            let extra = if line.words.is_empty() { w } else { space + w };
            if !line.words.is_empty() && line.width + extra > max_w {
                lines.push(std::mem::replace(&mut line, Line { words: Vec::new(), width: 0.0, group }));
                line.width = w;
            } else {
                line.width += extra;
            }
            line.words.push((word.to_string(), w));
        }
        if !line.words.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// Places every word of `c` in a `w`×`h` frame.
fn layout(c: &TextClip, w: f32, h: f32) -> Vec<Word> {
    let f = fonts();
    let short = w.min(h);
    let (title_k, sub_k) = sizes(c.size);
    let title_face = Face::new(if c.size == TextSize::Large { &f.bold } else { &f.semibold }, title_k * short);
    let sub_face = Face::new(&f.semibold, sub_k * short);
    let beside = matches!(c.place, TextPlace::Left | TextPlace::Right);
    let max_w = if beside { 0.37 * w } else { 0.84 * w };

    let title = wrap(&title_face, &c.title, max_w);
    let sub = wrap(&sub_face, &c.subtitle, max_w);
    let title_lh = title_face.size * if c.size == TextSize::Large { 1.0 } else { 1.12 };
    let sub_lh = sub_face.size * 1.25;
    let gap = if title.is_empty() || sub.is_empty() {
        0.0
    } else if c.size == TextSize::Large {
        0.12 * title_face.size
    } else {
        0.3 * sub_face.size
    };
    let block_h = title.len() as f32 * title_lh + gap + sub.len() as f32 * sub_lh;
    let block_w = title.iter().chain(&sub).map(|l| l.width).fold(0.0, f32::max);

    let top = match c.place {
        TextPlace::Left | TextPlace::Right | TextPlace::Center => (h - block_h) * 0.5,
        TextPlace::Top => 0.08 * h,
        TextPlace::Bottom => 0.92 * h - block_h,
    };
    let left = match c.place {
        TextPlace::Left => 0.075 * w,
        TextPlace::Right => (0.56 * w).min(w - 0.06 * w - block_w),
        _ => 0.0,
    };

    let mut words = Vec::new();
    let mut index = 0;
    let mut y = top;
    for (lines, face, lh, is_title) in [(&title, title_face, title_lh, true), (&sub, sub_face, sub_lh, false)] {
        if !is_title {
            y += gap;
        }
        let space = face.width("a a") - face.width("aa");
        for line in lines {
            // Centre the text block's lines on the line height, then sit them on a baseline.
            let baseline = y + (lh - face.size) * 0.5 + face.ascent() * 0.98;
            let mut x = if beside { left } else { (w - line.width) * 0.5 };
            for (text, ww) in &line.words {
                words.push(Word { text: text.clone(), face, x, baseline, index, group: is_title.then_some(line.group) });
                index += 1;
                x += ww + space;
            }
            y += lh;
        }
    }
    words
}

/// Opacity and upward offset (fraction of the font size) of a word at `local`.
fn animate(c: &TextClip, word: &Word, words: usize, groups: usize, local: f32) -> (f32, f32) {
    let delay = |group: usize| -> f32 {
        let step = ((c.duration - IN - OUT - 0.6) / (groups.max(2) - 1) as f32).clamp(0.35, 1.2);
        group as f32 * step
    };
    let start = match c.reveal {
        TextReveal::Fade => {
            if word.group.is_some() {
                0.0
            } else {
                0.15
            }
        }
        TextReveal::Words => word.index as f32 * (0.45 * c.duration / words.max(1) as f32).clamp(0.05, 0.16),
        TextReveal::Lines => match word.group {
            Some(g) => delay(g),
            None => delay(groups.saturating_sub(1)) + 0.25,
        },
    };
    let a = ease_out((local - start) / IN);
    let mut alpha = a;
    if c.reveal == TextReveal::Lines {
        if let Some(g) = word.group.filter(|g| g + 1 < groups) {
            alpha *= 1.0 - 0.55 * ease_out((local - delay(g + 1)) / IN);
        }
    }
    alpha *= smooth((c.duration - local) / OUT);
    (alpha, (1.0 - a) * 0.3)
}

/// Draws the titles on screen into `out` (RGBA8, `w`×`h`).
pub fn draw(out: &mut [u8], w: usize, h: usize, style: &Style, shown: &[Shown]) {
    for l in layers(w, h, style, shown) {
        l.blend(out, w);
    }
}

/// The titles on screen as one premultiplied RGBA layer over the region they cover, for the
/// GPU to put over the frame. `None` when nothing shows.
pub struct Overlay {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

pub fn overlay(w: usize, h: usize, style: &Style, shown: &[Shown]) -> Option<Overlay> {
    let layers = layers(w, h, style, shown);
    let x0 = layers.iter().map(|l| l.x0).min()?;
    let y0 = layers.iter().map(|l| l.y0).min()?;
    let x1 = layers.iter().map(|l| l.x0 + l.w).max()?;
    let y1 = layers.iter().map(|l| l.y0 + l.h).max()?;
    let (ow, oh) = (x1 - x0, y1 - y0);
    // Premultiplied colour and coverage; each layer goes over the ones before.
    let mut acc = vec![[0.0f32; 4]; ow * oh];
    for l in &layers {
        for y in 0..l.h {
            for x in 0..l.w {
                let (g, a) = l.at(x, y);
                if g < 0.002 && a < 0.002 {
                    continue;
                }
                // Halo then glyph, as `blend` does: d → d·(1-g)(1-a) + halo·g·(1-a) + col·a.
                let keep = (1.0 - g) * (1.0 - a);
                let o = &mut acc[(y + l.y0 - y0) * ow + x + l.x0 - x0];
                for k in 0..3 {
                    o[k] = o[k] * keep + l.halo[k] * g * (1.0 - a) + l.col[k] * a;
                }
                o[3] = o[3] * keep + (1.0 - keep);
            }
        }
    }
    // Colour is already 0..255 (premultiplied); coverage goes from 0..1 to 0..255.
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    let data = acc.iter().flat_map(|o| [q(o[0]), q(o[1]), q(o[2]), q(o[3] * 255.0)]).collect();
    Some(Overlay { x: x0, y: y0, w: ow, h: oh, data })
}

/// One title's ink: glyph coverage and the soft halo around it, over a region of the frame.
struct Layer {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    mask: Vec<f32>,
    glow: Vec<f32>,
    col: [f32; 3],
    halo: [f32; 3],
}

impl Layer {
    /// Halo and glyph opacity at a point of the region.
    #[inline(always)]
    fn at(&self, x: usize, y: usize) -> (f32, f32) {
        let i = y * self.w + x;
        ((self.glow[i] * 1.8).min(1.0) * 0.55, self.mask[i])
    }

    /// Paints the glyphs in `col` over a soft glow of the opposite tone, which only shows where
    /// the text crosses something of its own colour (the recording, during a zoom).
    fn blend(&self, out: &mut [u8], w: usize) {
        for y in 0..self.h {
            for x in 0..self.w {
                let (g, a) = self.at(x, y);
                if g < 0.002 && a < 0.002 {
                    continue;
                }
                let i = ((y + self.y0) * w + x + self.x0) * 4;
                for k in 0..3 {
                    let d = out[i + k] as f32;
                    let d = d + (self.halo[k] - d) * g;
                    out[i + k] = (d + (self.col[k] - d) * a).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

fn layers(w: usize, h: usize, style: &Style, shown: &[Shown]) -> Vec<Layer> {
    let mut out = Vec::new();
    for s in shown {
        let c = s.clip;
        if is_blank(c) || s.local < 0.0 || s.local > c.duration {
            continue;
        }
        let col = color(style, c.color);
        let words = layout(c, w as f32, h as f32);
        let groups = words.iter().filter_map(|w| w.group).max().map(|g| g + 1).unwrap_or(0);
        let mut glyphs = Vec::new();
        for word in &words {
            let (alpha, rise) = animate(c, word, words.len(), groups, s.local);
            if alpha > 0.003 {
                outline_word(word, word.baseline + rise * word.face.size, alpha, &mut glyphs);
            }
        }
        let biggest = words.iter().map(|w| w.face.size).fold(0.0, f32::max);
        out.extend(layer(w, h, &glyphs, col, (0.12 * biggest).max(1.0) as usize));
    }
    out
}

fn outline_word(word: &Word, baseline: f32, alpha: f32, glyphs: &mut Vec<(ab_glyph::OutlinedGlyph, f32)>) {
    let face = word.face;
    let scale = PxScale::from(face.size);
    let sf = face.font.as_scaled(scale);
    let mut caret = word.x;
    let mut prev: Option<GlyphId> = None;
    for ch in word.text.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            caret += sf.kern(p, id);
        }
        if let Some(glyph) = face.font.outline_glyph(id.with_scale_and_position(scale, point(caret, baseline))) {
            glyphs.push((glyph, alpha));
        }
        caret += sf.h_advance(id) + face.tracking;
        prev = Some(id);
    }
}

/// Two box blurs in each direction (close to a Gaussian) of a `w`×`h` coverage mask.
fn blur(mask: &mut [f32], w: usize, h: usize, r: usize) {
    let mut tmp = vec![0.0f32; mask.len()];
    for _ in 0..2 {
        box_pass(mask, &mut tmp, w, 1, h, w, r);
        box_pass(&tmp, mask, h, w, w, 1, r);
    }
}

/// Box-blurs `lines` runs of `n` samples `stride` apart, each run starting `step` after the last.
fn box_pass(from: &[f32], to: &mut [f32], n: usize, stride: usize, lines: usize, step: usize, r: usize) {
    let r = r as isize;
    for line in 0..lines {
        let base = line * step;
        let at = |i: isize| if i < 0 || i >= n as isize { 0.0 } else { from[base + i as usize * stride] };
        let mut sum: f32 = (-r..=r).map(at).sum();
        for i in 0..n as isize {
            to[base + i as usize * stride] = sum / (2 * r + 1) as f32;
            sum += at(i + r + 1) - at(i - r);
        }
    }
}

fn layer(w: usize, h: usize, glyphs: &[(ab_glyph::OutlinedGlyph, f32)], col: [f32; 3], r: usize) -> Option<Layer> {
    let bounds = glyphs.iter().map(|(g, _)| g.px_bounds()).reduce(|a, b| ab_glyph::Rect {
        min: point(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
        max: point(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
    })?;
    let pad = 3 * r as i64;
    let x0 = (bounds.min.x as i64 - pad).max(0);
    let y0 = (bounds.min.y as i64 - pad).max(0);
    let x1 = (bounds.max.x as i64 + pad).min(w as i64);
    let y1 = (bounds.max.y as i64 + pad).min(h as i64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (mw, mh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut mask = vec![0.0f32; mw * mh];
    for (g, alpha) in glyphs {
        let b = g.px_bounds();
        g.draw(|gx, gy, cov| {
            let (px, py) = (b.min.x as i64 + gx as i64 - x0, b.min.y as i64 + gy as i64 - y0);
            if px >= 0 && py >= 0 && px < mw as i64 && py < mh as i64 {
                let m = &mut mask[py as usize * mw + px as usize];
                *m = m.max((cov * alpha).clamp(0.0, 1.0));
            }
        });
    }
    let mut glow = mask.clone();
    blur(&mut glow, mw, mh, r);
    let light = col[0] + col[1] + col[2] > 384.0;
    let halo = if light { [0.0, 0.0, 0.0] } else { [255.0, 255.0, 255.0] };
    Some(Layer { x0: x0 as usize, y0: y0 as usize, w: mw, h: mh, mask, glow, col, halo })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(title: &str, subtitle: &str, reveal: TextReveal) -> TextClip {
        TextClip { title: title.into(), subtitle: subtitle.into(), reveal, duration: 4.0, color: TextColor::Light, ..TextClip::default() }
    }

    /// Bounding box of the pixels that changed, and how many.
    fn ink(c: &TextClip, local: f32) -> (usize, usize, usize, usize, usize) {
        let (w, h) = (640, 360);
        let mut out = vec![0u8; w * h * 4];
        draw(&mut out, w, h, &Style::default(), &[Shown { clip: c, local }]);
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (w, h, 0, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if out[(y * w + x) * 4] > 40 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                    n += 1;
                }
            }
        }
        (x0, y0, x1, y1, n)
    }

    #[test]
    fn titles_fade_in_and_out_beside_the_device() {
        let c = clip("Camina.", "Tus pasos te mueven.", TextReveal::Fade);
        assert_eq!(ink(&c, 0.0).4, 0, "nothing before it fades in");
        let (x0, y0, x1, y1, n) = ink(&c, 2.0);
        assert!(n > 500, "{n}");
        // On the left third, roughly centred vertically.
        assert!(x0 > 30 && x0 < 60 && x1 < 300, "{x0}..{x1}");
        assert!(y0 > 100 && y1 < 260, "{y0}..{y1}");
        assert_eq!(ink(&c, 4.0).4, 0, "gone at the end");
    }

    #[test]
    fn words_and_lines_build_up() {
        let words = clip("Corre hacia tu meta diaria", "", TextReveal::Words);
        assert!(ink(&words, 0.3).4 < ink(&words, 2.0).4);
        let lines = clip("Día.\nNoche.\nLluvia.", "", TextReveal::Lines);
        let (_, _, _, early_bottom, _) = ink(&lines, 0.6);
        let (_, _, _, late_bottom, _) = ink(&lines, 2.8);
        assert!(late_bottom > early_bottom + 20, "{early_bottom} → {late_bottom}");
    }

    #[test]
    fn long_titles_wrap_inside_their_column() {
        let c = clip("Una frase bastante larga que no cabe en una sola línea junto al móvil", "", TextReveal::Fade);
        let (x0, _, x1, _, _) = ink(&c, 2.0);
        assert!(x1 - x0 <= (0.37 * 640.0) as usize + 4, "{x0}..{x1}");
    }

    #[test]
    fn the_overlay_matches_drawing_on_the_frame() {
        let (w, h) = (640, 360);
        let a = TextClip { title: "Dos títulos".into(), subtitle: "a la vez".into(), color: TextColor::Light, ..TextClip::default() };
        let b = TextClip { title: "Abajo".into(), place: TextPlace::Bottom, color: TextColor::Dark, ..TextClip::default() };
        let shown = [Shown { clip: &a, local: 1.5 }, Shown { clip: &b, local: 1.5 }];
        let base: Vec<u8> = (0..w * h * 4).map(|i| ((i / 4) % 251) as u8).collect();
        let mut drawn = base.clone();
        draw(&mut drawn, w, h, &Style::default(), &shown);
        let o = overlay(w, h, &Style::default(), &shown).expect("overlay");
        let mut composed = base.clone();
        for y in 0..o.h {
            for x in 0..o.w {
                let s = &o.data[(y * o.w + x) * 4..][..4];
                let i = ((y + o.y) * w + x + o.x) * 4;
                for k in 0..3 {
                    composed[i + k] = (s[k] as f32 + composed[i + k] as f32 * (1.0 - s[3] as f32 / 255.0)).round().min(255.0) as u8;
                }
            }
        }
        let worst = drawn.iter().zip(&composed).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(worst <= 2, "{worst}");
        assert!(overlay(w, h, &Style::default(), &[]).is_none());
    }

    #[test]
    fn the_device_makes_room() {
        let wide = 16.0 / 9.0;
        assert_eq!(room(TextPlace::Left, 0.0, wide), (0.0, 0.0, 1.0));
        let (dx, dy, s) = room(TextPlace::Left, 1.0, wide);
        assert!(dx > 0.3 && dy == 0.0 && s == 1.0, "{dx} {dy} {s}");
        assert!(room(TextPlace::Right, 1.0, wide).0 < -0.3);
        let (_, dy, s) = room(TextPlace::Top, 1.0, 9.0 / 16.0);
        assert!(dy > 0.1 && s < 1.0, "{dy} {s}");
        assert_eq!(room(TextPlace::Center, 1.0, wide), (0.0, 0.0, 1.0));
    }
}
