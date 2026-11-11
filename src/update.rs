//! Updates from the public download page (GitHub Pages of blzkz/rec-ording-releases). The app
//! reads its `latest.json` when it opens and installs the new
//! version on request: with the system's
//! package manager through pkexec (pacman, apt, dnf or zypper), by replacing the AppImage, or by
//! swapping the files next to the .exe on Windows.

use crate::platform::command;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use crate::i18n::lz;

/// The download page, where the release workflow publishes the packages and `latest.json`.
pub const DOWNLOADS: &str = "https://blzkz.github.io/rec-ording-releases";

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
    /// Installed from the .deb (Debian, Ubuntu, Mint…): `pkexec apt-get install`.
    Deb,
    /// Installed from the .rpm (Fedora, openSUSE…): `pkexec dnf install` or zypper.
    Rpm,
    /// Running as an AppImage: the file is replaced.
    AppImage(PathBuf),
    /// Unzipped folder on Windows: replace the files in place.
    Folder(PathBuf),
    /// Anything else (a build from source): download from the release page.
    Manual,
}

pub fn method() -> Method {
    static METHOD: std::sync::OnceLock<Method> = std::sync::OnceLock::new();
    METHOD.get_or_init(find_method).clone()
}

/// Whether the package manager `program` says it installed the app's binary.
fn owns_binary(program: &str, args: &[&str]) -> bool {
    command(program).args(args).arg("/usr/bin/rec-ording").output().is_ok_and(|o| o.status.success())
}

fn find_method() -> Method {
    let Ok(exe) = std::env::current_exe() else {
        return Method::Manual;
    };
    if cfg!(windows) {
        return Method::Folder(exe);
    }
    // Set by the AppImage runtime to the .AppImage file itself.
    if let Some(file) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Method::AppImage(file);
    }
    if exe != Path::new("/usr/bin/rec-ording") {
        return Method::Manual;
    }
    if owns_binary("pacman", &["-Qqo"]) {
        Method::Pacman
    } else if owns_binary("dpkg", &["-S"]) {
        Method::Deb
    } else if owns_binary("rpm", &["-qf"]) {
        Method::Rpm
    } else {
        Method::Manual
    }
}

/// The end of the name of the asset `how` installs.
fn asset_suffix(how: &Method) -> &'static str {
    match how {
        Method::Folder(_) => "-windows-x64.zip",
        Method::Deb => "_amd64.deb",
        Method::Rpm => ".x86_64.rpm",
        Method::AppImage(_) => "-x86_64.AppImage",
        Method::Pacman => "-x86_64.pkg.tar.zst",
        // Only to tell whether the release has something for this system.
        Method::Manual if cfg!(windows) => "-windows-x64.zip",
        Method::Manual => "-x86_64.AppImage",
    }
}

fn latest_url() -> String {
    std::env::var("REC_ORDING_UPDATE_URL").unwrap_or_else(|_| format!("{DOWNLOADS}/latest.json"))
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

/// Reads the download page's `latest.json`; `None` when it is not newer than `current`:
/// `{"version", "notes", "page", "assets": [{"name", "url", "sha256"}]}`.
pub fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    parse_release_for(json, current, &method())
}

fn parse_release_for(json: &str, current: &str, how: &Method) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| tr!("Respuesta no válida: {}", e))?;
    let version = v["version"].as_str().ok_or(lz("La versión publicada no tiene número"))?;
    if !is_newer(version, current) {
        return Ok(None);
    }
    let asset = v["assets"].as_array().into_iter().flatten().find_map(|a| {
        let name = a["name"].as_str().filter(|n| n.ends_with(asset_suffix(how)))?;
        Some(Asset {
            name: name.to_string(),
            url: a["url"].as_str()?.to_string(),
            sha256: a["sha256"].as_str().map(str::to_lowercase),
        })
    });
    Ok(Some(Release {
        version: version.trim_start_matches(['v', 'V']).to_string(),
        notes: plain_text(v["notes"].as_str().unwrap_or_default()),
        page: v["page"].as_str().map(str::to_string).unwrap_or_else(|| format!("{DOWNLOADS}/")),
        asset,
    }))
}

/// The notes are Markdown; the app shows them as plain text: bold marks dropped and links
/// reduced to their text.
fn plain_text(md: &str) -> String {
    let mut out = String::new();
    let mut rest = md.replace("**", "");
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| after[close + 2..].find(')').map(|end| (close, close + 2 + end)));
        let Some((close, end)) = link else { break };
        out.push_str(&rest[..open]);
        out.push_str(&after[..close]);
        rest = after[end + 1..].to_string();
    }
    out.push_str(&rest);
    out.trim().to_string()
}

