use crate::model::*;
use crate::workers::run_multicore_ffmpeg_export;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

pub fn run_stdio_mcp_server() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(req) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

        // Notifications do not have an id and do not expect a response
        if id.is_none() {
            continue;
        }
        let req_id = id.unwrap();

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "rec-ording-rust-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            "resources/list" => {
                let path = default_project_path();
                json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "resources": [
                            {
                                "uri": format!("file://{}", path.display()),
                                "name": "Active rec-ording Project (.recproj.json)",
                                "mimeType": "application/json"
                            }
                        ]
                    }
                })
            }
            "resources/read" => {
                let proj = load_or_create_project();
                let text = serde_json::to_string_pretty(&proj).unwrap_or_default();
                let path = default_project_path();
                json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "contents": [
                            {
                                "uri": format!("file://{}", path.display()),
                                "mimeType": "application/json",
                                "text": text
                            }
                        ]
                    }
                })
            }
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": mcp_tools_list()
                }
            }),
            "tools/call" => {
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let result = handle_tool_call(tool_name, &args);
                json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": result
                })
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                }
            }),
        };

        if let Ok(serialized) = serde_json::to_string(&response) {
            let _ = writeln!(stdout, "{}", serialized);
            let _ = stdout.flush();
        }
    }
}

fn mcp_tools_list() -> Value {
    json!([
        {
            "name": "get_project_summary",
            "description": "Devuelve el estado completo del proyecto activo de rec-ording (pistas V1..V4, A1..A4, T1..T5, maqueta 3D, cámara Director, máscaras de privacidad, subtítulos y exportación).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "import_recording_clip",
            "description": "Importa un clip de vídeo o audio en una pista de la línea de tiempo (video-track-1..4 o audio-track-1..4).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trackId": { "type": "string" },
                    "name": { "type": "string" },
                    "mediaPath": { "type": "string" },
                    "startSec": { "type": "number" },
                    "durationSec": { "type": "number" }
                },
                "required": ["trackId", "name", "mediaPath"]
            }
        },
        {
            "name": "edit_clip",
            "description": "Edita recorte, velocidad (0.25..4.0), volumen o congelado de imagen en un clip existente.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clipId": { "type": "string" },
                    "speed": { "type": "number" },
                    "volume": { "type": "number" },
                    "startSec": { "type": "number" },
                    "durationSec": { "type": "number" }
                },
                "required": ["clipId"]
            }
        },
        {
            "name": "set_3d_mockup",
            "description": "Cambia la maqueta 3D (pixel-11-pro, galaxy-s26-ultra, galaxy-tab-s10-ultra, desktop-window, custom-glb), modo dual y orientación.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "primaryModel": { "type": "string" },
                    "dualLayout": { "type": "string" },
                    "orientation": { "type": "string" },
                    "motionBlurEnabled": { "type": "boolean" }
                }
            }
        },
        {
            "name": "plan_director_camera",
            "description": "Genera automáticamente fotogramas clave de cámara y zoom desde los toques registrados o añade un fotograma clave manual.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "autoFromTouches": { "type": "boolean" },
                    "timeSec": { "type": "number" },
                    "zoom": { "type": "number" },
                    "focusX": { "type": "number" },
                    "focusY": { "type": "number" },
                    "rotXDeg": { "type": "number" },
                    "rotYDeg": { "type": "number" }
                }
            }
        },
        {
            "name": "add_privacy_mask",
            "description": "Añade una zona de difuminado (blur), pixelado o tapado sólido sobre datos sensibles.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "label": { "type": "string" },
                    "startSec": { "type": "number" },
                    "endSec": { "type": "number" },
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "width": { "type": "number" },
                    "height": { "type": "number" }
                }
            }
        },
        {
            "name": "add_animated_title",
            "description": "Añade un título animado a las pistas T1..T5.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trackId": { "type": "string" },
                    "text": { "type": "string" },
                    "subtitleText": { "type": "string" },
                    "startSec": { "type": "number" },
                    "durationSec": { "type": "number" }
                },
                "required": ["text"]
            }
        },
        {
            "name": "update_subtitles",
            "description": "Actualiza los subtítulos palabra por palabra del proyecto.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" }
                }
            }
        },
        {
            "name": "render_export_ffmpeg",
            "description": "Ejecuta el motor de renderizado 3D multihilo en Rust + FFmpeg para exportar el vídeo.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "outputPath": { "type": "string" },
                    "width": { "type": "number" },
                    "height": { "type": "number" },
                    "fps": { "type": "number" }
                }
            }
        }
    ])
}

