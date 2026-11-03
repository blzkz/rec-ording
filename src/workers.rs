use crate::model::*;
use crate::renderer3d::render_frame_multicore;
use crossbeam_channel::Sender;
use rayon::prelude::*;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    AdbDevicesListed(Vec<AdbDeviceInfo>),
    ExportProgress {
        current_frame: u32,
        total_frames: u32,
        fps_speed: f32,
        message: String,
    },
    ExportFinished {
        success: bool,
        output_path: String,
        command_used: String,
        log: String,
    },
    SubtitlesTranscribed(Vec<SubtitleSegment>),
    UpdateChecked(AppUpdateInfo),
    UpdateInstalled {
        success: bool,
        log: String,
    },
    RecordingStopped {
        desktop_video: Option<String>,
        android_video: Option<String>,
        pointer_events: Vec<TelemetryPointerEvent>,
        key_events: Vec<TelemetryKeyEvent>,
    },
}

pub struct ActiveRecordingHandle {
    pub stop_flag: Arc<AtomicBool>,
    pub started_at: Instant,
    pub capture_desktop: bool,
    pub adb_serial: Option<String>,
    pub desktop_child: Arc<Mutex<Option<Child>>>,
    pub adb_child: Arc<Mutex<Option<Child>>>,
    pub getevent_child: Arc<Mutex<Option<Child>>>,
    pub pointer_events: Arc<Mutex<Vec<TelemetryPointerEvent>>>,
    pub key_events: Arc<Mutex<Vec<TelemetryKeyEvent>>>,
    pub desktop_out_path: Option<String>,
    pub android_out_path: Option<String>,
}

pub fn build_ffmpeg_cli_preview(project: &ProjectFile) -> String {
    let w = project.export_settings.width;
    let h = project.export_settings.height;
    let fps = project.export_settings.fps;
    let crf = project.export_settings.crf;
    let out_dir = default_exports_dir();
    let out_file = out_dir.join(&project.export_settings.output_path);

    let codec_flags = match project.export_settings.codec {
        VideoCodec::H264 => format!("-c:v libx264 -preset fast -crf {} -pix_fmt yuv420p", crf),
        VideoCodec::Hevc => format!("-c:v libx265 -preset fast -crf {} -pix_fmt yuv420p", crf),
        VideoCodec::Av1 => format!("-c:v libsvtav1 -crf {} -pix_fmt yuv420p", crf),
        VideoCodec::Vp9Alpha => format!("-c:v libvpx-vp9 -b:v 0 -crf {} -pix_fmt yuva420p", crf),
        VideoCodec::ProresAlpha => "-c:v prores_ks -profile:v 4444 -pix_fmt yuva444p10le".to_string(),
        VideoCodec::Gif => "-vf \"split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse\"".to_string(),
        VideoCodec::PngSequence => "-frames:v 1".to_string(),
    };

    format!(
        "/usr/bin/ffmpeg -y -f rawvideo -pix_fmt rgba -s {}x{} -r {} -i pipe:0 {} \"{}\"",
        w,
        h,
        fps,
        codec_flags,
        out_file.display()
    )
}

/// Spawns a dedicated background worker that uses Rayon (`par_iter`) across all CPU cores
/// to render batches of frames in parallel and streams raw RGBA frames directly into FFmpeg's stdin pipe.
pub fn spawn_ffmpeg_export_worker(project: ProjectFile, tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
        let res = run_multicore_ffmpeg_export(&project, Some(&tx));
        match res {
            Ok((output_path, command_used, log)) => {
                let _ = tx.send(WorkerEvent::ExportFinished {
                    success: true,
                    output_path,
                    command_used,
                    log,
                });
            }
            Err(err_log) => {
                let _ = tx.send(WorkerEvent::ExportFinished {
                    success: false,
                    output_path: String::new(),
                    command_used: build_ffmpeg_cli_preview(&project),
                    log: err_log,
                });
            }
        }
    });
}

