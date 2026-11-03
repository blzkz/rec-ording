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
    /// For imported models: the file or folder in the gallery that holds it (renamable, removable).
    pub entry: Option<PathBuf>,
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
                    entry: None,
                });
            }
        }
    }
    // Imported models: a single `.glb`, or a folder holding a model and the files it uses.
    let mut mine: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(user_dir()).map(|rd| rd.flatten().collect::<Vec<_>>()).unwrap_or_default() {
        let p = entry.path();
        if p.is_dir() {
            let model = std::fs::read_dir(&p)
                .map(|rd| rd.flatten().map(|e| e.path()).collect::<Vec<_>>())
                .unwrap_or_default()
                .into_iter()
                .find(|f| is_model_file(f));
            if let Some(f) = model {
                mine.push((p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), f, p));
            }
        } else if is_model_file(&p) {
            mine.push((p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), p.clone(), p));
        }
    }
    mine.sort();
    for (name, p, entry) in mine {
        out.push(LibraryModel { name, path: p.to_string_lossy().into_owned(), author: None, license: None, entry: Some(entry) });
    }
    out
}

fn is_model_file(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb") || e.eq_ignore_ascii_case("gltf"))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Files a model loads from next to itself (textures, `.bin` buffers), as relative paths.
fn external_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    let (doc, _) = model3d::read_document(path)?;
    let mut uris = Vec::new();
    for b in doc.buffers() {
        if let gltf::buffer::Source::Uri(u) = b.source() {
            uris.push(u.to_string());
        }
    }
    for i in doc.images() {
        if let gltf::image::Source::Uri { uri, .. } = i.source() {
            uris.push(uri.to_string());
        }
    }
    uris.retain(|u| !u.starts_with("data:"));
    uris.sort();
    uris.dedup();
    Ok(uris.iter().map(|u| PathBuf::from(percent_decode(u))).collect())
}

/// Imports what the user picked: a model file or a `.zip` download containing one. The model is
/// loaded first, so nothing is added to the gallery if it cannot be used. Returns the gallery
/// path and the loaded model.
pub fn import_any(path: &Path) -> Result<(PathBuf, ModelSource), String> {
    let is_zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip {
        let src = model3d::load(&path.to_string_lossy())?;
        let dest = import(path)?;
        return Ok((dest, src));
    }

    let tmp = crate::model::cache_dir().join(format!("import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        let ok = std::process::Command::new("unzip")
            .args(["-q", "-o"])
            .arg(path)
            .arg("-d")
            .arg(&tmp)
            .status()
            .map(|s| s.success())
            .or_else(|_| {
                std::process::Command::new("bsdtar").arg("-xf").arg(path).arg("-C").arg(&tmp).status().map(|s| s.success())
            })
            .map_err(|_| "Para importar un .zip hace falta unzip (sudo pacman -S unzip).".to_string())?;
        if !ok {
            return Err("No se pudo descomprimir el .zip".to_string());
        }
        // Prefer a .glb (self-contained) over a .gltf; among several, the largest.
        let mut found: Vec<(bool, u64, PathBuf)> = Vec::new();
        let mut stack = vec![tmp.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if is_model_file(&p) {
                    let glb = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("glb"));
                    found.push((glb, e.metadata().map(|m| m.len()).unwrap_or(0), p));
                }
            }
        }
        found.sort();
        let (_, _, model) = found.pop().ok_or("El .zip no contiene ningún modelo .glb ni .gltf")?;
        let src = model3d::load(&model.to_string_lossy())?;
        // Name the copy after the download, not after names like "Untitled" or "scene".
        let named = model.with_file_name(format!(
            "{}.{}",
            path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "modelo".into()),
            model.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "glb".into())
        ));
        std::fs::rename(&model, &named).map_err(|e| e.to_string())?;
        let dest = import(&named)?;
        Ok((dest, src))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn inside_gallery(entry: &Path) -> Result<PathBuf, String> {
    let root = user_dir().canonicalize().map_err(|e| e.to_string())?;
    let e = entry.canonicalize().map_err(|_| "Ese modelo ya no existe".to_string())?;
    if e.parent() != Some(root.as_path()) {
        return Err("Solo se pueden cambiar los modelos que has importado".into());
    }
    Ok(e)
}

