use crate::camera::{auto_zooms, CamState, CameraTrack};
use crate::capture::{self, CaptureResult, Device, DeviceState, Method, RecordOptions, Recording, Tools};
use crate::compositor::{compose, ComposeInput, Layout};
use crate::export::{self, ExportEvent};
use crate::media::{self, DecodedFrame, PreviewDecoder, PREVIEW_FPS};
use crate::model::*;
use crate::library::{self, LibraryModel};
use crate::model3d::{self, ModelSource, PreparedModel};
use std::collections::HashMap;
use crate::theme::{self, *};
use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{
    Align, Align2, Color32, ColorImage, CursorIcon, Id, Key, Margin, Order, Pos2, Rect, RichText, Rounding, Sense,
    Stroke, TextureHandle, TextureOptions, Ui, UiBuilder, Vec2,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

enum AppEvent {
    Devices(Vec<Device>),
    MediaLoaded { info: MediaInfo, touches: Vec<Touch>, recorded: bool },
    MediaFailed(String),
    RecordingFailed(String),
    ProxyReady { source: String, proxy: PathBuf },
    Thumbnails { source: String, size: (usize, usize), frames: Vec<Vec<u8>> },
    ImportPicked(PathBuf),
    FolderPicked(PathBuf),
    PickerUnavailable,
    ModelPicked(PathBuf),
    ModelLoaded { path: String, result: Result<Arc<ModelSource>, String> },
    ModelThumb { path: String, src: Option<Arc<ModelSource>>, size: (usize, usize), rgba: Option<Vec<u8>> },
    ModelImported(Result<(String, Arc<ModelSource>), String>),
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Sheet {
    None,
    Record,
    Export,
    RenameModel,
    DeleteModel,
}

/// An imported model being renamed or deleted.
struct ModelEdit {
    entry: PathBuf,
    path: String,
    name: String,
    error: Option<String>,
}

enum ExportUi {
    Idle,
    Running { fraction: f32, eta: Option<f32>, cancel: Arc<AtomicBool> },
    Done(PathBuf),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Scrub,
    TrimStart,
    TrimEnd,
    ZoomStart(u64),
    ZoomEnd(u64),
    ZoomMove { id: u64, grab: f32 },
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Ruler,
    Video,
    TrimStart,
    TrimEnd,
    ZoomStart(u64),
    ZoomEnd(u64),
    ZoomBody(u64, f32),
    ZoomEmpty,
    None,
}

#[derive(PartialEq, Clone)]
struct RenderKey {
    time: f32,
    frame: usize,
    style: Style,
    cam: (u32, u32, u32),
    size: (usize, usize),
    touches: usize,
    model: usize,
    supersample: usize,
}

/// Timeline geometry: maps seconds to x and back.
struct Tl {
    x0: f32,
    x1: f32,
    dur: f32,
    ruler: Rect,
    video: Rect,
    zoom: Rect,
}

impl Tl {
    fn x(&self, t: f32) -> f32 {
        self.x0 + (t / self.dur) * (self.x1 - self.x0)
    }
    fn t(&self, x: f32) -> f32 {
        ((x - self.x0) / (self.x1 - self.x0) * self.dur).clamp(0.0, self.dur)
    }
}

pub struct RecOrdingApp {
    project: Project,
    last_saved: String,
    last_save_check: Instant,
    tools: Tools,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    ctx: egui::Context,

    // Playback
    time: f32,
    playing: bool,
    last_tick: Instant,
    decoder: PreviewDecoder,
    gen: u64,
    shown: Option<DecodedFrame>,
    queued: Option<DecodedFrame>,
    requested_time: Option<f32>,
    preview_size: (usize, usize),

    // Viewer
    viewer_tex: Option<TextureHandle>,
    viewer_buf: Vec<u8>,
    last_render: Option<RenderKey>,
    cam_track: CameraTrack,
    cam_sig: Vec<ZoomSegment>,
    cam_dur: f32,
    thumbs: Vec<TextureHandle>,
    thumb_aspect: f32,

    // Imported 3D model: the loaded file, and the copy prepared for the current settings.
    model_cache: HashMap<String, Arc<ModelSource>>,
    model_prepared: Option<(CustomModel, Arc<PreparedModel>)>,
    model_loading: Option<String>,
    model_error: Option<CustomModel>,
    /// A model picked in the gallery that is still loading; gets default settings once loaded.
    model_pending: Option<String>,
    library: Vec<LibraryModel>,
    library_thumbs: HashMap<String, TextureHandle>,
    /// Gallery models that could not be loaded.
    library_broken: std::collections::HashSet<String>,
    importing: bool,
    model_edit: Option<ModelEdit>,

    // Editing
    selected_zoom: Option<u64>,
    drag: Option<Drag>,

    // Sheets
    sheet: Sheet,
    devices: Vec<Device>,
    scanning: bool,
    last_scan: Option<Instant>,
    selected_serial: Option<String>,
    rec_opts: RecordOptions,
    recording: Option<Recording>,
    saving_recording: bool,
    record_error: Option<String>,
    export_ui: ExportUi,
    export_rx: Option<Receiver<ExportEvent>>,

    toast: Option<(String, Instant)>,
}

fn fmt_time(t: f32) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0).floor() as u32;
    let s = t - m as f32 * 60.0;
    format!("{m}:{s:04.1}").replace('.', ",")
}

fn fmt_secs(t: f32) -> String {
    format!("{t:.1} s").replace('.', ",")
}

