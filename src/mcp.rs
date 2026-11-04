//! MCP server: `rec-ording --mcp` lets an agent (Claude, Cursor, Codex…) build the video. It
//! speaks JSON-RPC 2.0 over stdio and edits the project that is open in the app through the
//! same operations as the interface; the app notices the saved changes and reloads them.

use crate::capture::{self, CaptureResult, Recording};
use crate::compositor::{compose, ComposeInput, Frame, Mips};
use crate::export::{self, ExportEvent};
use crate::library;
use crate::media;
use crate::model::*;
use crate::model3d::{self, PreparedModel};
use crate::timeline::{self, DevicePreset};
use crossbeam_channel::{unbounded, Receiver};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use crate::platform::command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

enum Reply {
    Text(String),
    Image { png: Vec<u8>, caption: String },
}

type ToolResult = Result<Reply, String>;

struct ExportJob {
    rx: Receiver<ExportEvent>,
    cancel: Arc<AtomicBool>,
    status: String,
    /// What encodes it (graphics card or CPU).
    encoder: String,
    done: bool,
}

pub struct Server {
    undo: Vec<Project>,
    redo: Vec<Project>,
    recording: Option<Recording>,
    export: Option<ExportJob>,
    model_cache: Option<(CustomModel, Arc<PreparedModel>)>,
}

pub fn run() {
    let mut server = Server { undo: Vec::new(), redo: Vec::new(), recording: None, export: None, model_cache: None };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("JSON no válido: {e}")}})),
        };
        if let Some(r) = response {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
    if let Some(rec) = server.recording.take() {
        let _ = rec.stop_blocking();
    }
}