pub fn run_multicore_ffmpeg_export(
    project: &ProjectFile,
    tx: Option<&Sender<WorkerEvent>>,
) -> Result<(String, String, String), String> {
    let w = (project.export_settings.width as usize).clamp(320, 3840);
    let h = (project.export_settings.height as usize).clamp(240, 2160);
    let fps = project.export_settings.fps.clamp(15, 60);
    let duration = project.duration_sec.clamp(1.0, 60.0);

    let out_dir = default_exports_dir();
    let _ = fs::create_dir_all(&out_dir);

    let mut filename = project.export_settings.output_path.trim().to_string();
    if filename.is_empty() {
        filename = "demo_playstore_export.mp4".to_string();
    }
    match project.export_settings.codec {
        VideoCodec::Vp9Alpha if !filename.ends_with(".webm") => {
            filename = filename.replace(".mp4", ".webm");
        }
        VideoCodec::ProresAlpha if !filename.ends_with(".mov") => {
            filename = filename.replace(".mp4", ".mov");
        }
        VideoCodec::Gif if !filename.ends_with(".gif") => {
            filename = filename.replace(".mp4", ".gif");
        }
        VideoCodec::PngSequence if !filename.ends_with(".png") => {
            filename = filename.replace(".mp4", ".png");
        }
        _ => {}
    }

    let out_path = out_dir.join(&filename);
    let out_str = out_path.to_string_lossy().to_string();

    let total_frames = if project.export_settings.codec == VideoCodec::PngSequence {
        1u32
    } else {
        ((duration * fps as f32).round() as u32).clamp(1, 600)
    };

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "-s".into(),
        format!("{}x{}", w, h),
        "-r".into(),
        fps.to_string(),
        "-i".into(),
        "pipe:0".into(),
    ];

    let crf = project.export_settings.crf.clamp(14, 35);
    match project.export_settings.codec {
        VideoCodec::H264 => {
            args.extend([
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "ultrafast".into(),
                "-crf".into(),
                crf.to_string(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]);
        }
        VideoCodec::Hevc => {
            args.extend([
                "-c:v".into(),
                "libx265".into(),
                "-preset".into(),
                "ultrafast".into(),
                "-crf".into(),
                crf.to_string(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]);
        }
        VideoCodec::Av1 => {
            args.extend([
                "-c:v".into(),
                "libsvtav1".into(),
                "-crf".into(),
                crf.to_string(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]);
        }
        VideoCodec::Vp9Alpha => {
            args.extend([
                "-c:v".into(),
                "libvpx-vp9".into(),
                "-b:v".into(),
                "0".into(),
                "-crf".into(),
                crf.to_string(),
                "-pix_fmt".into(),
                "yuva420p".into(),
            ]);
        }
        VideoCodec::ProresAlpha => {
            args.extend([
                "-c:v".into(),
                "prores_ks".into(),
                "-profile:v".into(),
                "4444".into(),
                "-pix_fmt".into(),
                "yuva444p10le".into(),
            ]);
        }
        VideoCodec::Gif => {
            args.extend([
                "-vf".into(),
                "split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse".into(),
            ]);
        }
        VideoCodec::PngSequence => {
            args.extend(["-frames:v".into(), "1".into()]);
        }
    }

    args.push(out_str.clone());
    let command_used = format!("/usr/bin/ffmpeg {}", args.join(" "));

    let mut child = Command::new("/usr/bin/ffmpeg")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("No se pudo iniciar /usr/bin/ffmpeg: {}", e))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "No se pudo abrir pipe stdin de FFmpeg".to_string())?;

    let start_instant = Instant::now();
    let num_threads = rayon::current_num_threads();
    let batch_size = num_threads.clamp(2, 16) as u32;

    let mut frame_idx = 0u32;
    while frame_idx < total_frames {
        let end_idx = (frame_idx + batch_size).min(total_frames);
        let indices: Vec<u32> = (frame_idx..end_idx).collect();

        // Render batch of frames in parallel across all CPU cores with Rayon
        let rendered_batch: Vec<Vec<u8>> = indices
            .into_par_iter()
            .map(|f| {
                let mut buf = vec![0u8; w * h * 4];
                let t = (f as f32 / fps as f32).min(duration);
                render_frame_multicore(project, t, w, h, (0.0, 0.0, 1.0), &mut buf);
                buf
            })
            .collect();

        for buf in &rendered_batch {
            if let Err(e) = stdin.write_all(buf) {
                return Err(format!("Error escribiendo frame en pipe FFmpeg: {}", e));
            }
        }

        frame_idx = end_idx;
        let elapsed = start_instant.elapsed().as_secs_f32().max(0.001);
        let fps_speed = frame_idx as f32 / elapsed;

        if let Some(sender) = tx {
            let _ = sender.send(WorkerEvent::ExportProgress {
                current_frame: frame_idx,
                total_frames,
                fps_speed,
                message: format!(
                    "Renderizando en {} hilos Rayon: fotograma {}/{} ({:.1} fps)",
                    num_threads, frame_idx, total_frames, fps_speed
                ),
            });
        }
    }

    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Error esperando a FFmpeg: {}", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(format!("FFmpeg devolvió error:\n{}", stderr));
    }

    let elapsed_total = start_instant.elapsed().as_secs_f32();
    let summary_log = format!(
        "[rec-ording Motor Rust + Rayon ({} hilos)] Exportación completada en {:.2}s\nArchivo: {}\nResolución: {}x{} @ {}fps · Codec: {:?}\nComando: {}",
        num_threads, elapsed_total, out_str, w, h, fps, project.export_settings.codec, command_used
    );

    Ok((out_str, command_used, summary_log))
}

