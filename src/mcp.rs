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
    /// Undo and redo steps, with the project file each belongs to.
    undo: Vec<(PathBuf, Project)>,
    redo: Vec<(PathBuf, Project)>,
    /// Project named by the current call's `project` argument (else the one open in the app).
    target: Option<PathBuf>,
    /// Inside `batch`: the project being edited in memory, saved once at the end.
    batch: Option<Project>,
    recording: Option<Recording>,
    export: Option<ExportJob>,
    model_cache: Option<(CustomModel, Arc<PreparedModel>)>,
}

/// Runs the server. `resumed`: this process replaced an older one mid-session (see
/// `restart_if_updated`), so the client is already initialized and must reload the tools.
pub fn run(resumed: bool) {
    let mut server = Server { undo: Vec::new(), redo: Vec::new(), target: None, batch: None, recording: None, export: None, model_cache: None };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let exe = std::env::current_exe().ok().map(|p| PathBuf::from(p.to_string_lossy().trim_end_matches(" (deleted)")));
    let started = exe.as_ref().and_then(|e| std::fs::metadata(e).and_then(|m| m.modified()).ok());
    if resumed {
        let _ = writeln!(stdout, "{}", json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
        let _ = stdout.flush();
    }
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Invalid JSON: {e}")}})),
        };
        if let Some(r) = response {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
        if server.recording.is_none() && server.export.as_ref().is_none_or(|e| e.done) {
            restart_if_updated(exe.as_deref(), started);
        }
    }
    if let Some(rec) = server.recording.take() {
        let _ = rec.stop_blocking();
    }
}

/// When rec·ording has been updated while a client stays connected, replaces this process with
/// the new executable (same stdin and stdout), which then tells the client the tools changed.
#[cfg(unix)]
fn restart_if_updated(exe: Option<&Path>, started: Option<std::time::SystemTime>) {
    use std::os::unix::process::CommandExt;
    let (Some(exe), Some(started)) = (exe, started) else {
        return;
    };
    let now = std::fs::metadata(exe).and_then(|m| m.modified()).ok();
    if now.is_some_and(|t| t != started) {
        // Only returns if exec failed; then keep serving with this version.
        let _ = std::process::Command::new(exe).args(["--mcp", "--resumed"]).exec();
    }
}

#[cfg(not(unix))]
fn restart_if_updated(_exe: Option<&Path>, _started: Option<std::time::SystemTime>) {}

/// One gesture of play_gestures.
fn gesture(g: &Value) -> Result<capture::Gesture, String> {
    use capture::Gesture;
    let n = |k: &str| g[k].as_f64().map(|v| v as f32).ok_or(format!("The gesture is missing {k}"));
    Ok(match g["type"].as_str().unwrap_or("") {
        "tap" => Gesture::Tap { x: n("x")?, y: n("y")? },
        "swipe" => Gesture::Swipe { x0: n("x")?, y0: n("y")?, x1: n("to_x")?, y1: n("to_y")?, duration: n("duration").unwrap_or(0.3) },
        "long_press" => {
            let (x, y) = (n("x")?, n("y")?);
            Gesture::Swipe { x0: x, y0: y, x1: x, y1: y, duration: n("duration").unwrap_or(0.8) }
        }
        "text" => Gesture::Text(g["text"].as_str().ok_or("The gesture is missing text")?.to_string()),
        "key" => Gesture::Key(g["key"].as_str().ok_or("The gesture is missing key")?.to_string()),
        "wait" => Gesture::Wait(n("duration")?),
        "pinch" => return Err("Pinching isn't possible: adb only injects one finger".into()),
        other => return Err(format!("Unknown gesture “{other}” (tap, swipe, long_press, text, key, wait)")),
    })
}

/// Tools that take `mark` (a screen change) instead of a time.
const MARKED_TOOLS: [&str; 8] = ["add_text", "add_zoom", "add_device_motion", "add_camera_keyframe", "freeze_frame", "director_add_target", "add_image", "add_sound"];

/// Tools that cannot run inside `batch` (they are not edits of one project).
const NOT_IN_BATCH: [&str; 12] =
    ["batch", "undo", "redo", "create_project", "open_project", "start_recording", "stop_recording", "play_gestures", "export", "cancel_export", "render_preview", "import_texts_file"];