impl Server {
    /// Answers one JSON-RPC message (notifications get no answer).
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?.clone();
        let method = msg["method"].as_str().unwrap_or("");
        let params = &msg["params"];
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "rec-ording", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.call(name, &args) {
                    Ok(Reply::Text(t)) => json!({"content": [{"type": "text", "text": t}], "isError": false}),
                    Ok(Reply::Image { png, caption }) => json!({"content": [
                        {"type": "image", "data": base64(&png), "mimeType": "image/png"},
                        {"type": "text", "text": caption}
                    ], "isError": false}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            _ => Err(json!({"code": -32601, "message": format!("Método desconocido: {method}")})),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        })
    }

    /// Loads the open project, applies `f` and saves it (one undo step) if it changed.
    fn edit(&mut self, f: impl FnOnce(&mut Project) -> Result<String, String>) -> ToolResult {
        let path = current_project_path();
        let before = load_project(&path)?;
        let mut p = before.clone();
        let msg = f(&mut p)?;
        if p != before {
            save_project_to(&path, &mut p)?;
            self.undo.push(before);
            if self.undo.len() > 50 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        Ok(Reply::Text(msg))
    }

    fn read(&self) -> Result<Project, String> {
        load_project(&current_project_path())
    }

    fn prepared_model(&mut self, p: &Project) -> Result<Option<Arc<PreparedModel>>, String> {
        let Some(cm) = p.style.model.clone().filter(|_| p.style.frame == FrameStyle::Custom) else { return Ok(None) };
        if let Some((k, m)) = &self.model_cache {
            if k.path == cm.path && k.screen_part == cm.screen_part && k.quarter_turns == cm.quarter_turns {
                return Ok(Some(m.clone()));
            }
        }
        let src = model3d::load(&cm.path)?;
        let part = cm.screen(src.guess, src.parts.len()).ok_or("No se encontró la pantalla del modelo")?;
        let m = Arc::new(model3d::prepare(&src, part, cm.quarter_turns)?);
        self.model_cache = Some((cm, m.clone()));
        Ok(Some(m))
    }

    fn call(&mut self, name: &str, a: &Value) -> ToolResult {
        match name {
            // ----- Projects -----
            "get_project" => Ok(Reply::Text(summary(&self.read()?, &current_project_path()).to_string())),
            "list_projects" => {
                let current = current_project_path();
                let list: Vec<Value> = list_projects()
                    .into_iter()
                    .map(|e| json!({"name": e.name, "file": e.path.file_name().map(|f| f.to_string_lossy().into_owned()), "open": e.path == current}))
                    .collect();
                Ok(Reply::Text(Value::Array(list).to_string()))
            }
            "create_project" => {
                let name = text(a, "name")?;
                let template = self.read().ok();
                let (path, _) = create_project(&name, template.as_ref())?;
                set_current_project(&path);
                self.undo.clear();
                self.redo.clear();
                Ok(Reply::Text(format!("Proyecto «{name}» creado y abierto")))
            }
            "open_project" => {
                let name = text(a, "name")?;
                let entry = list_projects()
                    .into_iter()
                    .find(|e| e.name == name || e.path.file_name().is_some_and(|f| f.to_string_lossy() == name))
                    .ok_or(format!("No hay ningún proyecto «{name}»"))?;
                set_current_project(&entry.path);
                self.undo.clear();
                self.redo.clear();
                Ok(Reply::Text(format!("Proyecto «{}» abierto", entry.name)))
            }
            "rename_project" => {
                let name = text(a, "name")?;
                self.edit(|p| {
                    p.name = name.clone();
                    Ok(format!("Proyecto renombrado a «{name}»"))
                })
            }
            "undo" | "redo" => {
                let path = current_project_path();
                let now = load_project(&path)?;
                let (from, to) = if name == "undo" { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
                let mut prev = from.pop().ok_or("No hay nada que deshacer o rehacer en esta sesión")?;
                prev.revision = now.revision;
                save_project_to(&path, &mut prev)?;
                to.push(now);
                Ok(Reply::Text(if name == "undo" { "Deshecho".into() } else { "Rehecho".into() }))
            }

            // ----- Recording and media -----
            "list_android_devices" => {
                let devs: Vec<Value> = capture::list_devices()
                    .into_iter()
                    .map(|d| json!({"serial": d.serial, "model": d.model, "ready": d.state == capture::DeviceState::Ready}))
                    .collect();
                Ok(Reply::Text(Value::Array(devs).to_string()))
            }
            "start_recording" => {
                if self.recording.is_some() {
                    return Err("Ya hay una grabación en marcha".into());
                }
                let serial = match a["serial"].as_str() {
                    Some(s) => s.to_string(),
                    None => capture::list_devices()
                        .into_iter()
                        .find(|d| d.state == capture::DeviceState::Ready)
                        .map(|d| d.serial)
                        .ok_or("No hay ningún móvil Android conectado y autorizado")?,
                };
                let opts = capture::RecordOptions { audio: a["audio"].as_bool().unwrap_or(true), mirror: a["mirror"].as_bool().unwrap_or(false) };
                self.recording = Some(capture::start(&serial, &opts, capture::detect_tools())?);
                Ok(Reply::Text(format!("Grabando {serial}. Llama a stop_recording para terminar.")))
            }
            "stop_recording" => {
                let rec = self.recording.take().ok_or("No hay ninguna grabación en marcha")?;
                match rec.stop_blocking() {
                    CaptureResult::Done { path, touches } => {
                        let info = media::probe(&path.to_string_lossy())?;
                        let n = touches.len();
                        self.edit(move |p| {
                            let id = timeline::append_media(p, info, touches);
                            let scale = p.style.auto_zoom_scale;
                            let zooms = if n > 0 { timeline::regenerate_auto_zooms(p, scale) } else { 0 };
                            Ok(format!("Grabación añadida como clip {id} ({n} toques, {zooms} zooms automáticos)"))
                        })
                    }
                    CaptureResult::Failed(e) => Err(e),
                }
            }
            "import_video" => {
                let path = text(a, "path")?;
                let info = media::probe(&path)?;
                self.edit(move |p| {
                    let id = timeline::append_media(p, info, Vec::new());
                    Ok(format!("Vídeo añadido al final como clip {id}"))
                })
            }
            "split_clip" => {
                let t = num(a, "time")?;
                self.edit(|p| timeline::split_at(p, t).map(|id| format!("Dividido; el clip nuevo es {id}")))
            }
            "trim_clip" => {
                let id = int(a, "clip_id")?;
                let (i, o) = (opt_num(a, "source_in"), opt_num(a, "source_out"));
                self.edit(|p| timeline::trim_clip(p, id, i, o).map(|_| "Clip recortado".into()))
            }
            "move_clip" => {
                let (id, index) = (int(a, "clip_id")?, int(a, "index")? as usize);
                self.edit(|p| timeline::move_clip(p, id, index).map(|_| "Clip movido".into()))
            }
            "delete_clip" => {
                let id = int(a, "clip_id")?;
                self.edit(|p| timeline::delete_clip(p, id).map(|_| "Clip eliminado".into()))
            }

            // ----- Camera -----
            "auto_zoom" => {
                let scale = opt_num(a, "zoom");
                self.edit(|p| {
                    let s = scale.unwrap_or(p.style.auto_zoom_scale).clamp(1.1, 4.0);
                    p.style.auto_zoom_scale = s;
                    let n = timeline::regenerate_auto_zooms(p, s);
                    Ok(format!("{n} zooms automáticos a partir de los toques"))
                })
            }
            "add_zoom" => {
                let (start, end) = (num(a, "start")?, num(a, "end")?);
                let (zoom, x, y) = (opt_num(a, "zoom").unwrap_or(2.0), opt_num(a, "x").unwrap_or(0.5), opt_num(a, "y").unwrap_or(0.5));
                self.edit(|p| timeline::add_zoom(p, start, end, zoom, x, y).map(|id| format!("Zoom creado (clip de cámara {id})")))
            }
            "add_camera_keyframe" => {
                let t = num(a, "time")?;
                let a = a.clone();
                self.edit(move |p| {
                    let (id, i) = timeline::add_camera_key(p, t, |k| apply_cam_fields(k, &a))?;
                    Ok(format!("Fotograma clave {i} del clip de cámara {id}"))
                })
            }
            "update_camera_keyframe" => {
                let (id, index) = (int(a, "clip_id")?, int(a, "index")? as usize);
                let a = a.clone();
                self.edit(move |p| {
                    let c = p.camera.iter_mut().find(|c| c.id == id).ok_or(format!("No hay ningún clip de cámara {id}"))?;
                    let k = c.keys.get_mut(index).ok_or("Ese fotograma clave no existe")?;
                    apply_cam_fields(k, &a);
                    if let Some(t) = opt_num(&a, "time") {
                        k.t = t - c.start;
                        timeline::normalize_camera_clip(c);
                    }
                    Ok("Fotograma clave actualizado".into())
                })
            }
            "delete_camera_keyframe" => {
                let (id, index) = (int(a, "clip_id")?, int(a, "index")? as usize);
                self.edit(|p| timeline::delete_camera_key(p, id, index).map(|_| "Fotograma clave eliminado".into()))
            }
            "delete_camera_clip" => {
                let id = int(a, "clip_id")?;
                self.edit(|p| timeline::delete_camera_clip(p, id).map(|_| "Clip de cámara eliminado".into()))
            }
            "move_camera_clip" => {
                let (id, start) = (int(a, "clip_id")?, num(a, "start")?);
                self.edit(|p| timeline::move_camera_clip(p, id, start).map(|_| "Clip de cámara movido".into()))
            }

            // ----- Director -----
            "director_add_target" => {
                let (t, x0, y0, x1, y1) = (num(a, "time")?, num(a, "x0")?, num(a, "y0")?, num(a, "x1")?, num(a, "y1")?);
                self.edit(|p| timeline::add_target(p, t, x0, y0, x1, y1).map(|id| format!("Objetivo {id} en {t:.2} s")))
            }
            "director_remove_target" => {
                let id = int(a, "target_id")?;
                self.edit(|p| timeline::remove_target(p, id).map(|_| "Objetivo eliminado".into()))
            }
            "director_clear" => self.edit(|p| {
                p.director.targets.clear();
                p.camera.retain(|c| c.kind != CameraKind::Director);
                Ok("Objetivos y planos del Director eliminados".into())
            }),
            "director_settings" => {
                let a = a.clone();
                self.edit(move |p| {
                    if let Some(m) = a["motion"].as_str() {
                        p.director.motion = match m {
                            "2d" | "2D" => DirectorMotion::TwoD,
                            "3d" | "3D" => DirectorMotion::ThreeD,
                            _ => return Err("motion debe ser \"3d\" o \"2d\"".into()),
                        };
                    }
                    if let Some(i) = opt_num(&a, "intensity") {
                        p.director.intensity = i.clamp(0.0, 1.0);
                    }
                    Ok("Ajustes del Director cambiados".into())
                })
            }
            "director_retake" => {
                let id = a["target_id"].as_u64();
                let model = self.read().and_then(|p| self.prepared_model(&p))?;
                self.edit(move |p| {
                    timeline::retake(p, id);
                    let (w, h) = p.export.dimensions(p.style.aspect);
                    let (n, replaced) = timeline::director_generate(p, w as f32, h as f32, model.as_deref().map(|m| &m.meta))?;
                    Ok(format!("{n} planos generados de nuevo{}", replaced_note(replaced)))
                })
            }
            "director_generate" => {
                let model = self.read().and_then(|p| self.prepared_model(&p))?;
                self.edit(move |p| {
                    let (w, h) = p.export.dimensions(p.style.aspect);
                    let (n, replaced) = timeline::director_generate(p, w as f32, h as f32, model.as_deref().map(|m| &m.meta))?;
                    Ok(format!("{n} planos generados{}", replaced_note(replaced)))
                })
            }

            // ----- Device -----
            "list_device_options" => {
                let frames: Vec<&str> = FRAME_KEYS.iter().map(|(k, _)| *k).collect();
                let models: Vec<Value> = library::list()
                    .into_iter()
                    .map(|m| json!({"name": m.name, "path": m.path, "license": m.license, "author": m.author}))
                    .collect();
                Ok(Reply::Text(json!({"frames": frames, "frame_colors": COLOR_KEYS.iter().map(|(k, _)| *k).collect::<Vec<_>>(), "models": models, "motion_presets": DevicePreset::ALL.iter().map(|p| p.key()).collect::<Vec<_>>()}).to_string()))
            }
            "set_device" => {
                let a = a.clone();
                self.edit(move |p| {
                    if let Some(f) = a["frame"].as_str() {
                        p.style.frame = FRAME_KEYS.iter().find(|(k, _)| *k == f).map(|(_, v)| *v).ok_or(format!("Marco desconocido: {f}"))?;
                    }
                    if let Some(c) = a["frame_color"].as_str() {
                        p.style.frame_color = COLOR_KEYS.iter().find(|(k, _)| *k == c).map(|(_, v)| *v).ok_or(format!("Color desconocido: {c}"))?;
                    }
                    if let Some(m) = a["model"].as_str() {
                        let entry = library::list()
                            .into_iter()
                            .find(|e| e.name == m || e.path == m)
                            .ok_or(format!("No hay ningún modelo «{m}» en la galería (usa import_model)"))?;
                        let src = model3d::load(&entry.path)?;
                        p.style.model = Some(CustomModel { path: entry.path, animation: (!src.animations.is_empty()).then_some(0), ..Default::default() });
                        p.style.frame = FrameStyle::Custom;
                    }
                    Ok("Dispositivo cambiado".into())
                })
            }
            "import_model" => {
                let path = PathBuf::from(text(a, "path")?);
                let (dest, src) = library::import_any(&path)?;
                let dest = dest.to_string_lossy().into_owned();
                let anims = src.animations.len();
                self.edit(move |p| {
                    p.style.model = Some(CustomModel { path: dest.clone(), animation: (anims > 0).then_some(0), ..Default::default() });
                    p.style.frame = FrameStyle::Custom;
                    Ok(format!("Modelo importado a la galería y en uso ({anims} animaciones)"))
                })
            }
            "set_model_options" => {
                let a = a.clone();
                self.edit(move |p| {
                    let cm = p.style.model.as_mut().filter(|_| p.style.frame == FrameStyle::Custom).ok_or("No hay ningún modelo 3D en uso")?;
                    match &a["screen_part"] {
                        Value::String(s) if s == "auto" => cm.screen_part = None,
                        Value::Number(n) => cm.screen_part = n.as_u64().map(|v| v as usize),
                        _ => {}
                    }
                    if let Some(q) = a["quarter_turns"].as_u64() {
                        cm.quarter_turns = (q % 4) as u8;
                    }
                    match &a["animation"] {
                        Value::Null => {}
                        Value::String(s) if s == "none" => cm.animation = None,
                        v => cm.animation = v.as_u64().map(|v| v as usize),
                    }
                    if let Some(l) = a["loop"].as_bool() {
                        cm.loop_animation = l;
                    }
                    if let Some(t) = opt_num(&a, "animation_start") {
                        cm.animation_start = t.max(0.0);
                    }
                    if let Some(f) = a["video_fit"].as_str() {
                        p.style.video_fit = match f {
                            "fill" => VideoFit::Fill,
                            "fit" => VideoFit::Fit,
                            "stretch" => VideoFit::Stretch,
                            _ => return Err("video_fit debe ser fill, fit o stretch".into()),
                        };
                    }
                    if let Some(v) = opt_num(&a, "fit_pan") {
                        p.style.fit_pan = v.clamp(0.0, 1.0);
                    }
                    Ok("Ajustes del modelo cambiados".into())
                })
            }
            "set_angle" => {
                let a = a.clone();
                self.edit(move |p| {
                    let s = &mut p.style;
                    if let Some(v) = opt_num(&a, "tilt") {
                        s.tilt = v.clamp(-60.0, 60.0);
                    }
                    if let Some(v) = opt_num(&a, "turn") {
                        s.turn = v.clamp(-80.0, 80.0);
                    }
                    if let Some(v) = opt_num(&a, "roll") {
                        s.roll = v.clamp(-45.0, 45.0);
                    }
                    if let Some(v) = a["straighten_on_zoom"].as_bool() {
                        s.straighten_on_zoom = v;
                    }
                    if let Some(v) = a["float"].as_bool() {
                        s.float_motion = v;
                    }
                    if let Some(v) = a["intro"].as_bool() {
                        s.intro = v;
                    }
                    Ok("Ángulo cambiado".into())
                })
            }
            "add_device_motion" => {
                let key = text(a, "preset")?;
                let preset = DevicePreset::from_key(&key).ok_or(format!("Movimiento desconocido: {key}"))?;
                let t = num(a, "time")?;
                let (degrees, secs) = (opt_num(a, "degrees"), opt_num(a, "duration"));
                self.edit(move |p| {
                    let id = timeline::add_device_preset(p, preset, t);
                    let c = p.device.iter_mut().find(|c| c.id == id).ok_or("No se pudo añadir")?;
                    if let Some(d) = degrees {
                        if timeline::spin_of(c).is_none() {
                            return Err("degrees solo vale para full_turn y flip".into());
                        }
                        timeline::set_spin(c, d);
                    }
                    if let Some(s) = secs {
                        timeline::set_device_duration(c, s);
                    }
                    Ok(format!("Movimiento «{}» añadido de {:.2} a {:.2} s (clip {id})", preset.label(), c.start, c.end()))
                })
            }
            "delete_device_motion" => {
                let id = int(a, "clip_id")?;
                self.edit(|p| timeline::delete_device_clip(p, id).map(|_| "Movimiento eliminado".into()))
            }

            // ----- Titles -----
            "add_text" => {
                let a = a.clone();
                let t = num(&a, "time")?;
                let title = text(&a, "title")?;
                self.edit(move |p| {
                    let id = timeline::add_text(p, t, &title)?;
                    let c = p.texts.iter_mut().find(|c| c.id == id).ok_or("No se pudo añadir")?;
                    set_text_fields(c, &a)?;
                    let start = c.start;
                    if let Some(d) = opt_num(&a, "duration") {
                        timeline::place_text(p, id, start, start + d)?;
                    }
                    let c = p.texts.iter().find(|c| c.id == id).ok_or("No se pudo añadir")?;
                    Ok(format!("Texto añadido de {:.2} a {:.2} s (texto {id})", c.start, c.end()))
                })
            }
            "update_text" => {
                let a = a.clone();
                let id = int(&a, "text_id")?;
                self.edit(move |p| {
                    let c = p.texts.iter_mut().find(|c| c.id == id).ok_or(format!("No hay ningún texto {id}"))?;
                    set_text_fields(c, &a)?;
                    if let Some(v) = opt_str(&a, "title") {
                        c.title = v;
                    }
                    let start = opt_num(&a, "start").unwrap_or(c.start);
                    let end = start + opt_num(&a, "duration").unwrap_or(c.duration);
                    timeline::place_text(p, id, start, end)?;
                    Ok("Texto actualizado".into())
                })
            }
            "delete_text" => {
                let id = int(a, "text_id")?;
                self.edit(|p| timeline::delete_text(p, id).map(|_| "Texto eliminado".into()))
            }

            // ----- Look -----
            "set_style" => {
                let a = a.clone();
                self.edit(move |p| {
                    let s = &mut p.style;
                    if let Some(v) = a["aspect"].as_str() {
                        s.aspect = Aspect::ALL.into_iter().find(|x| x.label() == v).ok_or("aspect debe ser 16:9, 9:16, 1:1 o 4:5")?;
                    }
                    if let Some(v) = a["background_preset"].as_str() {
                        let b = BACKGROUND_PRESETS.iter().find(|b| b.name.eq_ignore_ascii_case(v)).ok_or(format!("Fondo desconocido: {v}"))?;
                        s.background = Background { a: b.a, b: b.b, angle: b.angle };
                    }
                    if let Some(v) = a["background_a"].as_str() {
                        s.background.a = hex(v)?;
                    }
                    if let Some(v) = a["background_b"].as_str() {
                        s.background.b = hex(v)?;
                    }
                    if let Some(v) = opt_num(&a, "background_angle") {
                        s.background.angle = v;
                    }
                    if let Some(v) = opt_num(&a, "padding") {
                        s.padding = v.clamp(0.02, 0.25);
                    }
                    if let Some(v) = opt_num(&a, "shadow") {
                        s.shadow = v.clamp(0.0, 1.0);
                    }
                    if let Some(v) = a["show_touches"].as_bool() {
                        s.show_touches = v;
                    }
                    if let Some(v) = opt_num(&a, "touch_size") {
                        s.touch_size = v.clamp(0.5, 2.0);
                    }
                    Ok("Estilo cambiado".into())
                })
            }

            // ----- Seeing and exporting -----
            "render_frame" => {
                let p = self.read()?;
                let t = num(a, "time")?.clamp(0.0, timeline::duration(&p));
                let width = a["width"].as_u64().unwrap_or(960).clamp(160, 1920) as usize;
                let model = self.prepared_model(&p)?;
                let png = render_png(&p, t, width, model.as_deref())?;
                Ok(Reply::Image { png, caption: format!("Fotograma en {t:.2} s") })
            }
            "export" => {
                if self.export.as_ref().is_some_and(|e| !e.done) {
                    return Err("Ya hay una exportación en marcha (mira export_status)".into());
                }
                let a = a.clone();
                let mut p = self.read()?;
                if p.is_empty() {
                    return Err("El proyecto no tiene vídeo".into());
                }
                if let Some(v) = a["resolution"].as_str() {
                    p.export.resolution = Resolution::ALL.into_iter().find(|r| r.label().eq_ignore_ascii_case(v)).ok_or("resolution debe ser 720p, 1080p o 4K")?;
                }
                if let Some(v) = a["fps"].as_u64() {
                    p.export.fps = if v >= 45 { 60 } else { 30 };
                }
                if let Some(v) = a["format"].as_str() {
                    p.export.format = match v {
                        "h264" => ExportFormat::H264,
                        "hevc" => ExportFormat::Hevc,
                        "gif" => ExportFormat::Gif,
                        _ => return Err("format debe ser h264, hevc o gif".into()),
                    };
                }
                if let Some(v) = a["quality"].as_str() {
                    p.export.quality = match v {
                        "standard" => Quality::Standard,
                        "high" => Quality::High,
                        "max" => Quality::Max,
                        _ => return Err("quality debe ser standard, high o max".into()),
                    };
                }
                if let Some(v) = a["hardware"].as_bool() {
                    p.export.hardware = v;
                }
                if let Some(v) = a["include_audio"].as_bool() {
                    p.export.include_audio = v;
                }
                if let Some(v) = a["folder"].as_str() {
                    if !Path::new(v).is_dir() {
                        return Err(format!("La carpeta {v} no existe"));
                    }
                    p.export.folder = v.to_string();
                }
                let _ = std::fs::create_dir_all(&p.export.folder);
                let (tx, rx) = unbounded();
                let cancel = Arc::new(AtomicBool::new(false));
                export::start(p, cancel.clone(), tx);
                self.export = Some(ExportJob { rx, cancel, status: "Exportando… 0 %".into(), encoder: String::new(), done: false });
                Ok(Reply::Text("Exportación iniciada; consulta export_status".into()))
            }
            "export_status" => {
                let job = self.export.as_mut().ok_or("No se ha iniciado ninguna exportación")?;
                while let Ok(ev) = job.rx.try_recv() {
                    match ev {
                        ExportEvent::Progress { fraction, .. } => job.status = format!("Exportando… {:.0} % ({})", fraction * 100.0, job.encoder),
                        ExportEvent::Encoder(name) => job.encoder = name,
                        ExportEvent::Done(path) => {
                            job.status = format!("Terminado: {}", path.display());
                            job.done = true;
                        }
                        ExportEvent::Failed(e) => {
                            job.status = format!("Error: {e}");
                            job.done = true;
                        }
                        ExportEvent::Cancelled => {
                            job.status = "Cancelada".into();
                            job.done = true;
                        }
                    }
                }
                Ok(Reply::Text(job.status.clone()))
            }
            "cancel_export" => {
                let job = self.export.as_ref().ok_or("No se ha iniciado ninguna exportación")?;
                job.cancel.store(true, Ordering::Relaxed);
                Ok(Reply::Text("Cancelando…".into()))
            }
            _ => Err(format!("Herramienta desconocida: {name}")),
        }
    }
}

const INSTRUCTIONS: &str = "rec-ording crea vídeos promocionales de apps Android: grabaciones del móvil dentro de un marco o modelo 3D, sobre un fondo, con zooms y movimientos de cámara. Los tiempos son segundos de la línea de tiempo (los clips de vídeo se reproducen seguidos). Las coordenadas x/y son de la grabación, de 0 a 1 (0,0 arriba a la izquierda). Empieza con get_project; usa render_frame para ver el resultado. Cada cambio se guarda en el proyecto abierto en la app y se puede deshacer con undo.";

const FRAME_KEYS: &[(&str, FrameStyle)] = &[
    ("rounded", FrameStyle::Rounded),
    ("flat", FrameStyle::Flat),
    ("curved", FrameStyle::Curved),
    ("tablet", FrameStyle::Tablet),
    ("none", FrameStyle::None),
];

const COLOR_KEYS: &[(&str, FrameColor)] = &[
    ("obsidian", FrameColor::Obsidian),
    ("silver", FrameColor::Silver),
    ("sand", FrameColor::Sand),
    ("blue", FrameColor::Blue),
    ("white", FrameColor::White),
];

fn replaced_note(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => " (sustituyen 1 movimiento de cámara que se solapaba; undo lo recupera)".into(),
        n => format!(" (sustituyen {n} movimientos de cámara que se solapaban; undo los recupera)"),
    }
}