fn curl(url: &str, out: Option<&Path>, max_secs: u32) -> Result<Vec<u8>, String> {
    let mut c = command("curl");
    c.args(["-fsSL", "--connect-timeout", "10", "--max-time", &max_secs.to_string()])
        .args(["-H", "Accept: application/vnd.github+json", "-H", "User-Agent: rec-ording"])
        .arg(url);
    if let Some(out) = out {
        c.arg("-o").arg(out);
    }
    let o = c.output().map_err(|_| lz("No se encontró curl").to_string())?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        return Err(if err.is_empty() { lz("No se pudo conectar").to_string() } else { err });
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
    let asset = r.asset.as_ref().ok_or(lz("Esta versión no tiene paquete para tu sistema"))?;
    let dir = crate::model::cache_dir().join("actualizaciones");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&asset.name);
    curl(&asset.url, Some(&file), 900).map_err(|e| tr!("No se pudo descargar: {}", e))?;
    if let Some(expected) = &asset.sha256 {
        if &sha256_file(&file)? != expected {
            let _ = fs::remove_file(&file);
            return Err(lz("La descarga está dañada (no coincide la suma de comprobación)").into());
        }
    }
    Ok(file)
}

/// Downloads and installs `r`. The running app keeps working; the new version starts on restart.
pub fn install(r: &Release, how: &Method) -> Result<(), String> {
    let file = download(r)?;
    match how {
        Method::Pacman => pkexec(&["pacman", "-U", "--noconfirm"], &file, "pacman"),
        // apt resolves the dependencies of a local .deb when given its path.
        Method::Deb => pkexec(&["apt-get", "install", "-y", "--allow-downgrades"], &file, "apt"),
        Method::Rpm if Path::new("/usr/bin/zypper").exists() && !Path::new("/usr/bin/dnf").exists() => {
            pkexec(&["zypper", "--non-interactive", "install", "--allow-unsigned-rpm"], &file, "zypper")
        }
        Method::Rpm => pkexec(&["dnf", "install", "-y"], &file, "dnf"),
        Method::AppImage(target) => replace_appimage(&file, target),
        Method::Folder(exe) => {
            let unpacked = file.with_extension("");
            fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
            let ok = command("tar").arg("-xf").arg(&file).arg("-C").arg(&unpacked).status().map(|s| s.success()).unwrap_or(false);
            if !ok {
                return Err(lz("No se pudo descomprimir la actualización").into());
            }
            let exe_name = exe.file_name().ok_or(lz("Ruta del programa no válida"))?;
            let new_dir = find_dir_with(&unpacked, exe_name).ok_or(lz("La actualización no contiene rec-ording.exe"))?;
            replace_install(&new_dir, exe)
        }
        Method::Manual => Err(lz("Esta copia no se puede actualizar sola").into()),
    }
}

/// Installs a package file with the system's package manager, asking for the password.
fn pkexec(args: &[&str], file: &Path, tool: &str) -> Result<(), String> {
    let status = command("pkexec")
        .args(args)
        .arg(file)
        .status()
        .map_err(|_| lz("Falta pkexec (polkit) para instalar el paquete").to_string())?;
    match status.code() {
        Some(0) => Ok(()),
        // pkexec: the password dialog was dismissed or refused.
        Some(126) | Some(127) => Err(lz("Instalación cancelada").into()),
        _ => Err(tr!("{} no pudo instalar el paquete", tool)),
    }
}

/// Puts the downloaded AppImage in place of the running one (renaming over it is safe while it
/// runs; the new one starts on restart).
fn replace_appimage(new: &Path, target: &Path) -> Result<(), String> {
    let dir = target.parent().ok_or(lz("Ruta del programa no válida"))?;
    let tmp = dir.join(".rec-ording-nuevo.AppImage");
    fs::copy(new, &tmp).map_err(|e| tr!("No se puede escribir en {}: {}", dir.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        tr!("No se pudo reemplazar el AppImage: {}", e)
    })
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
    let dir = exe.parent().ok_or(lz("Ruta del programa no válida"))?;
    let new_exe = new_dir.join(exe.file_name().ok_or(lz("Ruta del programa no válida"))?);
    let old = old_exe(exe);
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| tr!("No se puede escribir en {}: {}", dir.display(), e))?;
    if let Err(e) = fs::copy(&new_exe, exe) {
        let _ = fs::rename(&old, exe);
        return Err(tr!("No se pudo copiar la versión nueva: {}", e));
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
                fs::copy(&p, &dest).map_err(|e| tr!("No se pudo copiar {}: {}", rel.display(), e))?;
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
    // On Linux the running executable shows up as deleted once the package manager replaces it.
    let exe = match method() {
        Method::Pacman | Method::Deb | Method::Rpm => PathBuf::from("/usr/bin/rec-ording"),
        Method::AppImage(file) => file,
        _ => {
        std::env::current_exe().map(|p| {
            let s = p.to_string_lossy();
            PathBuf::from(s.trim_end_matches(" (deleted)"))
        }).unwrap_or_else(|_| PathBuf::from("rec-ording"))
        }
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
    save_setting("omitir", serde_json::json!(version));
}

/// Whether the app looks for a new version when it opens (it does unless turned off).
pub fn auto_enabled() -> bool {
    let raw = fs::read_to_string(skip_path()).unwrap_or_default();
    auto_in(&raw)
}

pub fn set_auto(on: bool) {
    save_setting("auto", serde_json::json!(on));
}

fn auto_in(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw).ok().and_then(|v| v["auto"].as_bool()).unwrap_or(true)
}