/// Renames an imported model. Returns the new path of its model file.
pub fn rename(entry: &Path, new_name: &str) -> Result<PathBuf, String> {
    let entry = inside_gallery(entry)?;
    let name = new_name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err("Escribe un nombre válido (sin barras)".into());
    }
    let (target, model_file) = if entry.is_dir() {
        let model = std::fs::read_dir(&entry)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| is_model_file(p))
            .ok_or("No se encontró el modelo")?;
        let target = entry.with_file_name(name);
        let file = target.join(model.file_name().unwrap_or_default());
        (target, file)
    } else {
        let ext = entry.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "glb".into());
        let target = entry.with_file_name(format!("{name}.{ext}"));
        (target.clone(), target)
    };
    if target == entry {
        return Ok(model_file);
    }
    if target.exists() {
        return Err(format!("Ya hay un modelo llamado «{name}»"));
    }
    std::fs::rename(&entry, &target).map_err(|e| format!("No se pudo renombrar: {e}"))?;
    Ok(model_file)
}

/// Deletes an imported model (its file, or its folder with textures) from the gallery.
pub fn remove(entry: &Path) -> Result<(), String> {
    let entry = inside_gallery(entry)?;
    let r = if entry.is_dir() { std::fs::remove_dir_all(&entry) } else { std::fs::remove_file(&entry) };
    r.map_err(|e| format!("No se pudo eliminar: {e}"))
}