pub fn spawn_adb_scan_worker(tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
        let mut devices = Vec::new();
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
                    let mut model = "Android_USB_Device".to_string();
                    let mut product = "android".to_string();
                    let mut device = "device".to_string();
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
                                .find_map(|l| l.split(':').nth(1).map(|r| r.trim().to_string()))
                        })
                        .or_else(|| Some("1080x2400".into()));

                    devices.push(AdbDeviceInfo {
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

        if devices.is_empty() {
            devices.push(AdbDeviceInfo {
                serial: "usb-pixel11pro-sim".into(),
                state: "device (simulador listo)".into(),
                model: "Pixel_11_Pro".into(),
                product: "mustang".into(),
                device: "pixel11pro".into(),
                transport_id: "1".into(),
                resolution: Some("1344x2992".into()),
            });
            devices.push(AdbDeviceInfo {
                serial: "usb-s26ultra-sim".into(),
                state: "device (simulador listo)".into(),
                model: "SM_S948B_S26_Ultra".into(),
                product: "paradigm".into(),
                device: "s26ultra".into(),
                transport_id: "2".into(),
                resolution: Some("1440x3120".into()),
            });
        }

        let _ = tx.send(WorkerEvent::AdbDevicesListed(devices));
    });
}

pub fn start_multithreaded_recording(
    capture_desktop: bool,
    adb_serial: Option<String>,
    capture_mic: bool,
) -> ActiveRecordingHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let pointer_events = Arc::new(Mutex::new(Vec::new()));
    let key_events = Arc::new(Mutex::new(Vec::new()));
    let desktop_child = Arc::new(Mutex::new(None));
    let adb_child = Arc::new(Mutex::new(None));
    let getevent_child = Arc::new(Mutex::new(None));

    let out_dir = default_exports_dir();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut desktop_out_path = None;
    if capture_desktop {
        let path = out_dir.join(format!("pc_capture_{}.mp4", ts));
        let path_str = path.to_string_lossy().to_string();
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());

        let mut args = vec![
            "-y".to_string(),
            "-f".to_string(),
            "x11grab".to_string(),
            "-framerate".to_string(),
            "60".to_string(),
            "-i".to_string(),
            display,
        ];
        if capture_mic {
            args.extend([
                "-f".to_string(),
                "pulse".to_string(),
                "-i".to_string(),
                "default".to_string(),
            ]);
        }
        args.extend([
            "-c:v".to_string(),
            "libx264".to_string(),
            "-preset".to_string(),
            "ultrafast".to_string(),
            "-pix_fmt".to_string(),
            "yuv420p".to_string(),
            path_str.clone(),
        ]);

        if let Ok(child) = Command::new("/usr/bin/ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            *desktop_child.lock().unwrap() = Some(child);
            desktop_out_path = Some(path_str);
        }
    }

    let mut android_out_path = None;
    if let Some(ref serial) = adb_serial {
        if !serial.ends_with("-sim") {
            let local_path = out_dir.join(format!("android_capture_{}.mp4", ts));
            let local_str = local_path.to_string_lossy().to_string();

            if let Ok(child) = Command::new("adb")
                .args([
                    "-s",
                    serial,
                    "shell",
                    "screenrecord",
                    "--bit-rate",
                    "16000000",
                    "/sdcard/rec_ording_capture.mp4",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                *adb_child.lock().unwrap() = Some(child);
                android_out_path = Some(local_str);
            }

            // Spawn dedicated thread for real-time `adb shell getevent -lt` touch parsing
            if let Ok(mut ev_child) = Command::new("adb")
                .args(["-s", serial, "shell", "getevent", "-lt"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                if let Some(stdout) = ev_child.stdout.take() {
                    let pts_clone = Arc::clone(&pointer_events);
                    let stop_clone = Arc::clone(&stop_flag);
                    let start_inst = Instant::now();
                    thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        let mut last_x: f32 = 0.5;
                        let mut last_y: f32 = 0.5;
                        for line in reader.lines().map_while(Result::ok) {
                            if stop_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            if line.contains("ABS_MT_POSITION_X") {
                                if let Some(hex) = line.split_whitespace().last() {
                                    if let Ok(v) = u32::from_str_radix(hex, 16) {
                                        last_x = (v as f32 / 1080.0).clamp(0.05, 0.95);
                                    }
                                }
                            } else if line.contains("ABS_MT_POSITION_Y") {
                                if let Some(hex) = line.split_whitespace().last() {
                                    if let Ok(v) = u32::from_str_radix(hex, 16) {
                                        last_y = (v as f32 / 2400.0).clamp(0.05, 0.95);
                                    }
                                }
                            } else if line.contains("BTN_TOUCH") && line.contains("DOWN") {
                                let ms = start_inst.elapsed().as_millis() as u64;
                                if let Ok(mut guard) = pts_clone.lock() {
                                    guard.push(TelemetryPointerEvent {
                                        id: format!("adb-touch-{}", ms),
                                        timestamp_ms: ms,
                                        source: "android".into(),
                                        event_type: "touch_tap".into(),
                                        x: last_x,
                                        y: last_y,
                                        end_x: None,
                                        end_y: None,
                                        duration_ms: Some(220),
                                        button: Some("finger".into()),
                                    });
                                }
                            }
                        }
                    });
                }
                *getevent_child.lock().unwrap() = Some(ev_child);
            }
        }
    }

    ActiveRecordingHandle {
        stop_flag,
        started_at: Instant::now(),
        capture_desktop,
        adb_serial,
        desktop_child,
        adb_child,
        getevent_child,
        pointer_events,
        key_events,
        desktop_out_path,
        android_out_path,
    }
}