fn open_path(path: &std::path::Path) {
    let _ = Command::new("xdg-open").arg(path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

fn home_relative(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// Runs the desktop's file dialog (kdialog on KDE, zenity elsewhere).
/// `Err` means no dialog tool is available, `Ok(None)` that the user cancelled.
fn run_picker(zenity: &[&str], kdialog: &[&str]) -> Result<Option<String>, ()> {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").map(|d| d.to_uppercase().contains("KDE")).unwrap_or(false);
    let mut tries: Vec<(&str, &[&str])> = vec![("zenity", zenity), ("kdialog", kdialog)];
    if kde {
        tries.reverse();
    }
    for (cmd, args) in tries {
        if let Ok(out) = Command::new(cmd).args(args).output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((out.status.success() && !s.is_empty()).then_some(s));
        }
    }
    Err(())
}

impl RecOrdingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let (tx, rx) = unbounded();
        let project = load_last_project();
        let notify_ctx = cc.egui_ctx.clone();
        let mut app = Self {
            last_saved: serde_json::to_string(&project).unwrap_or_default(),
            project,
            last_save_check: Instant::now(),
            tools: capture::detect_tools(),
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            time: 0.0,
            playing: false,
            last_tick: Instant::now(),
            decoder: PreviewDecoder::new(move || notify_ctx.request_repaint()),
            gen: 0,
            shown: None,
            queued: None,
            requested_time: None,
            preview_size: (2, 2),
            viewer_tex: None,
            viewer_buf: Vec::new(),
            last_render: None,
            cam_track: CameraTrack::empty(),
            cam_sig: Vec::new(),
            cam_dur: -1.0,
            thumbs: Vec::new(),
            thumb_aspect: 0.5,
            model_cache: HashMap::new(),
            model_prepared: None,
            model_loading: None,
            model_error: None,
            model_pending: None,
            library: Vec::new(),
            library_thumbs: HashMap::new(),
            library_broken: Default::default(),
            importing: false,
            model_edit: None,
            selected_zoom: None,
            drag: None,
            sheet: Sheet::None,
            devices: Vec::new(),
            scanning: false,
            last_scan: None,
            selected_serial: None,
            rec_opts: RecordOptions { audio: true, mirror: true },
            recording: None,
            saving_recording: false,
            record_error: None,
            export_ui: ExportUi::Idle,
            export_rx: None,
            toast: None,
        };
        if let Some(m) = app.project.media.clone() {
            app.time = app.project.trim_start;
            app.open_media(&m);
        }
        if let Some(cm) = app.project.style.model.clone() {
            app.load_model(cm.path);
        }
        app.refresh_library();
        app
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // ---------- Media ----------

    fn open_media(&mut self, m: &MediaInfo) {
        self.preview_size = media::preview_size(m);
        self.decoder.set_source(&m.path, self.preview_size);
        self.shown = None;
        self.queued = None;
        self.requested_time = None;
        self.thumbs.clear();
        self.thumb_aspect = m.width as f32 / m.height.max(1) as f32;
        self.last_render = None;

        let (tx, ctx, info) = (self.tx.clone(), self.ctx.clone(), m.clone());
        thread::spawn(move || {
            let Ok(proxy) = media::build_proxy(&info) else { return };
            let _ = tx.send(AppEvent::ProxyReady { source: info.path.clone(), proxy: proxy.clone() });
            ctx.request_repaint();
            let aspect = info.width as f32 / info.height.max(1) as f32;
            if let Some((size, frames)) = media::thumbnails(&proxy, info.duration, aspect, 24, 120) {
                let _ = tx.send(AppEvent::Thumbnails { source: info.path.clone(), size, frames });
                ctx.request_repaint();
            }
        });
    }

    fn import(&mut self, path: PathBuf) {
        self.toast("Abriendo vídeo…");
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let ev = match media::probe(&path.to_string_lossy()) {
                Ok(info) => AppEvent::MediaLoaded { info, touches: Vec::new(), recorded: false },
                Err(e) => AppEvent::MediaFailed(e),
            };
            let _ = tx.send(ev);
            ctx.request_repaint();
        });
    }

    fn pick_video(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let home = std::env::var("HOME").unwrap_or_default();
        thread::spawn(move || {
            let res = run_picker(
                &[
                    "--file-selection",
                    "--title=Importar vídeo",
                    "--file-filter=Vídeos | *.mp4 *.MP4 *.mov *.MOV *.mkv *.webm",
                    "--file-filter=Todos los archivos | *",
                ],
                &["--getopenfilename", &home, "Vídeos (*.mp4 *.MP4 *.mov *.MOV *.mkv *.webm)"],
            );
            match res {
                Ok(Some(p)) => {
                    let _ = tx.send(AppEvent::ImportPicked(PathBuf::from(p)));
                }
                Ok(None) => {}
                Err(()) => {
                    let _ = tx.send(AppEvent::PickerUnavailable);
                }
            }
            ctx.request_repaint();
        });
    }

    fn pick_model(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let home = std::env::var("HOME").unwrap_or_default();
        thread::spawn(move || {
            let res = run_picker(
                &["--file-selection", "--title=Importar modelo 3D", "--file-filter=Modelos 3D | *.glb *.gltf *.GLB *.GLTF *.zip *.ZIP"],
                &["--getopenfilename", &home, "Modelos 3D (*.glb *.gltf *.zip)"],
            );
            match res {
                Ok(Some(p)) => {
                    let _ = tx.send(AppEvent::ModelPicked(PathBuf::from(p)));
                }
                Ok(None) => {}
                Err(()) => {
                    let _ = tx.send(AppEvent::PickerUnavailable);
                }
            }
            ctx.request_repaint();
        });
    }

    fn load_model(&mut self, path: String) {
        if self.model_cache.contains_key(&path) || self.model_loading.as_deref() == Some(path.as_str()) {
            return;
        }
        self.model_loading = Some(path.clone());
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let result = model3d::load(&path).map(Arc::new);
            let _ = tx.send(AppEvent::ModelLoaded { path, result });
            ctx.request_repaint();
        });
    }

    /// Re-reads the gallery and renders thumbnails for models that do not have one yet.
    fn refresh_library(&mut self) {
        self.library = library::list();
        let todo: Vec<String> = self
            .library
            .iter()
            .map(|m| m.path.clone())
            .filter(|p| !self.library_thumbs.contains_key(p) && !self.library_broken.contains(p))
            .collect();
        if todo.is_empty() {
            return;
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let cached: HashMap<String, Arc<ModelSource>> =
            todo.iter().filter_map(|p| self.model_cache.get(p).map(|s| (p.clone(), s.clone()))).collect();
        thread::spawn(move || {
            let size = (132usize, 156usize);
            for path in todo {
                let src = cached.get(&path).cloned().or_else(|| model3d::load(&path).ok().map(Arc::new));
                let rgba = src.as_ref().and_then(|s| library::thumbnail(s, size.0, size.1));
                let _ = tx.send(AppEvent::ModelThumb { path, src, size, rgba });
                ctx.request_repaint();
            }
        });
    }

    fn default_model_settings(path: &str, src: &ModelSource) -> CustomModel {
        CustomModel {
            path: path.to_string(),
            animation: (!src.animations.is_empty()).then_some(0),
            ..Default::default()
        }
    }

    /// Uses a gallery model as the device.
    fn select_model(&mut self, path: String) {
        let style = &mut self.project.style;
        style.frame = FrameStyle::Custom;
        if style.model.as_ref().is_some_and(|m| m.path == path) {
            return;
        }
        match self.model_cache.get(&path).cloned() {
            Some(src) => {
                if src.guess.is_none() {
                    self.toast("No se encontró la pantalla del modelo: elígela en «Pantalla».");
                }
                self.project.style.model = Some(Self::default_model_settings(&path, &src));
            }
            None => {
                self.model_pending = Some(path.clone());
                self.load_model(path);
            }
        }
    }

    /// Keeps the prepared model in step with the style (screen part, turns, file).
    fn sync_model(&mut self) {
        let Some(cm) = self.project.style.model.clone() else {
            self.model_prepared = None;
            return;
        };
        let same = |k: &CustomModel| k.path == cm.path && k.screen_part == cm.screen_part && k.quarter_turns == cm.quarter_turns;
        if self.model_prepared.as_ref().is_some_and(|(k, _)| same(k)) || self.model_error.as_ref().is_some_and(same) {
            return;
        }
        let Some(src) = self.model_cache.get(&cm.path) else {
            self.load_model(cm.path.clone());
            return;
        };
        let Some(part) = cm.screen(src.guess, src.parts.len()) else {
            self.model_prepared = None;
            self.model_error = Some(cm);
            self.toast("No se encontró la pantalla del modelo: elígela en «Pantalla».");
            return;
        };
        match model3d::prepare(src, part, cm.quarter_turns) {
            Ok(m) => {
                self.model_prepared = Some((cm, Arc::new(m)));
                self.model_error = None;
                self.last_render = None;
            }
            Err(e) => {
                self.model_prepared = None;
                self.model_error = Some(cm);
                self.toast(e);
            }
        }
    }

    fn active_model(&self) -> Option<&PreparedModel> {
        if self.project.style.frame != FrameStyle::Custom {
            return None;
        }
        self.model_prepared.as_ref().map(|(_, m)| m.as_ref())
    }

    fn pick_folder(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let current = self.project.export.folder.clone();
        thread::spawn(move || {
            let res = run_picker(
                &["--file-selection", "--directory", "--title=Guardar vídeos en", &format!("--filename={current}/")],
                &["--getexistingdirectory", &current],
            );
            if let Ok(Some(p)) = res {
                let _ = tx.send(AppEvent::FolderPicked(PathBuf::from(p)));
            }
            ctx.request_repaint();
        });
    }

    fn handle_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::Devices(devs) => {
                    self.scanning = false;
                    let still_ok = self
                        .selected_serial
                        .as_ref()
                        .is_some_and(|s| devs.iter().any(|d| &d.serial == s && d.state == DeviceState::Ready));
                    if !still_ok {
                        self.selected_serial =
                            devs.iter().find(|d| d.state == DeviceState::Ready).map(|d| d.serial.clone());
                    }
                    self.devices = devs;
                }
                AppEvent::MediaLoaded { info, touches, recorded } => {
                    self.saving_recording = false;
                    let n_touches = touches.len();
                    self.project = self.project.with_new_media(info.clone(), touches);
                    if n_touches > 0 {
                        self.project.zooms = auto_zooms(
                            &self.project.touches,
                            self.project.style.auto_zoom_scale,
                            0.0,
                            info.duration,
                            1,
                        );
                    }
                    self.time = 0.0;
                    self.playing = false;
                    self.selected_zoom = None;
                    self.open_media(&info);
                    if recorded {
                        self.sheet = Sheet::None;
                        let z = self.project.zooms.len();
                        self.toast(if n_touches > 0 {
                            format!("Grabación lista · {n_touches} toques → {z} zooms automáticos")
                        } else {
                            "Grabación lista. Añade zooms con + Zoom o la tecla Z.".to_string()
                        });
                    } else {
                        self.toast("Vídeo importado. Añade zooms con + Zoom o la tecla Z.");
                    }
                }
                AppEvent::MediaFailed(e) => self.toast(e),
                AppEvent::RecordingFailed(e) => {
                    self.saving_recording = false;
                    self.record_error = Some(e);
                    self.sheet = Sheet::Record;
                }
                AppEvent::ProxyReady { source, proxy } => {
                    if self.project.media.as_ref().is_some_and(|m| m.path == source) {
                        self.decoder.set_source(&proxy.to_string_lossy(), self.preview_size);
                        self.restart_decoding();
                    }
                }
                AppEvent::Thumbnails { source, size, frames } => {
                    if self.project.media.as_ref().is_some_and(|m| m.path == source) {
                        self.thumbs = frames
                            .iter()
                            .enumerate()
                            .map(|(i, f)| {
                                let img = ColorImage::from_rgba_unmultiplied([size.0, size.1], f);
                                self.ctx.load_texture(format!("thumb-{i}"), img, TextureOptions::LINEAR)
                            })
                            .collect();
                        self.thumb_aspect = size.0 as f32 / size.1 as f32;
                    }
                }
                AppEvent::ImportPicked(p) => self.import(p),
                AppEvent::FolderPicked(p) => self.project.export.folder = p.to_string_lossy().into_owned(),
                AppEvent::ModelPicked(p) => {
                    // Load before copying, so a model that cannot be used never enters the gallery.
                    self.importing = true;
                    let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                    thread::spawn(move || {
                        let res = library::import_any(&p).map(|(dest, src)| (dest.to_string_lossy().into_owned(), Arc::new(src)));
                        let _ = tx.send(AppEvent::ModelImported(res));
                        ctx.request_repaint();
                    });
                }
                AppEvent::ModelImported(res) => {
                    self.importing = false;
                    match res {
                        Ok((path, src)) => {
                            self.library_broken.remove(&path);
                            self.library_thumbs.remove(&path);
                            self.model_cache.insert(path.clone(), src);
                            self.refresh_library();
                            self.select_model(path);
                        }
                        Err(e) => self.toast(e),
                    }
                }
                AppEvent::ModelLoaded { path, result } => {
                    if self.model_loading.as_deref() == Some(path.as_str()) {
                        self.model_loading = None;
                    }
                    match result {
                        Ok(src) => {
                            if self.model_pending.as_deref() == Some(path.as_str()) {
                                self.model_pending = None;
                                if src.guess.is_none() {
                                    self.toast("No se encontró la pantalla del modelo: elígela en «Pantalla».");
                                }
                                self.project.style.model = Some(Self::default_model_settings(&path, &src));
                                self.project.style.frame = FrameStyle::Custom;
                            }
                            self.model_cache.insert(path, src);
                            self.model_error = None;
                        }
                        Err(e) => {
                            if self.model_pending.as_deref() == Some(path.as_str()) {
                                self.model_pending = None;
                            }
                            self.toast(e);
                        }
                    }
                }
                AppEvent::ModelThumb { path, src, size, rgba } => {
                    if let Some(src) = src {
                        self.model_cache.entry(path.clone()).or_insert(src);
                    }
                    match rgba {
                        Some(rgba) => {
                            let img = ColorImage::from_rgba_unmultiplied([size.0, size.1], &rgba);
                            let tex = self.ctx.load_texture(format!("model-{path}"), img, TextureOptions::LINEAR);
                            self.library_thumbs.insert(path, tex);
                        }
                        None => {
                            self.library_broken.insert(path);
                        }
                    }
                }
                AppEvent::PickerUnavailable => {
                    self.toast("No hay diálogo de archivos (instala zenity). Puedes arrastrar el vídeo a la ventana.")
                }
            }
        }

        if let Some(rx) = &self.export_rx {
            let mut finished = false;
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    ExportEvent::Progress { fraction, eta_secs } => {
                        if let ExportUi::Running { fraction: f, eta, .. } = &mut self.export_ui {
                            *f = fraction;
                            *eta = eta_secs;
                        }
                    }
                    ExportEvent::Done(p) => {
                        self.export_ui = ExportUi::Done(p);
                        finished = true;
                    }
                    ExportEvent::Failed(e) => {
                        self.export_ui = ExportUi::Failed(e);
                        finished = true;
                    }
                    ExportEvent::Cancelled => {
                        self.export_ui = ExportUi::Idle;
                        finished = true;
                    }
                }
            }
            if finished {
                self.export_rx = None;
            }
        }
    }

    // ---------- Playback ----------

    fn restart_decoding(&mut self) {
        self.gen += 1;
        self.queued = None;
        if self.playing {
            self.decoder.play(self.gen, self.time);
        } else {
            self.requested_time = None;
        }
    }

    fn toggle_play(&mut self) {
        if self.project.media.is_none() {
            return;
        }
        if self.playing {
            self.playing = false;
            self.decoder.stop();
            self.restart_decoding();
        } else {
            if self.time >= self.project.trim_end - 0.05 {
                self.time = self.project.trim_start;
            }
            self.playing = true;
            self.last_tick = Instant::now();
            self.restart_decoding();
        }
    }

    fn seek(&mut self, t: f32) {
        if self.playing {
            self.playing = false;
            self.decoder.stop();
            self.gen += 1;
        }
        self.time = t.clamp(self.project.trim_start, self.project.trim_end.max(self.project.trim_start));
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;
        if self.playing {
            self.time += dt;
            if self.time >= self.project.trim_end {
                self.time = self.project.trim_end;
                self.toggle_play();
            }
            self.ctx.request_repaint();
        }

        loop {
            let next = self.queued.take().or_else(|| self.decoder.frames.try_recv().ok());
            let Some(f) = next else { break };
            if f.gen != self.gen {
                continue;
            }
            if self.playing && f.time > self.time + 0.5 / PREVIEW_FPS {
                self.queued = Some(f);
                break;
            }
            self.shown = Some(f);
        }

        if !self.playing && self.project.media.is_some() && self.requested_time != Some(self.time) {
            self.gen += 1;
            self.decoder.seek(self.gen, self.time);
            self.requested_time = Some(self.time);
        }

        let dur = self.project.duration();
        if self.cam_sig != self.project.zooms || self.cam_dur != dur {
            self.cam_track = CameraTrack::build(&self.project.zooms, 0.0, dur);
            self.cam_sig = self.project.zooms.clone();
            self.cam_dur = dur;
        }
    }

    // ---------- Zooms ----------

    fn add_zoom_at(&mut self, t: f32) {
        let end = (t + 2.5).min(self.project.trim_end);
        let start = (end - 2.5).max(self.project.trim_start).min(t);
        if end - start < 0.3 {
            return;
        }
        // Start from the nearest touch, if there is one.
        let (x, y) = self
            .project
            .touches
            .iter()
            .filter(|tc| (tc.time - t).abs() < 1.5)
            .min_by(|a, b| (a.time - t).abs().total_cmp(&(b.time - t).abs()))
            .map(|tc| (tc.x, tc.y))
            .unwrap_or((0.5, 0.5));
        let id = self.project.next_zoom_id();
        self.project.zooms.push(ZoomSegment {
            id,
            start,
            end,
            scale: self.project.style.auto_zoom_scale,
            x,
            y,
            auto: false,
        });
        self.selected_zoom = Some(id);
    }

    fn delete_selected_zoom(&mut self) {
        if let Some(id) = self.selected_zoom.take() {
            self.project.zooms.retain(|z| z.id != id);
        }
    }

    fn regenerate_auto_zooms(&mut self) {
        let p = &self.project;
        let mut zooms: Vec<ZoomSegment> = p.zooms.iter().filter(|z| !z.auto).cloned().collect();
        let first = p.next_zoom_id();
        zooms.extend(auto_zooms(&p.touches, p.style.auto_zoom_scale, 0.0, p.duration(), first));
        zooms.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.project.zooms = zooms;
        self.selected_zoom = None;
    }

    fn zoom_mut(&mut self, id: u64) -> Option<&mut ZoomSegment> {
        self.project.zooms.iter_mut().find(|z| z.id == id)
    }

    fn editing_zoom(&self) -> Option<ZoomSegment> {
        if self.playing {
            return None;
        }
        let id = self.selected_zoom?;
        self.project.zooms.iter().find(|z| z.id == id).copied()
    }

    // ---------- Recording & export ----------

    fn scan_devices(&mut self) {
        if self.scanning || !self.tools.adb {
            return;
        }
        self.scanning = true;
        self.last_scan = Some(Instant::now());
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(AppEvent::Devices(capture::list_devices()));
            ctx.request_repaint();
        });
    }

    fn start_recording(&mut self) {
        let Some(serial) = self.selected_serial.clone() else { return };
        if self.playing {
            self.toggle_play();
        }
        match capture::start(&serial, &self.rec_opts, self.tools) {
            Ok(r) => {
                self.recording = Some(r);
                self.record_error = None;
            }
            Err(e) => self.record_error = Some(e),
        }
    }

    fn stop_recording(&mut self) {
        let Some(rec) = self.recording.take() else { return };
        self.saving_recording = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        rec.stop(move |res| {
            let ev = match res {
                CaptureResult::Done { path, touches } => match media::probe(&path.to_string_lossy()) {
                    Ok(info) => AppEvent::MediaLoaded { info, touches, recorded: true },
                    Err(e) => AppEvent::RecordingFailed(format!("La grabación no se puede abrir: {e}")),
                },
                CaptureResult::Failed(e) => AppEvent::RecordingFailed(e),
            };
            let _ = tx.send(ev);
            ctx.request_repaint();
        });
    }

    fn start_export(&mut self) {
        if self.playing {
            self.toggle_play();
        }
        let _ = std::fs::create_dir_all(&self.project.export.folder);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();
        export::start(self.project.clone(), cancel.clone(), tx);
        self.export_rx = Some(rx);
        self.export_ui = ExportUi::Running { fraction: 0.0, eta: None, cancel };
    }

    fn autosave(&mut self) {
        if self.last_save_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_save_check = Instant::now();
        if let Ok(s) = serde_json::to_string(&self.project) {
            if s != self.last_saved && save_project(&self.project).is_ok() {
                self.last_saved = s;
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if self.sheet != Sheet::None {
            let busy = self.recording.is_some()
                || self.saving_recording
                || matches!(self.export_ui, ExportUi::Running { .. });
            if !busy && ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.sheet = Sheet::None;
            }
            return;
        }
        if self.project.media.is_none() {
            return;
        }
        let (space, del, esc, z, left, right, home, end, shift) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Z) && !i.modifiers.command,
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.modifiers.shift,
            )
        });
        if space {
            self.toggle_play();
        }
        if del {
            self.delete_selected_zoom();
        }
        if esc {
            self.selected_zoom = None;
        }
        if z {
            self.add_zoom_at(self.time);
        }
        let step = if shift { 1.0 } else { 1.0 / PREVIEW_FPS };
        if left {
            self.seek(self.time - step);
        }
        if right {
            self.seek(self.time + step);
        }
        if home {
            self.seek(self.project.trim_start);
        }
        if end {
            self.seek(self.project.trim_end);
        }
    }
}

