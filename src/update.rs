//! Updates from the public releases repository (the source repository is private). The app
//! checks the latest release when it opens and installs it on request: with pacman through
//! pkexec on Linux, or by swapping the files next to the .exe on Windows.

use crate::platform::command;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Where the release workflows publish the packages.
pub const RELEASES_REPO: &str = "blzkz/rec-ording-releases";

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    /// The release on GitHub, for downloading by hand.
    pub page: String,
    /// The package for this system, if the release has one.
    pub asset: Option<Asset>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub sha256: Option<String>,
}

/// How this copy of the app can be updated.
#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    /// Installed from the Arch package: `pkexec pacman -U`.
    Pacman,
    /// Unzipped folder on Windows: replace the files in place.
    Folder(PathBuf),
    /// Anything else (a build from source): download from the release page.
    Manual,
}

pub fn method() -> Method {
    let Ok(exe) = std::env::current_exe() else {
        return Method::Manual;
    };
    if cfg!(windows) {
        return Method::Folder(exe);
    }
    if exe == Path::new("/usr/bin/rec-ording") && Path::new("/usr/bin/pacman").exists() {
        Method::Pacman
    } else {
        Method::Manual
    }
}

/// The asset name this system installs.
fn asset_suffix() -> &'static str {
    if cfg!(windows) {
        "-windows-x64.zip"
    } else {
        "-x86_64.pkg.tar.zst"
    }
}

fn latest_url() -> String {
    std::env::var("REC_ORDING_UPDATE_URL").unwrap_or_else(|_| format!("https://api.github.com/repos/{RELEASES_REPO}/releases/latest"))
}

/// `"v0.7.10"` → `[0, 7, 10]`; anything after a `-` (pre-release tags) is ignored.
fn version_parts(v: &str) -> Vec<u64> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let v = v.split('-').next().unwrap_or("");
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (version_parts(candidate), version_parts(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// Reads GitHub's "latest release" JSON; `None` when it is not newer than `current`.
pub fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Respuesta no válida: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("La release no tiene versión")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let asset = v["assets"].as_array().into_iter().flatten().find_map(|a| {
        let name = a["name"].as_str().filter(|n| n.ends_with(asset_suffix()))?;
        Some(Asset {
            name: name.to_string(),
            url: a["browser_download_url"].as_str()?.to_string(),
            sha256: a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase),
        })
    });
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or_default().trim().to_string(),
        page: v["html_url"].as_str().map(str::to_string).unwrap_or_else(|| format!("https://github.com/{RELEASES_REPO}/releases/latest")),
        asset,
    }))
}

fn curl(url: &str, out: Option<&Path>, max_secs: u32) -> Result<Vec<u8>, String> {
    let mut c = command("curl");
    c.args(["-fsSL", "--connect-timeout", "10", "--max-time", &max_secs.to_string()])
        .args(["-H", "Accept: application/vnd.github+json", "-H", "User-Agent: rec-ording"])
        .arg(url);
    if let Some(out) = out {
        c.arg("-o").arg(out);
    }
    let o = c.output().map_err(|_| "No se encontró curl".to_string())?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        return Err(if err.is_empty() { "No se pudo conectar".to_string() } else { err });
    }
    Ok(o.stdout)
}

/// Asks for the latest release. `Ok(None)` means this version is up to date.
pub fn check() -> Result<Option<Release>, String> {
    let body = curl(&latest_url(), None, 20)?;
    parse_release(&String::from_utf8_lossy(&body), CURRENT)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads the release's package and checks it against the checksum GitHub publishes.
pub fn download(r: &Release) -> Result<PathBuf, String> {
    let asset = r.asset.as_ref().ok_or("Esta versión no tiene paquete para tu sistema")?;
    let dir = crate::model::cache_dir().join("actualizaciones");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&asset.name);
    curl(&asset.url, Some(&file), 900).map_err(|e| format!("No se pudo descargar: {e}"))?;
    if let Some(expected) = &asset.sha256 {
        if &sha256_file(&file)? != expected {
            let _ = fs::remove_file(&file);
            return Err("La descarga está dañada (no coincide la suma de comprobación)".into());
        }
    }
    Ok(file)
}

/// Downloads and installs `r`. The running app keeps working; the new version starts on restart.
pub fn install(r: &Release, how: &Method) -> Result<(), String> {
    let file = download(r)?;
    match how {
        Method::Pacman => {
            let status = command("pkexec")
                .args(["pacman", "-U", "--noconfirm"])
                .arg(&file)
                .status()
                .map_err(|_| "Falta pkexec (polkit) para instalar el paquete".to_string())?;
            match status.code() {
                Some(0) => Ok(()),
                // pkexec: the password dialog was dismissed or refused.
                Some(126) | Some(127) => Err("Instalación cancelada".into()),
                _ => Err("pacman no pudo instalar el paquete".into()),
            }
        }
        Method::Folder(exe) => {
            let unpacked = file.with_extension("");
            fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
            let ok = command("tar").arg("-xf").arg(&file).arg("-C").arg(&unpacked).status().map(|s| s.success()).unwrap_or(false);
            if !ok {
                return Err("No se pudo descomprimir la actualización".into());
            }
            let exe_name = exe.file_name().ok_or("Ruta del programa no válida")?;
            let new_dir = find_dir_with(&unpacked, exe_name).ok_or("La actualización no contiene rec-ording.exe")?;
            replace_install(&new_dir, exe)
        }
        Method::Manual => Err("Esta copia no se puede actualizar sola".into()),
    }
}

fn find_dir_with(root: &Path, file: &std::ffi::OsStr) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join(file).is_file() {
            return Some(d);
        }
        for e in fs::read_dir(&d).ok()?.flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            }
        }
    }
    None
}