fn ease_key(e: Ease) -> &'static str {
    match e {
        Ease::Spring => "spring",
        Ease::Smooth => "smooth",
        Ease::Linear => "linear",
        Ease::Hold => "hold",
    }
}

fn apply_cam_fields(k: &mut CamKey, a: &Value) {
    if let Some(v) = opt_num(a, "zoom") {
        k.zoom = v.clamp(0.5, 5.0);
    }
    if let Some(v) = opt_num(a, "x") {
        k.x = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_num(a, "y") {
        k.y = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_num(a, "tilt") {
        k.tilt = v;
    }
    if let Some(v) = opt_num(a, "turn") {
        k.turn = v;
    }
    if let Some(v) = opt_num(a, "roll") {
        k.roll = v;
    }
    if let Some(e) = a["ease"].as_str().and_then(|s| Ease::ALL.into_iter().find(|e| ease_key(*e) == s)) {
        k.ease = e;
    }
}

fn text(a: &Value, k: &str) -> Result<String, String> {
    a[k].as_str().map(str::to_string).ok_or(format!("Falta el parámetro «{k}»"))
}

fn num(a: &Value, k: &str) -> Result<f32, String> {
    opt_num(a, k).ok_or(format!("Falta el parámetro numérico «{k}»"))
}

fn opt_num(a: &Value, k: &str) -> Option<f32> {
    a[k].as_f64().map(|v| v as f32).filter(|v| v.is_finite())
}

fn int(a: &Value, k: &str) -> Result<u64, String> {
    a[k].as_u64().ok_or(format!("Falta el parámetro entero «{k}»"))
}

fn hex(s: &str) -> Result<[u8; 3], String> {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("Color no válido: {s} (usa #rrggbb)"))?;
    if h.len() != 6 {
        return Err(format!("Color no válido: {s} (usa #rrggbb)"));
    }
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// The project as an agent sees it.
fn summary(p: &Project, path: &Path) -> Value {
    let placed = timeline::placed(p);
    json!({
        "name": p.name,
        "file": path.file_name().map(|f| f.to_string_lossy().into_owned()),
        "duration": timeline::duration(p),
        "clips": placed.iter().map(|c| {
            let m = p.media(c.clip.media);
            json!({"id": c.clip.id, "start": c.start, "end": c.end, "source_in": c.clip.source_in, "source_out": c.clip.source_out,
                   "video": m.map(|m| m.info.path.clone()), "size": m.map(|m| format!("{}x{}", m.info.width, m.info.height)),
                   "touches": m.map(|m| m.touches.iter().filter(|t| t.time >= c.clip.source_in && t.time < c.clip.source_out).count())})
        }).collect::<Vec<_>>(),
        "touches": timeline::timeline_touches(p).iter().map(|t| json!({"time": t.time, "x": t.x, "y": t.y, "kind": if t.kind == TouchKind::Tap {"tap"} else {"swipe"}})).collect::<Vec<_>>(),
        "camera": p.camera.iter().map(|c| json!({"id": c.id, "kind": c.kind.label(), "start": c.start, "end": c.end(),
            "keyframes": c.keys.iter().map(|k| json!({"time": c.start + k.t, "zoom": k.zoom, "x": k.x, "y": k.y, "tilt": k.tilt, "turn": k.turn, "roll": k.roll, "ease": ease_key(k.ease)})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "device_motion": p.device.iter().map(|d| {
            let spin = timeline::spin_of(d).map(|(axis, deg)| {
                let axis = ["vertical", "horizontal", "plane"][axis];
                json!({"axis": axis, "degrees": deg})
            });
            json!({"id": d.id, "name": d.name, "start": d.start, "end": d.end(), "spin": spin})
        }).collect::<Vec<_>>(),
        "texts": p.texts.iter().map(|c| json!({"id": c.id, "start": c.start, "end": c.end(), "title": c.title, "subtitle": c.subtitle,
            "place": key_of(&TEXT_PLACES, c.place), "size": key_of(&TEXT_SIZES, c.size), "reveal": key_of(&TEXT_REVEALS, c.reveal), "color": key_of(&TEXT_COLORS, c.color), "make_room": c.make_room})).collect::<Vec<_>>(),
        "director": {
            "motion": if p.director.motion == DirectorMotion::ThreeD {"3d"} else {"2d"},
            "intensity": p.director.intensity,
            "targets": p.director.targets.iter().map(|t| json!({"id": t.id, "time": t.time, "x0": t.x0, "y0": t.y0, "x1": t.x1, "y1": t.y1})).collect::<Vec<_>>(),
        },
        "style": {
            "aspect": p.style.aspect.label(),
            "frame": FRAME_KEYS.iter().find(|(_, f)| *f == p.style.frame).map(|(k, _)| *k).unwrap_or("model"),
            "frame_color": COLOR_KEYS.iter().find(|(_, c)| *c == p.style.frame_color).map(|(k, _)| *k),
            "model": p.style.model.as_ref().filter(|_| p.style.frame == FrameStyle::Custom).map(|m| json!({"path": m.path, "screen_part": m.screen_part, "quarter_turns": m.quarter_turns, "animation": m.animation, "loop": m.loop_animation, "animation_start": m.animation_start})),
            "angle": {"tilt": p.style.tilt, "turn": p.style.turn, "roll": p.style.roll, "straighten_on_zoom": p.style.straighten_on_zoom, "float": p.style.float_motion, "intro": p.style.intro},
            "background": {"a": format!("#{:02x}{:02x}{:02x}", p.style.background.a[0], p.style.background.a[1], p.style.background.a[2]), "b": format!("#{:02x}{:02x}{:02x}", p.style.background.b[0], p.style.background.b[1], p.style.background.b[2]), "angle": p.style.background.angle},
            "padding": p.style.padding, "shadow": p.style.shadow, "show_touches": p.style.show_touches, "video_fit": format!("{:?}", p.style.video_fit).to_lowercase(),
        },
        "export": {"resolution": p.export.resolution.label(), "fps": p.export.fps, "format": p.export.format.label(), "quality": p.export.quality.label(), "folder": p.export.folder},
    })
}

/// Decodes one frame of the recording under `t`.
fn decode_frame(p: &Project, t: f32) -> Result<(Frame, f32, f32), String> {
    let c = timeline::locate(p, t).ok_or("El proyecto no tiene vídeo")?;
    let m = p.media(c.clip.media).ok_or("Falta la grabación")?;
    let (w, h) = media::preview_size(&m.info);
    let mut child = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-ss", &format!("{:.3}", timeline::source_time(&c, t)), "-i", &m.info.path])
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut data = vec![0u8; w * h * 4];
    child.stdout.as_mut().unwrap().read_exact(&mut data).map_err(|_| "No se pudo leer ese fotograma".to_string())?;
    let _ = child.wait();
    Ok((Frame { w, h, data }, m.info.width as f32, m.info.height as f32))
}

/// Renders the frame at `t` as the export would, `width` pixels wide, and encodes it as PNG.
pub fn render_png(p: &Project, t: f32, width: usize, model: Option<&PreparedModel>) -> Result<Vec<u8>, String> {
    let (frame, vw, vh) = decode_frame(p, t)?;
    let mips = Mips::build(frame);
    let ratio = p.style.aspect.ratio();
    let (ow, oh) = (width, ((width as f32 / ratio).round() as usize).max(2));
    let mut rgba = vec![0u8; ow * oh * 4];
    let touches = timeline::timeline_touches(p);
    let texts = timeline::texts_at(p, t);
    let blur = timeline::shutter(p, t, p.export.fps as f32);
    compose(
        &mut rgba,
        ow,
        oh,
        &ComposeInput {
            style: &p.style,
            screen: Some(&mips),
            video_w: vw,
            video_h: vh,
            cam: timeline::camera_at(p, t),
            pose: timeline::pose_at(p, t),
            touches: &touches,
            time: t,
            since_start: t,
            model,
            supersample: 2,
            texts: &texts,
            blur: &blur,
        },
    );
    let mut enc = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{ow}x{oh}"), "-i", "-"])
        .args(["-frames:v", "1", "-c:v", "png", "-f", "image2pipe", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = enc.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&rgba);
    });
    let out = enc.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if out.stdout.is_empty() {
        return Err("No se pudo crear la imagen".into());
    }
    Ok(out.stdout)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ---------- Tool catalogue ----------

const TEXT_PLACES: [(&str, TextPlace); 5] =
    [("left", TextPlace::Left), ("right", TextPlace::Right), ("top", TextPlace::Top), ("bottom", TextPlace::Bottom), ("center", TextPlace::Center)];
const TEXT_SIZES: [(&str, TextSize); 3] = [("small", TextSize::Small), ("medium", TextSize::Medium), ("large", TextSize::Large)];
const TEXT_REVEALS: [(&str, TextReveal); 3] = [("fade", TextReveal::Fade), ("words", TextReveal::Words), ("lines", TextReveal::Lines)];
const TEXT_COLORS: [(&str, TextColor); 3] = [("auto", TextColor::Auto), ("light", TextColor::Light), ("dark", TextColor::Dark)];

fn key_of<T: PartialEq + Copy>(table: &[(&'static str, T)], v: T) -> &'static str {
    table.iter().find(|(_, x)| *x == v).map(|(k, _)| *k).unwrap_or("")
}

fn pick<T: Copy>(table: &[(&str, T)], a: &Value, field: &str) -> Result<Option<T>, String> {
    let Some(v) = a[field].as_str() else {
        return Ok(None);
    };
    let keys: Vec<&str> = table.iter().map(|(k, _)| *k).collect();
    table.iter().find(|(k, _)| *k == v).map(|(_, x)| Some(*x)).ok_or(format!("{field} debe ser {}", keys.join(", ")))
}

fn opt_str(a: &Value, field: &str) -> Option<String> {
    a[field].as_str().map(str::to_string)
}

/// Optional fields shared by add_text and update_text.
fn set_text_fields(c: &mut TextClip, a: &Value) -> Result<(), String> {
    if let Some(v) = opt_str(a, "subtitle") {
        c.subtitle = v;
    }
    if let Some(v) = pick(&TEXT_PLACES, a, "place")? {
        c.place = v;
    }
    if let Some(v) = pick(&TEXT_SIZES, a, "size")? {
        c.size = v;
    }
    if let Some(v) = pick(&TEXT_REVEALS, a, "reveal")? {
        c.reveal = v;
    }
    if let Some(v) = pick(&TEXT_COLORS, a, "color")? {
        c.color = v;
    }
    if let Some(v) = a["make_room"].as_bool() {
        c.make_room = v;
    }
    Ok(())
}

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({"name": name, "description": description, "inputSchema": {"type": "object", "properties": props, "required": required}})
}

fn n(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}
fn i(desc: &str) -> Value {
    json!({"type": "integer", "description": desc})
}
fn s(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}
fn b(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}

fn cam_props(extra: Value) -> Value {
    let mut base = json!({
        "zoom": n("Zoom (1 = sin zoom)"), "x": n("Punto de enfoque, 0..1 de la grabación"), "y": n("Punto de enfoque, 0..1"),
        "tilt": n("Inclinación añadida, grados"), "turn": n("Giro añadido, grados"), "roll": n("Rotación añadida, grados"),
        "ease": s("Cómo se llega: spring, smooth, linear o hold"),
    });
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        b.extend(e.clone());
    }
    base
}

pub fn tools() -> Vec<Value> {
    vec![
        tool("get_project", "Estado completo del proyecto abierto: clips, toques, cámara, Director, estilo y exportación.", json!({}), &[]),
        tool("list_projects", "Proyectos guardados.", json!({}), &[]),
        tool("create_project", "Crea un proyecto vacío (con el estilo del actual) y lo abre.", json!({"name": s("Nombre")}), &["name"]),
        tool("open_project", "Abre un proyecto por nombre o archivo.", json!({"name": s("Nombre o archivo")}), &["name"]),
        tool("rename_project", "Cambia el nombre del proyecto abierto.", json!({"name": s("Nombre nuevo")}), &["name"]),
        tool("undo", "Deshace el último cambio hecho por este servidor.", json!({}), &[]),
        tool("redo", "Rehace el último cambio deshecho.", json!({}), &[]),
        tool("list_android_devices", "Móviles Android conectados por USB.", json!({}), &[]),
        tool("start_recording", "Empieza a grabar la pantalla de un móvil (y sus toques).", json!({"serial": s("Serie del móvil (por defecto el primero listo)"), "audio": b("Grabar el sonido del móvil"), "mirror": b("Mostrar la pantalla del móvil en el ordenador")}), &[]),
        tool("stop_recording", "Termina la grabación y la añade al final de la línea de tiempo, con zooms automáticos.", json!({}), &[]),
        tool("import_video", "Añade un vídeo al final de la línea de tiempo.", json!({"path": s("Ruta del vídeo")}), &["path"]),
        tool("split_clip", "Divide el clip que hay en un momento.", json!({"time": n("Segundo de la línea de tiempo")}), &["time"]),
        tool("trim_clip", "Cambia qué parte de la grabación reproduce un clip.", json!({"clip_id": i("Clip"), "source_in": n("Inicio en la grabación, s"), "source_out": n("Fin en la grabación, s")}), &["clip_id"]),
        tool("move_clip", "Cambia el orden de un clip.", json!({"clip_id": i("Clip"), "index": i("Nueva posición, desde 0")}), &["clip_id", "index"]),
        tool("delete_clip", "Quita un clip de la línea de tiempo.", json!({"clip_id": i("Clip")}), &["clip_id"]),
        tool("auto_zoom", "Rehace los zooms automáticos a partir de los toques (conserva los hechos a mano).", json!({"zoom": n("Intensidad, 1.2..3")}), &[]),
        tool("add_zoom", "Zoom sobre un punto entre dos momentos.", json!({"start": n("Inicio, s"), "end": n("Fin, s"), "zoom": n("Zoom (por defecto 2)"), "x": n("0..1"), "y": n("0..1")}), &["start", "end"]),
        tool("add_camera_keyframe", "Fotograma clave de cámara en un momento: dentro del clip de cámara que haya ahí o como movimiento nuevo (que vuelve al reposo). Sirve para giros 3D a mitad del vídeo.", cam_props(json!({"time": n("Segundo de la línea de tiempo")})), &["time"]),
        tool("update_camera_keyframe", "Cambia un fotograma clave de cámara.", cam_props(json!({"clip_id": i("Clip de cámara"), "index": i("Índice del fotograma clave"), "time": n("Nuevo momento, s")})), &["clip_id", "index"]),
        tool("delete_camera_keyframe", "Elimina un fotograma clave de cámara.", json!({"clip_id": i("Clip de cámara"), "index": i("Índice")}), &["clip_id", "index"]),
        tool("delete_camera_clip", "Elimina un clip de cámara (zoom, movimiento o plano).", json!({"clip_id": i("Clip de cámara")}), &["clip_id"]),
        tool("move_camera_clip", "Mueve un clip de cámara.", json!({"clip_id": i("Clip de cámara"), "start": n("Nuevo inicio, s")}), &["clip_id", "start"]),
        tool("director_add_target", "Objetivo del Director: un recuadro de la grabación en un momento (sustituye al que hubiera en ese momento).", json!({"time": n("s"), "x0": n("0..1"), "y0": n("0..1"), "x1": n("0..1"), "y1": n("0..1")}), &["time", "x0", "y0", "x1", "y1"]),
        tool("director_remove_target", "Elimina un objetivo.", json!({"target_id": i("Objetivo")}), &["target_id"]),
        tool("director_clear", "Elimina todos los objetivos y planos del Director.", json!({}), &[]),
        tool("director_settings", "Tipo de movimiento e intensidad del Director.", json!({"motion": s("3d o 2d"), "intensity": n("0..1")}), &[]),
        tool("director_generate", "Genera un plano de cámara por objetivo (sustituye los clips de cámara que se solapen).", json!({}), &[]),
        tool("director_retake", "Otra variación del movimiento para un objetivo (o todos) y regenera.", json!({"target_id": i("Objetivo (todos si se omite)")}), &[]),
        tool("list_device_options", "Marcos, colores, modelos 3D de la galería y movimientos del dispositivo disponibles.", json!({}), &[]),
        tool("set_device", "Elige marco integrado, color o modelo 3D de la galería.", json!({"frame": s("rounded, flat, curved, tablet o none"), "frame_color": s("obsidian, silver, sand, blue o white"), "model": s("Nombre o ruta de un modelo de la galería")}), &[]),
        tool("import_model", "Importa un modelo .glb/.gltf (o el .zip de la descarga) a la galería y lo usa.", json!({"path": s("Ruta")}), &["path"]),
        tool("set_model_options", "Ajustes del modelo 3D en uso.", json!({"screen_part": json!({"description": "\"auto\" o índice de pieza"}), "quarter_turns": i("Giros de 90° de la pantalla"), "animation": json!({"description": "Índice de animación o \"none\""}), "loop": b("Repetir en bucle"), "animation_start": n("Segundo en que empieza la animación"), "video_fit": s("fill, fit o stretch"), "fit_pan": n("Encuadre con fill, 0..1")}), &[]),
        tool("set_angle", "Ángulo de reposo del dispositivo y movimientos globales.", json!({"tilt": n("grados"), "turn": n("grados"), "roll": n("grados"), "straighten_on_zoom": b("Ponerse de frente al hacer zoom"), "float": b("Balanceo suave"), "intro": b("Entrar girando al empezar")}), &[]),
        tool(
            "add_device_motion",
            "Añade un movimiento del dispositivo en un momento. full_turn gira 360° sobre el eje vertical y flip sobre el horizontal; cerca de un cambio de clip se centran en él, para que el corte ocurra mientras se ve la trasera.",
            json!({"preset": s("full_turn, flip, spin_in, drop_in, rise_up, hop o exit"), "time": n("s (exit termina en ese momento)"), "degrees": n("Solo giros: grados (360, 720, -360 para el otro sentido…)"), "duration": n("s")}),
            &["preset", "time"],
        ),
        tool(
            "add_text",
            "Añade un título sobre el vídeo, al estilo de los anuncios de Apple: frases cortas junto al móvil, que se aparta para dejarle sitio. Usa saltos de línea (\\n) para varias líneas.",
            json!({"time": n("s"), "title": s("Titular"), "subtitle": s("Línea más pequeña debajo (opcional)"), "duration": n("s (por defecto 3)"),
                   "place": s("left, right, top, bottom o center"), "size": s("small, medium o large (large: cifra o titular grande)"),
                   "reveal": s("fade, words (palabra a palabra) o lines (línea a línea, las anteriores se atenúan)"), "color": s("auto, light o dark"), "make_room": b("Apartar el móvil (por defecto sí)")}),
            &["time", "title"],
        ),
        tool(
            "update_text",
            "Cambia un título: texto, momento, duración o estilo.",
            json!({"text_id": i("Texto"), "title": s("Titular"), "subtitle": s("Línea pequeña"), "start": n("s"), "duration": n("s"), "place": s("left, right, top, bottom o center"),
                   "size": s("small, medium o large"), "reveal": s("fade, words o lines"), "color": s("auto, light o dark"), "make_room": b("Apartar el móvil")}),
            &["text_id"],
        ),
        tool("delete_text", "Elimina un título.", json!({"text_id": i("Texto")}), &["text_id"]),
        tool("delete_device_motion", "Elimina un movimiento del dispositivo.", json!({"clip_id": i("Clip")}), &["clip_id"]),
        tool("set_style", "Formato, fondo, margen, sombra y toques.", json!({"aspect": s("16:9, 9:16, 1:1 o 4:5"), "background_preset": s("Medianoche, Aurora, Atardecer, Lavanda, Menta, Océano, Melocotón, Bosque, Grafito o Nube"), "background_a": s("#rrggbb"), "background_b": s("#rrggbb"), "background_angle": n("grados"), "padding": n("0.02..0.25"), "shadow": n("0..1"), "show_touches": b("Mostrar los toques"), "touch_size": n("0.5..2")}), &[]),
        tool("render_frame", "Imagen de cómo queda el vídeo en un momento.", json!({"time": n("s"), "width": i("Ancho en píxeles (por defecto 960)")}), &["time"]),
        tool(
            "export",
            "Exporta el vídeo (en segundo plano). Por defecto codifica con la tarjeta gráfica si hay una compatible (NVIDIA, AMD o Intel); si falla, con el procesador.",
            json!({"resolution": s("720p, 1080p o 4K"), "fps": i("30 o 60"), "format": s("h264, hevc o gif"), "quality": s("standard, high o max"), "include_audio": b("Incluir sonido"), "folder": s("Carpeta de destino"), "hardware": b("Codificar con la tarjeta gráfica")}),
            &[],
        ),
        tool("export_status", "Progreso o resultado de la exportación.", json!({}), &[]),
        tool("cancel_export", "Cancela la exportación en marcha.", json!({}), &[]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn tools_have_unique_names_and_schemas() {
        let t = tools();
        let mut names: Vec<&str> = t.iter().map(|x| x["name"].as_str().unwrap()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
        assert!(t.iter().all(|x| x["inputSchema"]["type"] == "object"));
    }
}