impl Drop for RecOrdingApp {
    fn drop(&mut self) {
        if let Some(rec) = self.recording.take() {
            let _ = rec.stop_blocking();
        }
        let _ = save_project(&self.project);
    }
}

impl eframe::App for RecOrdingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();

        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(p) = dropped.into_iter().next() {
            if self.recording.is_none() && !self.saving_recording {
                self.sheet = Sheet::None;
                self.import(p);
            }
        }

        self.shortcuts(ctx);
        self.tick();
        self.sync_model();
        self.autosave();

        if let Some(rec) = &mut self.recording {
            if rec.started.elapsed() > Duration::from_secs(2) && rec.check_exited().is_some() {
                self.stop_recording();
            }
        }

        self.toolbar(ctx);
        if self.project.media.is_some() {
            self.inspector(ctx);
            self.timeline(ctx);
            self.viewer(ctx);
        } else {
            self.welcome(ctx);
        }

        match self.sheet {
            Sheet::Record => self.record_sheet(ctx),
            Sheet::Export => self.export_sheet(ctx),
            Sheet::RenameModel | Sheet::DeleteModel => self.model_edit_sheet(ctx),
            Sheet::None => {}
        }
        if self.sheet == Sheet::Record && self.recording.is_none() {
            if self.last_scan.map_or(true, |t| t.elapsed() > Duration::from_secs(2)) {
                self.scan_devices();
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if matches!(self.export_ui, ExportUi::Running { .. }) || self.recording.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        self.paint_toast(ctx);
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            self.paint_drop_overlay(ctx);
        }
    }
}

// ---------- UI ----------