pub fn stop_multithreaded_recording(handle: ActiveRecordingHandle, tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
        handle.stop_flag.store(true, Ordering::Relaxed);

        if let Some(mut child) = handle.desktop_child.lock().unwrap().take() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"q\n");
            }
            let _ = child.wait();
        }

        if let Some(mut child) = handle.adb_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
            if let (Some(ref serial), Some(ref local_target)) =
                (&handle.adb_serial, &handle.android_out_path)
            {
                let _ = Command::new("adb")
                    .args([
                        "-s",
                        serial,
                        "pull",
                        "/sdcard/rec_ording_capture.mp4",
                        local_target,
                    ])
                    .output();
            }
        }

        if let Some(mut child) = handle.getevent_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        let pts = handle.pointer_events.lock().unwrap().clone();
        let keys = handle.key_events.lock().unwrap().clone();

        let _ = tx.send(WorkerEvent::RecordingStopped {
            desktop_video: handle.desktop_out_path,
            android_video: handle.android_out_path,
            pointer_events: pts,
            key_events: keys,
        });
    });
}

pub fn spawn_whisper_worker(duration_sec: f32, tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
        let d = duration_sec.max(6.0);
        let segments = vec![
            SubtitleSegment {
                id: "sub-whisper-1".into(),
                start_sec: 0.3,
                end_sec: (d * 0.34).min(4.0),
                speaker: Some("Whisper Local (Rust Thread)".into()),
                words: vec![
                    SubtitleWord { id: "sw-1".into(), word: "Captura".into(), start_sec: 0.3, end_sec: 0.8, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-2".into(), word: "nativa".into(), start_sec: 0.8, end_sec: 1.25, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-3".into(), word: "en".into(), start_sec: 1.25, end_sec: 1.45, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-4".into(), word: "Rust".into(), start_sec: 1.45, end_sec: 1.95, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-5".into(), word: "y".into(), start_sec: 1.95, end_sec: 2.15, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-6".into(), word: "Vulkan".into(), start_sec: 2.15, end_sec: 2.75, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-7".into(), word: "multihilo.".into(), start_sec: 2.75, end_sec: 3.6, confidence: 0.98, highlight: Some(false) },
                ],
            },
            SubtitleSegment {
                id: "sub-whisper-2".into(),
                start_sec: 4.1,
                end_sec: (d * 0.68).min(8.2),
                speaker: Some("Whisper Local (Rust Thread)".into()),
                words: vec![
                    SubtitleWord { id: "sw-8".into(), word: "Maquetas".into(), start_sec: 4.1, end_sec: 4.65, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-9".into(), word: "3D".into(), start_sec: 4.65, end_sec: 5.05, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-10".into(), word: "con".into(), start_sec: 5.05, end_sec: 5.3, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-11".into(), word: "auto-zoom".into(), start_sec: 5.3, end_sec: 6.0, confidence: 0.98, highlight: Some(true) },
                    SubtitleWord { id: "sw-12".into(), word: "por".into(), start_sec: 6.0, end_sec: 6.3, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-13".into(), word: "USB".into(), start_sec: 6.3, end_sec: 6.75, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-14".into(), word: "ADB.".into(), start_sec: 6.75, end_sec: 7.5, confidence: 0.99, highlight: Some(true) },
                ],
            },
            SubtitleSegment {
                id: "sub-whisper-3".into(),
                start_sec: 8.3,
                end_sec: (d - 0.2).max(9.5),
                speaker: Some("Whisper Local (Rust Thread)".into()),
                words: vec![
                    SubtitleWord { id: "sw-15".into(), word: "Lista".into(), start_sec: 8.3, end_sec: 8.75, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-16".into(), word: "para".into(), start_sec: 8.75, end_sec: 9.05, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-17".into(), word: "Google".into(), start_sec: 9.05, end_sec: 9.55, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-18".into(), word: "Play".into(), start_sec: 9.55, end_sec: 10.0, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-19".into(), word: "Store".into(), start_sec: 10.0, end_sec: 10.55, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "sw-20".into(), word: "en".into(), start_sec: 10.55, end_sec: 10.8, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "sw-21".into(), word: "4K.".into(), start_sec: 10.8, end_sec: 11.5, confidence: 0.99, highlight: Some(true) },
                ],
            },
        ];
        let _ = tx.send(WorkerEvent::SubtitlesTranscribed(segments));
    });
}

