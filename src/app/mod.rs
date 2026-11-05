mod inspector;
mod sheets;
mod timeline_ui;
mod viewer;

use crate::capture::{self, CaptureResult, Device, DeviceState, Method, RecordOptions, Recording, Tools};
use crate::compositor::{compose, CamState, ComposeInput, Layout, Mips};
use crate::export::{self, ExportEvent};
use crate::library::{self, LibraryModel};
use crate::media::{self, DecodedFrame, PreviewDecoder, PREVIEW_FPS};
use crate::model::*;
use crate::model3d::{self, ModelSource, PreparedModel};
use crate::theme::{self, *};
use crate::timeline::{self, CameraValue, DevicePreset, PoseValue};
use crate::update;
use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{
    Align, Align2, Color32, ColorImage, CursorIcon, Id, Key, Margin, Order, Pos2, Rect, RichText, Rounding, Sense,
    Stroke, TextureHandle, TextureOptions, Ui, UiBuilder, Vec2,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

enum AppEvent {
    Devices(Vec<Device>),
    MediaLoaded { info: MediaInfo, touches: Vec<Touch>, recorded: bool },
    /// A recording picked to replace the one in a clip.
    ReplaceLoaded { clip: u64, info: MediaInfo },
    /// Screen changes found in a recording (recording seconds).
    Marks { source: String, marks: Vec<f32> },
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
    UpdateFound(update::Release),
    UpdateInstalled(Result<(), String>),
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Sheet {
    None,
    Record,
    Export,
    RenameModel,
    DeleteModel,
    Projects,
    Assistant,
    Update,
}

enum UpdateUi {
    Idle,
    Installing,
    Installed,
    Failed(String),
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
    Running { fraction: f32, eta: Option<f32>, cancel: Arc<AtomicBool>, encoder: Option<String> },
    Done(PathBuf),
    Failed(String),
}

/// What is selected in the timeline (and shown in the inspector).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Selection {
    None,
    Clip(u64),
    Camera { id: u64, key: Option<usize> },
    Device { id: u64, key: Option<usize> },
    Text(u64),
    Target(u64),
}

#[derive(PartialEq, Clone)]
struct RenderKey {
    /// Titles on screen and motion blur samples, plus a hash of every title.
    texts: (usize, usize, u64),
    time: f32,
    frame: usize,
    style: Style,
    cam: [u32; 6],
    pose: [u32; 6],
    size: (usize, usize),
    video: (u32, u32),
    touches: usize,
    model: usize,
    supersample: usize,
    /// The previous clip's frame during a dissolve (pointer and weight bits).
    from: (usize, u32),
}

/// Preview resources of one recording.
#[derive(Default)]
struct MediaView {
    proxy: Option<PathBuf>,
    thumbs: Vec<TextureHandle>,
    thumb_aspect: f32,
}

pub struct RecOrdingApp {
    project: Project,
    project_path: PathBuf,
    /// Revision of the project file as we last wrote or read it (the in-memory project keeps 0).
    disk_revision: u64,
    last_saved: Project,
    last_save_check: Instant,
    last_disk_check: Instant,
    undo: Vec<Project>,
    redo: Vec<Project>,
    committed: Project,
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
    /// Timeline time = source time of the decoded recording + this offset.
    /// Maps the current request's recording seconds to the timeline: (clip start, its
    /// source_in, its speed).
    gen_map: (f32, f32, f32),
    decoder_source: Option<(String, (usize, usize))>,
    playing_clip: Option<u64>,
    shown: Option<DecodedFrame>,
    queued: Option<DecodedFrame>,
    requested_time: Option<f32>,
    media_views: HashMap<u64, MediaView>,

    // Viewer
    viewer_tex: Option<TextureHandle>,
    /// The window's graphics card, when the preview can be drawn on it.
    gpu_state: Option<eframe::egui_wgpu::RenderState>,
    gpu_view: Option<viewer::GpuView>,
    /// The texture with the last frame drawn (on the card or by the CPU).
    viewer_shown: Option<egui::TextureId>,
    viewer_buf: Vec<u8>,
    last_render: Option<RenderKey>,

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
    selection: Selection,
    drag: Option<timeline_ui::Drag>,
    director_mode: bool,
    /// Director box being drawn: start point in recording coordinates.
    drawing_target: Option<(f32, f32)>,

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
    projects: Vec<ProjectEntry>,
    new_project_name: String,
    update: Option<update::Release>,
    update_ui: UpdateUi,
    /// Put the cursor in the headline of the selected title (just added).
    focus_title: bool,
    /// The clip whose video the file picker replaces (instead of adding a clip).
    replacing: Option<u64>,
    /// While playing across a cut: the last frame of the clip before, and the clip after.
    dissolve: Option<(Arc<Mips>, u64)>,
    /// Recordings already searched for screen changes in this session.
    marks_done: std::collections::HashSet<String>,

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

use crate::platform::open_path;

fn home_relative(p: &str) -> String {
    let h = crate::platform::home().to_string_lossy().into_owned();
    match p.strip_prefix(&h) {
        Some(rest) if !h.is_empty() => format!("~{rest}"),
        _ => p.to_string(),
    }
}

/// Reads a project and returns it with the in-memory revision (0) plus its revision on disk.
fn load_for_app(path: &std::path::Path) -> (Project, u64) {
    let mut p = load_project(path).unwrap_or_default();
    let rev = p.revision;
    p.revision = 0;
    (p, rev)
}

impl RecOrdingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let (tx, rx) = unbounded();
        let project_path = current_project_path();
        let (project, disk_revision) = load_for_app(&project_path);
        let notify_ctx = cc.egui_ctx.clone();
        let mut app = Self {
            last_saved: project.clone(),
            committed: project.clone(),
            project,
            project_path,
            disk_revision,
            last_save_check: Instant::now(),
            last_disk_check: Instant::now(),
            undo: Vec::new(),
            redo: Vec::new(),
            tools: capture::detect_tools(),
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            time: 0.0,
            playing: false,
            last_tick: Instant::now(),
            decoder: PreviewDecoder::new(move || notify_ctx.request_repaint()),
            gen: 0,
            gen_map: (0.0, 0.0, 1.0),
            decoder_source: None,
            playing_clip: None,
            shown: None,
            queued: None,
            requested_time: None,
            media_views: HashMap::new(),
            viewer_tex: None,
            // A software renderer would be slower than our own CPU compositor.
            gpu_state: cc
                .wgpu_render_state
                .clone()
                .filter(|s| s.adapter.get_info().device_type != wgpu::DeviceType::Cpu && std::env::var_os("REC_CPU_PREVIEW").is_none()),
            gpu_view: None,
            viewer_shown: None,
            viewer_buf: Vec::new(),
            last_render: None,
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
            selection: Selection::None,
            drag: None,
            director_mode: false,
            drawing_target: None,
            sheet: Sheet::None,
            devices: Vec::new(),
            scanning: false,
            last_scan: None,
            selected_serial: None,
            rec_opts: RecordOptions { audio: true, mirror: true, clean_status_bar: true },
            recording: None,
            saving_recording: false,
            record_error: None,
            export_ui: ExportUi::Idle,
            export_rx: None,
            projects: Vec::new(),
            new_project_name: String::new(),
            update: None,
            update_ui: UpdateUi::Idle,
            focus_title: false,
            replacing: None,
            dissolve: None,
            marks_done: Default::default(),
            toast: None,
        };
        app.project_opened();
        app.refresh_library();
        app.check_for_update();
        // Find the graphics card encoder now, so exporting does not wait for it.
        let ctx = cc.egui_ctx.clone();
        thread::spawn(move || {
            crate::gpu::detect();
            crate::hwenc::detect();
            ctx.request_repaint();
        });
        app
    }

    /// Looks for a newer release in the background; a button appears in the toolbar if there is one.
    fn check_for_update(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            if let Ok(Some(r)) = update::check() {
                if update::skipped().as_deref() != Some(r.version.as_str()) {
                    let _ = tx.send(AppEvent::UpdateFound(r));
                    ctx.request_repaint();
                }
            }
        });
    }

    fn install_update(&mut self) {
        let Some(r) = self.update.clone() else {
            return;
        };
        self.update_ui = UpdateUi::Installing;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let res = update::install(&r, &update::method());
            let _ = tx.send(AppEvent::UpdateInstalled(res));
            ctx.request_repaint();
        });
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // ---------- Projects ----------

    /// Resets what depends on the open project (after opening, switching or reloading it).
    fn project_opened(&mut self) {
        self.time = self.time.min(timeline::duration(&self.project));
        self.playing = false;
        self.selection = Selection::None;
        self.director_mode = false;
        self.requested_time = None;
        self.shown = None;
        self.last_render = None;
        let media: Vec<MediaInfo> = self.project.media.iter().map(|m| m.info.clone()).collect();
        for info in media {
            self.prepare_media(&info);
        }
        if let Some(cm) = self.project.style.model.clone() {
            self.load_model(cm.path);
        }
    }

    fn save_now(&mut self) {
        let mut copy = self.project.clone();
        copy.revision = self.disk_revision;
        if save_project_to(&self.project_path, &mut copy).is_ok() {
            self.disk_revision = copy.revision;
            self.last_saved = self.project.clone();
        }
    }

    fn open_project(&mut self, path: PathBuf) {
        self.save_now();
        set_current_project(&path);
        let (p, rev) = load_for_app(&path);
        self.project_path = path;
        self.disk_revision = rev;
        self.project = p;
        self.last_saved = self.project.clone();
        self.committed = self.project.clone();
        self.undo.clear();
        self.redo.clear();
        self.time = 0.0;
        self.project_opened();
    }

    fn new_project(&mut self, name: &str) {
        match create_project(name, Some(&self.project)) {
            Ok((path, _)) => {
                self.open_project(path);
                self.toast(format!("Proyecto «{}» creado", name.trim()));
            }
            Err(e) => self.toast(e),
        }
    }

    /// Picks up changes made outside the app (the MCP server) to the open project, or another
    /// project being opened.
    fn check_disk(&mut self) {
        if self.last_disk_check.elapsed() < Duration::from_millis(700) {
            return;
        }
        self.last_disk_check = Instant::now();
        let current = current_project_path();
        if current != self.project_path {
            self.open_project(current);
            self.toast("Se ha abierto otro proyecto desde el asistente");
            return;
        }
        let Ok(disk) = load_project(&self.project_path) else { return };
        if disk.revision != self.disk_revision {
            let mut p = disk;
            self.disk_revision = p.revision;
            p.revision = 0;
            if p != self.project {
                self.undo.push(std::mem::replace(&mut self.project, p));
                self.redo.clear();
                self.committed = self.project.clone();
                self.last_saved = self.project.clone();
                let duration = timeline::duration(&self.project);
                self.time = self.time.min(duration);
                self.requested_time = None;
                self.last_render = None;
                let media: Vec<MediaInfo> = self.project.media.iter().map(|m| m.info.clone()).collect();
                for info in media {
                    self.prepare_media(&info);
                }
                self.toast("Cambios del asistente aplicados");
            }
        }
    }

    // ---------- Media ----------

    /// Builds the preview proxy and filmstrip of a recording (once).
    fn prepare_media(&mut self, info: &MediaInfo) {
        let ids: Vec<u64> = self.project.media.iter().filter(|m| m.info.path == info.path).map(|m| m.id).collect();
        let fresh: Vec<u64> = ids.iter().copied().filter(|id| !self.media_views.contains_key(id)).collect();
        if fresh.is_empty() {
            return;
        }
        for id in fresh {
            self.media_views.insert(id, MediaView { thumb_aspect: info.width as f32 / info.height.max(1) as f32, ..Default::default() });
        }
        let (tx, ctx, info) = (self.tx.clone(), self.ctx.clone(), info.clone());
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

    /// The recording shown at the current time: (media id, its info).
    fn current_media(&self) -> Option<&Media> {
        let c = timeline::locate(&self.project, self.time)?;
        self.project.media(c.clip.media)
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
        thread::spawn(move || {
            let res = crate::platform::pick_file("Importar vídeo", "Vídeos", &["mp4", "mov", "mkv", "webm"], &videos_dir());
            match res {
                Ok(Some(p)) => {
                    let _ = tx.send(AppEvent::ImportPicked(p));
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
        thread::spawn(move || {
            let res = crate::platform::pick_file("Importar modelo 3D", "Modelos 3D", &["glb", "gltf", "zip"], &crate::platform::home());
            match res {
                Ok(Some(p)) => {
                    let _ = tx.send(AppEvent::ModelPicked(p));
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
            if let Ok(Some(p)) = crate::platform::pick_folder("Guardar vídeos en", std::path::Path::new(&current)) {
                let _ = tx.send(AppEvent::FolderPicked(p));
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
                    let was_empty = self.project.is_empty();
                    let start = timeline::duration(&self.project);
                    let clip = timeline::append_media(&mut self.project, info.clone(), touches);
                    let scale = self.project.style.auto_zoom_scale;
                    let zooms = if n_touches > 0 { timeline::regenerate_auto_zooms(&mut self.project, scale) } else { 0 };
                    self.prepare_media(&info);
                    self.seek(start);
                    self.selection = Selection::Clip(clip);
                    if recorded {
                        self.sheet = Sheet::None;
                    }
                    let what = if recorded { "Grabación" } else { "Vídeo" };
                    let place = if was_empty { "lista" } else { "añadida al final" };
                    self.toast(if n_touches > 0 {
                        format!("{what} {place} · {n_touches} toques → {zooms} zooms automáticos")
                    } else {
                        format!("{what} {place}. Añade zooms con + Zoom o la tecla Z.")
                    });
                }
                AppEvent::MediaFailed(e) => self.toast(e),
                AppEvent::RecordingFailed(e) => {
                    self.saving_recording = false;
                    self.record_error = Some(e);
                    self.sheet = Sheet::Record;
                }
                AppEvent::ProxyReady { source, proxy } => {
                    let ids: Vec<u64> = self.project.media.iter().filter(|m| m.info.path == source).map(|m| m.id).collect();
                    for id in &ids {
                        self.media_views.entry(*id).or_default().proxy = Some(proxy.clone());
                    }
                    if !ids.is_empty() {
                        // Switch to the light copy if this recording is the one on screen.
                        if self.decoder_source.as_ref().is_some_and(|(p, _)| *p == source) {
                            self.restart_decoding();
                        }
                        // Look for screen changes (in the light copy, which is quick to read).
                        let missing = self.project.media.iter().any(|m| m.info.path == source && m.marks.is_empty());
                        if missing && self.marks_done.insert(source.clone()) {
                            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                            let proxy = proxy.to_string_lossy().into_owned();
                            thread::spawn(move || {
                                let marks = media::screen_changes(&proxy);
                                let _ = tx.send(AppEvent::Marks { source, marks });
                                ctx.request_repaint();
                            });
                        }
                    }
                }
                AppEvent::Marks { source, marks } => {
                    for m in self.project.media.iter_mut().filter(|m| m.info.path == source) {
                        m.marks = marks.clone();
                    }
                }
                AppEvent::Thumbnails { source, size, frames } => {
                    let ids: Vec<u64> = self.project.media.iter().filter(|m| m.info.path == source).map(|m| m.id).collect();
                    if !ids.is_empty() {
                        let thumbs: Vec<TextureHandle> = frames
                            .iter()
                            .enumerate()
                            .map(|(i, f)| {
                                let img = ColorImage::from_rgba_unmultiplied([size.0, size.1], f);
                                self.ctx.load_texture(format!("thumb-{source}-{i}"), img, TextureOptions::LINEAR)
                            })
                            .collect();
                        for id in ids {
                            let v = self.media_views.entry(id).or_default();
                            v.thumbs = thumbs.clone();
                            v.thumb_aspect = size.0 as f32 / size.1 as f32;
                        }
                    }
                }
                AppEvent::ImportPicked(p) => match self.replacing.take() {
                    Some(clip) => {
                        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                        thread::spawn(move || {
                            let ev = match media::probe(&p.to_string_lossy()) {
                                Ok(info) => AppEvent::ReplaceLoaded { clip, info },
                                Err(e) => AppEvent::MediaFailed(e),
                            };
                            let _ = tx.send(ev);
                            ctx.request_repaint();
                        });
                    }
                    None => self.import(p),
                },
                AppEvent::ReplaceLoaded { clip, info } => {
                    match timeline::replace_clip_video(&mut self.project, clip, info.clone(), Vec::new()) {
                        Ok(()) => {
                            self.prepare_media(&info);
                            self.requested_time = None;
                            self.toast("Vídeo cambiado; el recorte y lo que hay encima se conservan");
                        }
                        Err(e) => self.toast(e),
                    }
                }
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
                AppEvent::UpdateFound(r) => self.update = Some(r),
                AppEvent::UpdateInstalled(res) => {
                    self.update_ui = match res {
                        Ok(()) => UpdateUi::Installed,
                        Err(e) => UpdateUi::Failed(e),
                    };
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
                    ExportEvent::Encoder(name) => {
                        if let ExportUi::Running { encoder, .. } = &mut self.export_ui {
                            *encoder = Some(name);
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

    /// Where to decode for timeline second `t`: (file, preview size, source second, clip).
    fn decode_target(&self, t: f32) -> Option<(String, (usize, usize), f32, timeline::Placed)> {
        let c = timeline::locate(&self.project, t)?;
        let m = self.project.media(c.clip.media)?;
        let path = self
            .media_views
            .get(&m.id)
            .and_then(|v| v.proxy.as_ref())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| m.info.path.clone());
        Some((path, media::preview_size(&m.info), timeline::source_time(&c, t), c))
    }

    /// Asks the decoder for the frame at the current time (or to play from it).
    fn start_decode(&mut self, play: bool) {
        let Some((path, size, src_t, c)) = self.decode_target(self.time) else { return };
        if self.decoder_source.as_ref() != Some(&(path.clone(), size)) {
            self.decoder.set_source(&path, size);
            self.decoder_source = Some((path, size));
        }
        self.gen += 1;
        self.gen_map = (c.start, c.clip.source_in, c.clip.speed.max(0.01));
        self.queued = None;
        self.playing_clip = Some(c.clip.id);
        // A still is one frame, playing or not.
        if play && c.clip.freeze.is_none() {
            self.decoder.play(self.gen, src_t, c.clip.speed);
        } else {
            self.decoder.seek(self.gen, src_t);
        }
        self.requested_time = Some(self.time);
    }

    fn restart_decoding(&mut self) {
        if self.playing {
            self.start_decode(true);
        } else {
            self.requested_time = None;
        }
    }

    fn toggle_play(&mut self) {
        if self.project.is_empty() {
            return;
        }
        if self.playing {
            self.playing = false;
            self.decoder.stop();
            self.requested_time = None;
        } else {
            if self.time >= timeline::duration(&self.project) - 0.05 {
                self.time = 0.0;
            }
            self.playing = true;
            self.last_tick = Instant::now();
            self.start_decode(true);
        }
    }

    fn seek(&mut self, t: f32) {
        if self.playing {
            self.playing = false;
            self.decoder.stop();
        }
        self.time = t.clamp(0.0, timeline::duration(&self.project));
        self.requested_time = None;
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;
        let total = timeline::duration(&self.project);
        if self.playing {
            self.time += dt;
            if self.time >= total {
                self.time = total;
                self.toggle_play();
            } else if timeline::locate(&self.project, self.time).map(|c| c.clip.id) != self.playing_clip {
                // Crossed into the next clip: dissolve from the last frame shown.
                let next = timeline::locate(&self.project, self.time).map(|c| c.clip.id);
                self.dissolve = self.shown.as_ref().zip(next).map(|(f, id)| (f.mips.clone(), id));
                self.start_decode(true);
            }
            self.ctx.request_repaint();
        }
        self.time = self.time.min(total);

        loop {
            let next = self.queued.take().or_else(|| self.decoder.frames.try_recv().ok());
            let Some(f) = next else { break };
            if f.gen != self.gen {
                continue;
            }
            let (start, source_in, speed) = self.gen_map;
            let at = start + (f.time - source_in) / speed;
            if self.playing && at > self.time + 0.5 / PREVIEW_FPS {
                self.queued = Some(f);
                break;
            }
            self.shown = Some(f);
        }

        if !self.playing && !self.project.is_empty() && self.requested_time != Some(self.time) {
            self.start_decode(false);
        }
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
        self.export_ui = ExportUi::Running { fraction: 0.0, eta: None, cancel, encoder: None };
    }

    fn autosave(&mut self) {
        if self.last_save_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_save_check = Instant::now();
        if self.project != self.last_saved {
            // If the assistant saved in the meantime, take its version first instead of
            // overwriting it.
            let newer = load_project(&self.project_path).map(|d| d.revision != self.disk_revision).unwrap_or(false);
            if newer {
                self.last_disk_check = Instant::now() - Duration::from_secs(1);
                self.check_disk();
                return;
            }
            self.save_now();
        }
    }

    /// Turns finished edits into undo steps (a slider drag counts once, on release).
    fn commit_history(&mut self, ctx: &egui::Context) {
        // Typing a title is one step, taken when the field loses focus.
        if ctx.input(|i| i.pointer.any_down()) || self.drag.is_some() || ctx.wants_keyboard_input() {
            return;
        }
        if self.project != self.committed {
            let before = std::mem::replace(&mut self.committed, self.project.clone());
            self.undo.push(before);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo_redo(&mut self, undo: bool) {
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let Some(prev) = from.pop() else {
            self.toast(if undo { "No hay nada que deshacer" } else { "No hay nada que rehacer" });
            return;
        };
        to.push(std::mem::replace(&mut self.project, prev));
        self.committed = self.project.clone();
        self.selection = Selection::None;
        self.time = self.time.min(timeline::duration(&self.project));
        self.requested_time = None;
        let media: Vec<MediaInfo> = self.project.media.iter().map(|m| m.info.clone()).collect();
        for info in media {
            self.prepare_media(&info);
        }
    }

    /// Deletes whatever is selected.
    fn delete_selection(&mut self) {
        let p = &mut self.project;
        let r = match self.selection {
            Selection::Clip(id) => match timeline::delete_clip(p, id) {
                Ok(lost) if lost > 0 => Err(format!("Clip eliminado, con {lost} elementos que estaban sobre él (Ctrl+Z para recuperarlos)")),
                other => other.map(|_| ()),
            },
            Selection::Camera { id, key: Some(k) } if p.camera.iter().any(|c| c.id == id && c.kind == CameraKind::Move) => timeline::delete_camera_key(p, id, k),
            Selection::Camera { id, .. } => timeline::delete_camera_clip(p, id),
            Selection::Device { id, .. } => timeline::delete_device_clip(p, id),
            Selection::Text(id) => timeline::delete_text(p, id),
            Selection::Target(id) => timeline::remove_target(p, id),
            Selection::None => return,
        };
        if let Err(e) = r {
            self.toast(e);
        }
        self.selection = Selection::None;
        self.time = self.time.min(timeline::duration(&self.project));
        self.requested_time = None;
    }

    fn add_zoom_at(&mut self, t: f32) {
        let total = timeline::duration(&self.project);
        let end = (t + 2.5).min(total);
        let start = (end - 2.5).max(0.0).min(t);
        // Start from the nearest touch, if there is one.
        let (x, y) = timeline::timeline_touches(&self.project)
            .iter()
            .filter(|tc| (tc.time - t).abs() < 1.5)
            .min_by(|a, b| (a.time - t).abs().total_cmp(&(b.time - t).abs()))
            .map(|tc| (tc.x, tc.y))
            .unwrap_or((0.5, 0.5));
        let scale = self.project.style.auto_zoom_scale;
        match timeline::add_zoom(&mut self.project, start, end, scale, x, y) {
            Ok(id) => self.selection = Selection::Camera { id, key: None },
            Err(e) => self.toast(e),
        }
    }

    fn add_text_at(&mut self, t: f32) {
        let id = match timeline::add_text(&mut self.project, t, "") {
            Ok(id) => id,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        self.selection = Selection::Text(id);
        self.focus_title = true;
        if self.playing {
            self.toggle_play();
        }
        // Show the title once it is fully in.
        if let Some(c) = self.project.texts.iter().find(|c| c.id == id) {
            self.seek(c.start + 1.0);
        }
    }

    fn split_here(&mut self) {
        match timeline::split_at(&mut self.project, self.time) {
            Ok(id) => self.selection = Selection::Clip(id),
            Err(e) => self.toast(e),
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
        let (undo, redo) = ctx.input(|i| {
            let z = i.key_pressed(Key::Z) && i.modifiers.command;
            (z && !i.modifiers.shift, (z && i.modifiers.shift) || (i.key_pressed(Key::Y) && i.modifiers.command))
        });
        if undo {
            self.undo_redo(true);
        }
        if redo {
            self.undo_redo(false);
        }
        if self.project.is_empty() {
            return;
        }
        let (space, del, esc, z, split, left, right, home, end, shift, title) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Z) && !i.modifiers.command,
                i.key_pressed(Key::B) && i.modifiers.command,
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.modifiers.shift,
                i.key_pressed(Key::T) && !i.modifiers.command,
            )
        });
        if title {
            self.add_text_at(self.time);
        }
        if space {
            self.toggle_play();
        }
        if del {
            self.delete_selection();
        }
        if esc {
            if self.selection != Selection::None {
                self.selection = Selection::None;
            } else {
                self.director_mode = false;
            }
        }
        if z {
            self.add_zoom_at(self.time);
        }
        if split {
            self.split_here();
        }
        let step = if shift { 1.0 } else { 1.0 / PREVIEW_FPS };
        if left {
            self.seek(self.time - step);
        }
        if right {
            self.seek(self.time + step);
        }
        if home {
            self.seek(0.0);
        }
        if end {
            self.seek(timeline::duration(&self.project));
        }
    }
}

impl Drop for RecOrdingApp {
    fn drop(&mut self) {
        if let Some(rec) = self.recording.take() {
            let _ = rec.stop_blocking();
        }
        if self.project != self.last_saved {
            self.save_now();
        }
    }
}

impl eframe::App for RecOrdingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        self.check_disk();

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

        if let Some(rec) = &mut self.recording {
            if rec.started.elapsed() > Duration::from_secs(2) && rec.check_exited().is_some() {
                self.stop_recording();
            }
        }

        self.toolbar(ctx);
        if !self.project.is_empty() {
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
            Sheet::Projects => self.projects_sheet(ctx),
            Sheet::Assistant => self.assistant_sheet(ctx),
            Sheet::Update => self.update_sheet(ctx),
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
        // Keep polling for the assistant's edits.
        ctx.request_repaint_after(Duration::from_millis(800));

        self.paint_toast(ctx);
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            self.paint_drop_overlay(ctx);
        }
        self.commit_history(ctx);
        self.autosave();
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
                left.add_space(2.0);
                wordmark(&mut left, 14.0);
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

                if self.toolbar_button(&mut left, "Proyectos", |p, c, col| {
                    for dy in [-4.0, 0.0, 4.0] {
                        p.line_segment([c + Vec2::new(-6.0, dy), c + Vec2::new(6.0, dy)], Stroke::new(1.6_f32, col));
                    }
                })
                .on_hover_text("Abrir, crear o cambiar de proyecto")
                .clicked()
                {
                    self.projects = list_projects();
                    self.new_project_name.clear();
                    self.sheet = Sheet::Projects;
                }

                {
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
                let can_export = !self.project.is_empty() && !busy;
                if primary_button(&mut right, "Exportar", can_export).clicked() {
                    self.sheet = Sheet::Export;
                    if !matches!(self.export_ui, ExportUi::Running { .. }) {
                        self.export_ui = ExportUi::Idle;
                    }
                }
                right.add_space(6.0);
                if self
                    .toolbar_button(&mut right, "Asistente", |p, c, col| {
                        p.circle_stroke(c, 5.5, Stroke::new(1.5_f32, col));
                        p.circle_filled(c, 1.8, col);
                    })
                    .on_hover_text("Conectar un agente (Claude, Cursor, Codex…) por MCP")
                    .clicked()
                {
                    self.sheet = Sheet::Assistant;
                }
                if let Some(r) = &self.update {
                    let tip = format!("{APP_NAME} {} está disponible", r.version);
                    right.add_space(6.0);
                    if self
                        .toolbar_button(&mut right, "Actualización", |p, c, _| {
                            p.circle_filled(c, 6.5, ACCENT);
                            p.line_segment([c + Vec2::new(0.0, 3.2), c + Vec2::new(0.0, -3.2)], Stroke::new(1.5_f32, Color32::WHITE));
                            p.line_segment([c + Vec2::new(-2.6, -0.6), c + Vec2::new(0.0, -3.2)], Stroke::new(1.5_f32, Color32::WHITE));
                            p.line_segment([c + Vec2::new(2.6, -0.6), c + Vec2::new(0.0, -3.2)], Stroke::new(1.5_f32, Color32::WHITE));
                        })
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.sheet = Sheet::Update;
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
            c.label(with_name("rec·ording le pone un marco, un fondo y zooms suaves en cada toque.", regular(15.0), TEXT_2));
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
                    RichText::new(format!("Para usar todas las funciones instala:\n{}", crate::platform::install_hint(&missing)))
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
                // The detected screen is the default: it is listed first, tagged, and stands for
                // "automatic" (no manual choice saved).
                let recommended = src.guess.and_then(|g| src.parts.get(g).map(|p| (g, p.name.clone())));
                if cm.screen_part.is_some() && cm.screen_part == src.guess {
                    cm.screen_part = None;
                }
                let current = match (cm.screen_part.and_then(|i| src.parts.get(i)), &recommended) {
                    (Some(p), _) => part_label(&p.name, false),
                    // Tag first, so a long name is what gets cut short.
                    (None, Some((_, name))) => part_label_tag_first(name),
                    (None, None) => part_label("Elige una pieza", false),
                };
                egui::ComboBox::from_id_salt("model-screen")
                    .selected_text(current)
                    .width(ui.available_width() - 8.0)
                    // Long part names are cut short instead of widening the whole panel.
                    .truncate()
                    .show_ui(ui, |ui| {
                        if let Some((_, name)) = &recommended {
                            ui.selectable_value(&mut cm.screen_part, None, part_label(name, true));
                            ui.separator();
                        }
                        for (i, p) in src.parts.iter().enumerate() {
                            if recommended.as_ref().is_some_and(|(g, _)| *g == i) {
                                continue;
                            }
                            ui.selectable_value(&mut cm.screen_part, Some(i), part_label(&p.name, false));
                        }
                    });
            })
            .response
            .on_hover_text("La pieza del modelo donde se ve tu grabación");
            if cm.screen_part.is_some() && src.guess.is_some() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Elegida a mano.").color(YELLOW).font(regular(11.5)));
                    if ui.link(RichText::new("Volver a la recomendada").font(regular(11.5))).clicked() {
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
                        // Long animation names are cut short instead of widening the whole panel.
                        .truncate()
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

        section(ui, "Iluminación", |ui| {
            let l = &mut style.lighting;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for s in LightStyle::ALL {
                    if chip(ui, s.label(), l.style == s).clicked() {
                        l.style = s;
                    }
                }
            });
            ui.add_space(2.0);
            slider_row(ui, "Dirección", &mut l.angle, -180.0..=180.0, |v| format!("{v:.0}°"));
            slider_row(ui, "Intensidad", &mut l.intensity, 0.5..=1.5, |v| format!("{:.0} %", v * 100.0));
            slider_row(ui, "Reflejo", &mut l.glare, 0.0..=2.0, |v| format!("{:.0} %", v * 100.0));
            ui.label(RichText::new("El reflejo es el brillo que cruza el cristal de la pantalla al girar.").color(TEXT_3).font(regular(11.5)));
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

        section(ui, "Acabado", |ui| {
            let f = &mut style.finish;
            fn row<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, all: &[T], name: impl Fn(T) -> &'static str) {
                ui.horizontal(|ui| {
                    fixed_label(ui, 78.0, RichText::new(label).color(TEXT), Align::Min);
                    let opts: Vec<(T, &str)> = all.iter().map(|v| (*v, name(*v))).collect();
                    segmented(ui, value, &opts, ui.available_width());
                });
            }
            row(ui, "Desenfoque", &mut f.motion_blur, &MotionBlur::ALL, MotionBlur::label);
            ui.label(RichText::new("En giros, zooms y movimientos rápidos, como una cámara de cine.").color(TEXT_3).font(regular(11.5)));
            row(ui, "Bordes", &mut f.antialias, &Antialias::ALL, Antialias::label);
            ui.label(RichText::new("Más alta: bordes del modelo 3D más limpios y zooms sin saltos de nitidez; exporta algo más despacio.").color(TEXT_3).font(regular(11.5)));
            slider_row(ui, "Viñeta", &mut f.vignette, 0.0..=1.0, |v| format!("{:.0} %", v * 100.0));
            slider_row(ui, "Grano", &mut f.grain, 0.0..=1.0, |v| format!("{:.0} %", v * 100.0));
            let secs = |v: f32| if v <= 0.0 { "No".to_string() } else { format!("{v:.1} s").replace('.', ",") };
            slider_row(ui, "Entrada", &mut f.fade_in, 0.0..=3.0, secs);
            slider_row(ui, "Salida", &mut f.fade_out, 0.0..=3.0, secs);
            slider_row(ui, "Entre clips", &mut f.crossfade, 0.0..=1.5, secs);
            ui.label(RichText::new("Fundidos desde y a negro (con el sonido) y entre clips.").color(TEXT_3).font(regular(11.5)));
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

        let n_touches = timeline::timeline_touches(&self.project).len();
        let has_touches = n_touches > 0;
        let mut regenerate = false;
        section(ui, "Zoom automático", |ui| {
            slider_row(ui, "Intensidad", &mut self.project.style.auto_zoom_scale, 1.2..=3.0, |v| {
                format!("{v:.1}×").replace('.', ",")
            });
            if has_touches {
                ui.label(
                    RichText::new(format!("{n_touches} toques registrados en las grabaciones."))
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
            let scale = self.project.style.auto_zoom_scale;
            let n = timeline::regenerate_auto_zooms(&mut self.project, scale);
            self.selection = Selection::None;
            self.toast(format!("{n} zooms automáticos"));
        }
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
                let hint = crate::platform::install_hint(&["android-tools", "scrcpy"]);
                info_box(ui, YELLOW, &format!("Para grabar necesitas adb y scrcpy. Instálalos con:\n{hint}"));
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
                        toggle_row(ui, "Barra de estado limpia", "Hora 9:41, batería llena y sin notificaciones mientras grabas", &mut self.rec_opts.clean_status_bar);
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
        let has_audio = self.project.media.iter().any(|m| m.info.has_audio);
        let dur = timeline::duration(&self.project);
        Self::sheet_window(ctx, "export", 460.0, |ui| match &self.export_ui {
            ExportUi::Running { fraction, eta, encoder, .. } => {
                ui.label(RichText::new("Exportando…").font(medium(20.0)).color(TEXT));
                if let Some(how) = encoder {
                    ui.label(RichText::new(how.as_str()).color(TEXT_3).font(regular(12.0)));
                }
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
                if has_audio && s.format != ExportFormat::Gif {
                    form_row(ui, "Sonido", |ui| {
                        toggle(ui, &mut s.include_audio);
                    });
                }
                if s.format != ExportFormat::Gif {
                    form_row(ui, "Gráfica", |ui| match (crate::gpu::detected(), crate::hwenc::detected()) {
                        (None, _) | (_, None) => {
                            ui.add(egui::Spinner::new().size(14.0).color(TEXT_3));
                            ui.label(RichText::new("Buscando la tarjeta gráfica…").color(TEXT_3).font(regular(12.0)));
                        }
                        (Some(None), Some(None)) => {
                            ui.label(RichText::new("No hay ninguna compatible; se usa el procesador").color(TEXT_3).font(regular(12.0)));
                        }
                        (Some(card), Some(enc)) => {
                            toggle(ui, &mut s.hardware);
                            let codec = if s.format == ExportFormat::Hevc { crate::hwenc::Codec::Hevc } else { crate::hwenc::Codec::H264 };
                            let encodes = enc.as_ref().filter(|e| e.supports(codec));
                            let note = match (s.hardware, card, encodes) {
                                (false, ..) => "Desactivada: lo hace todo el procesador".to_string(),
                                (true, Some(c), Some(_)) => format!("{c} · dibuja y codifica"),
                                (true, Some(c), None) => format!("{c} · dibuja; codifica el procesador"),
                                (true, None, Some(e)) => format!("{} · codifica; dibuja el procesador", e.name),
                                (true, None, None) => "Se usa el procesador".to_string(),
                            };
                            ui.label(RichText::new(note).color(TEXT_3).font(regular(12.0)));
                        }
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

/// A model part's name, with a "RECOMENDADA" tag for the detected screen.
fn part_label_tag_first(name: &str) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.append(" RECOMENDADA ", 0.0, TextFormat { font_id: medium(9.5), color: Color32::WHITE, background: ACCENT, valign: Align::Center, ..Default::default() });
    job.append(name, 8.0, TextFormat { font_id: regular(13.0), color: TEXT, ..Default::default() });
    job
}

fn part_label(name: &str, recommended: bool) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.append(name, 0.0, TextFormat { font_id: regular(13.0), color: TEXT, ..Default::default() });
    if recommended {
        job.append(
            " RECOMENDADA ",
            8.0,
            TextFormat { font_id: medium(9.5), color: Color32::WHITE, background: ACCENT, valign: Align::Center, ..Default::default() },
        );
    }
    job
}