/// Adds a model to the gallery. A self-contained `.glb` is copied in as is; a model that loads
/// textures or buffers from next to it (any `.gltf`, and some `.glb`) is copied into its own
/// folder together with those files. Returns where the gallery copy lives.
pub fn import(path: &Path) -> Result<PathBuf, String> {
    if !is_model_file(path) {
        return Err("Elige un archivo .glb o .gltf".into());
    }
    let deps = external_files(path)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "modelo".into());
    let dir = user_dir();

    if deps.is_empty() {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "glb".into());
        let mut dest = dir.join(format!("{stem}.{ext}"));
        let mut n = 2;
        while dest.exists() {
            if std::fs::read(&dest).ok() == std::fs::read(path).ok() {
                return Ok(dest);
            }
            dest = dir.join(format!("{stem} {n}.{ext}"));
            n += 1;
        }
        std::fs::copy(path, &dest).map_err(|e| format!("No se pudo copiar el modelo: {e}"))?;
        return Ok(dest);
    }

    let src_dir = path.parent().unwrap_or(Path::new("."));
    // Files outside the model's folder (absolute or "../") cannot be bundled: use it in place.
    let contained = deps.iter().all(|d| d.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)));
    if !contained {
        return Ok(path.to_path_buf());
    }
    let missing: Vec<String> = deps.iter().filter(|d| !src_dir.join(d).is_file()).map(|d| d.display().to_string()).collect();
    if !missing.is_empty() {
        return Err(format!(
            "Al modelo le faltan archivos que deberían estar junto a él: {}. Descomprime la descarga completa y vuelve a importarlo.",
            missing.join(", ")
        ));
    }
    let mut folder = dir.join(&stem);
    let mut n = 2;
    while folder.exists() {
        folder = dir.join(format!("{stem} {n}"));
        n += 1;
    }
    let copy = |rel: &Path| -> Result<(), String> {
        let to = folder.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(src_dir.join(rel), &to).map(|_| ()).map_err(|e| format!("No se pudo copiar {}: {e}", rel.display()))
    };
    let file_name = PathBuf::from(path.file_name().unwrap_or_default());
    let result = copy(&file_name).and_then(|_| deps.iter().try_for_each(|d| copy(d)));
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(e);
    }
    Ok(folder.join(file_name))
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
        model: Some(CustomModel::default()),
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

    /// Downloads with loose textures. Run with a throwaway data folder:
    /// XDG_DATA_HOME=<tmp> MODEL_DIR=<dir with descarga_glb/, descarga_gltf/, phone_named.glb> cargo test imports_ -- --ignored
    #[test]
    #[ignore]
    fn imports_models_with_their_files() {
        let Ok(dir) = std::env::var("MODEL_DIR") else { return };
        let dir = PathBuf::from(dir);

        let glb = import(&dir.join("descarga_glb/movil.glb")).expect("glb with loose texture");
        assert!(glb.ends_with("movil/movil.glb"), "{}", glb.display());
        assert!(glb.parent().unwrap().join("textures/checker con espacio.png").is_file());
        model3d::load(&glb.to_string_lossy()).expect("loads from the gallery copy");

        let gltf = import(&dir.join("descarga_gltf/scene.gltf")).expect("gltf");
        assert!(gltf.parent().unwrap().join("scene.bin").is_file());
        model3d::load(&gltf.to_string_lossy()).expect("gltf loads from the gallery copy");

        let single = import(&dir.join("phone_named.glb")).expect("self-contained glb");
        assert!(single.is_file() && single.parent().unwrap() == user_dir());

        // Only the .glb, without its texture folder.
        let lonely = dir.join("solo");
        let _ = std::fs::create_dir_all(&lonely);
        std::fs::copy(dir.join("descarga_glb/movil.glb"), lonely.join("movil.glb")).unwrap();
        let err = import(&lonely.join("movil.glb")).unwrap_err();
        assert!(err.contains("faltan") && err.contains("checker con espacio.png"), "{err}");

        let names: Vec<String> = list().into_iter().filter(|m| m.license.is_none()).map(|m| m.name).collect();
        for n in ["movil", "scene", "phone_named"] {
            assert!(names.iter().any(|x| x == n), "{n} not in {names:?}");
        }
    }

    /// The user's flows: zip download, a broken file, rename and delete. Run with a throwaway data folder:
    /// XDG_DATA_HOME=<tmp> ZIP=<model download .zip> BROKEN_DIR=<tmp dir> cargo test manages_ -- --ignored
    #[test]
    #[ignore]
    fn manages_imported_models() {
        let (Ok(zip), Ok(tmp)) = (std::env::var("ZIP"), std::env::var("BROKEN_DIR")) else { return };
        let (dest, src) = import_any(Path::new(&zip)).expect("zip import");
        assert!(src.guess.is_some());
        let stem = Path::new(&zip).file_stem().unwrap().to_string_lossy().into_owned();
        assert_eq!(dest.file_stem().unwrap().to_string_lossy(), stem, "named after the download: {}", dest.display());

        // A broken file never reaches the gallery.
        let broken = Path::new(&tmp).join("roto.glb");
        std::fs::write(&broken, b"glTF esto no es un modelo").unwrap();
        let before = list().len();
        assert!(import_any(&broken).is_err());
        assert_eq!(list().len(), before);

        let entry = list().into_iter().find(|m| m.path == dest.to_string_lossy()).and_then(|m| m.entry).expect("listed");
        let renamed = rename(&entry, "Mi Galaxy").expect("rename");
        assert!(renamed.is_file() && renamed.file_stem().unwrap() == "Mi Galaxy");
        assert!(rename(&renamed, "a/b").is_err());
        let entry = list().into_iter().find(|m| m.name == "Mi Galaxy").and_then(|m| m.entry).expect("renamed in list");
        crate::model3d::load(&renamed.to_string_lossy()).expect("still loads after rename");

        // Nothing outside the gallery can be touched.
        assert!(remove(&broken).is_err() && broken.exists());
        remove(&entry).expect("remove");
        assert!(!renamed.exists() && list().iter().all(|m| m.name != "Mi Galaxy"));
    }

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