impl RecOrdingApp {
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(52.0)
            .frame(
                egui::Frame::none()
                    .fill(TOOLBAR)
                    .inner_margin(Margin::symmetric(14.0, 0.0))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(18, 18, 20))),
            )
            .show(ctx, |ui| {
                let full = ui.max_rect();

                let mut left = ui.new_child(UiBuilder::new().max_rect(full).layout(egui::Layout::left_to_right(Align::Center)));
                let (logo, _) = left.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                fill_gradient(left.painter(), &rounded_rect_points(logo, 6.0), Color32::from_rgb(255, 95, 80), PURPLE, 135.0);
                left.painter().circle_filled(logo.center(), 4.5, Color32::WHITE);
                left.label(RichText::new("rec-ording").font(medium(14.0)).color(TEXT));
                left.add_space(18.0);
                let busy = self.recording.is_some() || self.saving_recording;
                if self.toolbar_button(&mut left, "Grabar", |p, c, col| {
                    p.circle_filled(c, 5.5, RED);
                    let _ = col;
                })
                .on_hover_text("Grabar la pantalla del móvil por USB")
                .clicked()
                {
                    self.sheet = Sheet::Record;
                    self.record_error = None;
                    self.scan_devices();
                }
                if self
                    .toolbar_button(&mut left, "Importar", |p, c, col| icon_import(p, c, 13.0, col))
                    .on_hover_text("Abrir un vídeo que ya tienes grabado")
                    .clicked()
                    && !busy
                {
                    self.pick_video();
                }

                if self.project.media.is_some() {
                    let center = Rect::from_center_size(full.center(), Vec2::new(280.0, 26.0));
                    let mut c = ui.new_child(UiBuilder::new().max_rect(center).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
                    c.add(
                        egui::TextEdit::singleline(&mut self.project.name)
                            .frame(false)
                            .horizontal_align(Align::Center)
                            .font(medium(14.0))
                            .text_color(TEXT)
                            .hint_text("Nombre del proyecto"),
                    )
                    .on_hover_text("Nombre del proyecto (se usa para el archivo exportado)");
                }

                let mut right = ui.new_child(UiBuilder::new().max_rect(full).layout(egui::Layout::right_to_left(Align::Center)));
                let can_export = self.project.media.is_some() && !busy;
                if primary_button(&mut right, "Exportar", can_export).clicked() {
                    self.sheet = Sheet::Export;
                    if !matches!(self.export_ui, ExportUi::Running { .. }) {
                        self.export_ui = ExportUi::Idle;
                    }
                }
            });
    }

    fn toolbar_button(&self, ui: &mut Ui, label: &str, icon: impl Fn(&egui::Painter, Pos2, Color32)) -> egui::Response {
        let galley = ui.painter().layout_no_wrap(label.to_string(), regular(13.0), TEXT);
        let size = Vec2::new(galley.size().x + 40.0, 30.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
        ui.painter().rect_filled(rect, Rounding::same(7.0), lerp_color(Color32::from_rgb(56, 56, 60), CONTROL_HOVER, hover));
        icon(ui.painter(), Pos2::new(rect.left() + 17.0, rect.center().y), TEXT);
        ui.painter().galley(Pos2::new(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, TEXT);
        resp.on_hover_cursor(CursorIcon::PointingHand)
    }

    fn welcome(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(VIEWER)).show(ctx, |ui| {
            let r = ui.max_rect();
            let col = Rect::from_center_size(r.center() - Vec2::new(0.0, 20.0), Vec2::new(660.0, 440.0));
            let mut c = ui.new_child(UiBuilder::new().max_rect(col).layout(egui::Layout::top_down(Align::Center)));
            c.label(RichText::new("Crea el vídeo de tu app").font(medium(30.0)).color(TEXT));
            c.add_space(2.0);
            c.label(RichText::new("Graba tu móvil Android por USB o importa una grabación.").font(regular(15.0)).color(TEXT_2));
            c.label(
                RichText::new("rec-ording le pone un marco, un fondo y zooms suaves en cada toque.")
                    .font(regular(15.0))
                    .color(TEXT_2),
            );
            c.add_space(30.0);

            let (row, _) = c.allocate_exact_size(Vec2::new(640.0, 190.0), Sense::hover());
            let a = Rect::from_min_size(row.min, Vec2::new(310.0, 190.0));
            let b = Rect::from_min_size(Pos2::new(row.right() - 310.0, row.top()), Vec2::new(310.0, 190.0));
            if self
                .welcome_card(&mut c, a, "rec", "Grabar el móvil", "Conecta el móvil por USB y graba lo que haces en él.", RED, |p, ctr| {
                    icon_phone(p, ctr, 28.0, RED)
                })
                .clicked()
            {
                self.sheet = Sheet::Record;
                self.record_error = None;
                self.scan_devices();
            }
            if self
                .welcome_card(&mut c, b, "imp", "Importar vídeo", "Elige una grabación o arrástrala a esta ventana.", ACCENT, |p, ctr| {
                    icon_import(p, ctr, 24.0, ACCENT)
                })
                .clicked()
            {
                self.pick_video();
            }

            c.add_space(26.0);
            c.label(RichText::new("1  Graba      ·      2  Ajusta el estilo      ·      3  Exporta").font(regular(13.0)).color(TEXT_3));

            let mut missing = Vec::new();
            if !self.tools.ffmpeg {
                missing.push("ffmpeg");
            }
            if !self.tools.adb {
                missing.push("android-tools");
            }
            if !self.tools.scrcpy {
                missing.push("scrcpy");
            }
            if !missing.is_empty() {
                c.add_space(18.0);
                c.label(
                    RichText::new(format!("Para usar todas las funciones instala: sudo pacman -S {}", missing.join(" ")))
                        .font(regular(12.5))
                        .color(YELLOW),
                );
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn welcome_card(
        &self,
        ui: &mut Ui,
        rect: Rect,
        id: &str,
        title: &str,
        body: &str,
        tint: Color32,
        icon: impl Fn(&egui::Painter, Pos2),
    ) -> egui::Response {
        let resp = ui.interact(rect, Id::new(("welcome", id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let h = ui.ctx().animate_bool(resp.id, resp.hovered());
        let p = ui.painter();
        let r = rect.translate(Vec2::new(0.0, -3.0 * h));
        p.rect_filled(r.translate(Vec2::new(0.0, 6.0)), Rounding::same(18.0), Color32::from_black_alpha((30.0 + 40.0 * h) as u8));
        p.rect_filled(r, Rounding::same(18.0), lerp_color(CARD, Color32::from_rgb(58, 58, 64), h));
        p.rect_stroke(r, Rounding::same(18.0), Stroke::new(1.0_f32, Color32::from_rgb(66, 66, 72)));
        let ic = Pos2::new(r.left() + 52.0, r.top() + 54.0);
        p.circle_filled(ic, 28.0, lerp_color(CARD, tint, 0.22));
        icon(p, ic);
        p.text(Pos2::new(r.left() + 26.0, r.top() + 104.0), Align2::LEFT_TOP, title, medium(18.0), TEXT);
        let g = p.layout(body.to_string(), regular(13.0), TEXT_2, r.width() - 52.0);
        p.galley(Pos2::new(r.left() + 26.0, r.top() + 132.0), g, TEXT_2);
        resp
    }

    // ----- Viewer -----

    fn viewer(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(VIEWER).inner_margin(Margin::same(16.0)))
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let top = Rect::from_min_size(full.min, Vec2::new(full.width(), 26.0));
                let seg = Rect::from_center_size(top.center(), Vec2::new(240.0, 26.0));
                let mut c = ui.new_child(UiBuilder::new().max_rect(seg));
                let opts: Vec<(Aspect, &str)> = Aspect::ALL.iter().map(|a| (*a, a.label())).collect();
                segmented(&mut c, &mut self.project.style.aspect, &opts, 240.0);
                ui.painter().text(
                    Pos2::new(seg.right() + 14.0, seg.center().y),
                    Align2::LEFT_CENTER,
                    self.project.style.aspect.hint(),
                    regular(12.0),
                    TEXT_3,
                );

                let transport = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 36.0), full.max);
                let area = Rect::from_min_max(
                    Pos2::new(full.left(), top.bottom() + 14.0),
                    Pos2::new(full.right(), transport.top() - 10.0),
                );
                let ratio = self.project.style.aspect.ratio();
                let mut w = area.width();
                let mut h = w / ratio;
                if h > area.height() {
                    h = area.height();
                    w = h * ratio;
                }
                let canvas = Rect::from_center_size(area.center(), Vec2::new(w.max(16.0), h.max(16.0)));
                self.canvas(ui, canvas);
                self.transport(ui, transport);
            });
    }

    fn canvas(&mut self, ui: &mut Ui, canvas: Rect) {
        let Some(media) = self.project.media.clone() else { return };
        let ppp = ui.ctx().pixels_per_point();
        let mut ow = (canvas.width() * ppp).round().max(16.0);
        let mut oh = (canvas.height() * ppp).round().max(16.0);
        let k = (1600.0 / ow.max(oh)).min(1.0);
        ow = (ow * k).round();
        oh = (oh * k).round();
        let (ow, oh) = (ow as usize, oh as usize);

        let editing = self.editing_zoom();
        let cam = if editing.is_some() { CamState::IDENTITY } else { self.cam_track.sample(self.time) };
        // While picking a zoom's focus, show the device straight on so clicks land where they look.
        let style = if editing.is_some() { self.project.style.flattened() } else { self.project.style.clone() };
        let key = RenderKey {
            time: self.time,
            frame: self.shown.as_ref().map(|f| Arc::as_ptr(&f.mips) as usize).unwrap_or(0),
            style: style.clone(),
            cam: (cam.scale.to_bits(), cam.fx.to_bits(), cam.fy.to_bits()),
            size: (ow, oh),
            touches: self.project.touches.len(),
            model: self.active_model().map(|m| m as *const PreparedModel as usize).unwrap_or(0),
            supersample: if self.playing { 1 } else { 2 },
        };
        if self.last_render.as_ref() != Some(&key) {
            self.viewer_buf.resize(ow * oh * 4, 0);
            compose(
                &mut self.viewer_buf,
                ow,
                oh,
                &ComposeInput {
                    style: &style,
                    screen: self.shown.as_ref().map(|f| f.mips.as_ref()),
                    video_w: media.width as f32,
                    video_h: media.height as f32,
                    cam,
                    touches: &self.project.touches,
                    time: self.time,
                    since_start: self.time - self.project.trim_start,
                    model: self.model_prepared.as_ref().map(|(_, m)| m.as_ref()).filter(|_| style.frame == FrameStyle::Custom),
                    supersample: key.supersample,
                },
            );
            let img = ColorImage::from_rgba_unmultiplied([ow, oh], &self.viewer_buf);
            match &mut self.viewer_tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.viewer_tex = Some(ui.ctx().load_texture("viewer", img, TextureOptions::LINEAR)),
            }
            self.last_render = Some(key);
        }

        let p = ui.painter();
        p.rect_filled(canvas.translate(Vec2::new(0.0, 8.0)).expand(2.0), Rounding::same(4.0), Color32::from_black_alpha(60));
        if let Some(tex) = &self.viewer_tex {
            p.image(tex.id(), canvas, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }

        let resp = ui.interact(canvas, Id::new("canvas"), Sense::click_and_drag());
        let Some(z) = editing else { return };

        // Editing a zoom: show the whole frame and the area the camera will frame.
        let layout = Layout::new(
            ow as f32,
            oh as f32,
            &self.project.style,
            media.width as f32,
            media.height as f32,
            self.active_model().map(|m| &m.meta),
        );
        let cam = layout.camera(CamState { scale: z.scale, fx: z.x, fy: z.y });
        let (ax, ay) = layout.to_scene(cam, 0.0, 0.0);
        let (bx, by) = layout.to_scene(cam, ow as f32, oh as f32);
        let to_pt = |x: f32, y: f32| {
            Pos2::new(canvas.left() + x / ow as f32 * canvas.width(), canvas.top() + y / oh as f32 * canvas.height())
        };
        let region = Rect::from_two_pos(to_pt(ax, ay), to_pt(bx, by)).intersect(canvas);
        let dim = Color32::from_black_alpha(120);
        let p = ui.painter_at(canvas);
        p.rect_filled(Rect::from_min_max(canvas.min, Pos2::new(canvas.right(), region.top())), 0.0, dim);
        p.rect_filled(Rect::from_min_max(Pos2::new(canvas.left(), region.bottom()), canvas.max), 0.0, dim);
        p.rect_filled(Rect::from_min_max(Pos2::new(canvas.left(), region.top()), Pos2::new(region.left(), region.bottom())), 0.0, dim);
        p.rect_filled(Rect::from_min_max(Pos2::new(region.right(), region.top()), Pos2::new(canvas.right(), region.bottom())), 0.0, dim);
        p.rect_stroke(region, Rounding::same(4.0), Stroke::new(2.0_f32, YELLOW));
        let focus = to_pt(layout.video.x0 + z.x * layout.video.w(), layout.video.y0 + z.y * layout.video.h());
        p.circle_filled(focus, 8.0, Color32::from_black_alpha(80));
        p.circle_filled(focus, 6.0, YELLOW);
        p.circle_stroke(focus, 6.0, Stroke::new(1.5_f32, Color32::WHITE));

        let chip = format!("Zoom {:.1}×  ·  Haz clic donde quieras enfocar  ·  Esc para ver el resultado", z.scale).replace('.', ",");
        let g = p.layout_no_wrap(chip, regular(12.0), Color32::WHITE);
        let chip_rect = Rect::from_center_size(Pos2::new(canvas.center().x, canvas.top() + 20.0), g.size() + Vec2::new(20.0, 10.0));
        p.rect_filled(chip_rect, Rounding::same(12.0), Color32::from_black_alpha(170));
        p.galley(chip_rect.min + Vec2::new(10.0, 5.0), g, Color32::WHITE);

        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if resp.clicked() || resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let x = (pos.x - canvas.left()) / canvas.width() * ow as f32;
                let y = (pos.y - canvas.top()) / canvas.height() * oh as f32;
                let u = ((x - layout.video.x0) / layout.video.w()).clamp(0.0, 1.0);
                let v = ((y - layout.video.y0) / layout.video.h()).clamp(0.0, 1.0);
                if let Some(zm) = self.zoom_mut(z.id) {
                    zm.x = u;
                    zm.y = v;
                    zm.auto = false;
                }
            }
        }
    }

    fn icon_button(&self, ui: &mut Ui, rect: Rect, id: &str, filled: bool, paint: impl Fn(&egui::Painter, Pos2, Color32)) -> egui::Response {
        let resp = ui.interact(rect, Id::new(("icon", id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let h = ui.ctx().animate_bool(resp.id, resp.hovered());
        let p = ui.painter();
        if filled {
            p.circle_filled(rect.center(), rect.width() / 2.0, lerp_color(Color32::from_rgb(58, 58, 63), Color32::from_rgb(76, 76, 82), h));
        } else if h > 0.0 {
            p.circle_filled(rect.center(), rect.width() / 2.0, Color32::from_white_alpha((14.0 * h) as u8));
        }
        paint(p, rect.center(), TEXT);
        resp
    }

    fn transport(&mut self, ui: &mut Ui, rect: Rect) {
        let c = rect.center();
        let play = Rect::from_center_size(c, Vec2::splat(34.0));
        let back = Rect::from_center_size(c - Vec2::new(44.0, 0.0), Vec2::splat(28.0));
        let playing = self.playing;
        if self
            .icon_button(ui, play, "play", true, |p, c, col| {
                if playing {
                    icon_pause(p, c, 13.0, col)
                } else {
                    icon_play(p, c + Vec2::new(1.5, 0.0), 13.0, col)
                }
            })
            .on_hover_text("Reproducir / pausar (Espacio)")
            .clicked()
        {
            self.toggle_play();
        }
        if self.icon_button(ui, back, "start", false, |p, c, col| icon_to_start(p, c, 12.0, col)).on_hover_text("Ir al inicio (Inicio)").clicked() {
            self.seek(self.project.trim_start);
        }

        let p = ui.painter();
        let rel = self.time - self.project.trim_start;
        p.text(
            Pos2::new(rect.left(), c.y),
            Align2::LEFT_CENTER,
            format!("{}  /  {}", fmt_time(rel), fmt_time(self.project.trimmed_duration())),
            medium(13.0),
            TEXT,
        );
        let info = if self.project.touches.is_empty() {
            format!("{} zooms", self.project.zooms.len())
        } else {
            format!("{} zooms  ·  {} toques", self.project.zooms.len(), self.project.touches.len())
        };
        p.text(Pos2::new(rect.right(), c.y), Align2::RIGHT_CENTER, info, regular(12.5), TEXT_3);
    }

    // ----- Inspector -----

    fn inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector")
            .exact_width(304.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(SIDEBAR)
                    .inner_margin(Margin { left: 14.0, right: 14.0, top: 12.0, bottom: 12.0 })
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(18, 18, 20))),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    if let Some(z) = self.editing_zoom() {
                        self.zoom_inspector(ui, z);
                    }
                    self.style_inspector(ui);
                });
            });
    }

    fn zoom_inspector(&mut self, ui: &mut Ui, z: ZoomSegment) {
        let mut delete = false;
        let mut close = false;
        section(ui, "Zoom seleccionado", |ui| {
            let mut scale = z.scale;
            if slider_row(ui, "Nivel", &mut scale, 1.1..=3.5, |v| format!("{v:.1}×").replace('.', ",")) {
                if let Some(zm) = self.zoom_mut(z.id) {
                    zm.scale = scale;
                    zm.auto = false;
                }
            }
            ui.label(
                RichText::new(format!("De {} a {} · {}", fmt_time(z.start), fmt_time(z.end), fmt_secs(z.end - z.start)))
                    .color(TEXT_2)
                    .font(regular(12.0)),
            );
            ui.label(RichText::new("Haz clic en la vista previa para elegir dónde enfocar. Arrastra el zoom en la línea de tiempo para moverlo o alargarlo.").color(TEXT_3).font(regular(12.0)));
            ui.horizontal(|ui| {
                if secondary_button(ui, "Listo").clicked() {
                    close = true;
                }
                if ui.add(egui::Button::new(RichText::new("Eliminar").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    delete = true;
                }
            });
        });
        if delete {
            self.delete_selected_zoom();
        }
        if close {
            self.selected_zoom = None;
        }
        ui.add_space(6.0);
    }

    fn models_inspector(&mut self, ui: &mut Ui) {
        let mut pick: Option<String> = None;
        let mut import = false;
        let mut edit: Option<(LibraryModel, Sheet)> = None;
        let current = (self.project.style.frame == FrameStyle::Custom)
            .then(|| self.project.style.model.as_ref().map(|m| m.path.clone()))
            .flatten();
        let loading = self.model_loading.clone().or_else(|| self.model_pending.clone());

        section(ui, "Modelos 3D", |ui| {
            let gap = 6.0;
            let tile_w = ((ui.available_width() - gap * 2.0) / 3.0).floor();
            let mut tiles: Vec<Option<&LibraryModel>> = self.library.iter().map(Some).collect();
            tiles.push(None);
            for row in tiles.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for t in row {
                        match t {
                            Some(m) => {
                                let selected = current.as_deref() == Some(m.path.as_str());
                                let busy = loading.as_deref() == Some(m.path.as_str());
                                let broken = self.library_broken.contains(&m.path);
                                let resp = gallery_tile(ui, m, self.library_thumbs.get(&m.path), selected, busy, broken, tile_w);
                                if resp.clicked() {
                                    pick = Some(m.path.clone());
                                }
                                if m.entry.is_some() {
                                    resp.context_menu(|ui| {
                                        if ui.button("Renombrar…").clicked() {
                                            edit = Some(((*m).clone(), Sheet::RenameModel));
                                            ui.close_menu();
                                        }
                                        if ui.button(RichText::new("Eliminar…").color(RED)).clicked() {
                                            edit = Some(((*m).clone(), Sheet::DeleteModel));
                                            ui.close_menu();
                                        }
                                    });
                                }
                            }
                            None => {
                                if import_tile(ui, tile_w, self.importing).clicked() && !self.importing {
                                    import = true;
                                }
                            }
                        }
                    }
                });
            }
        });

        if let Some(p) = pick {
            self.select_model(p);
        }
        if let Some((m, sheet)) = edit.take() {
            self.start_model_edit(&m, sheet);
        }
        if import {
            self.pick_model();
        }

        // Settings of the model in use.
        if self.project.style.frame != FrameStyle::Custom {
            return;
        }
        let Some(cm) = self.project.style.model.as_ref() else { return };
        let Some(src) = self.model_cache.get(&cm.path).cloned() else { return };
        let entry = self.library.iter().find(|m| m.path == cm.path).cloned();
        let style = &mut self.project.style;
        let Some(cm) = style.model.as_mut() else { return };

        section(ui, "Ajustes del modelo", |ui| {
            ui.horizontal(|ui| {
                fixed_label(ui, 78.0, RichText::new("Pantalla").color(TEXT), Align::Min);
                let detected = src.guess.and_then(|g| src.parts.get(g)).map(|p| p.name.clone());
                let current = match cm.screen_part.and_then(|i| src.parts.get(i)) {
                    Some(p) => p.name.clone(),
                    None => match &detected {
                        Some(d) => format!("Automática ({d})"),
                        None => "Elige una pieza".to_string(),
                    },
                };
                egui::ComboBox::from_id_salt("model-screen")
                    .selected_text(current)
                    .width(ui.available_width() - 8.0)
                    .show_ui(ui, |ui| {
                        if src.guess.is_some() {
                            ui.selectable_value(&mut cm.screen_part, None, "Automática (recomendada)");
                            ui.separator();
                        }
                        for (i, p) in src.parts.iter().enumerate() {
                            let label = if Some(i) == src.guess { format!("{} · detectada", p.name) } else { p.name.clone() };
                            ui.selectable_value(&mut cm.screen_part, Some(i), label);
                        }
                    });
            })
            .response
            .on_hover_text("La pieza del modelo donde se ve tu grabación");
            if cm.screen_part.is_some() && src.guess.is_some() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Elegida a mano.").color(YELLOW).font(regular(11.5)));
                    if ui.link(RichText::new("Volver a automática").font(regular(11.5))).clicked() {
                        cm.screen_part = None;
                    }
                });
            }
            ui.horizontal(|ui| {
                fixed_label(ui, 78.0, RichText::new("Orientación").color(TEXT), Align::Min);
                if secondary_button(ui, "Girar 90°").on_hover_text("Si la grabación sale tumbada o al revés").clicked() {
                    cm.quarter_turns = (cm.quarter_turns + 1) % 4;
                }
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                fixed_label(ui, 78.0, RichText::new("Vídeo").color(TEXT), Align::Min);
                let opts: Vec<(VideoFit, &str)> = VideoFit::ALL.iter().map(|f| (*f, f.label())).collect();
                segmented(ui, &mut style.video_fit, &opts, ui.available_width());
            });
            ui.label(RichText::new(style.video_fit.hint()).color(TEXT_3).font(regular(11.5)));
            if style.video_fit == VideoFit::Fill {
                slider_row(ui, "Encuadre", &mut style.fit_pan, 0.0..=1.0, |v| {
                    match v {
                        v if v < 0.15 => "Inicio",
                        v if v > 0.85 => "Final",
                        _ => "Centro",
                    }
                    .to_string()
                });
            }
            if !src.animations.is_empty() {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    fixed_label(ui, 78.0, RichText::new("Animación").color(TEXT), Align::Min);
                    let name = |a: Option<usize>| match a {
                        Some(i) => src.animations.get(i).map(|a| a.name.clone()).unwrap_or_default(),
                        None => "Sin animación".to_string(),
                    };
                    egui::ComboBox::from_id_salt("model-anim")
                        .selected_text(name(cm.animation))
                        .width(ui.available_width() - 8.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut cm.animation, None, "Sin animación");
                            for (i, a) in src.animations.iter().enumerate() {
                                ui.selectable_value(&mut cm.animation, Some(i), format!("{} · {}", a.name, fmt_secs(a.duration)));
                            }
                        });
                });
                if cm.animation.is_some() {
                    toggle_row(ui, "Repetir en bucle", "Si no, se reproduce una vez al empezar", &mut cm.loop_animation);
                }
            }
            if let Some(e) = entry.as_ref().filter(|e| e.entry.is_some()) {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if secondary_button(ui, "Renombrar…").clicked() {
                        edit = Some((e.clone(), Sheet::RenameModel));
                    }
                    if ui.add(egui::Button::new(RichText::new("Eliminar…").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                        edit = Some((e.clone(), Sheet::DeleteModel));
                    }
                });
            }
            if let Some(e) = &entry {
                if let (Some(author), Some(license)) = (&e.author, &e.license) {
                    ui.add_space(2.0);
                    let note = if e.needs_credit() {
                        format!("Modelo de {author} ({license}). Menciona al autor al publicar el vídeo.")
                    } else {
                        format!("Modelo de {author} ({license}). Uso libre, sin atribución.")
                    };
                    ui.label(RichText::new(note).color(TEXT_3).font(regular(11.5)));
                }
            }
        });
        if let Some((m, sheet)) = edit {
            self.start_model_edit(&m, sheet);
        }
    }

    fn start_model_edit(&mut self, m: &LibraryModel, sheet: Sheet) {
        let Some(entry) = m.entry.clone() else { return };
        self.model_edit = Some(ModelEdit { entry, path: m.path.clone(), name: m.name.clone(), error: None });
        self.sheet = sheet;
    }

    /// Re-keys everything that refers to a model by path after it was renamed.
    fn model_moved(&mut self, old: &str, new: String) {
        if let Some(src) = self.model_cache.remove(old) {
            self.model_cache.insert(new.clone(), src);
        }
        if let Some(t) = self.library_thumbs.remove(old) {
            self.library_thumbs.insert(new.clone(), t);
        }
        if let Some(cm) = self.project.style.model.as_mut().filter(|m| m.path == old) {
            cm.path = new.clone();
        }
        if let Some((cm, _)) = self.model_prepared.as_mut().filter(|(m, _)| m.path == old) {
            cm.path = new;
        }
    }

    fn model_edit_sheet(&mut self, ctx: &egui::Context) {
        let Some(edit) = self.model_edit.as_mut() else {
            self.sheet = Sheet::None;
            return;
        };
        let renaming = self.sheet == Sheet::RenameModel;
        let mut action: Option<bool> = None; // Some(true) = confirm, Some(false) = cancel
        Self::sheet_window(ctx, "model-edit", 400.0, |ui| {
            if renaming {
                ui.label(RichText::new("Renombrar modelo").font(medium(20.0)).color(TEXT));
                ui.add_space(8.0);
                let resp = ui.add(egui::TextEdit::singleline(&mut edit.name).desired_width(f32::INFINITY).font(regular(14.0)));
                if !resp.has_focus() && edit.error.is_none() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    action = Some(true);
                }
            } else {
                ui.label(RichText::new(format!("¿Eliminar «{}»?", edit.name)).font(medium(20.0)).color(TEXT));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Se borrará de tu galería junto con sus texturas. Los vídeos que ya has exportado no cambian.")
                        .color(TEXT_2),
                );
            }
            if let Some(e) = &edit.error {
                ui.add_space(4.0);
                info_box(ui, RED, e);
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let ok = if renaming { primary_button(ui, "Guardar", true) } else { tinted_button(ui, "Eliminar", RED, true) };
                if ok.clicked() {
                    action = Some(true);
                }
                if secondary_button(ui, "Cancelar").clicked() {
                    action = Some(false);
                }
            });
        });
        match action {
            Some(false) => {
                self.model_edit = None;
                self.sheet = Sheet::None;
            }
            Some(true) => {
                let edit = self.model_edit.as_mut().unwrap();
                if renaming {
                    match library::rename(&edit.entry, &edit.name) {
                        Ok(new_path) => {
                            let (old, name) = (edit.path.clone(), edit.name.trim().to_string());
                            self.model_moved(&old, new_path.to_string_lossy().into_owned());
                            self.model_edit = None;
                            self.sheet = Sheet::None;
                            self.refresh_library();
                            self.toast(format!("Modelo renombrado a «{name}»"));
                        }
                        Err(e) => edit.error = Some(e),
                    }
                } else {
                    match library::remove(&edit.entry) {
                        Ok(()) => {
                            let (path, name) = (edit.path.clone(), edit.name.clone());
                            self.model_cache.remove(&path);
                            self.library_thumbs.remove(&path);
                            self.library_broken.remove(&path);
                            if self.project.style.model.as_ref().is_some_and(|m| m.path == path) {
                                self.project.style.model = None;
                                if self.project.style.frame == FrameStyle::Custom {
                                    self.project.style.frame = FrameStyle::Rounded;
                                }
                            }
                            self.model_edit = None;
                            self.sheet = Sheet::None;
                            self.refresh_library();
                            self.toast(format!("«{name}» eliminado de la galería"));
                        }
                        Err(e) => edit.error = Some(e),
                    }
                }
            }
            None => {}
        }
    }

    fn style_inspector(&mut self, ui: &mut Ui) {
        let style = &mut self.project.style;
        section(ui, "Dispositivo", |ui| {
            let gap = 6.0;
            let tile_w = ((ui.available_width() - gap * 2.0) / 3.0).floor();
            let builtin: Vec<FrameStyle> = FrameStyle::ALL.into_iter().filter(|f| *f != FrameStyle::Custom).collect();
            for row in builtin.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for f in row {
                        if model_tile(ui, *f, style.frame == *f, tile_w).clicked() {
                            style.frame = *f;
                        }
                    }
                });
            }
            if style.frame != FrameStyle::None && style.frame != FrameStyle::Custom {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for c in FrameColor::ALL {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        let resp = resp.on_hover_text(c.label()).on_hover_cursor(CursorIcon::PointingHand);
                        let p = ui.painter();
                        p.circle_filled(r.center(), 11.0, rgb(c.rgb()));
                        p.circle_stroke(r.center(), 11.0, Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
                        if style.frame_color == c {
                            p.circle_stroke(r.center(), 14.0, Stroke::new(2.0_f32, ACCENT));
                        }
                        if resp.clicked() {
                            style.frame_color = c;
                        }
                    }
                    ui.label(RichText::new(style.frame_color.label()).color(TEXT_2).font(regular(12.0)));
                });
            }
        });
        self.models_inspector(ui);
        let style = &mut self.project.style;

        section(ui, "Ángulo 3D", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for p in ANGLE_PRESETS {
                    let on = (style.tilt - p.tilt).abs() < 0.01 && (style.turn - p.turn).abs() < 0.01 && (style.roll - p.roll).abs() < 0.01;
                    if chip(ui, p.name, on).clicked() {
                        style.tilt = p.tilt;
                        style.turn = p.turn;
                        style.roll = p.roll;
                    }
                }
            });
            ui.add_space(2.0);
            let deg = |v: f32| format!("{v:.0}°");
            slider_row(ui, "Inclinación", &mut style.tilt, -40.0..=40.0, deg);
            slider_row(ui, "Giro", &mut style.turn, -45.0..=45.0, deg);
            slider_row(ui, "Rotación", &mut style.roll, -25.0..=25.0, deg);
            ui.add_space(2.0);
            toggle_row(ui, "Enderezar al hacer zoom", "Para que la app se lea bien de cerca", &mut style.straighten_on_zoom);
            toggle_row(ui, "Flotar suavemente", "Un leve balanceo continuo", &mut style.float_motion);
            toggle_row(ui, "Animación de entrada", "El móvil entra girando al empezar", &mut style.intro);
        });

        section(ui, "Fondo", |ui| {
            let gap = 8.0;
            let size = ((ui.available_width() - gap * 4.0) / 5.0).floor();
            for row in BACKGROUND_PRESETS.chunks(5) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for preset in row {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
                        let resp = resp.on_hover_text(preset.name).on_hover_cursor(CursorIcon::PointingHand);
                        let p = ui.painter();
                        fill_gradient(p, &rounded_rect_points(r.shrink(2.0), 8.0), rgb(preset.a), rgb(preset.b), preset.angle);
                        let selected = style.background.a == preset.a && style.background.b == preset.b;
                        if selected {
                            p.rect_stroke(r.expand(0.5), Rounding::same(10.0), Stroke::new(2.0_f32, ACCENT));
                        }
                        if resp.clicked() {
                            style.background = Background { a: preset.a, b: preset.b, angle: preset.angle };
                        }
                    }
                });
            }
            egui::CollapsingHeader::new(RichText::new("Personalizar").color(TEXT_2).font(regular(12.5)))
                .id_salt("bg-custom")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Colores").color(TEXT));
                        ui.add_space(26.0);
                        ui.color_edit_button_srgb(&mut style.background.a);
                        ui.color_edit_button_srgb(&mut style.background.b);
                    });
                    slider_row(ui, "Ángulo", &mut style.background.angle, 0.0..=360.0, |v| format!("{v:.0}°"));
                });
        });

        section(ui, "Composición", |ui| {
            slider_row(ui, "Margen", &mut style.padding, 0.02..=0.25, |v| format!("{:.0} %", v * 100.0));
            slider_row(ui, "Sombra", &mut style.shadow, 0.0..=1.0, |v| format!("{:.0} %", v * 100.0));
        });

        section(ui, "Toques", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mostrar los toques").color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    toggle(ui, &mut style.show_touches);
                });
            });
            if style.show_touches {
                slider_row(ui, "Tamaño", &mut style.touch_size, 0.5..=2.0, |v| format!("{:.0} %", v * 100.0));
            }
        });

        let has_touches = !self.project.touches.is_empty();
        let mut regenerate = false;
        section(ui, "Zoom automático", |ui| {
            slider_row(ui, "Intensidad", &mut self.project.style.auto_zoom_scale, 1.2..=3.0, |v| {
                format!("{v:.1}×").replace('.', ",")
            });
            if has_touches {
                ui.label(
                    RichText::new(format!("{} toques registrados en la grabación.", self.project.touches.len()))
                        .color(TEXT_3)
                        .font(regular(12.0)),
                );
                if secondary_button(ui, "Regenerar zooms desde los toques").on_hover_text("Sustituye los zooms automáticos; los que has creado o editado tú se mantienen.").clicked() {
                    regenerate = true;
                }
            } else {
                ui.label(
                    RichText::new("Este vídeo no tiene toques registrados. Añade zooms con + Zoom o la tecla Z.")
                        .color(TEXT_3)
                        .font(regular(12.0)),
                );
            }
        });
        if regenerate {
            self.regenerate_auto_zooms();
        }
    }

    // ----- Timeline -----

    fn timeline(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("timeline")
            .exact_height(200.0)
            .frame(
                egui::Frame::none()
                    .fill(WINDOW)
                    .inner_margin(Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 14.0 })
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(16, 16, 18))),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if secondary_button(ui, "+ Zoom").on_hover_text("Añadir un zoom en el cursor (Z)").clicked() {
                        self.add_zoom_at(self.time);
                    }
                    if self.selected_zoom.is_some() && secondary_button(ui, "Eliminar zoom").on_hover_text("Supr").clicked() {
                        self.delete_selected_zoom();
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let total = self.project.duration();
                        let trimmed = self.project.trimmed_duration();
                        let txt = if (total - trimmed).abs() > 0.05 {
                            format!("Duración {} (recortado de {})", fmt_time(trimmed), fmt_time(total))
                        } else {
                            format!("Duración {}", fmt_time(trimmed))
                        };
                        ui.label(RichText::new(txt).color(TEXT_2).font(regular(12.5)));
                    });
                });
                ui.add_space(4.0);
                let body = ui.available_rect_before_wrap();
                self.timeline_body(ui, body);
            });
    }

    fn hit(&self, tl: &Tl, pos: Pos2) -> Hit {
        const EDGE: f32 = 7.0;
        if tl.ruler.contains(pos) {
            return Hit::Ruler;
        }
        if tl.video.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            let (a, b) = (tl.x(self.project.trim_start), tl.x(self.project.trim_end));
            if (pos.x - a).abs() <= EDGE {
                return Hit::TrimStart;
            }
            if (pos.x - b).abs() <= EDGE {
                return Hit::TrimEnd;
            }
            return Hit::Video;
        }
        if tl.zoom.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            // Topmost (last drawn) first.
            for z in self.project.zooms.iter().rev() {
                let (a, b) = (tl.x(z.start), tl.x(z.end));
                if pos.x >= a - EDGE && pos.x <= b + EDGE {
                    if (pos.x - b).abs() <= EDGE && b - a > 2.0 * EDGE {
                        return Hit::ZoomEnd(z.id);
                    }
                    if (pos.x - a).abs() <= EDGE && b - a > 2.0 * EDGE {
                        return Hit::ZoomStart(z.id);
                    }
                    if pos.x >= a && pos.x <= b {
                        return Hit::ZoomBody(z.id, tl.t(pos.x) - z.start);
                    }
                }
            }
            return Hit::ZoomEmpty;
        }
        Hit::None
    }

    fn timeline_body(&mut self, ui: &mut Ui, body: Rect) {
        let dur = self.project.duration().max(0.1);
        let ruler = Rect::from_min_size(body.min, Vec2::new(body.width(), 18.0));
        let video = Rect::from_min_size(Pos2::new(body.left(), ruler.bottom() + 6.0), Vec2::new(body.width(), 66.0));
        let zoom = Rect::from_min_size(Pos2::new(body.left(), video.bottom() + 8.0), Vec2::new(body.width(), 32.0));
        let tl = Tl { x0: body.left() + 1.0, x1: body.right() - 1.0, dur, ruler, video, zoom };

        // Interaction first, so painting reflects this frame's changes.
        let resp = ui.interact(Rect::from_min_max(ruler.min, zoom.max), Id::new("timeline"), Sense::click_and_drag());
        let hover_hit = resp.hover_pos().map(|p| self.hit(&tl, p)).unwrap_or(Hit::None);
        let active = self.drag.map(|d| match d {
            Drag::TrimStart => Hit::TrimStart,
            Drag::TrimEnd => Hit::TrimEnd,
            Drag::ZoomStart(id) => Hit::ZoomStart(id),
            Drag::ZoomEnd(id) => Hit::ZoomEnd(id),
            Drag::ZoomMove { id, .. } => Hit::ZoomBody(id, 0.0),
            Drag::Scrub => Hit::Ruler,
        });
        match active.unwrap_or(hover_hit) {
            Hit::TrimStart | Hit::TrimEnd | Hit::ZoomStart(_) | Hit::ZoomEnd(_) => ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal),
            Hit::ZoomBody(..) if self.drag.is_some() => ui.ctx().set_cursor_icon(CursorIcon::Grabbing),
            Hit::ZoomBody(..) => ui.ctx().set_cursor_icon(CursorIcon::Grab),
            _ => {}
        }

        if resp.drag_started() {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                self.drag = match self.hit(&tl, origin) {
                    Hit::TrimStart => Some(Drag::TrimStart),
                    Hit::TrimEnd => Some(Drag::TrimEnd),
                    Hit::ZoomStart(id) => Some(Drag::ZoomStart(id)),
                    Hit::ZoomEnd(id) => Some(Drag::ZoomEnd(id)),
                    Hit::ZoomBody(id, grab) => Some(Drag::ZoomMove { id, grab }),
                    Hit::Ruler | Hit::Video | Hit::ZoomEmpty => Some(Drag::Scrub),
                    Hit::None => None,
                };
                if let Some(Drag::ZoomStart(id) | Drag::ZoomEnd(id) | Drag::ZoomMove { id, .. }) = self.drag {
                    self.selected_zoom = Some(id);
                }
            }
        }
        if resp.dragged() {
            if let (Some(d), Some(pos)) = (self.drag, resp.interact_pointer_pos()) {
                let t = tl.t(pos.x);
                match d {
                    Drag::Scrub => self.seek(t),
                    Drag::TrimStart => {
                        self.project.trim_start = t.min(self.project.trim_end - 0.5).max(0.0);
                        self.seek(self.project.trim_start);
                    }
                    Drag::TrimEnd => {
                        self.project.trim_end = t.max(self.project.trim_start + 0.5).min(dur);
                        self.seek(self.project.trim_end);
                    }
                    Drag::ZoomStart(id) => {
                        if let Some(z) = self.zoom_mut(id) {
                            z.start = t.min(z.end - 0.3).max(0.0);
                            z.auto = false;
                        }
                    }
                    Drag::ZoomEnd(id) => {
                        if let Some(z) = self.zoom_mut(id) {
                            z.end = t.max(z.start + 0.3).min(dur);
                            z.auto = false;
                        }
                    }
                    Drag::ZoomMove { id, grab } => {
                        if let Some(z) = self.zoom_mut(id) {
                            let len = z.end - z.start;
                            z.start = (t - grab).clamp(0.0, (dur - len).max(0.0));
                            z.end = z.start + len;
                            z.auto = false;
                        }
                    }
                }
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                match self.hit(&tl, pos) {
                    Hit::ZoomBody(id, _) | Hit::ZoomStart(id) | Hit::ZoomEnd(id) => self.selected_zoom = Some(id),
                    Hit::ZoomEmpty => {
                        self.selected_zoom = None;
                        self.seek(tl.t(pos.x));
                    }
                    Hit::Ruler | Hit::Video => self.seek(tl.t(pos.x)),
                    _ => {}
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if self.hit(&tl, pos) == Hit::ZoomEmpty {
                    self.add_zoom_at(tl.t(pos.x));
                }
            }
        }

        self.paint_timeline(ui, &tl, hover_hit);
    }

    fn paint_timeline(&self, ui: &Ui, tl: &Tl, hover: Hit) {
        let p = ui.painter_at(Rect::from_min_max(tl.ruler.min - Vec2::new(0.0, 2.0), tl.zoom.max + Vec2::new(0.0, 2.0)));
        let dur = tl.dur;

        // Ruler
        let px_per_s = (tl.x1 - tl.x0) / dur;
        let step = [0.5f32, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0]
            .into_iter()
            .find(|s| s * px_per_s >= 70.0)
            .unwrap_or(600.0);
        let minor = if step >= 5.0 { step / 5.0 } else { step / 2.0 };
        let mut t = 0.0;
        while t <= dur + 1e-3 {
            let x = tl.x(t);
            let major = ((t / step).round() * step - t).abs() < 1e-3;
            let h = if major { 7.0 } else { 4.0 };
            p.line_segment(
                [Pos2::new(x, tl.ruler.bottom() - h), Pos2::new(x, tl.ruler.bottom())],
                Stroke::new(1.0_f32, if major { TEXT_3 } else { Color32::from_rgb(70, 70, 76) }),
            );
            if major {
                let s = fmt_time(t);
                let s = s.split(',').next().unwrap_or(&s).to_string();
                p.text(Pos2::new(x + 4.0, tl.ruler.top()), Align2::LEFT_TOP, s, regular(10.5), TEXT_3);
            }
            t += minor;
        }

        // Video track: filmstrip, trimmed-away parts dimmed.
        let full = Rect::from_min_max(Pos2::new(tl.x(0.0), tl.video.top()), Pos2::new(tl.x(dur), tl.video.bottom()));
        let clip = Rect::from_min_max(
            Pos2::new(tl.x(self.project.trim_start), tl.video.top()),
            Pos2::new(tl.x(self.project.trim_end), tl.video.bottom()),
        );
        p.rect_filled(full, Rounding::same(7.0), CLIP.linear_multiply(0.55));
        if !self.thumbs.is_empty() {
            let tp = ui.painter_at(full.shrink(1.0));
            let tile_w = (tl.video.height() * self.thumb_aspect).max(8.0);
            let mut x = full.left();
            while x < full.right() {
                let t = tl.t(x + tile_w * 0.5);
                let idx = ((t / dur) * self.thumbs.len() as f32).floor() as usize;
                let tex = &self.thumbs[idx.min(self.thumbs.len() - 1)];
                tp.image(
                    tex.id(),
                    Rect::from_min_size(Pos2::new(x, full.top()), Vec2::new(tile_w, full.height())),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                x += tile_w;
            }
        }
        let shade = Color32::from_black_alpha(165);
        if clip.left() > full.left() {
            p.rect_filled(Rect::from_min_max(full.min, Pos2::new(clip.left(), full.bottom())), Rounding::same(7.0), shade);
        }
        if clip.right() < full.right() {
            p.rect_filled(Rect::from_min_max(Pos2::new(clip.right(), full.top()), full.max), Rounding::same(7.0), shade);
        }
        // Round the filmstrip corners by drawing the track colour over them.
        p.rect_stroke(full.expand(1.5), Rounding::same(8.5), Stroke::new(3.0_f32, WINDOW));

        let trim_active = matches!(self.drag, Some(Drag::TrimStart | Drag::TrimEnd))
            || matches!(hover, Hit::TrimStart | Hit::TrimEnd | Hit::Video);
        if trim_active {
            p.rect_stroke(clip, Rounding::same(7.0), Stroke::new(2.0_f32, YELLOW));
            for x in [clip.left(), clip.right()] {
                p.rect_filled(Rect::from_center_size(Pos2::new(x, clip.center().y), Vec2::new(5.0, 26.0)), Rounding::same(2.5), YELLOW);
            }
        } else {
            p.rect_stroke(clip, Rounding::same(7.0), Stroke::new(1.0_f32, Color32::from_white_alpha(60)));
        }
        let name = self
            .project
            .media
            .as_ref()
            .and_then(|m| std::path::Path::new(&m.path).file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let g = p.layout_no_wrap(name, regular(11.0), Color32::WHITE);
        let label = Rect::from_min_size(clip.min + Vec2::new(6.0, 5.0), g.size() + Vec2::new(10.0, 4.0));
        if label.right() < clip.right() - 4.0 {
            p.rect_filled(label, Rounding::same(4.0), Color32::from_black_alpha(140));
            p.galley(label.min + Vec2::new(5.0, 2.0), g, Color32::WHITE);
        }

        // Touches
        let ty = tl.video.bottom() - 9.0;
        for t in &self.project.touches {
            let x = tl.x(t.time);
            if t.kind == TouchKind::Swipe {
                let x2 = tl.x(t.time + t.duration).max(x + 4.0);
                p.line_segment([Pos2::new(x, ty), Pos2::new(x2, ty)], Stroke::new(4.0_f32, Color32::from_white_alpha(170)));
            }
            p.circle_filled(Pos2::new(x, ty), 3.5, Color32::WHITE);
            p.circle_stroke(Pos2::new(x, ty), 3.5, Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
        }

        // Zoom track
        p.rect_filled(tl.zoom, Rounding::same(7.0), TRACK);
        if self.project.zooms.is_empty() {
            p.text(
                tl.zoom.center(),
                Align2::CENTER_CENTER,
                "Sin zooms · pulsa + Zoom, la tecla Z o haz doble clic aquí",
                regular(12.0),
                TEXT_3,
            );
        }
        for z in &self.project.zooms {
            let r = Rect::from_min_max(Pos2::new(tl.x(z.start), tl.zoom.top() + 2.0), Pos2::new(tl.x(z.end), tl.zoom.bottom() - 2.0));
            let selected = self.selected_zoom == Some(z.id);
            let hovered = matches!(hover, Hit::ZoomBody(id, _) | Hit::ZoomStart(id) | Hit::ZoomEnd(id) if id == z.id);
            let fill = if selected || hovered { lerp_color(PURPLE, Color32::WHITE, 0.12) } else { PURPLE };
            p.rect_filled(r, Rounding::same(6.0), fill);
            if selected {
                p.rect_stroke(r, Rounding::same(6.0), Stroke::new(2.0_f32, YELLOW));
            }
            let label = format!("{:.1}×", z.scale).replace('.', ",");
            if r.width() > 34.0 {
                p.text(r.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, label, medium(11.5), Color32::WHITE);
            }
            if z.auto && r.width() > 80.0 {
                p.text(r.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, "auto", regular(10.5), Color32::from_white_alpha(150));
            }
        }

        // Playhead
        let x = tl.x(self.time);
        p.line_segment([Pos2::new(x, tl.ruler.top() + 4.0), Pos2::new(x, tl.zoom.bottom())], Stroke::new(1.5_f32, RED));
        let head = vec![
            Pos2::new(x - 5.0, tl.ruler.top()),
            Pos2::new(x + 5.0, tl.ruler.top()),
            Pos2::new(x + 5.0, tl.ruler.top() + 6.0),
            Pos2::new(x, tl.ruler.top() + 11.0),
            Pos2::new(x - 5.0, tl.ruler.top() + 6.0),
        ];
        p.add(egui::Shape::convex_polygon(head, RED, Stroke::NONE));
    }

    // ----- Sheets -----

    fn sheet_window(ctx: &egui::Context, id: &str, width: f32, add: impl FnOnce(&mut Ui)) {
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("sheet-dim"))
            .order(Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(130));
                ui.allocate_rect(screen, Sense::click_and_drag());
            });
        egui::Window::new(id)
            .id(Id::new(("sheet", id)))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, -20.0))
            .min_width(width)
            .max_width(width)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(SHEET)
                    .inner_margin(Margin::same(24.0))
                    .rounding(Rounding::same(14.0)),
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                add(ui);
            });
    }

    fn record_sheet(&mut self, ctx: &egui::Context) {
        let mut action: Option<&str> = None;
        Self::sheet_window(ctx, "record", 440.0, |ui| {
            if let Some(rec) = &self.recording {
                ui.vertical_centered(|ui| {
                    let t = ui.input(|i| i.time) as f32;
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    let pulse = 0.55 + 0.45 * (t * 4.0).sin().abs();
                    ui.painter().circle_filled(r.center(), 7.0, RED.linear_multiply(pulse));
                    ui.label(RichText::new("Grabando").font(medium(14.0)).color(RED));
                    let secs = rec.started.elapsed().as_secs();
                    ui.label(RichText::new(format!("{}:{:02}", secs / 60, secs % 60)).font(medium(46.0)).color(TEXT));
                    let n = rec.touch_count.load(Ordering::Relaxed);
                    let msg = if rec.tracks_touches() {
                        format!("{n} toques registrados")
                    } else {
                        "Este móvil no permite registrar los toques: añadirás los zooms a mano".into()
                    };
                    ui.label(RichText::new(msg).color(TEXT_2));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Usa el móvil con los dedos con normalidad. Cada toque se convertirá en un zoom suave.")
                            .color(TEXT_3)
                            .font(regular(12.5)),
                    );
                    if rec.method == Method::ScreenRecord && secs > 150 {
                        ui.label(RichText::new("Sin scrcpy la grabación se corta a los 3 minutos.").color(YELLOW).font(regular(12.5)));
                    }
                    ui.add_space(16.0);
                    ui.spacing_mut().button_padding = Vec2::new(28.0, 8.0);
                    if tinted_button(ui, "Detener grabación", RED, true).clicked() {
                        action = Some("stop");
                    }
                });
                return;
            }
            if self.saving_recording {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.add(egui::Spinner::new().size(28.0).color(TEXT_2));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Guardando la grabación…").font(medium(16.0)).color(TEXT));
                    ui.label(RichText::new("Copiando el vídeo y sincronizando los toques.").color(TEXT_2));
                    ui.add_space(10.0);
                });
                return;
            }

            ui.label(RichText::new("Grabar el móvil").font(medium(20.0)).color(TEXT));
            ui.label(RichText::new("Graba la pantalla de tu móvil Android conectado por USB.").color(TEXT_2));
            ui.add_space(10.0);

            if !self.tools.adb {
                info_box(ui, YELLOW, "Para grabar necesitas adb y scrcpy. Instálalos con:\nsudo pacman -S android-tools scrcpy");
            } else {
                ui.label(RichText::new("Dispositivo").font(medium(12.5)).color(TEXT_2));
                if self.devices.is_empty() {
                    egui::Frame::none().fill(CARD).rounding(Rounding::same(10.0)).inner_margin(Margin::same(14.0)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(30.0, 40.0), Sense::hover());
                            icon_phone(ui.painter(), r.center(), 30.0, TEXT_2);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Conecta tu móvil por USB").font(medium(14.0)).color(TEXT));
                                ui.label(RichText::new("1. Activa Opciones de desarrollador › Depuración USB").color(TEXT_2).font(regular(12.5)));
                                ui.label(RichText::new("2. Conecta el cable y acepta el aviso en el móvil").color(TEXT_2).font(regular(12.5)));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(12.0).color(TEXT_3));
                            ui.label(RichText::new("Buscando dispositivos…").color(TEXT_3).font(regular(12.0)));
                        });
                    });
                } else {
                    for d in self.devices.clone() {
                        let selected = self.selected_serial.as_deref() == Some(&d.serial);
                        if device_row(ui, &d, selected).clicked() && d.state == DeviceState::Ready {
                            self.selected_serial = Some(d.serial.clone());
                        }
                    }
                }

                ui.add_space(6.0);
                egui::Frame::none().fill(CARD).rounding(Rounding::same(10.0)).inner_margin(Margin::symmetric(14.0, 10.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add_enabled_ui(self.tools.scrcpy, |ui| {
                        toggle_row(ui, "Grabar el sonido del móvil", "Necesita Android 11 o superior", &mut self.rec_opts.audio);
                        toggle_row(ui, "Ver el móvil en el ordenador", "Abre una ventana con la pantalla del móvil", &mut self.rec_opts.mirror);
                    });
                });
                if !self.tools.scrcpy {
                    ui.label(
                        RichText::new("Sin scrcpy se usa adb screenrecord: máximo 3 minutos y sin sonido. Instala scrcpy para mejores resultados.")
                            .color(YELLOW)
                            .font(regular(12.0)),
                    );
                }
            }

            if let Some(e) = &self.record_error {
                ui.add_space(4.0);
                info_box(ui, RED, e);
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let ready = self.selected_serial.is_some() && self.tools.adb;
                    if tinted_button(ui, "Empezar a grabar", RED, ready).clicked() {
                        action = Some("start");
                    }
                    if secondary_button(ui, "Cancelar").clicked() {
                        action = Some("close");
                    }
                });
            });
        });
        match action {
            Some("start") => self.start_recording(),
            Some("stop") => self.stop_recording(),
            Some("close") => self.sheet = Sheet::None,
            _ => {}
        }
    }

    fn export_sheet(&mut self, ctx: &egui::Context) {
        let mut action: Option<&str> = None;
        let media = self.project.media.clone();
        Self::sheet_window(ctx, "export", 460.0, |ui| match &self.export_ui {
            ExportUi::Running { fraction, eta, .. } => {
                ui.label(RichText::new("Exportando…").font(medium(20.0)).color(TEXT));
                ui.add_space(10.0);
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
                ui.painter().rect_filled(r, Rounding::same(3.0), CONTROL);
                let mut done = r;
                done.set_width(r.width() * fraction.clamp(0.0, 1.0));
                ui.painter().rect_filled(done, Rounding::same(3.0), ACCENT);
                let left = match eta {
                    Some(s) if *s >= 1.0 => format!(" · quedan unos {} s", s.round() as u32),
                    _ => String::new(),
                };
                ui.label(RichText::new(format!("{:.0} %{left}", fraction * 100.0)).color(TEXT_2));
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, "Cancelar").clicked() {
                        action = Some("cancel");
                    }
                });
            }
            ExportUi::Done(path) => {
                ui.vertical_centered(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 26.0, GREEN.linear_multiply(0.2));
                    icon_check(ui.painter(), r.center(), 30.0, GREEN);
                    ui.label(RichText::new("Vídeo listo").font(medium(20.0)).color(TEXT));
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(RichText::new(name).color(TEXT_2));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let w = 3.0 * 120.0 + 16.0;
                        ui.add_space((ui.available_width() - w) / 2.0);
                        if secondary_button(ui, "Mostrar en carpeta").clicked() {
                            action = Some("folder");
                        }
                        if secondary_button(ui, "Reproducir").clicked() {
                            action = Some("play");
                        }
                        if primary_button(ui, "Hecho", true).clicked() {
                            action = Some("close");
                        }
                    });
                });
            }
            ExportUi::Failed(msg) => {
                ui.label(RichText::new("No se pudo exportar").font(medium(20.0)).color(TEXT));
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| info_box(ui, RED, msg));
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if primary_button(ui, "Volver", true).clicked() {
                        action = Some("reset");
                    }
                });
            }
            ExportUi::Idle => {
                let s = &mut self.project.export;
                let style = &mut self.project.style;
                ui.label(RichText::new("Exportar vídeo").font(medium(20.0)).color(TEXT));
                let (w, h) = s.dimensions(style.aspect);
                let dur = self.project.trim_end - self.project.trim_start;
                ui.label(
                    RichText::new(format!("{} × {} px · {} fps · {}", w, h, if s.format == ExportFormat::Gif { s.fps.min(25) } else { s.fps }, fmt_time(dur)))
                        .color(TEXT_2),
                );
                ui.add_space(12.0);
                let cw = 290.0;
                form_row(ui, "Formato", |ui| {
                    let opts: Vec<(Aspect, &str)> = Aspect::ALL.iter().map(|a| (*a, a.label())).collect();
                    segmented(ui, &mut style.aspect, &opts, cw);
                });
                form_row(ui, "Resolución", |ui| {
                    let opts: Vec<(Resolution, &str)> = Resolution::ALL.iter().map(|r| (*r, r.label())).collect();
                    segmented(ui, &mut s.resolution, &opts, cw);
                });
                form_row(ui, "Fotogramas", |ui| {
                    segmented(ui, &mut s.fps, &[(30, "30 fps"), (60, "60 fps")], cw);
                });
                form_row(ui, "Archivo", |ui| {
                    let opts: Vec<(ExportFormat, &str)> = ExportFormat::ALL.iter().map(|f| (*f, f.label())).collect();
                    segmented(ui, &mut s.format, &opts, cw);
                });
                form_row(ui, "", |ui| {
                    ui.label(RichText::new(s.format.hint()).color(TEXT_3).font(regular(12.0)));
                });
                form_row(ui, "Calidad", |ui| {
                    let opts: Vec<(Quality, &str)> = Quality::ALL.iter().map(|q| (*q, q.label())).collect();
                    segmented(ui, &mut s.quality, &opts, cw);
                });
                if media.as_ref().is_some_and(|m| m.has_audio) && s.format != ExportFormat::Gif {
                    form_row(ui, "Sonido", |ui| {
                        toggle(ui, &mut s.include_audio);
                    });
                }
                form_row(ui, "Guardar en", |ui| {
                    fixed_label(ui, cw - 98.0, RichText::new(home_relative(&s.folder)).color(TEXT), Align::Min);
                    if secondary_button(ui, "Cambiar…").clicked() {
                        action = Some("pick-folder");
                    }
                });
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if primary_button(ui, "Exportar", true).clicked() {
                        action = Some("export");
                    }
                    if secondary_button(ui, "Cancelar").clicked() {
                        action = Some("close");
                    }
                });
            }
        });
        match action {
            Some("export") => self.start_export(),
            Some("cancel") => {
                if let ExportUi::Running { cancel, .. } = &self.export_ui {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            Some("folder") => {
                if let ExportUi::Done(p) = &self.export_ui {
                    if let Some(dir) = p.parent() {
                        open_path(dir);
                    }
                }
            }
            Some("play") => {
                if let ExportUi::Done(p) = &self.export_ui {
                    open_path(p);
                }
            }
            Some("pick-folder") => self.pick_folder(),
            Some("reset") => self.export_ui = ExportUi::Idle,
            Some("close") => {
                self.sheet = Sheet::None;
                self.export_ui = ExportUi::Idle;
            }
            _ => {}
        }
    }

    fn paint_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 4.5 {
            self.toast = None;
            return;
        }
        let alpha = (4.5 - age).min(0.3) / 0.3;
        egui::Area::new(Id::new("toast"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 66.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(Color32::from_rgb(64, 64, 70).linear_multiply(alpha))
                    .rounding(Rounding::same(18.0))
                    .inner_margin(Margin::symmetric(16.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(msg.as_str()).color(TEXT.linear_multiply(alpha)));
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn paint_drop_overlay(&self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("drop"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .interactable(false)
            .show(ctx, |ui| {
                let p = ui.painter();
                p.rect_filled(screen, 0.0, Color32::from_black_alpha(160));
                let r = screen.shrink(40.0);
                p.rect_stroke(r, Rounding::same(18.0), Stroke::new(2.0_f32, ACCENT));
                p.text(r.center(), Align2::CENTER_CENTER, "Suelta el vídeo para importarlo", medium(22.0), TEXT);
            });
    }
}

fn info_box(ui: &mut Ui, tint: Color32, text: &str) {
    egui::Frame::none()
        .fill(tint.linear_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, tint.linear_multiply(0.4)))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(TEXT).font(regular(12.5)));
        });
}

