//! Pictures over the video (logos, badges, stickers…), transparency included. They are drawn
//! under the titles: on the frame by the CPU, and into the titles' overlay for the graphics card,
//! so both look the same.

use crate::model::ImageClip;
use crate::platform::command;
use crate::text::Overlay;
use std::collections::HashMap;
use std::io::Read;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};

/// A picture on screen at some moment: `local` seconds into its clip.
pub struct Shown<'a> {
    pub clip: &'a ImageClip,
    pub local: f32,
}

pub fn shown(images: &[ImageClip], t: f32) -> Vec<Shown<'_>> {
    images.iter().filter(|c| t >= c.start && t < c.end()).map(|c| Shown { clip: c, local: t - c.start }).collect()
}

/// A decoded picture, premultiplied RGBA.
#[derive(Clone)]
pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// Size of a picture file, from ffprobe.
pub fn size(path: &str) -> Result<(usize, usize), String> {
    let out = command("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x"])
        .arg(path)
        .output()
        .map_err(|_| "No se encontró ffprobe".to_string())?;
    let s = String::from_utf8_lossy(&out.stdout);
    let (w, h) = s.trim().split_once('x').ok_or("No es una imagen que se pueda abrir")?;
    match (w.trim().parse::<usize>(), h.trim().parse::<usize>()) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 => Ok((w, h)),
        _ => Err("No es una imagen que se pueda abrir".into()),
    }
}

/// Reads a picture (PNG, WebP, JPEG…; the first frame of an animation) with ffmpeg.
fn decode(path: &str) -> Result<Picture, String> {
    let (w, h) = size(path)?;
    // Very large pictures are scaled down: no frame needs more.
    let k = (4096.0 / w.max(h) as f32).min(1.0);
    let (w, h) = (((w as f32 * k).round() as usize).max(1), ((h as f32 * k).round() as usize).max(1));
    let mut child = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}:flags=lanczos"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "No se encontró ffmpeg".to_string())?;
    let mut data = vec![0u8; w * h * 4];
    child.stdout.as_mut().unwrap().read_exact(&mut data).map_err(|_| "No se pudo leer la imagen".to_string())?;
    let _ = child.wait();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    Ok(Picture { w, h, data })
}

type Cache = Mutex<HashMap<String, Option<Arc<Picture>>>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// The picture at `path`, read once (`None` if it cannot be read).
pub fn load(path: &str) -> Option<Arc<Picture>> {
    if let Some(p) = cache().lock().unwrap().get(path) {
        return p.clone();
    }
    let p = decode(path).ok().map(Arc::new);
    cache().lock().unwrap().insert(path.to_string(), p.clone());
    p
}

/// The last sizes asked for, so every frame does not resample again.
fn scaled(path: &str, pic: &Picture, tw: usize, th: usize) -> Arc<Picture> {
    type Sized = Mutex<Vec<((String, usize, usize), Arc<Picture>)>>;
    static S: OnceLock<Sized> = OnceLock::new();
    let s = S.get_or_init(Default::default);
    let key = (path.to_string(), tw, th);
    if let Some((_, p)) = s.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return p.clone();
    }
    let p = Arc::new(resize(pic, tw, th));
    let mut list = s.lock().unwrap();
    list.push((key, p.clone()));
    if list.len() > 24 {
        list.remove(0);
    }
    p
}

/// `pic` at another size.
pub fn scale(pic: &Picture, w: usize, h: usize) -> Picture {
    resize(pic, w.max(1), h.max(1))
}

