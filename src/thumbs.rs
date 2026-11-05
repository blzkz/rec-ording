//! Pictures of projects for the start screen: a frame of the video as the export draws it,
//! kept in the cache until the project changes.

use crate::model::{cache_dir, load_project, FrameStyle, Project};
use crate::{model3d, timeline};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Width of the pictures, enough for the cards on a high-density screen.
const WIDTH: usize = 360;

pub struct Thumb {
    pub size: (usize, usize),
    pub rgba: Vec<u8>,
}

fn cache_file(project: &Path) -> PathBuf {
    let hash = Sha256::digest(project.to_string_lossy().as_bytes());
    let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    cache_dir().join("miniaturas").join(format!("{hex}.rgba"))
}

/// The cached picture, if it was drawn for this revision of the project.
fn cached(file: &Path, revision: u64) -> Option<Thumb> {
    let raw = fs::read(file).ok()?;
    let word = |i: usize| raw.get(i * 8..i * 8 + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let (rev, w, h) = (word(0)?, word(1)? as usize, word(2)? as usize);
    (rev == revision && raw.len() == 24 + w * h * 4).then(|| Thumb { size: (w, h), rgba: raw[24..].to_vec() })
}

fn model_for(p: &Project) -> Option<model3d::PreparedModel> {
    let cm = p.style.model.as_ref().filter(|_| p.style.frame == FrameStyle::Custom)?;
    let src = model3d::load(&cm.path).ok()?;
    let part = cm.screen(src.guess, src.parts.len())?;
    model3d::prepare(&src, part, cm.quarter_turns).ok()
}

/// The picture of the project at `path`; None when it has no video yet or cannot be drawn.
pub fn project(path: &Path) -> Option<Thumb> {
    let p = load_project(path).ok()?;
    if p.is_empty() {
        return None;
    }
    let file = cache_file(path);
    if let Some(t) = cached(&file, p.revision) {
        return Some(t);
    }
    // A moment past the opening, when the device and the style are in view.
    let t = timeline::duration(&p) * 0.35;
    let model = model_for(&p);
    let rgba = crate::mcp::render_rgba(&p, t, WIDTH, model.as_ref()).ok()?;
    let (w, h) = crate::mcp::out_size(&p, WIDTH);
    let mut raw = Vec::with_capacity(24 + rgba.len());
    for v in [p.revision, w as u64, h as u64] {
        raw.extend_from_slice(&v.to_le_bytes());
    }
    raw.extend_from_slice(&rgba);
    let _ = fs::create_dir_all(file.parent().unwrap());
    let _ = fs::write(&file, raw);
    Some(Thumb { size: (w, h), rgba })
}