fn form_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        fixed_label(ui, 92.0, RichText::new(label).color(TEXT_2), Align::Max);
        ui.add_space(6.0);
        add(ui);
    });
}

fn toggle_row(ui: &mut Ui, title: &str, subtitle: &str, value: &mut bool) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(title).color(TEXT));
            ui.label(RichText::new(subtitle).color(TEXT_3).font(regular(11.5)));
        });
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            toggle(ui, value);
        });
    });
}

fn device_row(ui: &mut Ui, d: &Device, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let p = ui.painter();
    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
    p.rect_filled(rect, Rounding::same(10.0), lerp_color(CARD, Color32::from_rgb(56, 56, 61), h));
    if selected {
        p.rect_stroke(rect, Rounding::same(10.0), Stroke::new(2.0_f32, ACCENT));
    }
    let ready = d.state == DeviceState::Ready;
    icon_phone(p, Pos2::new(rect.left() + 26.0, rect.center().y), 26.0, if ready { TEXT } else { TEXT_3 });
    p.text(Pos2::new(rect.left() + 50.0, rect.top() + 10.0), Align2::LEFT_TOP, &d.model, medium(14.0), TEXT);
    let (sub, col) = match d.state {
        DeviceState::Ready => (format!("Conectado · {}", d.serial), TEXT_3),
        DeviceState::Unauthorized => ("Acepta el aviso de depuración USB en el móvil".to_string(), YELLOW),
        DeviceState::Offline => ("No responde · desconecta y vuelve a conectar".to_string(), YELLOW),
    };
    p.text(Pos2::new(rect.left() + 50.0, rect.top() + 30.0), Align2::LEFT_TOP, sub, regular(12.0), col);
    if selected {
        p.circle_filled(Pos2::new(rect.right() - 22.0, rect.center().y), 9.0, ACCENT);
        icon_check(p, Pos2::new(rect.right() - 22.0, rect.center().y), 12.0, Color32::WHITE);
    }
    resp
}