/// Resamples premultiplied RGBA: halving by averaging while far bigger, then bilinear.
fn resize(src: &Picture, tw: usize, th: usize) -> Picture {
    let mut cur: std::borrow::Cow<Picture> = std::borrow::Cow::Borrowed(src);
    while cur.w >= tw * 2 && cur.h >= th * 2 && cur.w >= 2 && cur.h >= 2 {
        let (w, h) = (cur.w / 2, cur.h / 2);
        let mut data = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                for k in 0..4 {
                    let at = |xx: usize, yy: usize| cur.data[(yy * cur.w + xx) * 4 + k] as u32;
                    data[(y * w + x) * 4 + k] = ((at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1) + 2) / 4) as u8;
                }
            }
        }
        cur = std::borrow::Cow::Owned(Picture { w, h, data });
    }
    let (sw, sh) = (cur.w, cur.h);
    let mut data = vec![0u8; tw * th * 4];
    for y in 0..th {
        let fy = ((y as f32 + 0.5) * sh as f32 / th as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..tw {
            let fx = ((x as f32 + 0.5) * sw as f32 / tw as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for k in 0..4 {
                let at = |xx: usize, yy: usize| cur.data[(yy * sw + xx) * 4 + k] as f32;
                let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                data[(y * tw + x) * 4 + k] = (top + (bottom - top) * ty).round() as u8;
            }
        }
    }
    Picture { w: tw, h: th, data }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Opacity of a picture `local` seconds into its clip (fades included).
pub fn alpha(c: &ImageClip, local: f32) -> f32 {
    let fin = if c.fade_in > 0.0 { smooth(local / c.fade_in) } else { 1.0 };
    let fout = if c.fade_out > 0.0 { smooth((c.duration - local) / c.fade_out) } else { 1.0 };
    c.opacity.clamp(0.0, 1.0) * fin.min(fout)
}

/// Where a picture goes on a `w`×`h` frame: left, top, width, height in pixels.
pub fn place(c: &ImageClip, w: usize, h: usize) -> (i64, i64, usize, usize) {
    let pw = (c.width.max(0.01) * w as f32).round().max(1.0);
    let ph = (pw / c.aspect.max(0.01)).round().max(1.0);
    let x = (c.x * w as f32 - pw / 2.0).round() as i64;
    let y = (c.y * h as f32 - ph / 2.0).round() as i64;
    (x, y, pw as usize, ph as usize)
}

/// A picture ready to blend: premultiplied pixels at the frame position, and its opacity.
struct Placed {
    x: i64,
    y: i64,
    pic: Arc<Picture>,
    alpha: f32,
}

fn placed(w: usize, h: usize, shown: &[Shown]) -> Vec<Placed> {
    shown
        .iter()
        .filter_map(|s| {
            let a = alpha(s.clip, s.local);
            if a < 0.003 {
                return None;
            }
            let pic = load(&s.clip.path)?;
            let (x, y, pw, ph) = place(s.clip, w, h);
            // Wholly off the frame: nothing to draw (a picture partly off is cut at the edge).
            if x >= w as i64 || y >= h as i64 || x + pw as i64 <= 0 || y + ph as i64 <= 0 {
                return None;
            }
            Some(Placed { x, y, pic: scaled(&s.clip.path, &pic, pw, ph), alpha: a })
        })
        .collect()
}

/// Draws the pictures on screen into `out` (RGBA8, `w`×`h`), in order.
pub fn draw(out: &mut [u8], w: usize, h: usize, shown: &[Shown]) {
    for p in placed(w, h, shown) {
        for py in 0..p.pic.h {
            let y = p.y + py as i64;
            if y < 0 || y >= h as i64 {
                continue;
            }
            for px in 0..p.pic.w {
                let x = p.x + px as i64;
                if x < 0 || x >= w as i64 {
                    continue;
                }
                let s = &p.pic.data[(py * p.pic.w + px) * 4..][..4];
                let a = s[3] as f32 / 255.0 * p.alpha;
                if a < 0.002 {
                    continue;
                }
                let i = (y as usize * w + x as usize) * 4;
                for k in 0..3 {
                    let d = out[i + k] as f32;
                    out[i + k] = (s[k] as f32 * p.alpha + d * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// The pictures and the titles as one premultiplied layer for the graphics card (titles on top).
pub fn overlay(w: usize, h: usize, style: &crate::model::Style, images: &[Shown], texts: &[crate::text::Shown]) -> Option<Overlay> {
    let titles = crate::text::overlay(w, h, style, texts);
    let pics = placed(w, h, images);
    if pics.is_empty() {
        return titles;
    }
    // The region both cover, inside the frame.
    let clip = |x: i64, max: usize| x.clamp(0, max as i64) as usize;
    let mut x0 = pics.iter().map(|p| clip(p.x, w)).min().unwrap();
    let mut y0 = pics.iter().map(|p| clip(p.y, h)).min().unwrap();
    let mut x1 = pics.iter().map(|p| clip(p.x + p.pic.w as i64, w)).max().unwrap();
    let mut y1 = pics.iter().map(|p| clip(p.y + p.pic.h as i64, h)).max().unwrap();
    if let Some(t) = &titles {
        x0 = x0.min(t.x);
        y0 = y0.min(t.y);
        x1 = x1.max(t.x + t.w);
        y1 = y1.max(t.y + t.h);
    }
    let (ow, oh) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if ow == 0 || oh == 0 {
        return titles;
    }
    let mut acc = vec![[0.0f32; 4]; ow * oh];
    let mut over = |x: usize, y: usize, c: [f32; 4]| {
        let o = &mut acc[(y - y0) * ow + x - x0];
        let keep = 1.0 - c[3];
        for k in 0..4 {
            o[k] = c[k] + o[k] * keep;
        }
    };
    for p in &pics {
        for py in 0..p.pic.h {
            let y = p.y + py as i64;
            if y < y0 as i64 || y >= y1 as i64 {
                continue;
            }
            for px in 0..p.pic.w {
                let x = p.x + px as i64;
                if x < x0 as i64 || x >= x1 as i64 {
                    continue;
                }
                let s = &p.pic.data[(py * p.pic.w + px) * 4..][..4];
                if s[3] == 0 {
                    continue;
                }
                over(x as usize, y as usize, [s[0] as f32 * p.alpha, s[1] as f32 * p.alpha, s[2] as f32 * p.alpha, s[3] as f32 / 255.0 * p.alpha]);
            }
        }
    }
    if let Some(t) = &titles {
        for y in 0..t.h {
            for x in 0..t.w {
                let s = &t.data[(y * t.w + x) * 4..][..4];
                if s[3] == 0 && s[0] == 0 && s[1] == 0 && s[2] == 0 {
                    continue;
                }
                over(t.x + x, t.y + y, [s[0] as f32, s[1] as f32, s[2] as f32, s[3] as f32 / 255.0]);
            }
        }
    }
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    let data = acc.iter().flat_map(|o| [q(o[0]), q(o[1]), q(o[2]), q(o[3] * 255.0)]).collect();
    Some(Overlay { x: x0, y: y0, w: ow, h: oh, data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Style;

    /// A half-transparent red square, as a PNG written with ffmpeg.
    fn square() -> String {
        let path = std::env::temp_dir().join(format!("rec-ording-img-{}.png", std::process::id()));
        let ok = command("ffmpeg")
            .args(["-nostdin", "-y", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=red@0.5:s=40x20,format=rgba", "-frames:v", "1"])
            .arg(&path)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn pictures_keep_their_transparency_on_cpu_and_overlay() {
        let path = square();
        assert_eq!(size(&path).unwrap(), (40, 20));
        let clip = ImageClip { path: path.clone(), x: 0.5, y: 0.5, width: 0.5, aspect: 2.0, fade_in: 0.0, fade_out: 0.0, ..ImageClip::default() };
        let shown = [Shown { clip: &clip, local: 1.0 }];
        let (w, h) = (80, 60);
        // On a blue frame, the middle turns half red, half blue; the edges stay blue.
        let mut frame: Vec<u8> = (0..w * h).flat_map(|_| [0u8, 0, 200, 255]).collect();
        draw(&mut frame, w, h, &shown);
        let px = |x: usize, y: usize| &frame[(y * w + x) * 4..][..3];
        assert!((px(40, 30)[0] as i32 - 128).abs() <= 3 && (px(40, 30)[2] as i32 - 100).abs() <= 3, "{:?}", px(40, 30));
        assert_eq!(px(2, 2), &[0, 0, 200]);
        assert_eq!(place(&clip, w, h), (20, 20, 40, 20));

        // The overlay gives the same result once put over the frame.
        let o = overlay(w, h, &Style::default(), &shown, &[]).expect("overlay");
        assert_eq!((o.x, o.y, o.w, o.h), (20, 20, 40, 20));
        let s = &o.data[(10 * o.w + 20) * 4..][..4];
        let blended: Vec<f32> = (0..3).map(|k| s[k] as f32 + [0.0, 0.0, 200.0][k] * (1.0 - s[3] as f32 / 255.0)).collect();
        assert!((blended[0] - 128.0).abs() <= 3.0 && (blended[2] - 100.0).abs() <= 3.0, "{blended:?}");

        // Fading in, it is not there yet.
        let fading = ImageClip { fade_in: 1.0, ..clip.clone() };
        assert!(alpha(&fading, 0.0) < 0.01 && (alpha(&fading, 2.0) - 1.0).abs() < 1e-6);
        let _ = std::fs::remove_file(path);
    }
}