/// The previous executable, kept aside until the next start (Windows can rename a running
/// program but not delete it).
fn old_exe(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// Copies the unzipped release in `new_dir` over the folder of `exe`.
pub fn replace_install(new_dir: &Path, exe: &Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("Ruta del programa no válida")?;
    let new_exe = new_dir.join(exe.file_name().ok_or("Ruta del programa no válida")?);
    let old = old_exe(exe);
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| format!("No se puede escribir en {}: {e}", dir.display()))?;
    if let Err(e) = fs::copy(&new_exe, exe) {
        let _ = fs::rename(&old, exe);
        return Err(format!("No se pudo copiar la versión nueva: {e}"));
    }
    // Models, notes…: everything else in the release.
    let mut stack = vec![new_dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(new_dir).map_err(|e| e.to_string())?;
            let dest = dir.join(rel);
            if p.is_dir() {
                fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
                stack.push(p);
            } else if p != new_exe {
                fs::copy(&p, &dest).map_err(|e| format!("No se pudo copiar {}: {e}", rel.display()))?;
            }
        }
    }
    Ok(())
}

/// Removes the executable a previous update left aside (it may still be closing).
pub fn cleanup() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let old = old_exe(&exe);
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

/// Opens the app again (the updated one) and closes this one.
pub fn restart() -> ! {
    // On Linux the running executable shows up as deleted once pacman replaces it.
    let exe = if matches!(method(), Method::Pacman) {
        PathBuf::from("/usr/bin/rec-ording")
    } else {
        std::env::current_exe().map(|p| {
            let s = p.to_string_lossy();
            PathBuf::from(s.trim_end_matches(" (deleted)"))
        }).unwrap_or_else(|_| PathBuf::from("rec-ording"))
    };
    let _ = std::process::Command::new(exe).spawn();
    std::process::exit(0);
}

fn skip_path() -> PathBuf {
    crate::model::data_dir().join("actualizaciones.json")
}

/// The version the user chose not to be reminded about.
pub fn skipped() -> Option<String> {
    let raw = fs::read_to_string(skip_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v["omitir"].as_str().map(str::to_string)
}

pub fn skip(version: &str) {
    let _ = fs::write(skip_path(), serde_json::json!({ "omitir": version }).to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.7.1", "0.7.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.7.0", "0.7.0"));
        assert!(!is_newer("0.7", "0.7.0"));
        assert!(!is_newer("v0.6.9", "0.7.0"));
        assert!(!is_newer("v0.7.0-beta", "0.7.0"));
    }

    #[test]
    fn reads_github_releases() {
        let json = serde_json::json!({
            "tag_name": "v0.8.0",
            "html_url": "https://github.com/x/y/releases/tag/v0.8.0",
            "body": "Novedades\n",
            "assets": [
                {"name": "rec-ording-0.8.0-1-x86_64.pkg.tar.zst", "browser_download_url": "https://e/linux", "digest": "sha256:ABC"},
                {"name": "rec-ording-0.8.0-windows-x64.zip", "browser_download_url": "https://e/win"}
            ]
        })
        .to_string();
        let r = parse_release(&json, "0.7.0").unwrap().unwrap();
        assert_eq!(r.version, "0.8.0");
        assert_eq!(r.notes, "Novedades");
        let a = r.asset.unwrap();
        if cfg!(windows) {
            assert_eq!((a.url.as_str(), a.sha256), ("https://e/win", None));
        } else {
            assert_eq!((a.url.as_str(), a.sha256.as_deref()), ("https://e/linux", Some("abc")));
        }
        assert_eq!(parse_release(&json, "0.8.0").unwrap(), None);
        assert!(parse_release("{}", "0.7.0").is_err());
    }

    #[test]
    fn replaces_the_installed_folder() {
        let root = std::env::temp_dir().join(format!("rec-ording-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (app, new) = (root.join("app"), root.join("new"));
        fs::create_dir_all(app.join("models")).unwrap();
        fs::create_dir_all(new.join("models")).unwrap();
        let exe = app.join("rec-ording.exe");
        fs::write(&exe, "viejo").unwrap();
        fs::write(app.join("models/a.glb"), "a viejo").unwrap();
        fs::write(app.join("models/mio.glb"), "mío").unwrap();
        fs::write(new.join("rec-ording.exe"), "nuevo").unwrap();
        fs::write(new.join("models/a.glb"), "a nuevo").unwrap();
        fs::write(new.join("models/b.glb"), "b").unwrap();
        fs::write(new.join("LEEME.txt"), "léeme").unwrap();

        replace_install(&new, &exe).unwrap();
        let read = |p: &str| fs::read_to_string(app.join(p)).unwrap();
        assert_eq!(read("rec-ording.exe"), "nuevo");
        assert_eq!(read("rec-ording.old.exe"), "viejo");
        assert_eq!(read("models/a.glb"), "a nuevo");
        assert_eq!(read("models/b.glb"), "b");
        assert_eq!(read("models/mio.glb"), "mío");
        assert_eq!(read("LEEME.txt"), "léeme");
        // A second update replaces the leftover from the first.
        fs::write(new.join("rec-ording.exe"), "más nuevo").unwrap();
        replace_install(&new, &exe).unwrap();
        assert_eq!(read("rec-ording.old.exe"), "nuevo");
        let _ = fs::remove_dir_all(&root);
    }

    /// REC_ORDING_UPDATE_URL=<url of a release JSON> cargo test downloads_updates -- --ignored
    #[test]
    #[ignore]
    fn downloads_updates() {
        let r = check().unwrap().expect("a newer release");
        let f = download(&r).unwrap();
        assert!(f.metadata().unwrap().len() > 0);
    }
}