impl Server {
    /// Answers one JSON-RPC message (notifications get no answer).
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?.clone();
        let method = msg["method"].as_str().unwrap_or("");
        let params = &msg["params"];
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "rec-ording", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let target = args["project"].as_str().map(find_project).transpose();
                let reply = match target {
                    Ok(t) => {
                        self.target = t;
                        self.call(name, &args)
                    }
                    Err(e) => Err(e),
                };
                // Every answer says which project it acted on.
                let which = load_project(&self.path()).map(|p| format!("Project: “{}”", p.name)).unwrap_or_default();
                self.target = None;
                Ok(match reply {
                    Ok(Reply::Text(t)) => json!({"content": [{"type": "text", "text": t}, {"type": "text", "text": which}], "isError": false}),
                    Ok(Reply::Image { png, caption }) => json!({"content": [
                        {"type": "image", "data": base64(&png), "mimeType": "image/png"},
                        {"type": "text", "text": caption},
                        {"type": "text", "text": which}
                    ], "isError": false}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}, {"type": "text", "text": which}], "isError": true}),
                })
            }
            _ => Err(json!({"code": -32601, "message": format!("Unknown method: {method}")})),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        })
    }

    /// The project this call works on: the one it names, else the one open in the app.
    fn path(&self) -> PathBuf {
        self.target.clone().unwrap_or_else(current_project_path)
    }

    /// Loads the project, applies `f` and saves it (one undo step) if it changed. Inside
    /// `batch`, applies `f` to the batch's copy instead.
    fn edit(&mut self, f: impl FnOnce(&mut Project) -> Result<String, String>) -> ToolResult {
        if let Some(p) = self.batch.as_mut() {
            return f(p).map(Reply::Text);
        }
        let path = self.path();
        let before = load_project(&path)?;
        let mut p = before.clone();
        let msg = f(&mut p)?;
        if p != before {
            save_project_to(&path, &mut p)?;
            self.push_undo(path, before);
        }
        Ok(Reply::Text(msg))
    }

    fn push_undo(&mut self, path: PathBuf, before: Project) {
        self.undo.push((path, before));
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn read(&self) -> Result<Project, String> {
        match &self.batch {
            Some(p) => Ok(p.clone()),
            None => load_project(&self.path()),
        }
    }

    fn prepared_model(&mut self, p: &Project) -> Result<Option<Arc<PreparedModel>>, String> {
        let Some(cm) = p.style.model.clone().filter(|_| p.style.frame == FrameStyle::Custom) else { return Ok(None) };
        if let Some((k, m)) = &self.model_cache {
            if k.path == cm.path && k.screen_part == cm.screen_part && k.quarter_turns == cm.quarter_turns {
                return Ok(Some(m.clone()));
            }
        }
        let src = model3d::load(&cm.path)?;
        let part = cm.screen(src.guess, src.parts.len()).ok_or("The model's screen was not found")?;
        let m = Arc::new(model3d::prepare(&src, part, cm.quarter_turns)?);
        self.model_cache = Some((cm, m.clone()));
        Ok(Some(m))
    }

    fn call(&mut self, name: &str, a: &Value) -> ToolResult {
        // `mark`: the moment of a screen change instead of a time.
        let with_mark;
        let a = match a["mark"].as_u64() {
            Some(i) if MARKED_TOOLS.contains(&name) => {
                let marks = timeline::timeline_marks(&self.read()?);
                let t = *marks.get(i as usize).ok_or(format!("There is no mark {i}: there are {} (detect_marks finds them)", marks.len()))?;
                let mut b = a.clone();
                if name == "add_zoom" {
                    b["start"] = json!(t);
                    if b["end"].is_null() {
                        b["end"] = json!(t + 2.0);
                    }
                } else {
                    b["time"] = json!(t);
                }
                with_mark = b;
                &with_mark
            }
            _ => a,
        };
        match name {
            "detect_marks" => {
                let force = a["again"].as_bool().unwrap_or(false);
                let p = self.read()?;
                let todo: Vec<String> = p.media.iter().filter(|m| force || m.marks.is_empty()).map(|m| m.info.path.clone()).collect();
                let found: Vec<(String, Vec<f32>)> = todo.into_iter().map(|path| {
                    let marks = media::screen_changes(&path);
                    (path, marks)
                }).collect();
                self.edit(move |p| {
                    for (path, marks) in &found {
                        for m in p.media.iter_mut().filter(|m| &m.info.path == path) {
                            m.marks = marks.clone();
                        }
                    }
                    let all = timeline::timeline_marks(p);
                    Ok(format!("{} screen changes on the timeline: {:?}", all.len(), all.iter().map(|t| (t * 100.0).round() / 100.0).collect::<Vec<_>>()))
                })
            }
            // ----- Projects -----
            "get_project" => Ok(Reply::Text(summary(&self.read()?, &self.path()).to_string())),
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
                Ok(Reply::Text(format!("Project “{name}” created and opened")))
            }
            "open_project" => {
                let name = text(a, "name")?;
                let entry = list_projects()
                    .into_iter()
                    .find(|e| e.name == name || e.path.file_name().is_some_and(|f| f.to_string_lossy() == name))
                    .ok_or(format!("There is no project “{name}”"))?;
                set_current_project(&entry.path);
                self.undo.clear();
                self.redo.clear();
                Ok(Reply::Text(format!("Project “{}” opened", entry.name)))
            }
            "rename_project" => {
                let name = text(a, "name")?;
                self.edit(|p| {
                    p.name = name.clone();
                    Ok(format!("Project renamed to “{name}”"))
                })
            }
            "undo" | "redo" => {
                let (from, to) = if name == "undo" { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
                let (path, mut prev) = from.pop().ok_or("Nothing to undo or redo in this session")?;
                let now = load_project(&path)?;
                prev.revision = now.revision;
                save_project_to(&path, &mut prev)?;
                to.push((path, now));
                Ok(Reply::Text(if name == "undo" { format!("Undone in “{}”", prev.name) } else { format!("Redone in “{}”", prev.name) }))
            }

            "batch" => {
                if self.batch.is_some() {
                    return Err("batch can't be nested".into());
                }
                let ops = a["operations"].as_array().ok_or("Missing operations: a list of {name, arguments}")?.clone();
                let path = self.path();
                let before = load_project(&path)?;
                self.batch = Some(before.clone());
                let mut done = Vec::new();
                for (i, op) in ops.iter().enumerate() {
                    let tool = op["name"].as_str().unwrap_or("");
                    let args = op.get("arguments").cloned().unwrap_or(json!({}));
                    let r = if NOT_IN_BATCH.contains(&tool) { Err(format!("{tool} can't be used inside batch")) } else { self.call(tool, &args) };
                    match r {
                        Ok(Reply::Text(t)) => done.push(format!("{}. {tool}: {t}", i + 1)),
                        Ok(Reply::Image { caption, .. }) => done.push(format!("{}. {tool}: {caption}", i + 1)),
                        Err(e) => {
                            self.batch = None;
                            return Err(format!("Operation {} ({tool}) failed: {e}. None was applied.", i + 1));
                        }
                    }
                }
                let mut p = self.batch.take().unwrap_or(before.clone());
                if p != before {
                    save_project_to(&path, &mut p)?;
                    self.push_undo(path, before);
                }
                Ok(Reply::Text(format!("{} operations applied (undone in one step):\n{}", done.len(), done.join("\n"))))
            }

            // ----- Recording and media -----
            "list_android_devices" => {
                let devs: Vec<Value> = capture::list_devices()
                    .into_iter()
                    .map(|d| json!({"serial": d.serial, "model": d.model, "ready": d.state == capture::DeviceState::Ready, "emulator": d.emulator}))
                    .collect();
                Ok(Reply::Text(Value::Array(devs).to_string()))
            }
            "start_recording" => {
                if self.recording.is_some() {
                    return Err("A recording is already running".into());
                }
                let serial = match a["serial"].as_str() {
                    Some(s) => s.to_string(),
                    None => capture::list_devices()
                        .into_iter()
                        .find(|d| d.state == capture::DeviceState::Ready)
                        .map(|d| d.serial)
                        .ok_or("There is no Android phone connected and authorized")?,
                };
                let opts = capture::RecordOptions {
                    audio: a["audio"].as_bool().unwrap_or(true),
                    mirror: a["mirror"].as_bool().unwrap_or(false),
                    clean_status_bar: a["clean_status_bar"].as_bool().unwrap_or(true),
                };
                self.recording = Some(capture::start(&serial, &opts, capture::detect_tools())?);
                Ok(Reply::Text(format!("Recording {serial}. Call stop_recording to finish.")))
            }
            "play_gestures" => {
                let gestures = a["gestures"].as_array().ok_or("Missing gestures (a list)")?.iter().map(gesture).collect::<Result<Vec<_>, String>>()?;
                let n = match &self.recording {
                    Some(rec) => rec.play(&gestures)?,
                    None => {
                        let serial = match a["serial"].as_str() {
                            Some(s) => s.to_string(),
                            None => capture::list_devices()
                                .into_iter()
                                .find(|d| d.state == capture::DeviceState::Ready)
                                .map(|d| d.serial)
                                .ok_or("There is no Android phone connected and authorized")?,
                        };
                        capture::play(&serial, &gestures, |_, _, _| {})?
                    }
                };
                Ok(Reply::Text(if self.recording.is_some() {
                    format!("{n} gestures played and recorded (they get tap circles and automatic zooms)")
                } else {
                    format!("{n} gestures played (not recording, so they aren't recorded)")
                }))
            }
            "stop_recording" => {
                let rec = self.recording.take().ok_or("No recording is running")?;
                match rec.stop_blocking() {
                    CaptureResult::Done { path, touches } => {
                        let info = media::probe(&path.to_string_lossy())?;
                        let n = touches.len();
                        self.edit(move |p| {
                            let id = timeline::append_media(p, info, touches);
                            let scale = p.style.auto_zoom_scale;
                            let zooms = if n > 0 { timeline::regenerate_auto_zooms(p, scale) } else { 0 };
                            Ok(format!("Recording added as clip {id} ({n} taps, {zooms} automatic zooms)"))
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
                    Ok(format!("Video added at the end as clip {id}"))
                })
            }
            "split_clip" => {
                let t = num(a, "time")?;
                self.edit(|p| timeline::split_at(p, t).map(|id| format!("Split; the new clip is {id}")))
            }
            "trim_clip" => {
                let id = int(a, "clip_id")?;
                let (i, o) = (opt_num(a, "source_in"), opt_num(a, "source_out"));
                self.edit(|p| timeline::trim_clip(p, id, i, o).map(|_| "Clip trimmed".into()))
            }
            "move_clip" => {
                let (id, index) = (int(a, "clip_id")?, int(a, "index")? as usize);
                self.edit(|p| timeline::move_clip(p, id, index).map(|_| "Clip moved".into()))
            }
            "delete_clip" => {
                let id = int(a, "clip_id")?;
                self.edit(|p| {
                    timeline::delete_clip(p, id).map(|lost| match lost {
                        0 => "Clip deleted; everything else stays with its recording".into(),
                        n => format!("Clip deleted, with {n} items that were on it (zooms, moves, titles or targets); everything else stays with its recording"),
                    })
                })
            }

            "set_clip_speed" => {
                let (id, speed) = (int(a, "clip_id")?, num(a, "speed")?);
                self.edit(|p| timeline::set_clip_speed(p, id, speed).map(|_| format!("Speed {speed}×; what was on top moved with the clip")))
            }
            "freeze_frame" => {
                let (t, secs) = (num(a, "time")?, opt_num(a, "duration").unwrap_or(2.0));
                self.edit(|p| timeline::freeze_frame(p, t, secs).map(|id| format!("Frame frozen for {secs} s (clip {id}); what came after was shifted")))
            }
            "set_freeze_duration" => {
                let (id, secs) = (int(a, "clip_id")?, num(a, "duration")?);
                self.edit(|p| timeline::set_freeze_duration(p, id, secs).map(|_| "Duration changed".into()))
            }
            "replace_clip_video" => {
                let id = int(a, "clip_id")?;
                let path = text(a, "path")?;
                let info = media::probe(&path)?;
                self.edit(move |p| timeline::replace_clip_video(p, id, info, Vec::new()).map(|_| "Video replaced; the trim, the speed and everything on top are kept".into()))
            }

            // ----- Camera -----
            "auto_zoom" => {
                let scale = opt_num(a, "zoom");
                self.edit(|p| {
                    let s = scale.unwrap_or(p.style.auto_zoom_scale).clamp(1.1, 4.0);
                    p.style.auto_zoom_scale = s;
                    let n = timeline::regenerate_auto_zooms(p, s);
                    Ok(format!("{n} automatic zooms from the taps"))
                })
            }
            "add_zoom" => {
                let (start, end) = (num(a, "start")?, num(a, "end")?);
                let (zoom, x, y) = (opt_num(a, "zoom").unwrap_or(2.0), opt_num(a, "x").unwrap_or(0.5), opt_num(a, "y").unwrap_or(0.5));
                self.edit(|p| timeline::add_zoom(p, start, end, zoom, x, y).map(|id| format!("Zoom created (camera clip {id})")))
            }
            "add_camera_keyframe" => {
                let t = num(a, "time")?;
                let a = a.clone();
                self.edit(move |p| {
                    let (id, i) = timeline::add_camera_key(p, t, |k| apply_cam_fields(k, &a))?;
                    Ok(format!("Keyframe {i} of camera clip {id}"))
                })
            }
            "update_camera_keyframe" => {
                let (id, index) = (int(a, "clip_id")?, int(a, "index")? as usize);
                let a = a.clone();
                self.edit(move |p| {
                    let c = p.camera.iter_mut().find(|c| c.id == id).ok_or(format!("There is no camera clip {id}"))?;
                    let k = c.keys.get_mut(index).ok_or("That keyframe doesn't exist")?;
                    apply_cam_fields(k, &a);
                    if let Some(t) = opt_num(&a, "time") {
                        k.t = t - c.start;
                        timeline::normalize_camera_clip(c);
                    }
                    Ok("Keyframe updated".into())
                })
            }
            "delete_camera_keyframe" => {
                let (id, index) = (int(a, "clip_id")?, int(a, "index")? as usize);
                self.edit(|p| timeline::delete_camera_key(p, id, index).map(|_| "Keyframe deleted".into()))
            }
            "delete_camera_clip" => {
                let id = int(a, "clip_id")?;
                self.edit(|p| timeline::delete_camera_clip(p, id).map(|_| "Camera clip deleted".into()))
            }
            "move_camera_clip" => {
                let (id, start) = (int(a, "clip_id")?, num(a, "start")?);
                self.edit(|p| timeline::move_camera_clip(p, id, start).map(|_| "Camera clip moved".into()))
            }

            // ----- Director -----
            "director_add_target" => {
                let (t, x0, y0, x1, y1) = (num(a, "time")?, num(a, "x0")?, num(a, "y0")?, num(a, "x1")?, num(a, "y1")?);
                self.edit(|p| timeline::add_target(p, t, x0, y0, x1, y1).map(|id| format!("Target {id} at {t:.2} s")))
            }
            "director_remove_target" => {
                let id = int(a, "target_id")?;
                self.edit(|p| timeline::remove_target(p, id).map(|_| "Target deleted".into()))
            }
            "director_clear" => self.edit(|p| {
                p.director.targets.clear();
                p.camera.retain(|c| c.kind != CameraKind::Director);
                Ok("Director targets and shots deleted".into())
            }),
            "director_settings" => {
                let a = a.clone();
                self.edit(move |p| {
                    if let Some(m) = a["motion"].as_str() {
                        p.director.motion = match m {
                            "2d" | "2D" => DirectorMotion::TwoD,
                            "3d" | "3D" => DirectorMotion::ThreeD,
                            _ => return Err("motion must be \"3d\" or \"2d\"".into()),
                        };
                    }
                    if let Some(i) = opt_num(&a, "intensity") {
                        p.director.intensity = i.clamp(0.0, 1.0);
                    }
                    Ok("Director settings changed".into())
                })
            }
            "director_retake" => {
                let id = a["target_id"].as_u64();
                let model = self.read().and_then(|p| self.prepared_model(&p))?;
                self.edit(move |p| {
                    timeline::retake(p, id);
                    let (w, h) = p.export.dimensions(p.style.aspect);
                    let (n, replaced) = timeline::director_generate(p, w as f32, h as f32, model.as_deref().map(|m| &m.meta))?;
                    Ok(format!("{n} shots generated again{}", replaced_note(replaced)))
                })
            }
            "director_generate" => {
                let model = self.read().and_then(|p| self.prepared_model(&p))?;
                self.edit(move |p| {
                    let (w, h) = p.export.dimensions(p.style.aspect);
                    let (n, replaced) = timeline::director_generate(p, w as f32, h as f32, model.as_deref().map(|m| &m.meta))?;
                    Ok(format!("{n} shots generated{}", replaced_note(replaced)))
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
                        p.style.frame = FRAME_KEYS.iter().find(|(k, _)| *k == f).map(|(_, v)| *v).ok_or(format!("Unknown frame: {f}"))?;
                    }
                    if let Some(c) = a["frame_color"].as_str() {
                        p.style.frame_color = COLOR_KEYS.iter().find(|(k, _)| *k == c).map(|(_, v)| *v).ok_or(format!("Unknown color: {c}"))?;
                    }
                    if let Some(m) = a["model"].as_str() {
                        let entry = library::list()
                            .into_iter()
                            .find(|e| e.name == m || e.path == m)
                            .ok_or(format!("There is no model “{m}” in the gallery (use import_model)"))?;
                        let src = model3d::load(&entry.path)?;
                        p.style.model = Some(CustomModel { path: entry.path, animation: (!src.animations.is_empty()).then_some(0), ..Default::default() });
                        p.style.frame = FrameStyle::Custom;
                    }
                    Ok("Device changed".into())
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
                    Ok(format!("Model imported into the gallery and in use ({anims} animations)"))
                })
            }
            "set_model_options" => {
                let a = a.clone();
                self.edit(move |p| {
                    let cm = p.style.model.as_mut().filter(|_| p.style.frame == FrameStyle::Custom).ok_or("No 3D model is in use")?;
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
                            _ => return Err("video_fit must be fill, fit or stretch".into()),
                        };
                    }
                    if let Some(v) = opt_num(&a, "fit_pan") {
                        p.style.fit_pan = v.clamp(0.0, 1.0);
                    }
                    Ok("Model settings changed".into())
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
                    Ok("Angle changed".into())
                })
            }
            "add_device_motion" => {
                let key = text(a, "preset")?;
                let preset = DevicePreset::from_key(&key).ok_or(format!("Unknown move: {key}"))?;
                let t = num(a, "time")?;
                let (degrees, secs) = (opt_num(a, "degrees"), opt_num(a, "duration"));
                let ease = parse_ease(a)?;
                self.edit(move |p| {
                    let id = timeline::add_device_preset(p, preset, t);
                    let c = p.device.iter_mut().find(|c| c.id == id).ok_or("Couldn't add it")?;
                    if let Some(e) = ease {
                        c.keys.iter_mut().skip(1).for_each(|k| k.ease = e);
                    }
                    if let Some(d) = degrees {
                        if timeline::spin_of(c).is_none() {
                            return Err("degrees only applies to full_turn and flip".into());
                        }
                        timeline::set_spin(c, d);
                    }
                    if let Some(s) = secs {
                        timeline::set_device_duration(c, s);
                    }
                    Ok(format!("Move “{}” added from {:.2} to {:.2} s (clip {id})", preset.label(), c.start, c.end()))
                })
            }
            "update_device_motion" => {
                let id = int(a, "clip_id")?;
                let (start, degrees, secs) = (opt_num(a, "start"), opt_num(a, "degrees"), opt_num(a, "duration"));
                let ease = parse_ease(a)?;
                self.edit(move |p| {
                    let total = timeline::duration(p);
                    let c = p.device.iter_mut().find(|c| c.id == id).ok_or(format!("There is no device move {id}"))?;
                    if let Some(e) = ease {
                        c.keys.iter_mut().skip(1).for_each(|k| k.ease = e);
                    }
                    if let Some(d) = degrees {
                        if timeline::spin_of(c).is_none() {
                            return Err("degrees only applies to turns (full_turn and flip)".into());
                        }
                        timeline::set_spin(c, d);
                    }
                    if let Some(s) = secs {
                        timeline::set_device_duration(c, s);
                    }
                    if let Some(t) = start {
                        c.start = t.clamp(0.0, (total - c.duration()).max(0.0));
                    }
                    let (a, b) = (c.start, c.end());
                    p.device.sort_by(|x, y| x.start.total_cmp(&y.start));
                    Ok(format!("Move updated: from {a:.2} to {b:.2} s"))
                })
            }
            "delete_device_motion" => {
                let id = int(a, "clip_id")?;
                self.edit(|p| timeline::delete_device_clip(p, id).map(|_| "Move deleted".into()))
            }

            // ----- Titles -----
            "add_text" => {
                let a = a.clone();
                let t = num(&a, "time")?;
                let title = text(&a, "title")?;
                self.edit(move |p| {
                    let id = timeline::add_text(p, t, &title)?;
                    let c = p.texts.iter_mut().find(|c| c.id == id).ok_or("Couldn't add it")?;
                    set_text_fields(c, &a)?;
                    let start = c.start;
                    if let Some(d) = opt_num(&a, "duration") {
                        timeline::place_text(p, id, start, start + d)?;
                    }
                    let c = p.texts.iter().find(|c| c.id == id).ok_or("Couldn't add it")?;
                    Ok(format!("Title added from {:.2} to {:.2} s (text {id})", c.start, c.end()))
                })
            }
            "update_text" => {
                let a = a.clone();
                let id = int(&a, "text_id")?;
                self.edit(move |p| {
                    let c = p.texts.iter_mut().find(|c| c.id == id).ok_or(format!("There is no text {id}"))?;
                    set_text_fields(c, &a)?;
                    if let Some(v) = opt_str(&a, "title") {
                        c.title = v;
                    }
                    let start = opt_num(&a, "start").unwrap_or(c.start);
                    let end = start + opt_num(&a, "duration").unwrap_or(c.duration);
                    timeline::place_text(p, id, start, end)?;
                    Ok("Title updated".into())
                })
            }
            "export_texts" => Ok(Reply::Text(Value::Array(self.read()?.texts.iter().map(text_json).collect()).to_string())),
            "import_texts" => {
                let items = a["texts"].as_array().ok_or("Missing texts: the list that export_texts returns")?.clone();
                let replace = a["replace"].as_bool().unwrap_or(true);
                self.edit(move |p| {
                    if replace {
                        p.texts.clear();
                    }
                    for (i, it) in items.iter().enumerate() {
                        let mut c = TextClip {
                            id: p.next_id(),
                            start: num(it, "time").map_err(|e| format!("Title {}: {e}", i + 1))?.max(0.0),
                            duration: opt_num(it, "duration").unwrap_or(3.0).max(0.8),
                            title: it["title"].as_str().unwrap_or_default().to_string(),
                            ..TextClip::default()
                        };
                        set_text_fields(&mut c, it).map_err(|e| format!("Title {}: {e}", i + 1))?;
                        p.texts.push(c);
                    }
                    p.texts.sort_by(|a, b| a.start.total_cmp(&b.start));
                    Ok(format!("{} titles imported", items.len()))
                })
            }
            "delete_text" => {
                let id = int(a, "text_id")?;
                self.edit(|p| timeline::delete_text(p, id).map(|_| "Title deleted".into()))
            }

            // ----- Pictures and sounds -----
            "add_image" => {
                let a = a.clone();
                let path = text(&a, "path")?;
                let (w, h) = crate::images::size(&path)?;
                let t = num(&a, "time")?;
                self.edit(move |p| {
                    let id = timeline::add_image(p, &path, w as f32 / h as f32, t)?;
                    set_image_fields(p, id, &a)?;
                    let c = p.images.iter().find(|c| c.id == id).ok_or("Couldn't add it")?;
                    Ok(format!("Image added from {:.2} to {:.2} s (image {id})", c.start, c.end()))
                })
            }
            "update_image" => {
                let a = a.clone();
                let id = int(&a, "image_id")?;
                self.edit(move |p| set_image_fields(p, id, &a).map(|_| "Image updated".into()))
            }
            "delete_image" => {
                let id = int(a, "image_id")?;
                self.edit(|p| timeline::delete_image(p, id).map(|_| "Image deleted".into()))
            }
            "add_sound" => {
                let a = a.clone();
                let path = text(&a, "path")?;
                let track = AudioTrack::from_key(&text(&a, "track")?).ok_or("track: effects, voice or music")?;
                let length = crate::audio::length(&path)?;
                let t = num(&a, "time")?;
                self.edit(move |p| {
                    let id = timeline::add_audio(p, track, &path, length, t)?;
                    set_sound_fields(p, id, &a)?;
                    let c = p.audio.iter().find(|c| c.id == id).ok_or("Couldn't add it")?;
                    let looped = if c.looped { ", looped" } else { "" };
                    Ok(format!("Sound added to {} from {:.2} to {:.2} s{looped} (sound {id})", track.label().to_lowercase(), c.start, c.end()))
                })
            }
            "update_sound" => {
                let a = a.clone();
                let id = int(&a, "sound_id")?;
                self.edit(move |p| set_sound_fields(p, id, &a).map(|_| "Sound updated".into()))
            }
            "delete_sound" => {
                let id = int(a, "sound_id")?;
                self.edit(|p| timeline::delete_audio(p, id).map(|_| "Sound deleted".into()))
            }
            "set_mix" => {
                let a = a.clone();
                self.edit(move |p| {
                    let m = &mut p.mix;
                    for (k, v) in [("recording", &mut m.recording), ("effects", &mut m.effects), ("voice", &mut m.voice), ("music", &mut m.music)] {
                        if let Some(x) = opt_num(&a, k) {
                            *v = x.clamp(0.0, 2.0);
                        }
                    }
                    if let Some(b) = a["duck_music"].as_bool() {
                        m.duck_music = b;
                    }
                    Ok("Mix updated".into())
                })
            }

            // ----- Look -----
            "set_style" => {
                let a = a.clone();
                self.edit(move |p| {
                    let s = &mut p.style;
                    if let Some(v) = a["aspect"].as_str() {
                        s.aspect = Aspect::ALL.into_iter().find(|x| x.label() == v).ok_or("aspect must be 16:9, 9:16, 1:1 or 4:5")?;
                    }
                    if let Some(v) = a["background_preset"].as_str() {
                        let b = BACKGROUND_PRESETS.iter().find(|b| crate::i18n::lz(b.name).eq_ignore_ascii_case(v) || b.name.to_lowercase() == v.to_lowercase()).ok_or(format!("Unknown background: {v}"))?;
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
                    // Lighting
                    if let Some(v) = pick(&LIGHT_STYLES, &a, "lighting")? {
                        s.lighting.style = v;
                    }
                    if let Some(v) = opt_num(&a, "light_angle") {
                        s.lighting.angle = v.clamp(-180.0, 180.0);
                    }
                    if let Some(v) = opt_num(&a, "light_intensity") {
                        s.lighting.intensity = v.clamp(0.5, 1.5);
                    }
                    if let Some(v) = opt_num(&a, "glare") {
                        s.lighting.glare = v.clamp(0.0, 2.0);
                    }
                    // Finish
                    if let Some(v) = pick(&MOTION_BLURS, &a, "motion_blur")? {
                        s.finish.motion_blur = v;
                    }
                    if let Some(v) = pick(&ANTIALIAS, &a, "antialias")? {
                        s.finish.antialias = v;
                    }
                    for (key, field) in [("vignette", &mut s.finish.vignette), ("grain", &mut s.finish.grain)] {
                        if let Some(v) = opt_num(&a, key) {
                            *field = v.clamp(0.0, 1.0);
                        }
                    }
                    for (key, field, max) in [("fade_in", &mut s.finish.fade_in, 5.0), ("fade_out", &mut s.finish.fade_out, 5.0), ("crossfade", &mut s.finish.crossfade, 2.0)] {
                        if let Some(v) = opt_num(&a, key) {
                            *field = v.clamp(0.0, max);
                        }
                    }
                    Ok("Style changed".into())
                })
            }

            // ----- Seeing and exporting -----
            "render_frame" => {
                let p = self.read()?;
                let t = num(a, "time")?.clamp(0.0, timeline::duration(&p));
                let width = a["width"].as_u64().unwrap_or(960).clamp(160, 1920) as usize;
                let model = self.prepared_model(&p)?;
                let png = render_png(&p, t, width, model.as_deref())?;
                Ok(Reply::Image { png, caption: format!("Frame at {t:.2} s") })
            }
            "render_strip" => {
                let p = self.read()?;
                let total = timeline::duration(&p);
                let times: Vec<f32> = match a["times"].as_array() {
                    Some(list) => list.iter().filter_map(|v| v.as_f64()).map(|t| (t as f32).clamp(0.0, total)).collect(),
                    None => {
                        let (t0, t1) = (num(a, "start")?.clamp(0.0, total), num(a, "end")?.clamp(0.0, total));
                        let count = a["count"].as_u64().unwrap_or(8).clamp(2, 24) as usize;
                        (0..count).map(|i| t0 + (t1 - t0) * i as f32 / (count - 1) as f32).collect()
                    }
                };
                if times.is_empty() || times.len() > 24 {
                    return Err("Between 1 and 24 times are needed".into());
                }
                let width = a["width"].as_u64().unwrap_or(320).clamp(120, 960) as usize;
                let columns = a["columns"].as_u64().map(|c| c as usize).unwrap_or(if times.len() <= 4 { times.len() } else { 4 });
                let model = self.prepared_model(&p)?;
                let png = render_strip(&p, &times, width, columns, model.as_deref())?;
                Ok(Reply::Image { png, caption: format!("{} frames: {}", times.len(), times.iter().map(|t| format!("{t:.2} s")).collect::<Vec<_>>().join(", ")) })
            }
            "render_preview" => {
                let p = self.read()?;
                let total = timeline::duration(&p);
                let t0 = num(a, "start")?.clamp(0.0, total);
                let t1 = num(a, "end")?.clamp(t0, total);
                if t1 - t0 < 0.05 {
                    return Err("The stretch is too short".into());
                }
                if t1 - t0 > 20.0 {
                    return Err("20 seconds at most; for more, use export".into());
                }
                let gif = a["format"].as_str() == Some("gif");
                let width = a["width"].as_u64().unwrap_or(480).clamp(160, 960) as usize;
                let fps = a["fps"].as_u64().unwrap_or(if gif { 15 } else { 30 }).clamp(5, 60) as f32;
                let model = self.prepared_model(&p)?;
                let out = std::env::temp_dir().join(format!("rec-ording preview {}.{}", timestamp_for_filename(), if gif { "gif" } else { "mp4" }));
                let started = std::time::Instant::now();
                render_preview(&p, t0, t1, width, fps, gif, model.as_deref(), &out)?;
                Ok(Reply::Text(format!("Preview from {t0:.2} s to {t1:.2} s ({width} px, {fps} fps) at {} ({:.1} s)", out.display(), started.elapsed().as_secs_f32())))
            }
            "export" => {
                if self.export.as_ref().is_some_and(|e| !e.done) {
                    return Err("An export is already running (see export_status)".into());
                }
                let a = a.clone();
                let mut p = self.read()?;
                if p.is_empty() {
                    return Err("The project has no video".into());
                }
                if let Some(v) = a["resolution"].as_str() {
                    p.export.resolution = Resolution::ALL.into_iter().find(|r| r.label().eq_ignore_ascii_case(v)).ok_or("resolution must be 720p, 1080p or 4K")?;
                }
                if let Some(v) = a["fps"].as_u64() {
                    p.export.fps = if v >= 45 { 60 } else { 30 };
                }
                if let Some(v) = a["format"].as_str() {
                    p.export.format = match v {
                        "h264" => ExportFormat::H264,
                        "hevc" => ExportFormat::Hevc,
                        "gif" => ExportFormat::Gif,
                        _ => return Err("format must be h264, hevc or gif".into()),
                    };
                }
                if let Some(v) = a["quality"].as_str() {
                    p.export.quality = match v {
                        "standard" => Quality::Standard,
                        "high" => Quality::High,
                        "max" => Quality::Max,
                        _ => return Err("quality must be standard, high or max".into()),
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
                        return Err(format!("The folder {v} doesn't exist"));
                    }
                    p.export.folder = v.to_string();
                }
                let _ = std::fs::create_dir_all(&p.export.folder);
                let (tx, rx) = unbounded();
                let cancel = Arc::new(AtomicBool::new(false));
                export::start(p, cancel.clone(), tx);
                self.export = Some(ExportJob { rx, cancel, status: "Exporting… 0 %".into(), encoder: String::new(), done: false });
                Ok(Reply::Text("Export started; check export_status".into()))
            }
            "get_hardware" => {
                let card = crate::gpu::detect();
                let enc = crate::hwenc::detect();
                let v = json!({
                    "graphics_card": card,
                    "encoder": enc.as_ref().map(|e| json!({"name": e.name, "h264": e.h264, "hevc": e.hevc})),
                    "export_uses_it_by_default": true,
                    "note": "With a graphics card, the export draws the frames on it and encodes with its encoder if it has one; export with hardware=false does everything on the processor.",
                });
                Ok(Reply::Text(v.to_string()))
            }
            "export_status" => {
                let job = self.export.as_mut().ok_or("No export has been started")?;
                while let Ok(ev) = job.rx.try_recv() {
                    match ev {
                        ExportEvent::Progress { fraction, .. } => job.status = format!("Exporting… {:.0} % ({})", fraction * 100.0, job.encoder),
                        ExportEvent::Encoder(name) => job.encoder = name,
                        ExportEvent::Done(path) => {
                            job.status = format!("Done: {}", path.display());
                            job.done = true;
                        }
                        ExportEvent::Failed(e) => {
                            job.status = format!("Error: {e}");
                            job.done = true;
                        }
                        ExportEvent::Cancelled => {
                            job.status = "Cancelled".into();
                            job.done = true;
                        }
                    }
                }
                Ok(Reply::Text(job.status.clone()))
            }
            "cancel_export" => {
                let job = self.export.as_ref().ok_or("No export has been started")?;
                job.cancel.store(true, Ordering::Relaxed);
                Ok(Reply::Text("Cancelling…".into()))
            }
            _ => Err(format!("Unknown tool: {name}")),
        }
    }
}