/// A device model button with a small drawing of its shape.
fn model_tile(ui: &mut Ui, f: FrameStyle, selected: bool, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 66.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(8.0), lerp_color(Color32::from_rgb(56, 56, 61), Color32::from_rgb(68, 68, 74), h));
    if selected {
        p.rect_stroke(rect, Rounding::same(8.0), Stroke::new(2.0_f32, ACCENT));
    }
    let c = Pos2::new(rect.center().x, rect.top() + 25.0);
    let line = if selected { TEXT } else { TEXT_2 };
    let screen = Color32::from_rgb(92, 104, 130);
    let (size, r, bezel) = match f {
        FrameStyle::Rounded => (Vec2::new(17.0, 32.0), 5.0, 2.0),
        FrameStyle::Flat => (Vec2::new(17.0, 32.0), 1.5, 1.8),
        FrameStyle::Curved => (Vec2::new(17.0, 32.0), 3.5, 1.2),
        FrameStyle::Tablet => (Vec2::new(36.0, 26.0), 3.0, 3.0),
        FrameStyle::None => (Vec2::new(15.0, 30.0), 3.0, 0.0),
        FrameStyle::Custom => (Vec2::ZERO, 0.0, 0.0),
    };
    let body = Rect::from_center_size(c, size);
    if f == FrameStyle::Custom {
        // A small cube.
        let st = Stroke::new(1.5_f32, line);
        let r = 13.0;
        let pts: Vec<Pos2> = (0..6)
            .map(|i| {
                let a = (i as f32 * 60.0 - 90.0).to_radians();
                Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect();
        p.add(egui::Shape::closed_line(pts.clone(), st));
        for k in [1, 3, 5] {
            p.line_segment([c, pts[k]], st);
        }
    } else if f == FrameStyle::None {
        p.rect_filled(body, Rounding::same(r), screen);
    } else {
        p.rect_filled(body, Rounding::same(r), Color32::from_rgb(24, 24, 27));
        p.rect_filled(body.shrink(bezel), Rounding::same((r - bezel).max(0.5)), screen);
        p.rect_stroke(body, Rounding::same(r), Stroke::new(1.5_f32, line));
        if f == FrameStyle::Curved {
            for x in [body.left() + 2.6, body.right() - 2.6] {
                p.line_segment([Pos2::new(x, body.top() + 4.0), Pos2::new(x, body.bottom() - 4.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(90)));
            }
        }
    }
    p.text(Pos2::new(rect.center().x, rect.bottom() - 11.0), Align2::CENTER_CENTER, f.label(), regular(11.5), line);
    resp
}

/// A gallery model with its rendered thumbnail.
fn gallery_tile(ui: &mut Ui, m: &LibraryModel, thumb: Option<&TextureHandle>, selected: bool, busy: bool, broken: bool, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, width * 1.18 + 20.0), Sense::click());
    let tip = match (&m.author, &m.license) {
        (Some(a), Some(l)) => format!("{} · de {a} ({l})", m.name),
        _ if broken => format!("{} · no se puede cargar (clic derecho para eliminarlo)", m.name),
        _ => format!("{} · importado por ti (clic derecho para renombrar o eliminar)", m.name),
    };
    let resp = resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand);
    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    let img = Rect::from_min_size(rect.min, Vec2::new(width, width * 1.18));
    p.rect_filled(img, Rounding::same(8.0), lerp_color(Color32::from_rgb(56, 56, 61), Color32::from_rgb(68, 68, 74), h));
    match thumb {
        Some(t) => {
            p.image(t.id(), img.shrink(1.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            // Round the corners of the picture.
            p.rect_stroke(img.expand(1.0), Rounding::same(9.0), Stroke::new(2.5_f32, CARD));
        }
        None => {
            icon_phone(p, img.center(), 30.0, TEXT_3);
        }
    }
    if busy {
        p.rect_filled(img, Rounding::same(8.0), Color32::from_black_alpha(110));
        p.text(img.center(), Align2::CENTER_CENTER, "Cargando…", regular(11.0), TEXT);
    } else if broken {
        p.rect_filled(img, Rounding::same(8.0), Color32::from_black_alpha(140));
        p.text(img.center() - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, "⚠", regular(18.0), YELLOW);
        p.text(img.center() + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, "No carga", regular(11.0), YELLOW);
    }
    if selected {
        p.rect_stroke(img, Rounding::same(8.0), Stroke::new(2.0_f32, ACCENT));
    }
    if let Some(l) = &m.license {
        let badge = if l.starts_with("CC BY") { "CC BY" } else { "CC0" };
        let g = p.layout_no_wrap(badge.to_string(), medium(9.5), Color32::WHITE);
        let r = Rect::from_min_size(Pos2::new(img.right() - g.size().x - 12.0, img.top() + 5.0), g.size() + Vec2::new(8.0, 3.0));
        p.rect_filled(r, Rounding::same(4.0), Color32::from_black_alpha(130));
        p.galley(r.min + Vec2::new(4.0, 1.5), g, Color32::WHITE);
    }
    let col = if selected { TEXT } else { TEXT_2 };
    p.text(Pos2::new(rect.center().x, rect.bottom() - 9.0), Align2::CENTER_CENTER, &m.name, regular(11.5), col);
    resp
}

fn import_tile(ui: &mut Ui, width: f32, busy: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, width * 1.18 + 20.0), Sense::click());
    let resp = resp
        .on_hover_text("Añadir un modelo .glb, .gltf o la descarga .zip que lo contiene")
        .on_hover_cursor(CursorIcon::PointingHand);
    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    let img = Rect::from_min_size(rect.min, Vec2::new(width, width * 1.18));
    p.rect_filled(img, Rounding::same(8.0), lerp_color(Color32::from_rgb(48, 48, 52), Color32::from_rgb(62, 62, 67), h));
    p.rect_stroke(img.shrink(0.5), Rounding::same(8.0), Stroke::new(1.0_f32, Color32::from_rgb(84, 84, 90)));
    let c = img.center();
    if busy {
        p.text(c, Align2::CENTER_CENTER, "Importando…", regular(11.0), TEXT_2);
    } else {
        let st = Stroke::new(2.0_f32, TEXT_2);
        p.line_segment([c - Vec2::new(10.0, 0.0), c + Vec2::new(10.0, 0.0)], st);
        p.line_segment([c - Vec2::new(0.0, 10.0), c + Vec2::new(0.0, 10.0)], st);
    }
    p.text(Pos2::new(rect.center().x, rect.bottom() - 9.0), Align2::CENTER_CENTER, "Importar", regular(11.5), TEXT_2);
    resp
}