/// A saved setting (updates, language…).
pub fn setting(key: &str) -> Option<serde_json::Value> {
    let raw = fs::read_to_string(skip_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(v.get(key)?.clone())
}

pub fn save_setting(key: &str, value: serde_json::Value) {
    let raw = fs::read_to_string(skip_path()).unwrap_or_default();
    let _ = fs::write(skip_path(), with_setting(&raw, key, value));
}

/// The settings file with `key` changed and the others kept.
fn with_setting(raw: &str, key: &str, value: serde_json::Value) -> String {
    let mut v = serde_json::from_str::<serde_json::Value>(raw).ok().filter(|v| v.is_object()).unwrap_or_else(|| serde_json::json!({}));
    v[key] = value;
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_updates_are_on_unless_turned_off() {
        assert!(auto_in("") && auto_in("no es json") && auto_in(r#"{"omitir":"1.0"}"#));
        let off = with_setting(r#"{"omitir":"1.0"}"#, "auto", serde_json::json!(false));
        assert!(!auto_in(&off));
        // Skipping a version keeps the setting, and the other way round.
        let skipped = with_setting(&off, "omitir", serde_json::json!("2.0"));
        let v: serde_json::Value = serde_json::from_str(&skipped).unwrap();
        assert_eq!((v["omitir"].as_str(), v["auto"].as_bool()), (Some("2.0"), Some(false)));
        assert!(auto_in(&with_setting("no es json", "auto", serde_json::json!(true))));
    }

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
    fn reads_the_download_page() {
        let json = serde_json::json!({
            "version": "0.8.0",
            "page": "https://e/",
            "notes": "**Novedades** de [Sagar Modi](https://x/y) y [más](https://z)\n",
            "assets": [
                {"name": "rec-ording-0.8.0-1-x86_64.pkg.tar.zst", "url": "https://e/arch", "sha256": "ABC"},
                {"name": "rec-ording_0.8.0-1_amd64.deb", "url": "https://e/deb"},
                {"name": "rec-ording-0.8.0-1.x86_64.rpm", "url": "https://e/rpm"},
                {"name": "rec-ording-0.8.0-x86_64.AppImage", "url": "https://e/appimage"},
                {"name": "rec-ording-0.8.0-windows-x64.zip", "url": "https://e/win"}
            ]
        })
        .to_string();
        let r = parse_release(&json, "0.7.0").unwrap().unwrap();
        assert_eq!(r.version, "0.8.0");
        assert_eq!(r.notes, "Novedades de Sagar Modi y más");
        let url = |how: Method| parse_release_for(&json, "0.7.0", &how).unwrap().unwrap().asset.unwrap().url;
        assert_eq!(url(Method::Pacman), "https://e/arch");
        assert_eq!(url(Method::Deb), "https://e/deb");
        assert_eq!(url(Method::Rpm), "https://e/rpm");
        assert_eq!(url(Method::AppImage(PathBuf::new())), "https://e/appimage");
        assert_eq!(url(Method::Folder(PathBuf::new())), "https://e/win");
        let arch = parse_release_for(&json, "0.7.0", &Method::Pacman).unwrap().unwrap().asset.unwrap();
        assert_eq!(arch.sha256.as_deref(), Some("abc"));
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

    /// REC_ORDING_UPDATE_URL=<url of a latest.json> cargo test downloads_updates -- --ignored
    #[test]
    #[ignore]
    fn downloads_updates() {
        let r = check().unwrap().expect("a newer release");
        let f = download(&r).unwrap();
        assert!(f.metadata().unwrap().len() > 0);
    }
}