const INSTRUCTIONS: &str = "rec·ording makes promo videos of Android apps: phone recordings inside a frame or 3D model, on a background, with zooms and camera moves. Times are seconds on the timeline (video clips play back to back). x/y coordinates are in the recording, from 0 to 1 (0,0 is top left). Start with get_project; use render_frame to see the result. Every change is saved to the project open in the app and can be undone with undo.";

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
        1 => " (they replace 1 overlapping camera move; undo brings it back)".into(),
        n => format!(" (they replace {n} overlapping camera moves; undo brings them back)"),
    }
}

/// The optional `ease` argument.
fn parse_ease(a: &Value) -> Result<Option<Ease>, String> {
    match a["ease"].as_str() {
        None => Ok(None),
        Some(s) => Ease::ALL.into_iter().find(|e| ease_key(*e) == s).map(Some).ok_or("ease must be spring, smooth, linear or hold".into()),
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

/// Position, size, look and timing of a picture from the arguments that are present.
fn set_image_fields(p: &mut Project, id: u64, a: &Value) -> Result<(), String> {
    let c = p.images.iter_mut().find(|c| c.id == id).ok_or(format!("There is no image {id}"))?;
    for (k, v, lo, hi) in [("x", &mut c.x, -0.5, 1.5), ("y", &mut c.y, -0.5, 1.5), ("width", &mut c.width, 0.01, 2.0), ("opacity", &mut c.opacity, 0.0, 1.0), ("fade_in", &mut c.fade_in, 0.0, 10.0), ("fade_out", &mut c.fade_out, 0.0, 10.0)] {
        if let Some(x) = opt_num(a, k) {
            *v = x.clamp(lo, hi);
        }
    }
    let start = opt_num(a, "start").unwrap_or(c.start);
    let end = start + opt_num(a, "duration").unwrap_or(c.duration);
    timeline::place_image(p, id, start, end)
}

/// Track, timing, volume and fades of a sound from the arguments that are present.
fn set_sound_fields(p: &mut Project, id: u64, a: &Value) -> Result<(), String> {
    let c = p.audio.iter_mut().find(|c| c.id == id).ok_or(format!("There is no sound {id}"))?;
    if let Some(t) = opt_str(a, "track") {
        c.track = AudioTrack::from_key(&t).ok_or("track: effects, voice or music")?;
    }
    for (k, v, hi) in [("volume", &mut c.volume, 2.0), ("fade_in", &mut c.fade_in, 30.0), ("fade_out", &mut c.fade_out, 30.0)] {
        if let Some(x) = opt_num(a, k) {
            *v = x.clamp(0.0, hi);
        }
    }
    if let Some(o) = opt_num(a, "offset") {
        c.offset = o.clamp(0.0, (c.length - 0.05).max(0.0));
    }
    if let Some(l) = a["loop"].as_bool() {
        timeline::set_audio_loop(p, id, l)?;
    }
    let c = p.audio.iter().find(|c| c.id == id).ok_or("Not found")?;
    let (start, dur) = (opt_num(a, "start"), opt_num(a, "duration"));
    if start.is_some() || dur.is_some() {
        let (start, dur) = (start.unwrap_or(c.start), dur.unwrap_or(c.duration));
        timeline::move_audio(p, id, start)?;
        let c = p.audio.iter_mut().find(|c| c.id == id).unwrap();
        c.duration = if c.looped { dur.max(0.1) } else { dur.max(0.05).min(c.length - c.offset) };
    }
    Ok(())
}

fn text(a: &Value, k: &str) -> Result<String, String> {
    a[k].as_str().map(str::to_string).ok_or(format!("Missing parameter “{k}”"))
}

fn num(a: &Value, k: &str) -> Result<f32, String> {
    opt_num(a, k).ok_or(format!("Missing numeric parameter “{k}”"))
}

fn opt_num(a: &Value, k: &str) -> Option<f32> {
    a[k].as_f64().map(|v| v as f32).filter(|v| v.is_finite())
}

fn int(a: &Value, k: &str) -> Result<u64, String> {
    a[k].as_u64().ok_or(format!("Missing integer parameter “{k}”"))
}

fn hex(s: &str) -> Result<[u8; 3], String> {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("Invalid color: {s} (use #rrggbb)"))?;
    if h.len() != 6 {
        return Err(format!("Invalid color: {s} (use #rrggbb)"));
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
            json!({"id": c.clip.id, "start": c.start, "end": c.end, "source_in": c.clip.source_in, "source_out": c.clip.source_out, "speed": c.clip.speed, "freeze": c.clip.freeze,
                   "video": m.map(|m| m.info.path.clone()), "size": m.map(|m| format!("{}x{}", m.info.width, m.info.height)),
                   "touches": m.map(|m| m.touches.iter().filter(|t| t.time >= c.clip.source_in && t.time < c.clip.source_out).count())})
        }).collect::<Vec<_>>(),
        "marks": timeline::timeline_marks(p),
        "touches": timeline::timeline_touches(p).iter().map(|t| json!({"time": t.time, "x": t.x, "y": t.y, "kind": if t.kind == TouchKind::Tap {"tap"} else {"swipe"}})).collect::<Vec<_>>(),
        "camera": p.camera.iter().map(|c| json!({"id": c.id, "kind": c.kind.key(), "start": c.start, "end": c.end(),
            "keyframes": c.keys.iter().map(|k| json!({"time": c.start + k.t, "zoom": k.zoom, "x": k.x, "y": k.y, "tilt": k.tilt, "turn": k.turn, "roll": k.roll, "ease": ease_key(k.ease)})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "device_motion": p.device.iter().map(|d| {
            let spin = timeline::spin_of(d).map(|(axis, deg)| {
                let axis = ["vertical", "horizontal", "plane"][axis];
                json!({"axis": axis, "degrees": deg})
            });
            json!({"id": d.id, "name": crate::i18n::lz(&d.name), "start": d.start, "end": d.end(), "spin": spin})
        }).collect::<Vec<_>>(),
        "texts": p.texts.iter().map(|c| json!({"id": c.id, "start": c.start, "end": c.end(), "title": c.title, "subtitle": c.subtitle,
            "place": key_of(&TEXT_PLACES, c.place), "size": key_of(&TEXT_SIZES, c.size), "reveal": key_of(&TEXT_REVEALS, c.reveal), "color": key_of(&TEXT_COLORS, c.color), "make_room": c.make_room})).collect::<Vec<_>>(),
        "images": p.images.iter().map(|c| json!({"id": c.id, "path": c.path, "start": c.start, "end": c.end(), "x": c.x, "y": c.y, "width": c.width, "opacity": c.opacity, "fade_in": c.fade_in, "fade_out": c.fade_out})).collect::<Vec<_>>(),
        "sounds": p.audio.iter().map(|c| json!({"id": c.id, "track": c.track.key(), "path": c.path, "start": c.start, "end": c.end(), "offset": c.offset, "file_length": c.length, "volume": c.volume, "fade_in": c.fade_in, "fade_out": c.fade_out, "loop": c.looped})).collect::<Vec<_>>(),
        "mix": {"recording": p.mix.recording, "effects": p.mix.effects, "voice": p.mix.voice, "music": p.mix.music, "duck_music": p.mix.duck_music},
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
            "lighting": {"style": key_of(&LIGHT_STYLES, p.style.lighting.style), "angle": p.style.lighting.angle, "intensity": p.style.lighting.intensity, "glare": p.style.lighting.glare},
            "finish": {"motion_blur": key_of(&MOTION_BLURS, p.style.finish.motion_blur), "antialias": key_of(&ANTIALIAS, p.style.finish.antialias), "vignette": p.style.finish.vignette,
                "grain": p.style.finish.grain, "fade_in": p.style.finish.fade_in, "fade_out": p.style.finish.fade_out, "crossfade": p.style.finish.crossfade},
        },
        "export": {"resolution": p.export.resolution.label(), "fps": p.export.fps, "format": p.export.format.label(), "quality": p.export.quality.key(), "folder": p.export.folder, "hardware": p.export.hardware},
    })
}

/// Decodes one frame of the recording under `t`.
fn decode_frame(p: &Project, t: f32) -> Result<(Frame, f32, f32), String> {
    let c = timeline::locate(p, t).ok_or("The project has no video")?;
    let m = p.media(c.clip.media).ok_or("The recording is missing")?;
    let (w, h) = media::preview_size(&m.info);
    let fps = m.info.fps.max(1.0);
    let (input, timed) = media::timed_input(&m.info.path, timeline::source_time(&c, t), 1.0 / fps, fps, 1.0);
    let mut child = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error"])
        .args(input)
        .args(["-frames:v", "1", "-vf", &format!("{timed},scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut data = vec![0u8; w * h * 4];
    child.stdout.as_mut().unwrap().read_exact(&mut data).map_err(|_| "That frame couldn't be read".to_string())?;
    let _ = child.wait();
    Ok((Frame { w, h, data }, m.info.width as f32, m.info.height as f32))
}

/// Renders the frame at `t` as the export would, `width` pixels wide, and encodes it as PNG.
pub fn render_png(p: &Project, t: f32, width: usize, model: Option<&PreparedModel>) -> Result<Vec<u8>, String> {
    let (ow, oh) = out_size(p, width);
    let rgba = render_rgba(p, t, width, model)?;
    encode_png(rgba, ow, oh)
}

/// Output size for a width, in the project's aspect.
pub(crate) fn out_size(p: &Project, width: usize) -> (usize, usize) {
    (width, ((width as f32 / p.style.aspect.ratio()).round() as usize).max(2))
}

/// The frame at `t` as the export draws it, `width` pixels wide.
pub(crate) fn render_rgba(p: &Project, t: f32, width: usize, model: Option<&PreparedModel>) -> Result<Vec<u8>, String> {
    let (frame, vw, vh) = decode_frame(p, t)?;
    Ok(compose_at(p, t, &Mips::build(frame), vw, vh, width, model, p.style.finish.antialias.supersample()))
}

/// Composes the timeline at `t` over a decoded frame of its recording.
#[allow(clippy::too_many_arguments)]
fn compose_at(p: &Project, t: f32, mips: &Mips, vw: f32, vh: f32, width: usize, model: Option<&PreparedModel>, supersample: usize) -> Vec<u8> {
    let (ow, oh) = out_size(p, width);
    let mut rgba = vec![0u8; ow * oh * 4];
    let touches = timeline::timeline_touches(p);
    let texts = timeline::texts_at(p, t);
    let images = timeline::images_at(p, t);
    let blur = timeline::shutter(p, t, p.export.fps as f32);
    compose(
        &mut rgba,
        ow,
        oh,
        &ComposeInput {
            style: &p.style,
            screen: Some(mips),
            video_w: vw,
            video_h: vh,
            cam: timeline::camera_at(p, t),
            pose: timeline::pose_at(p, t),
            touches: &touches,
            time: t,
            since_start: t,
            model,
            supersample,
            texts: &texts,
            images: &images,
            blur: &blur,
            from: None,
        },
    );
    crate::post::apply(&mut rgba, ow, oh, &crate::post::at(&p.style, t, timeline::duration(p), p.export.fps as f32));
    rgba
}

fn encode_png(rgba: Vec<u8>, ow: usize, oh: usize) -> Result<Vec<u8>, String> {
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
        return Err("The image couldn't be created".into());
    }
    Ok(out.stdout)
}

/// Several moments in one image, in a grid, each with its time written on it.
pub fn render_strip(p: &Project, times: &[f32], width: usize, columns: usize, model: Option<&PreparedModel>) -> Result<Vec<u8>, String> {
    use rayon::prelude::*;
    let (fw, fh) = out_size(p, width);
    let frames: Vec<Vec<u8>> = times.par_iter().map(|&t| render_rgba(p, t, width, model)).collect::<Result<_, _>>()?;
    let cols = columns.clamp(1, times.len().max(1));
    let rows = times.len().div_ceil(cols);
    let gap = 6;
    let (w, h) = (cols * fw + (cols + 1) * gap, rows * fh + (rows + 1) * gap);
    let mut out = vec![0u8; w * h * 4];
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&[24, 24, 26, 255]);
    }
    for (k, (f, &t)) in frames.iter().zip(times).enumerate() {
        let (x0, y0) = (gap + (k % cols) * (fw + gap), gap + (k / cols) * (fh + gap));
        for y in 0..fh {
            let d = ((y0 + y) * w + x0) * 4;
            out[d..d + fw * 4].copy_from_slice(&f[y * fw * 4..(y + 1) * fw * 4]);
        }
        let size = (fw as f32 / 18.0).clamp(11.0, 22.0);
        crate::text::label(&mut out, w, h, x0 + 6, y0 + 6, &format!("{} s", format!("{t:.2}").replace('.', ",")), size);
    }
    encode_png(out, w, h)
}

/// A short, small video of the timeline from `t0` to `t1`, to see how something moves.
/// Returns where it was written.
#[allow(clippy::too_many_arguments)]
pub fn render_preview(p: &Project, t0: f32, t1: f32, width: usize, fps: f32, gif: bool, model: Option<&PreparedModel>, out: &Path) -> Result<(), String> {
    let n = (((t1 - t0) * fps).round() as usize).max(1);
    let (ow, oh) = out_size(p, width);
    let mut enc = command("ffmpeg");
    enc.args(["-nostdin", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{ow}x{oh}"), "-r", &format!("{fps}"), "-i", "-"]);
    if gif {
        enc.args(["-vf", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4", "-loop", "0"]);
    } else {
        enc.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "24", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
    }
    let mut enc = enc.arg(out).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let mut stdin = enc.stdin.take().unwrap();
    let total = timeline::duration(p);
    let mut k = 0;
    while k < n {
        let t = t0 + k as f32 / fps;
        let c = timeline::locate(p, t).ok_or("The project has no video")?;
        // The frames of this stretch that fall in this clip, decoded in one go.
        let run = (k..n).take_while(|&j| t0 + j as f32 / fps < c.end || c.end >= total).count().max(1);
        let m = p.media(c.clip.media).ok_or("The recording is missing")?;
        let (w, h) = media::preview_size(&m.info);
        let speed = c.clip.speed.max(0.01);
        let from = timeline::source_time(&c, t);
        let frames = if c.clip.freeze.is_some() { 1 } else { run };
        let len = (frames as f32 / fps * speed).min((c.clip.source_out - from).max(0.0)).max(1.0 / fps);
        let (input, timed) = media::timed_input(&m.info.path, from, len, fps, speed);
        let mut dec = command("ffmpeg")
            .args(["-nostdin", "-loglevel", "error"])
            .args(input)
            .args(["-frames:v", &frames.to_string(), "-vf", &format!("{timed},scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut last: Option<Mips> = None;
        let mut stdout = dec.stdout.take().unwrap();
        for j in 0..run {
            if j < frames {
                let mut data = vec![0u8; w * h * 4];
                if stdout.read_exact(&mut data).is_ok() {
                    last = Some(Mips::build(Frame { w, h, data }));
                }
            }
            let mips = last.as_ref().ok_or("The recording couldn't be read")?;
            let tj = t0 + (k + j) as f32 / fps;
            let rgba = compose_at(p, tj, mips, m.info.width as f32, m.info.height as f32, width, model, 1);
            stdin.write_all(&rgba).map_err(|_| "ffmpeg stopped accepting frames".to_string())?;
        }
        let _ = dec.kill();
        let _ = dec.wait();
        k += run;
    }
    drop(stdin);
    let res = enc.wait_with_output().map_err(|e| e.to_string())?;
    if !res.status.success() {
        return Err(format!("The preview couldn't be created: {}", String::from_utf8_lossy(&res.stderr).trim()));
    }
    Ok(())
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
const LIGHT_STYLES: [(&str, LightStyle); 6] = [
    ("studio", LightStyle::Studio),
    ("soft", LightStyle::Soft),
    ("dramatic", LightStyle::Dramatic),
    ("backlit", LightStyle::Backlit),
    ("warm", LightStyle::Warm),
    ("cool", LightStyle::Cool),
];
const MOTION_BLURS: [(&str, MotionBlur); 4] = [("off", MotionBlur::Off), ("soft", MotionBlur::Soft), ("cinematic", MotionBlur::Cinematic), ("strong", MotionBlur::Strong)];
const ANTIALIAS: [(&str, Antialias); 3] = [("normal", Antialias::Normal), ("high", Antialias::High), ("max", Antialias::Max)];
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
    table.iter().find(|(k, _)| *k == v).map(|(_, x)| Some(*x)).ok_or(format!("{field} must be {}", keys.join(", ")))
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
    if let Some(v) = a["face_text"].as_bool() {
        c.face_text = v;
    }
    if let Some(v) = opt_num(a, "move_time") {
        c.move_time = v.clamp(0.1, 3.0);
    }
    Ok(())
}

/// A project by name or file name.
fn find_project(name: &str) -> Result<PathBuf, String> {
    list_projects()
        .into_iter()
        .find(|e| e.name == name || e.path.file_name().is_some_and(|f| f.to_string_lossy() == name))
        .map(|e| e.path)
        .ok_or(format!("There is no project “{name}”"))
}

fn tool(name: &str, description: &str, mut props: Value, required: &[&str]) -> Value {
    // Any tool can act on a project other than the one open in the app.
    if let Some(map) = props.as_object_mut() {
        map.insert("project".into(), s("Project to act on (name or file); by default, the one open in the app"));
    }
    // A screen change (detect_marks) can stand in for the time.
    let marked = MARKED_TOOLS.contains(&name);
    if marked {
        if let Some(map) = props.as_object_mut() {
            map.insert("mark".into(), i("Mark number from detect_marks (a screen change) instead of the time; what is placed follows that moment when clips are trimmed"));
        }
    }
    let required: Vec<&str> = required.iter().copied().filter(|r| !marked || !matches!(*r, "time" | "start" | "end")).collect();
    json!({"name": name, "description": description, "inputSchema": {"type": "object", "properties": props, "required": required}})
}

/// A title as add_text takes it (for export_texts and import_texts).
fn text_json(c: &TextClip) -> Value {
    json!({"time": c.start, "duration": c.duration, "title": c.title, "subtitle": c.subtitle,
        "place": key_of(&TEXT_PLACES, c.place), "size": key_of(&TEXT_SIZES, c.size), "reveal": key_of(&TEXT_REVEALS, c.reveal),
        "color": key_of(&TEXT_COLORS, c.color), "make_room": c.make_room, "face_text": c.face_text, "move_time": c.move_time})
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
        "zoom": n("Zoom (1 = no zoom)"), "x": n("Focus point, 0..1 of the recording"), "y": n("Focus point, 0..1"),
        "tilt": n("Added tilt, degrees"), "turn": n("Added turn, degrees"), "roll": n("Added roll, degrees"),
        "ease": s("How it arrives: spring, smooth, linear or hold"),
    });
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        b.extend(e.clone());
    }
    base
}

pub fn tools() -> Vec<Value> {
    vec![
        tool("get_project", "Full state of the open project: clips, taps, camera, Director, style and export.", json!({}), &[]),
        tool("list_projects", "Saved projects.", json!({}), &[]),
        tool("create_project", "Creates an empty project (with the current one's style) and opens it.", json!({"name": s("Name")}), &["name"]),
        tool("open_project", "Opens a project by name or file.", json!({"name": s("Name or file")}), &["name"]),
        tool("rename_project", "Renames the open project.", json!({"name": s("New name")}), &["name"]),
        tool("undo", "Undoes the last change made by this server.", json!({}), &[]),
        tool("redo", "Redoes the last undone change.", json!({}), &[]),
        tool("list_android_devices", "Android phones connected over USB and open emulators.", json!({}), &[]),
        tool(
            "start_recording",
            "Starts recording a phone's screen (and its taps).",
            json!({"serial": s("Phone serial (by default the first one ready)"), "audio": b("Record the phone's sound"), "mirror": b("Show the phone's screen on the computer"), "clean_status_bar": b("Android demo mode while recording: 9:41, full battery, no notifications (on by default)")}),
            &[],
        ),
        tool(
            "play_gestures",
            "Taps, swipes and types on the phone (adb shell input), one after another. While recording with start_recording, every tap and swipe is recorded: it gets its circle and its automatic zooms. Coordinates from 0 to 1 of the screen. Pinching isn't possible (adb injects a single finger).",
            json!({
                "gestures": {"type": "array", "description": "Gestures in order", "items": {"type": "object", "properties": {
                    "type": {"type": "string", "enum": ["tap", "swipe", "long_press", "text", "key", "wait"]},
                    "x": {"type": "number"}, "y": {"type": "number"},
                    "to_x": {"type": "number", "description": "End of the swipe"}, "to_y": {"type": "number"},
                    "duration": {"type": "number", "description": "Seconds (swipe 0.3; long press 0.8; wait: the pause)"},
                    "text": {"type": "string", "description": "Text to type"},
                    "key": {"type": "string", "description": "back, home, enter, delete, tab, recents… or a KEYCODE_"}
                }, "required": ["type"]}},
                "serial": s("Phone, when not recording (by default the first one ready)")
            }),
            &["gestures"],
        ),
        tool("stop_recording", "Stops recording and adds it at the end of the timeline, with automatic zooms.", json!({}), &[]),
        tool("import_video", "Adds a video at the end of the timeline.", json!({"path": s("Video path")}), &["path"]),
        tool("split_clip", "Splits the clip at a moment.", json!({"time": n("Second on the timeline")}), &["time"]),
        tool("trim_clip", "Changes which part of the recording a clip plays.", json!({"clip_id": i("Clip"), "source_in": n("Start in the recording, s"), "source_out": n("End in the recording, s")}), &["clip_id"]),
        tool("move_clip", "Changes a clip's order.", json!({"clip_id": i("Clip"), "index": i("New position, from 0")}), &["clip_id", "index"]),
        tool("delete_clip", "Removes a clip from the timeline.", json!({"clip_id": i("Clip")}), &["clip_id"]),
        tool(
            "detect_marks",
            "Finds the screen changes in the recordings (scrolling, keyboard, dialogs…) and returns them as marks in timeline seconds. add_text, add_zoom, add_device_motion, add_camera_keyframe, freeze_frame and director_add_target take mark (the mark number) instead of the time. What is placed this way follows its moment of the recording when clips are trimmed or moved.",
            json!({"again": b("Search again even if they are there")}),
            &[],
        ),
        tool("set_clip_speed", "Plays a clip faster or slower (0.25 to 4). What is on top of it (zooms, titles…) moves with it.", json!({"clip_id": i("Clip"), "speed": n("0.25..4")}), &["clip_id", "speed"]),
        tool("freeze_frame", "Freezes the picture at a moment for a few seconds (for example, to make the ending longer); what comes after is shifted.", json!({"time": n("s"), "duration": n("s (2 by default)")}), &["time"]),
        tool("set_freeze_duration", "Changes how long a frozen frame lasts.", json!({"clip_id": i("Frozen clip"), "duration": n("s")}), &["clip_id", "duration"]),
        tool("replace_clip_video", "Puts another recording in a clip, keeping its trim, its speed and everything on top of it on the timeline.", json!({"clip_id": i("Clip"), "path": s("New video")}), &["clip_id", "path"]),
        tool("auto_zoom", "Rebuilds the automatic zooms from the taps (keeps the ones made by hand).", json!({"zoom": n("Intensity, 1.2..3")}), &[]),
        tool("add_zoom", "Zoom on a point between two moments.", json!({"start": n("Start, s"), "end": n("End, s"), "zoom": n("Zoom (2 by default)"), "x": n("0..1"), "y": n("0..1")}), &["start", "end"]),
        tool("add_camera_keyframe", "Camera keyframe at a moment: inside the camera clip at that moment, or as a new move (which returns to rest). Use it for 3D turns halfway through the video.", cam_props(json!({"time": n("Second on the timeline")})), &["time"]),
        tool("update_camera_keyframe", "Changes a camera keyframe.", cam_props(json!({"clip_id": i("Camera clip"), "index": i("Keyframe index"), "time": n("New moment, s")})), &["clip_id", "index"]),
        tool("delete_camera_keyframe", "Deletes a camera keyframe.", json!({"clip_id": i("Camera clip"), "index": i("Index")}), &["clip_id", "index"]),
        tool("delete_camera_clip", "Deletes a camera clip (zoom, move or shot).", json!({"clip_id": i("Camera clip")}), &["clip_id"]),
        tool("move_camera_clip", "Moves a camera clip.", json!({"clip_id": i("Camera clip"), "start": n("New start, s")}), &["clip_id", "start"]),
        tool("director_add_target", "Director target: a box of the recording at a moment (replaces the one at that moment).", json!({"time": n("s"), "x0": n("0..1"), "y0": n("0..1"), "x1": n("0..1"), "y1": n("0..1")}), &["time", "x0", "y0", "x1", "y1"]),
        tool("director_remove_target", "Deletes a target.", json!({"target_id": i("Target")}), &["target_id"]),
        tool("director_clear", "Deletes every Director target and shot.", json!({}), &[]),
        tool("director_settings", "Director movement type and intensity.", json!({"motion": s("3d or 2d"), "intensity": n("0..1")}), &[]),
        tool("director_generate", "Generates one camera shot per target (replaces the camera clips that overlap).", json!({}), &[]),
        tool("director_retake", "Another variation of the move for a target (or all of them) and regenerates.", json!({"target_id": i("Target (all if omitted)")}), &[]),
        tool("list_device_options", "Frames, colors, 3D models in the gallery and device moves available.", json!({}), &[]),
        tool("set_device", "Chooses a built-in frame, a color or a 3D model from the gallery.", json!({"frame": s("rounded, flat, curved, tablet or none"), "frame_color": s("obsidian, silver, sand, blue or white"), "model": s("Name or path of a gallery model")}), &[]),
        tool("import_model", "Imports a .glb/.gltf model (or the downloaded .zip) into the gallery and uses it.", json!({"path": s("Path")}), &["path"]),
        tool("set_model_options", "Settings of the 3D model in use.", json!({"screen_part": json!({"description": "\"auto\" or a part index"}), "quarter_turns": i("90° turns of the screen"), "animation": json!({"description": "Animation index or \"none\""}), "loop": b("Loop"), "animation_start": n("Second the animation starts at"), "video_fit": s("fill, fit or stretch"), "fit_pan": n("Framing with fill, 0..1")}), &[]),
        tool("set_angle", "Resting angle of the device and global movements.", json!({"tilt": n("degrees"), "turn": n("degrees"), "roll": n("degrees"), "straighten_on_zoom": b("Face the camera when zooming in"), "float": b("Gentle sway"), "intro": b("Spin in at the start")}), &[]),
        tool(
            "add_device_motion",
            "Adds a device move at a moment. full_turn turns 360° around the vertical axis and flip around the horizontal one; near a cut they center on it, so the cut happens while the back is showing.",
            json!({"preset": s("full_turn, flip, spin_in, drop_in, rise_up, hop or exit"), "time": n("s (exit ends at that moment)"), "degrees": n("Turns only: degrees (360, 720, -360 for the other direction…)"), "duration": n("s"), "ease": s("Curve: spring (with bounce), smooth, linear or hold")}),
            &["preset", "time"],
        ),
        tool(
            "add_text",
            "Adds a title over the video, in the style of Apple's ads: short phrases next to the phone, which moves aside to make room. Use line breaks (\\n) for several lines.",
            json!({"time": n("s"), "title": s("Headline"), "subtitle": s("Smaller line below (optional)"), "duration": n("s (3 by default)"),
                   "place": s("left, right, top, bottom or center"), "size": s("small, medium or large (large: a figure or a big headline)"),
                   "reveal": s("fade, words (word by word) or lines (line by line, earlier lines dim)"), "color": s("auto, light or dark"), "make_room": b("Move the phone aside (on by default)"), "face_text": b("The phone turns slightly towards the title as it moves aside"), "move_time": n("s the phone takes to move aside and back (0.7 by default)")}),
            &["time", "title"],
        ),
        tool(
            "update_text",
            "Changes a title: text, moment, duration or style.",
            json!({"text_id": i("Text"), "title": s("Headline"), "subtitle": s("Small line"), "start": n("s"), "duration": n("s"), "place": s("left, right, top, bottom or center"),
                   "size": s("small, medium or large"), "reveal": s("fade, words or lines"), "color": s("auto, light or dark"), "make_room": b("Move the phone aside"), "face_text": b("Turn the phone towards the title"), "move_time": n("s of the move aside")}),
            &["text_id"],
        ),
        tool("delete_text", "Deletes a title.", json!({"text_id": i("Text")}), &["text_id"]),
        tool(
            "add_image",
            "Puts an image (PNG, WebP or JPEG; with transparency) over the video, above the video and below the titles. Several images can show at once.",
            json!({"path": s("Image path"), "time": n("s"), "duration": n("s (3 by default)"), "x": n("Center, 0..1 of the video's width"), "y": n("Center, 0..1 of the height"),
                   "width": n("Width, fraction of the video's width (0.35 by default)"), "opacity": n("0..1"), "fade_in": n("s of fade in"), "fade_out": n("s of fade out")}),
            &["path", "time"],
        ),
        tool(
            "update_image",
            "Changes an image: moment, duration, position, size, opacity or fades.",
            json!({"image_id": i("Image"), "start": n("s"), "duration": n("s"), "x": n("0..1"), "y": n("0..1"), "width": n("Fraction of the width"), "opacity": n("0..1"), "fade_in": n("s"), "fade_out": n("s")}),
            &["image_id"],
        ),
        tool("delete_image", "Removes an image.", json!({"image_id": i("Image")}), &["image_id"]),
        tool(
            "add_sound",
            "Adds a sound to a track: effects, voice (voice-over) or music. The sounds of a track can overlap and are mixed. Music fills the rest of the video (looping if the file is shorter); effects and voice play once.",
            json!({"path": s("File path (mp3, wav, m4a, ogg, flac…)"), "track": s("effects, voice or music"), "time": n("s it starts at"), "duration": n("s it plays for"),
                   "offset": n("s into the file where it starts"), "volume": n("1 = as is (0..2)"), "fade_in": n("s"), "fade_out": n("s"), "loop": b("Start over when the file ends")}),
            &["path", "track", "time"],
        ),
        tool(
            "update_sound",
            "Changes a sound: track, moment, duration, where it starts in the file, volume, fades or looping.",
            json!({"sound_id": i("Sound"), "track": s("effects, voice or music"), "start": n("s"), "duration": n("s"), "offset": n("s into the file"), "volume": n("0..2"), "fade_in": n("s"), "fade_out": n("s"), "loop": b("Repeat")}),
            &["sound_id"],
        ),
        tool("delete_sound", "Removes a sound.", json!({"sound_id": i("Sound")}), &["sound_id"]),
        tool(
            "set_mix",
            "Volume of each source in the mix (1 = as is, 0..2) and whether the music lowers while the voice-over speaks.",
            json!({"recording": n("Sound of the recordings"), "effects": n("Effects"), "voice": n("Voice-over"), "music": n("Music"), "duck_music": b("Lower the music under the voice")}),
            &[],
        ),
        tool("export_texts", "Every title as a JSON list (the same fields as add_text), to edit them elsewhere and load them back with import_texts.", json!({}), &[]),
        tool(
            "import_texts",
            "Loads a list of titles (like the one from export_texts). By default it replaces the current ones.",
            json!({"texts": json!({"type": "array", "items": {"type": "object"}, "description": "Titles: time, duration, title, subtitle, place, size, reveal, color, make_room"}), "replace": b("Replace the current titles (yes by default)")}),
            &["texts"],
        ),
        tool(
            "batch",
            "Several operations in a single call and a single undo step. If one fails, none is applied. Useful for animations with many keyframes.",
            json!({"operations": json!({"type": "array", "items": {"type": "object", "properties": {"name": s("Tool"), "arguments": json!({"type": "object"})}, "required": ["name"]}, "description": "List of {name, arguments}"})}),
            &["operations"],
        ),
        tool(
            "update_device_motion",
            "Changes a device move already added: when it starts, how long it lasts and, for turns, how many degrees it turns (360, 720, -360 for the other direction…).",
            json!({"clip_id": i("Clip"), "start": n("s"), "duration": n("s"), "degrees": n("Turns only"), "ease": s("Curve: spring, smooth, linear or hold")}),
            &["clip_id"],
        ),
        tool("delete_device_motion", "Deletes a device move.", json!({"clip_id": i("Clip")}), &["clip_id"]),
        tool(
            "get_hardware",
            "Which graphics card and which hardware encoder the export will use on this computer (if any).",
            json!({}),
            &[],
        ),
        tool(
            "set_style",
            "Format, background, margin, shadow, taps, lighting and finish (motion blur, edges, vignette, grain and fades).",
            json!({"aspect": s("16:9, 9:16, 1:1 or 4:5"), "background_preset": s("Midnight, Aurora, Sunset, Lavender, Mint, Ocean, Peach, Forest, Graphite or Cloud"), "background_a": s("#rrggbb"), "background_b": s("#rrggbb"), "background_angle": n("degrees"), "padding": n("0.02..0.25"), "shadow": n("0..1"), "show_touches": b("Show the taps"), "touch_size": n("0.5..2"),
                "lighting": s("studio, soft, dramatic (side light and a rim of light, for dark backgrounds), backlit, warm or cool"), "light_angle": n("Turns the light, degrees (-180..180)"), "light_intensity": n("0.5..1.5"), "glare": n("Glare on the glass, 0..2"),
                "motion_blur": s("off, soft, cinematic or strong"), "antialias": s("normal, high or max (cleaner, slower)"), "vignette": n("0..1"), "grain": n("0..1"),
                "fade_in": n("s from black at the start"), "fade_out": n("s to black at the end"), "crossfade": n("s of crossfade between clips (0 = cut)")}),
            &[],
        ),
        tool("render_frame", "Picture of how the video looks at a moment.", json!({"time": n("s"), "width": i("Width in pixels (960 by default)")}), &["time"]),
        tool(
            "export",
            "Exports the video (in the background). By default it encodes on the graphics card if there is a compatible one (NVIDIA, AMD or Intel); if that fails, on the processor.",
            json!({"resolution": s("720p, 1080p or 4K"), "fps": i("30 or 60"), "format": s("h264, hevc or gif"), "quality": s("standard, high or max"), "include_audio": b("Include sound"), "folder": s("Destination folder"), "hardware": b("Encode on the graphics card")}),
            &[],
        ),
        tool(
            "render_strip",
            "Several moments in one picture, in a grid, with the time written on each one. Give times, or start, end and count (spread out).",
            json!({"times": {"type": "array", "items": {"type": "number"}, "description": "Seconds (up to 24)"}, "start": n("s"), "end": n("s"), "count": i("How many between start and end (8 by default)"), "width": i("Width of each frame (320 by default)"), "columns": i("Columns (4 by default)")}),
            &[],
        ),
        tool(
            "render_preview",
            "A short, small video of a stretch (up to 20 s) to see whether an animation is smooth or bounces. Returns the file's path.",
            json!({"start": n("s"), "end": n("s"), "format": {"type": "string", "enum": ["mp4", "gif"]}, "width": i("Width (480 by default)"), "fps": i("Frames per second (30 in mp4, 15 in gif)")}),
            &["start", "end"],
        ),
        tool("export_status", "Export progress or result.", json!({}), &[]),
        tool("cancel_export", "Cancels the running export.", json!({}), &[]),
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