fn handle_tool_call(name: &str, args: &Value) -> Value {
    let mut project = load_or_create_project();

    match name {
        "get_project_summary" => {
            let summary = serde_json::to_string_pretty(&project).unwrap_or_default();
            json!({
                "content": [{ "type": "text", "text": summary }]
            })
        }
        "import_recording_clip" => {
            let track_id = args
                .get("trackId")
                .and_then(|v| v.as_str())
                .unwrap_or("video-track-1");
            let clip_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Grabacion_Importada.mp4");
            let media_path = args
                .get("mediaPath")
                .and_then(|v| v.as_str())
                .unwrap_or("builtin://android-demo");
            let start_sec = args.get("startSec").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let duration_sec = args
                .get("durationSec")
                .and_then(|v| v.as_f64())
                .unwrap_or(8.0) as f32;

            if let Some(track) = project.tracks.iter_mut().find(|t| t.id == track_id) {
                track.clips.push(TimelineClip {
                    id: format!("clip-mcp-{}", track.clips.len() + 1),
                    track_id: track.id.clone(),
                    name: clip_name.into(),
                    clip_type: track.kind.clone(),
                    media_path: Some(media_path.into()),
                    source_role: Some("android-main".into()),
                    start_sec,
                    duration_sec,
                    trim_in_sec: 0.0,
                    trim_out_sec: duration_sec,
                    speed: 1.0,
                    volume: 1.0,
                    muted: false,
                    fade_in_sec: 0.2,
                    fade_out_sec: 0.2,
                    freeze_frames: vec![],
                    opacity_keyframes: vec![],
                    scale_keyframes: vec![],
                    text: None,
                    subtitle_text: None,
                    title_preset: None,
                    font_size: None,
                    text_color: None,
                    bg_color: None,
                    pos_x: None,
                    pos_y: None,
                });
            }
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": format!("Clip '{}' importado en pista {}", clip_name, track_id) }]
            })
        }
        "edit_clip" => {
            let clip_id = args.get("clipId").and_then(|v| v.as_str()).unwrap_or("");
            let mut found = false;
            for track in &mut project.tracks {
                if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                    if let Some(s) = args.get("speed").and_then(|v| v.as_f64()) {
                        clip.speed = (s as f32).clamp(0.25, 4.0);
                    }
                    if let Some(vol) = args.get("volume").and_then(|v| v.as_f64()) {
                        clip.volume = (vol as f32).clamp(0.0, 2.0);
                    }
                    if let Some(st) = args.get("startSec").and_then(|v| v.as_f64()) {
                        clip.start_sec = (st as f32).max(0.0);
                    }
                    if let Some(dur) = args.get("durationSec").and_then(|v| v.as_f64()) {
                        clip.duration_sec = (dur as f32).max(0.2);
                    }
                    found = true;
                }
            }
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": format!("Clip '{}' actualizado (encontrado: {})", clip_id, found) }]
            })
        }
        "set_3d_mockup" => {
            if let Some(m) = args.get("primaryModel").and_then(|v| v.as_str()) {
                project.device_3d.primary_model = match m {
                    "galaxy-s26-ultra" => DeviceModelType::GalaxyS26Ultra,
                    "galaxy-tab-s10-ultra" => DeviceModelType::GalaxyTabS10Ultra,
                    "desktop-window" => DeviceModelType::DesktopWindow,
                    "custom-glb" => DeviceModelType::CustomGlb,
                    "none" => DeviceModelType::None,
                    _ => DeviceModelType::Pixel11Pro,
                };
            }
            if let Some(l) = args.get("dualLayout").and_then(|v| v.as_str()) {
                project.device_3d.dual_layout = match l {
                    "side-by-side" => DualLayout::SideBySide,
                    "pip-floating" => DualLayout::PipFloating,
                    "angled-showcase" => DualLayout::AngledShowcase,
                    _ => DualLayout::Single,
                };
            }
            if let Some(o) = args.get("orientation").and_then(|v| v.as_str()) {
                project.device_3d.orientation = if o == "landscape" {
                    Orientation::Landscape
                } else {
                    Orientation::Portrait
                };
            }
            if let Some(mb) = args.get("motionBlurEnabled").and_then(|v| v.as_bool()) {
                project.background.motion_blur_enabled = mb;
            }
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": "Configuración 3D actualizada correctamente en active-project.recproj.json" }]
            })
        }
        "plan_director_camera" => {
            let auto = args
                .get("autoFromTouches")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if auto {
                let mut kfs = Vec::new();
                for (i, ev) in project.pointer_events.iter().enumerate() {
                    let t = (ev.timestamp_ms as f32 / 1000.0 - 0.2).max(0.0);
                    kfs.push(DirectorCameraKeyframe {
                        id: format!("dir-mcp-{}", i + 1),
                        time_sec: t,
                        duration_sec: 1.6,
                        focus_x: ev.x,
                        focus_y: ev.y,
                        zoom: 1.5,
                        rot_x_deg: (0.5 - ev.y) * 14.0,
                        rot_y_deg: (ev.x - 0.5) * 22.0,
                        rot_z_deg: 0.0,
                        easing: "spring".into(),
                        label: Some(format!("MCP Auto-Zoom #{}", i + 1)),
                        auto_generated: Some(true),
                    });
                }
                if !kfs.is_empty() {
                    project.director_keyframes = kfs;
                }
            }
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": format!("Cámara Director planificada con {} fotogramas clave.", project.director_keyframes.len()) }]
            })
        }
        "add_privacy_mask" => {
            let label = args
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("Zona Sensible MCP");
            let start_sec = args.get("startSec").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let end_sec = args.get("endSec").and_then(|v| v.as_f64()).unwrap_or(6.0) as f32;
            let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.35) as f32;
            let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.25) as f32;
            let width = args.get("width").and_then(|v| v.as_f64()).unwrap_or(0.30) as f32;
            let height = args.get("height").and_then(|v| v.as_f64()).unwrap_or(0.10) as f32;

            project.privacy_masks.push(PrivacyMask {
                id: format!("mask-mcp-{}", project.privacy_masks.len() + 1),
                label: label.into(),
                start_sec,
                end_sec,
                x,
                y,
                width,
                height,
                mode: PrivacyMaskMode::Blur,
                intensity: 18,
                color: Some("#0f172a".into()),
            });
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": format!("Máscara de privacidad '{}' añadida.", label) }]
            })
        }
        "add_animated_title" => {
            let track_id = args
                .get("trackId")
                .and_then(|v| v.as_str())
                .unwrap_or("text-track-1");
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("Nuevo Título Animado");
            let sub = args
                .get("subtitleText")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let start_sec = args.get("startSec").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let duration_sec = args
                .get("durationSec")
                .and_then(|v| v.as_f64())
                .unwrap_or(3.5) as f32;

            if let Some(track) = project.tracks.iter_mut().find(|t| t.id == track_id) {
                track.clips.push(TimelineClip {
                    id: format!("title-mcp-{}", track.clips.len() + 1),
                    track_id: track.id.clone(),
                    name: format!("Título: {}", text),
                    clip_type: "text".into(),
                    media_path: None,
                    source_role: None,
                    start_sec,
                    duration_sec,
                    trim_in_sec: 0.0,
                    trim_out_sec: duration_sec,
                    speed: 1.0,
                    volume: 1.0,
                    muted: false,
                    fade_in_sec: 0.25,
                    fade_out_sec: 0.25,
                    freeze_frames: vec![],
                    opacity_keyframes: vec![],
                    scale_keyframes: vec![],
                    text: Some(text.into()),
                    subtitle_text: Some(sub.into()),
                    title_preset: Some("pop-elastic".into()),
                    font_size: Some(40.0),
                    text_color: Some("#ffffff".into()),
                    bg_color: Some("#1e1b4b".into()),
                    pos_x: Some(0.5),
                    pos_y: Some(0.12),
                });
            }
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": format!("Título '{}' añadido en {}", text, track_id) }]
            })
        }
        "update_subtitles" => {
            if let Some(en) = args.get("enabled").and_then(|v| v.as_bool()) {
                project.subtitle_style.enabled = en;
            }
            let _ = save_project(&project);
            json!({
                "content": [{ "type": "text", "text": "Subtítulos actualizados en el proyecto." }]
            })
        }
        "render_export_ffmpeg" => {
            if let Some(out) = args.get("outputPath").and_then(|v| v.as_str()) {
                project.export_settings.output_path = out.into();
            }
            if let Some(w) = args.get("width").and_then(|v| v.as_u64()) {
                project.export_settings.width = w as u32;
            }
            if let Some(h) = args.get("height").and_then(|v| v.as_u64()) {
                project.export_settings.height = h as u32;
            }
            if let Some(fps) = args.get("fps").and_then(|v| v.as_u64()) {
                project.export_settings.fps = fps as u32;
            }
            let _ = save_project(&project);
            match run_multicore_ffmpeg_export(&project, None) {
                Ok((path, _cmd, log)) => json!({
                    "content": [{ "type": "text", "text": format!("Exportación completada: {}\n{}", path, log) }]
                }),
                Err(e) => json!({
                    "isError": true,
                    "content": [{ "type": "text", "text": e }]
                }),
            }
        }
        _ => json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("Herramienta desconocida: {}", name) }]
        }),
    }
}
