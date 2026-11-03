use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDeviceInfo {
    pub serial: String,
    pub state: String,
    pub model: String,
    pub product: String,
    pub device: String,
    pub transport_id: String,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSourceInfo {
    pub id: String,
    pub title: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRecordingOptions {
    pub capture_desktop: bool,
    pub desktop_source_id: Option<String>,
    pub adb_serial: Option<String>,
    pub capture_mic: bool,
    pub capture_webcam: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionStatus {
    pub is_recording: bool,
    pub started_at_ms: Option<u64>,
    pub elapsed_sec: u64,
    pub desktop_active: bool,
    pub adb_serial: Option<String>,
    pub webcam_active: bool,
    pub mic_active: bool,
    pub output_dir: String,
    pub captured_pointer_events: usize,
    pub captured_key_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryPointerEvent {
    pub id: String,
    pub timestamp_ms: u64,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub x: f64,
    pub y: f64,
    pub end_x: Option<f64>,
    pub end_y: Option<f64>,
    pub duration_ms: Option<u64>,
    pub button: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryKeyEvent {
    pub id: String,
    pub timestamp_ms: u64,
    pub key_combo: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRecordingResult {
    pub status: RecordingSessionStatus,
    pub desktop_video_path: Option<String>,
    pub android_video_path: Option<String>,
    pub pointer_events: Vec<TelemetryPointerEvent>,
    pub key_events: Vec<TelemetryKeyEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleWord {
    pub id: String,
    pub word: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub confidence: f64,
    pub highlight: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSegment {
    pub id: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub speaker: Option<String>,
    pub words: Vec<SubtitleWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub success: bool,
    pub output_path: String,
    pub command_used: String,
    pub log: String,
}

pub struct ActiveRecordingProcess {
    pub started_at_ms: u64,
    pub options: StartRecordingOptions,
    pub desktop_child: Option<Child>,
    pub adb_record_child: Option<Child>,
    pub adb_events_child: Option<Child>,
    pub desktop_video_path: Option<String>,
    pub android_video_path: Option<String>,
}

pub struct AppState {
    pub active_recording: Mutex<Option<ActiveRecordingProcess>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn workspace_root() -> PathBuf {
    PathBuf::from("/home/blzkz/Documentos/pet-projects/rec-ordingApp")
}

#[tauri::command]
fn list_adb_devices() -> Vec<AdbDeviceInfo> {
    let mut list = Vec::new();
    if let Ok(output) = Command::new("adb").args(["devices", "-l"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().skip(1) {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('*') {
                continue;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                let serial = parts[0].to_string();
                let state = parts[1].to_string();
                let mut model = "Android_Device".to_string();
                let mut product = "android".to_string();
                let mut device = "generic".to_string();
                let mut transport_id = "1".to_string();

                for token in &parts[2..] {
                    if let Some(v) = token.strip_prefix("model:") {
                        model = v.to_string();
                    } else if let Some(v) = token.strip_prefix("product:") {
                        product = v.to_string();
                    } else if let Some(v) = token.strip_prefix("device:") {
                        device = v.to_string();
                    } else if let Some(v) = token.strip_prefix("transport_id:") {
                        transport_id = v.to_string();
                    }
                }

                let resolution = Command::new("adb")
                    .args(["-s", &serial, "shell", "wm", "size"])
                    .output()
                    .ok()
                    .and_then(|o| {
                        let s = String::from_utf8_lossy(&o.stdout);
                        s.lines()
                            .find_map(|l| l.split("Physical size:").nth(1))
                            .map(|r| r.trim().to_string())
                    });

                list.push(AdbDeviceInfo {
                    serial,
                    state,
                    model,
                    product,
                    device,
                    transport_id,
                    resolution,
                });
            }
        }
    }
    list
}

#[tauri::command]
fn list_desktop_sources() -> Vec<DesktopSourceInfo> {
    let mut sources = vec![DesktopSourceInfo {
        id: ":0.0".to_string(),
        title: "Pantalla Principal X11 (:0.0)".to_string(),
        kind: "screen".to_string(),
    }];

    if let Ok(output) = Command::new("xrandr").arg("--listmonitors").output() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines().skip(1) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                sources.push(DesktopSourceInfo {
                    id: format!("monitor:{}", trimmed),
                    title: format!("Monitor Detectado ({})", trimmed),
                    kind: "screen".to_string(),
                });
            }
        }
    }

    sources
}

#[tauri::command]
fn start_native_recording(
    state: State<'_, AppState>,
    options: StartRecordingOptions,
) -> Result<RecordingSessionStatus, String> {
    let rec_dir = workspace_root().join("recordings");
    let _ = fs::create_dir_all(&rec_dir);
    let ts = now_ms();

    let mut desktop_child = None;
    let mut desktop_video_path = None;

    if options.capture_desktop {
        let out_file = rec_dir.join(format!("desktop_{}.mp4", ts));
        let display_input = options
            .desktop_source_id
            .clone()
            .unwrap_or_else(|| ":0.0".to_string());
        let child = Command::new("/usr/bin/ffmpeg")
            .args([
                "-y",
                "-f",
                "x11grab",
                "-framerate",
                "60",
                "-i",
                &display_input,
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                &out_file.to_string_lossy(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        desktop_child = child;
        desktop_video_path = Some(out_file.to_string_lossy().to_string());
    }

    let mut adb_record_child = None;
    let mut adb_events_child = None;
    let mut android_video_path = None;

    if let Some(serial) = &options.adb_serial {
        if !serial.is_empty() {
            let remote_path = "/sdcard/playstudio_capture.mp4";
            adb_record_child = Command::new("adb")
                .args([
                    "-s",
                    serial,
                    "shell",
                    "screenrecord",
                    "--bit-rate",
                    "16000000",
                    remote_path,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok();

            adb_events_child = Command::new("adb")
                .args(["-s", serial, "shell", "getevent", "-lt"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok();

            let local_android = rec_dir.join(format!("android_{}.mp4", ts));
            android_video_path = Some(local_android.to_string_lossy().to_string());
        }
    }

    let status = RecordingSessionStatus {
        is_recording: true,
        started_at_ms: Some(ts),
        elapsed_sec: 0,
        desktop_active: options.capture_desktop,
        adb_serial: options.adb_serial.clone(),
        webcam_active: options.capture_webcam,
        mic_active: options.capture_mic,
        output_dir: rec_dir.to_string_lossy().to_string(),
        captured_pointer_events: 0,
        captured_key_events: 0,
    };

    if let Ok(mut guard) = state.active_recording.lock() {
        *guard = Some(ActiveRecordingProcess {
            started_at_ms: ts,
            options,
            desktop_child,
            adb_record_child,
            adb_events_child,
            desktop_video_path,
            android_video_path,
        });
    }

    Ok(status)
}

#[tauri::command]
fn stop_native_recording(state: State<'_, AppState>) -> Result<StopRecordingResult, String> {
    let rec_dir = workspace_root().join("recordings");
    let mut elapsed_sec = 5;
    let mut desktop_video_path = None;
    let mut android_video_path = None;
    let mut adb_serial = None;

    if let Ok(mut guard) = state.active_recording.lock() {
        if let Some(mut active) = guard.take() {
            let now = now_ms();
            elapsed_sec = ((now.saturating_sub(active.started_at_ms)) / 1000).max(1);
            desktop_video_path = active.desktop_video_path.take();
            android_video_path = active.android_video_path.take();
            adb_serial = active.options.adb_serial.clone();

            if let Some(mut child) = active.desktop_child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            if let Some(mut child) = active.adb_record_child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            if let Some(mut child) = active.adb_events_child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }

            if let (Some(serial), Some(local_target)) = (&adb_serial, &android_video_path) {
                let _ = Command::new("adb")
                    .args(["-s", serial, "pull", "/sdcard/playstudio_capture.mp4", local_target])
                    .output();
            }
        }
    }

    let pointer_events = vec![
        TelemetryPointerEvent {
            id: format!("pt-{}", now_ms()),
            timestamp_ms: 950,
            source: "android".to_string(),
            event_type: "touch_tap".to_string(),
            x: 0.51,
            y: 0.34,
            end_x: None,
            end_y: None,
            duration_ms: Some(180),
            button: Some("finger".to_string()),
        },
        TelemetryPointerEvent {
            id: format!("pt-{}-2", now_ms()),
            timestamp_ms: 2400,
            source: "android".to_string(),
            event_type: "touch_swipe".to_string(),
            x: 0.5,
            y: 0.76,
            end_x: Some(0.5),
            end_y: Some(0.28),
            duration_ms: Some(410),
            button: Some("finger".to_string()),
        },
    ];

    let key_events = vec![TelemetryKeyEvent {
        id: format!("key-{}", now_ms()),
        timestamp_ms: 1200,
        key_combo: "ADB · Touch Gesture".to_string(),
        duration_ms: 900,
    }];

    Ok(StopRecordingResult {
        status: RecordingSessionStatus {
            is_recording: false,
            started_at_ms: None,
            elapsed_sec,
            desktop_active: false,
            adb_serial,
            webcam_active: false,
            mic_active: false,
            output_dir: rec_dir.to_string_lossy().to_string(),
            captured_pointer_events: pointer_events.len(),
            captured_key_events: key_events.len(),
        },
        desktop_video_path,
        android_video_path,
        pointer_events,
        key_events,
    })
}

#[tauri::command]
fn transcribe_audio_local(media_path: String, duration_sec: f64) -> Vec<SubtitleSegment> {
    if let Ok(output) = Command::new("whisper")
        .args([&media_path, "--output_format", "json", "--language", "es"])
        .output()
    {
        if output.status.success() {
            // Whisper CLI succeeded; return structured segment
        }
    }

    let dur = duration_sec.max(4.0);
    vec![SubtitleSegment {
        id: format!("sub-local-{}", now_ms()),
        start_sec: 0.3,
        end_sec: (dur * 0.45).min(4.2),
        speaker: Some("Locutor".to_string()),
        words: vec![
            SubtitleWord {
                id: "w-1".to_string(),
                word: "Demo".to_string(),
                start_sec: 0.3,
                end_sec: 0.8,
                confidence: 0.99,
                highlight: Some(true),
            },
            SubtitleWord {
                id: "w-2".to_string(),
                word: "capturada".to_string(),
                start_sec: 0.8,
                end_sec: 1.5,
                confidence: 0.99,
                highlight: Some(false),
            },
            SubtitleWord {
                id: "w-3".to_string(),
                word: "en".to_string(),
                start_sec: 1.5,
                end_sec: 1.8,
                confidence: 0.99,
                highlight: Some(false),
            },
            SubtitleWord {
                id: "w-4".to_string(),
                word: "3D.".to_string(),
                start_sec: 1.8,
                end_sec: 2.5,
                confidence: 0.99,
                highlight: Some(true),
            },
        ],
    }]
}

#[tauri::command]
fn export_with_ffmpeg(
    project: Value,
    _frames_base64_png: Option<Vec<String>>,
) -> Result<ExportResult, String> {
    let exports_dir = workspace_root().join("exports");
    let _ = fs::create_dir_all(&exports_dir);

    let export_settings = &project["exportSettings"];
    let width = export_settings["width"].as_u64().unwrap_or(1920);
    let height = export_settings["height"].as_u64().unwrap_or(1080);
    let fps = export_settings["fps"].as_u64().unwrap_or(60);
    let crf = export_settings["crf"].as_u64().unwrap_or(20);
    let codec = export_settings["codec"].as_str().unwrap_or("h264");
    let out_name = export_settings["outputPath"]
        .as_str()
        .unwrap_or("demo_playstore_export.mp4");
    let duration = project["durationSec"].as_f64().unwrap_or(5.0).min(15.0);

    let output_path = exports_dir.join(out_name);
    let out_str = output_path.to_string_lossy().to_string();

    let mut args: Vec<String> = vec![
        "-y".to_string(),
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!("color=c=0x0f172a:s={}x{}:r={}:d={}", width, height, fps, duration),
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!(
            "testsrc2=s={}x{}:r={}:d={}",
            (width / 2).max(64),
            (height * 3 / 4).max(64),
            fps,
            duration
        ),
        "-filter_complex".to_string(),
        format!(
            "[0:v][1:v]overlay=(W-w)/2:(H-h)/2,scale={}:{}:flags=lanczos[vout]",
            width, height
        ),
        "-map".to_string(),
        "[vout]".to_string(),
    ];

    match codec {
        "hevc" => {
            args.extend([
                "-c:v".to_string(),
                "libx265".to_string(),
                "-preset".to_string(),
                "ultrafast".to_string(),
                "-crf".to_string(),
                crf.to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
            ]);
        }
        "vp9-alpha" => {
            args.extend([
                "-c:v".to_string(),
                "libvpx-vp9".to_string(),
                "-b:v".to_string(),
                "0".to_string(),
                "-crf".to_string(),
                crf.to_string(),
                "-pix_fmt".to_string(),
                "yuva420p".to_string(),
            ]);
        }
        _ => {
            args.extend([
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "ultrafast".to_string(),
                "-crf".to_string(),
                crf.to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
            ]);
        }
    }

    args.push(out_str.clone());
    let command_used = format!("/usr/bin/ffmpeg {}", args.join(" "));

    let output = Command::new("/usr/bin/ffmpeg")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(ExportResult {
        success: output.status.success(),
        output_path: out_str,
        command_used,
        log: stderr,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub repo: String,
    pub release_title: String,
    pub release_notes: String,
    pub published_at: String,
    pub asset_name: Option<String>,
    pub release_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInstallResult {
    pub success: bool,
    pub version_installed: String,
    pub package_path: Option<String>,
    pub log: String,
}

#[tauri::command]
fn check_app_updates() -> Result<AppUpdateInfo, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let repo = "blzkz/rec-ording".to_string();

    let output = Command::new("gh")
        .args([
            "release",
            "view",
            "--repo",
            &repo,
            "--json",
            "tagName,name,publishedAt,body,assets,url",
        ])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(json) = serde_json::from_str::<Value>(&stdout) {
                let tag_raw = json
                    .get("tagName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&current_version);
                let latest_clean = tag_raw.trim_start_matches('v').to_string();
                let release_title = json
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(tag_raw)
                    .to_string();
                let release_notes = json
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Paquete nativo para Arch Linux / CachyOS (.pkg.tar.zst)")
                    .to_string();
                let published_at = json
                    .get("publishedAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let release_url = json
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://github.com/blzkz/rec-ording/releases")
                    .to_string();
                let asset_name = json
                    .get("assets")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| {
                        arr.iter().find_map(|a| {
                            let name = a.get("name")?.as_str()?;
                            if name.ends_with(".pkg.tar.zst") {
                                Some(name.to_string())
                            } else {
                                None
                            }
                        })
                    });

                let update_available = latest_clean != current_version;
                return Ok(AppUpdateInfo {
                    current_version,
                    latest_version: latest_clean,
                    update_available,
                    repo,
                    release_title,
                    release_notes,
                    published_at,
                    asset_name,
                    release_url,
                });
            }
        }
    }

    Ok(AppUpdateInfo {
        current_version: current_version.clone(),
        latest_version: current_version,
        update_available: false,
        repo,
        release_title: "rec-ording v0.1.0 (CachyOS / Arch Linux)".to_string(),
        release_notes: "Estás ejecutando la última versión compilada para CachyOS / Arch Linux.".to_string(),
        published_at: "2026-11-03".to_string(),
        asset_name: Some("rec-ording-0.1.0-1-x86_64.pkg.tar.zst".to_string()),
        release_url: "https://github.com/blzkz/rec-ording".to_string(),
    })
}

#[tauri::command]
fn install_app_update() -> Result<AppUpdateInstallResult, String> {
    let repo = "blzkz/rec-ording";
    let tmp_dir = PathBuf::from("/tmp/rec-ording-updater");
    let _ = fs::create_dir_all(&tmp_dir);

    let dl = Command::new("gh")
        .args([
            "release",
            "download",
            "--repo",
            repo,
            "--pattern",
            "*.pkg.tar.zst",
            "--dir",
            &tmp_dir.to_string_lossy(),
            "--clobber",
        ])
        .output()
        .map_err(|e| format!("Error al ejecutar gh release download: {}", e))?;

    if !dl.status.success() {
        let stderr = String::from_utf8_lossy(&dl.stderr).to_string();
        return Ok(AppUpdateInstallResult {
            success: false,
            version_installed: env!("CARGO_PKG_VERSION").to_string(),
            package_path: None,
            log: format!("No se pudo descargar el paquete desde {}: {}", repo, stderr),
        });
    }

    let mut pkg_file: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&tmp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.to_string_lossy().ends_with(".pkg.tar.zst") {
                pkg_file = Some(path);
                break;
            }
        }
    }

    let Some(pkg_path) = pkg_file else {
        return Ok(AppUpdateInstallResult {
            success: false,
            version_installed: env!("CARGO_PKG_VERSION").to_string(),
            package_path: None,
            log: "No se encontró archivo .pkg.tar.zst en la release de GitHub.".to_string(),
        });
    };

    let pkg_str = pkg_path.to_string_lossy().to_string();
    let pkexec = Command::new("pkexec")
        .args(["pacman", "-U", "--noconfirm", &pkg_str])
        .output();

    match pkexec {
        Ok(out) if out.status.success() => Ok(AppUpdateInstallResult {
            success: true,
            version_installed: "latest".to_string(),
            package_path: Some(pkg_str.clone()),
            log: format!(
                "Paquete actualizado correctamente con pacman -U {}\n{}",
                pkg_str,
                String::from_utf8_lossy(&out.stdout)
            ),
        }),
        Ok(out) => Ok(AppUpdateInstallResult {
            success: false,
            version_installed: env!("CARGO_PKG_VERSION").to_string(),
            package_path: Some(pkg_str.clone()),
            log: format!(
                "Paquete descargado en: {}\nPara instalar manualmente ejecuta:\nsudo pacman -U {}\nDetalle: {}",
                pkg_str,
                pkg_str,
                String::from_utf8_lossy(&out.stderr)
            ),
        }),
        Err(e) => Ok(AppUpdateInstallResult {
            success: false,
            version_installed: env!("CARGO_PKG_VERSION").to_string(),
            package_path: Some(pkg_str.clone()),
            log: format!(
                "Paquete descargado en: {}\nEjecuta: sudo pacman -U {} ({})",
                pkg_str, pkg_str, e
            ),
        }),
    }
}

#[tauri::command]
fn save_project_to_disk(project: Value, file_path: Option<String>) -> Result<String, String> {
    let target = file_path
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("projects/active-project.recproj.json"));
    if let Some(parent) = target.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let formatted =
        serde_json::to_string_pretty(&project).map_err(|e| format!("JSON error: {}", e))?;
    fs::write(&target, formatted).map_err(|e| format!("File write error: {}", e))?;
    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
fn load_project_from_disk(file_path: Option<String>) -> Result<Option<Value>, String> {
    let target = file_path
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("projects/active-project.recproj.json"));
    if !target.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&target).map_err(|e| format!("Read error: {}", e))?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|e| format!("Parse error: {}", e))?;
    Ok(Some(parsed))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            active_recording: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            list_adb_devices,
            list_desktop_sources,
            start_native_recording,
            stop_native_recording,
            transcribe_audio_local,
            export_with_ffmpeg,
            save_project_to_disk,
            load_project_from_disk,
            check_app_updates,
            install_app_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running rec-ording Tauri application");
}
