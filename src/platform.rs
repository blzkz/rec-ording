//! The few things that differ between Linux and Windows: running tools without a console
//! window, user folders, opening files, file dialogs, the local time and install hints.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A command for an external tool (ffmpeg, adb, scrcpy…). On Windows it runs without flashing
/// a console window; it also finds tools placed next to `rec-ording.exe`.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Where user data goes unless `XDG_DATA_HOME` says otherwise.
pub fn data_home() -> PathBuf {
    if cfg!(windows) {
        if let Some(d) = std::env::var_os("APPDATA") {
            return PathBuf::from(d);
        }
    }
    home().join(".local").join("share")
}

/// Where caches go unless `XDG_CACHE_HOME` says otherwise.
pub fn cache_home() -> PathBuf {
    if cfg!(windows) {
        if let Some(d) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(d);
        }
    }
    home().join(".cache")
}

/// The user's videos folder (localized through xdg-user-dir on Linux).
pub fn videos_home() -> PathBuf {
    command("xdg-user-dir")
        .arg("VIDEOS")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty() && Path::new(s).is_dir())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("Videos"))
}

/// Opens a file or folder with the desktop's default app.
pub fn open_path(path: &Path) {
    let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = command(opener).arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Local time for file names, e.g. `2026-11-03 14.32.10`.
#[cfg(windows)]
pub fn local_timestamp() -> Option<String> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime only writes the struct it is given.
    let t = unsafe {
        let mut t: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    Some(format!("{:04}-{:02}-{:02} {:02}.{:02}.{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond))
}

#[cfg(not(windows))]
pub fn local_timestamp() -> Option<String> {
    command("date")
        .arg("+%Y-%m-%d %H.%M.%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Asks for a file with the desktop's dialog. `exts` are lowercase extensions without the dot.
/// `Err` means no dialog is available, `Ok(None)` that the user cancelled.
#[cfg(windows)]
pub fn pick_file(title: &str, filter: &str, exts: &[&str], start: &Path) -> Result<Option<PathBuf>, ()> {
    Ok(rfd::FileDialog::new().set_title(title).add_filter(filter, exts).set_directory(start).pick_file())
}

#[cfg(windows)]
pub fn pick_folder(title: &str, start: &Path) -> Result<Option<PathBuf>, ()> {
    Ok(rfd::FileDialog::new().set_title(title).set_directory(start).pick_folder())
}

#[cfg(not(windows))]
pub fn pick_file(title: &str, filter: &str, exts: &[&str], start: &Path) -> Result<Option<PathBuf>, ()> {
    let globs: Vec<String> = exts.iter().flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_uppercase())]).collect();
    let globs = globs.join(" ");
    run_picker(
        &[
            "--file-selection".into(),
            format!("--title={title}"),
            format!("--filename={}/", start.display()),
            format!("--file-filter={filter} | {globs}"),
            "--file-filter=Todos los archivos | *".into(),
        ],
        &["--getopenfilename".into(), start.display().to_string(), format!("{filter} ({globs})"), format!("--title={title}")],
    )
}

#[cfg(not(windows))]
pub fn pick_folder(title: &str, start: &Path) -> Result<Option<PathBuf>, ()> {
    run_picker(
        &["--file-selection".into(), "--directory".into(), format!("--title={title}"), format!("--filename={}/", start.display())],
        &["--getexistingdirectory".into(), start.display().to_string(), format!("--title={title}")],
    )
}

/// Runs kdialog on KDE, zenity elsewhere (falling back to the other one).
#[cfg(not(windows))]
fn run_picker(zenity: &[String], kdialog: &[String]) -> Result<Option<PathBuf>, ()> {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").map(|d| d.to_uppercase().contains("KDE")).unwrap_or(false);
    let mut tries = vec![("zenity", zenity), ("kdialog", kdialog)];
    if kde {
        tries.reverse();
    }
    for (cmd, args) in tries {
        if let Ok(out) = command(cmd).args(args).output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((out.status.success() && !s.is_empty()).then(|| PathBuf::from(s)));
        }
    }
    Err(())
}

/// How to install missing tools, named as Arch packages: "ffmpeg", "android-tools", "scrcpy".
pub fn install_hint(packages: &[&str]) -> String {
    if cfg!(windows) {
        let lines: Vec<String> = packages
            .iter()
            .map(|p| match *p {
                "ffmpeg" => "Gyan.FFmpeg",
                "android-tools" => "Google.PlatformTools",
                "scrcpy" => "Genymobile.scrcpy",
                other => other,
            })
            .map(|id| format!("winget install {id}"))
            .collect();
        lines.join("\n")
    } else {
        format!("sudo pacman -S {}", packages.join(" "))
    }
}