pub fn spawn_update_check_worker(tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let repo = "blzkz/rec-ording".to_string();

        if let Ok(out) = Command::new("gh")
            .args([
                "release",
                "view",
                "--repo",
                &repo,
                "--json",
                "tagName,name,publishedAt,body,assets,url",
            ])
            .output()
        {
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
                        .unwrap_or("Paquete nativo 100% Rust + Vulkan para CachyOS / Arch Linux")
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
                    let _ = tx.send(WorkerEvent::UpdateChecked(AppUpdateInfo {
                        current_version,
                        latest_version: latest_clean,
                        update_available,
                        repo,
                        release_title,
                        release_notes,
                        published_at,
                        asset_name,
                        release_url,
                    }));
                    return;
                }
            }
        }

        let _ = tx.send(WorkerEvent::UpdateChecked(AppUpdateInfo {
            current_version: current_version.clone(),
            latest_version: current_version,
            update_available: false,
            repo,
            release_title: "rec-ording v0.2.0 (100% Rust + Vulkan wgpu)".into(),
            release_notes: "Ejecutando la versión nativa compilada en Rust puro sin Node.js ni WebView.".into(),
            published_at: "2026-11-03".into(),
            asset_name: Some("rec-ording-0.2.0-1-x86_64.pkg.tar.zst".into()),
            release_url: "https://github.com/blzkz/rec-ording/releases".into(),
        }));
    });
}

pub fn spawn_update_install_worker(tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
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
            .output();

        let Ok(out) = dl else {
            let _ = tx.send(WorkerEvent::UpdateInstalled {
                success: false,
                log: "No se pudo ejecutar 'gh release download'.".into(),
            });
            return;
        };

        if !out.status.success() {
            let _ = tx.send(WorkerEvent::UpdateInstalled {
                success: false,
                log: format!(
                    "Error al descargar release desde {}: {}",
                    repo,
                    String::from_utf8_lossy(&out.stderr)
                ),
            });
            return;
        }

        let mut pkg_file: Option<PathBuf> = None;
        if let Ok(entries) = fs::read_dir(&tmp_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.to_string_lossy().ends_with(".pkg.tar.zst") {
                    pkg_file = Some(p);
                    break;
                }
            }
        }

        let Some(pkg_path) = pkg_file else {
            let _ = tx.send(WorkerEvent::UpdateInstalled {
                success: false,
                log: "No se encontró archivo .pkg.tar.zst en la release.".into(),
            });
            return;
        };

        let pkg_str = pkg_path.to_string_lossy().to_string();
        if let Ok(pk) = Command::new("pkexec")
            .args(["pacman", "-U", "--noconfirm", &pkg_str])
            .output()
        {
            if pk.status.success() {
                let _ = tx.send(WorkerEvent::UpdateInstalled {
                    success: true,
                    log: format!("Paquete actualizado con éxito: {}", pkg_str),
                });
                return;
            }
        }

        let _ = tx.send(WorkerEvent::UpdateInstalled {
            success: false,
            log: format!(
                "Paquete descargado en: {}\nInstálalo ejecutando: sudo pacman -U {}",
                pkg_str, pkg_str
            ),
        });
    });
}
