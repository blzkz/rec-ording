//! The 3D model gallery: models bundled with the app plus the ones the user imported.

use crate::camera::CamState;
use crate::compositor::{compose, ComposeInput, Frame, Mips};
use crate::model::{data_dir, Background, CustomModel, FrameStyle, Style};
use crate::model3d::{self, ModelSource};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LibraryModel {
    pub path: String,
    pub name: String,
    pub author: Option<String>,
    pub license: Option<String>,
}

impl LibraryModel {
    /// CC BY models must credit their author when a video made with them is published.
    pub fn needs_credit(&self) -> bool {
        self.license.as_deref().is_some_and(|l| l.starts_with("CC BY"))
    }
}

#[derive(Deserialize)]
struct ManifestEntry {
    file: String,
    name: String,
    author: Option<String>,
    license: Option<String>,
}

/// Where the bundled models live: next to the installed binary (`/usr/share/rec-ording`), or in
/// the repository's `assets/` folder when running from `target/`.
fn bundled_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?;
    [
        bin.join("../share/rec-ording/models"),
        bin.join("../../assets/models"),
        PathBuf::from("/usr/share/rec-ording/models"),
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models")),
    ]
    .into_iter()
    .find(|d| d.join("models.json").is_file())
}

pub fn user_dir() -> PathBuf {
    let dir = data_dir().join("models");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn list() -> Vec<LibraryModel> {
    let mut out = Vec::new();
    if let Some(dir) = bundled_dir() {
        let entries: Vec<ManifestEntry> = std::fs::read_to_string(dir.join("models.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        for e in entries {
            let p = dir.join(&e.file);
            if p.is_file() {
                out.push(LibraryModel {
                    path: p.canonicalize().unwrap_or(p).to_string_lossy().into_owned(),
                    name: e.name,
                    author: e.author,
                    license: e.license,
                });
            }
        }
    }
    let mut mine: Vec<PathBuf> = std::fs::read_dir(user_dir())
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    mine.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")));
    mine.sort();
    for p in mine {
        out.push(LibraryModel {
            name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            path: p.to_string_lossy().into_owned(),
            author: None,
            license: None,
        });
    }
    out
}

/// Adds a model to the gallery: `.glb` files are copied in; a `.gltf` (which may reference
/// other files next to it) is used where it is.
pub fn import(path: &Path) -> Result<PathBuf, String> {
    let is_glb = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb"));
    if !is_glb {
        return Ok(path.to_path_buf());
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "modelo".into());
    let dir = user_dir();
    let mut dest = dir.join(format!("{stem}.glb"));
    let mut n = 2;
    while dest.exists() {
        if std::fs::read(&dest).ok() == std::fs::read(path).ok() {
            return Ok(dest);
        }
        dest = dir.join(format!("{stem} {n}.glb"));
        n += 1;
    }
    std::fs::copy(path, &dest).map_err(|e| format!("No se pudo copiar el modelo: {e}"))?;
    Ok(dest)
}

/// A stand-in app screen for thumbnails: a soft gradient with a few cards.
fn sample_screen() -> Mips {
    let (w, h) = (216usize, 480usize);
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let t = y as f32 / h as f32;
            let mut c = [10.0 + 150.0 * t, 132.0 - 40.0 * t, 255.0 - 25.0 * t];
            let card = |y0: usize, y1: usize| y >= y0 && y < y1 && x >= 18 && x < w - 18;
            if card(70, 170) || card(190, 250) || card(270, 330) || card(350, 410) {
                c = [c[0] * 0.15 + 255.0 * 0.85, c[1] * 0.15 + 255.0 * 0.85, c[2] * 0.15 + 255.0 * 0.85];
            }
            if y < 24 {
                c = [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7];
            }
            let i = (y * w + x) * 4;
            data[i..i + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    Mips::build(Frame { w, h, data })
}

/// Renders a gallery thumbnail of a model (RGBA, `w * h * 4`).
pub fn thumbnail(src: &ModelSource, w: usize, h: usize) -> Option<Vec<u8>> {
    let part = src.guess?;
    let model = model3d::prepare(src, part, 0).ok()?;
    let style = Style {
        frame: FrameStyle::Custom,
        model: Some(CustomModel { screen_part: part, ..Default::default() }),
        background: Background { a: [64, 64, 70], b: [46, 46, 51], angle: 90.0 },
        padding: 0.1,
        shadow: 0.5,
        tilt: 8.0,
        turn: -24.0,
        roll: 0.0,
        show_touches: false,
        ..Style::default()
    };
    let screen = sample_screen();
    let mut out = vec![0u8; w * h * 4];
    compose(
        &mut out,
        w,
        h,
        &ComposeInput {
            style: &style,
            screen: Some(&screen),
            video_w: 1080.0,
            video_h: 2400.0,
            cam: CamState::IDENTITY,
            touches: &[],
            time: 0.0,
            since_start: 10.0,
            model: Some(&model),
            supersample: 2,
        },
    );
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_models_load_and_render() {
        let bundled: Vec<LibraryModel> = list().into_iter().filter(|m| m.license.is_some()).collect();
        assert_eq!(bundled.len(), 5, "{bundled:?}");
        for m in bundled {
            let src = model3d::load(&m.path).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            assert!(src.guess.is_some(), "{}: no screen found", m.name);
            let px = thumbnail(&src, 66, 78).expect("thumbnail");
            // The device covers part of the picture (not just background).
            let bg = &px[0..3];
            let differs = px.chunks(4).filter(|p| (p[0] as i32 - bg[0] as i32).abs() > 30).count();
            assert!(differs > 66 * 78 / 10, "{}: thumbnail looks empty", m.name);
        }
    }
}
