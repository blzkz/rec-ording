use crate::model::*;
use crate::renderer3d::{
    evaluate_camera_pose, inspect_glb_file, parse_hex_rgb, render_frame_multicore, GlbModelMetadata,
};
use crate::workers::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{Color32, ColorImage, FontId, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use std::time::Instant;

#[derive(PartialEq, Eq, Clone, Copy)]
enum InspectorTab {
    Studio3D,
    DirectorPrivacy,
    Subtitles,
    McpAi,
    ExportFfmpeg,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum TrackFilter {
    All,
    Video,
    Audio,
    Text,
}

pub struct RecOrdingApp {
    project: ProjectFile,
    current_time_sec: f32,
    is_playing: bool,
    last_tick: Instant,
    selected_clip_id: Option<String>,
    inspector_tab: InspectorTab,
    track_filter: TrackFilter,
    timeline_zoom: f32,
    orbit_override: (f32, f32, f32),
    show_safe_zones: bool,

    // Vulkan/wgpu backed viewport texture + reusable RGBA buffer
    viewport_tex: Option<TextureHandle>,
    rgba_buf: Vec<u8>,
    render_res: (usize, usize),
    glb_meta: Option<GlbModelMetadata>,

    // Multithreaded worker channel
    worker_tx: Sender<WorkerEvent>,
    worker_rx: Receiver<WorkerEvent>,

    // Recording modal & state
    show_recording_window: bool,
    adb_devices: Vec<AdbDeviceInfo>,
    selected_adb_serial: String,
    rec_capture_desktop: bool,
    rec_capture_android: bool,
    rec_capture_mic: bool,
    rec_auto_zoom_on_stop: bool,
    active_recording: Option<ActiveRecordingHandle>,

    // Export state
    is_exporting: bool,
    export_progress: f32,
    export_status_msg: String,
    export_log: String,

    // Auto-Update modal & state
    show_update_window: bool,
    update_info: Option<AppUpdateInfo>,
    update_log: String,
    auto_check_updates: bool,

    // Status banner
    toast_message: Option<(String, Instant)>,
}

impl RecOrdingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(11, 15, 25);
        visuals.window_fill = Color32::from_rgb(15, 23, 42);
        visuals.extreme_bg_color = Color32::from_rgb(6, 9, 17);
        visuals.selection.bg_fill = Color32::from_rgb(79, 70, 229);
        cc.egui_ctx.set_visuals(visuals);

        let (worker_tx, worker_rx) = unbounded();
        let project = load_or_create_project();

        // Trigger initial background checks on worker threads
        spawn_adb_scan_worker(worker_tx.clone());
        spawn_update_check_worker(worker_tx.clone());

        let render_res = (640, 360);
        let rgba_buf = vec![0u8; render_res.0 * render_res.1 * 4];

        Self {
            project,
            current_time_sec: 1.8,
            is_playing: false,
            last_tick: Instant::now(),
            selected_clip_id: Some("clip-demo-android".into()),
            inspector_tab: InspectorTab::Studio3D,
            track_filter: TrackFilter::All,
            timeline_zoom: 1.0,
            orbit_override: (0.0, 0.0, 1.0),
            show_safe_zones: false,
            viewport_tex: None,
            rgba_buf,
            render_res,
            glb_meta: None,
            worker_tx,
            worker_rx,
            show_recording_window: false,
            adb_devices: Vec::new(),
            selected_adb_serial: "usb-pixel11pro-sim".into(),
            rec_capture_desktop: false,
            rec_capture_android: true,
            rec_capture_mic: true,
            rec_auto_zoom_on_stop: true,
            active_recording: None,
            is_exporting: false,
            export_progress: 0.0,
            export_status_msg: String::new(),
            export_log: String::new(),
            show_update_window: false,
            update_info: None,
            update_log: String::new(),
            auto_check_updates: true,
            toast_message: None,
        }
    }

    fn show_toast(&mut self, msg: impl Into<String>) {
        self.toast_message = Some((msg.into(), Instant::now()));
    }

    fn poll_workers(&mut self) {
        while let Ok(ev) = self.worker_rx.try_recv() {
            match ev {
                WorkerEvent::AdbDevicesListed(devs) => {
                    if let Some(first) = devs.first() {
                        if self.selected_adb_serial.is_empty() {
                            self.selected_adb_serial = first.serial.clone();
                        }
                    }
                    self.adb_devices = devs;
                }
                WorkerEvent::ExportProgress {
                    current_frame,
                    total_frames,
                    fps_speed: _,
                    message,
                } => {
                    self.export_progress =
                        (current_frame as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                    self.export_status_msg = message;
                }
                WorkerEvent::ExportFinished {
                    success,
                    output_path,
                    command_used: _,
                    log,
                } => {
                    self.is_exporting = false;
                    self.export_progress = 1.0;
                    self.export_log = log;
                    if success {
                        self.show_toast(format!("Exportación completada: {}", output_path));
                    } else {
                        self.show_toast("Error en exportación FFmpeg — revisa el registro");
                    }
                }
                WorkerEvent::SubtitlesTranscribed(segments) => {
                    self.project.subtitles = segments;
                    let _ = save_project(&self.project);
                    self.show_toast("Subtítulos transcritos en hilo secundario de Rust");
                }
                WorkerEvent::UpdateChecked(info) => {
                    if info.update_available {
                        self.show_toast(format!(
                            "Nueva versión v{} disponible en {}",
                            info.latest_version, info.repo
                        ));
                    }
                    self.update_info = Some(info);
                }
                WorkerEvent::UpdateInstalled { success, log } => {
                    self.update_log = log;
                    if success {
                        self.show_toast("Paquete actualizado con éxito en CachyOS / Arch Linux");
                    } else {
                        self.show_toast("Paquete descargado — revisa ventana de actualización");
                    }
                }
                WorkerEvent::RecordingStopped {
                    desktop_video,
                    android_video,
                    pointer_events,
                    key_events,
                } => {
                    if let Some(and_path) = android_video {
                        if let Some(t1) = self.project.tracks.iter_mut().find(|t| t.id == "video-track-1") {
                            t1.clips.push(TimelineClip {
                                id: format!("clip-adb-{}", t1.clips.len() + 1),
                                track_id: t1.id.clone(),
                                name: "Captura_ADB_USB.mp4".into(),
                                clip_type: "video".into(),
                                media_path: Some(and_path),
                                source_role: Some("android-main".into()),
                                start_sec: 0.0,
                                duration_sec: self.project.duration_sec,
                                trim_in_sec: 0.0,
                                trim_out_sec: self.project.duration_sec,
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
                    }
                    if let Some(pc_path) = desktop_video {
                        if let Some(t2) = self.project.tracks.iter_mut().find(|t| t.id == "video-track-2") {
                            t2.clips.push(TimelineClip {
                                id: format!("clip-pc-{}", t2.clips.len() + 1),
                                track_id: t2.id.clone(),
                                name: "Captura_Pantalla_PC.mp4".into(),
                                clip_type: "video".into(),
                                media_path: Some(pc_path),
                                source_role: Some("pc-screen".into()),
                                start_sec: 0.0,
                                duration_sec: self.project.duration_sec,
                                trim_in_sec: 0.0,
                                trim_out_sec: self.project.duration_sec,
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
                    }
                    if !pointer_events.is_empty() {
                        self.project.pointer_events.extend(pointer_events);
                    }
                    if !key_events.is_empty() {
                        self.project.key_events.extend(key_events);
                    }
                    if self.rec_auto_zoom_on_stop {
                        self.generate_auto_zoom_from_touches();
                    }
                    let _ = save_project(&self.project);
                    self.show_toast("Grabación finalizada e importada en la línea de tiempo");
                }
            }
        }
    }

    fn generate_auto_zoom_from_touches(&mut self) {
        let mut kfs = vec![DirectorCameraKeyframe {
            id: "dir-auto-intro".into(),
            time_sec: 0.0,
            duration_sec: 1.5,
            focus_x: 0.5,
            focus_y: 0.5,
            zoom: 1.02,
            rot_x_deg: 6.0,
            rot_y_deg: -14.0,
            rot_z_deg: 1.0,
            easing: "spring".into(),
            label: Some("Intro 3D".into()),
            auto_generated: Some(true),
        }];

        for (idx, tap) in self.project.pointer_events.iter().enumerate() {
            let t = (tap.timestamp_ms as f32 / 1000.0 - 0.25).max(0.2);
            kfs.push(DirectorCameraKeyframe {
                id: format!("dir-auto-{}", idx + 1),
                time_sec: t,
                duration_sec: 1.6,
                focus_x: tap.x,
                focus_y: tap.y,
                zoom: if tap.event_type == "touch_swipe" { 1.32 } else { 1.55 },
                rot_x_deg: (0.5 - tap.y) * 16.0,
                rot_y_deg: (tap.x - 0.5) * 24.0,
                rot_z_deg: if idx % 2 == 0 { 1.5 } else { -1.5 },
                easing: "spring".into(),
                label: Some(format!("Auto-Zoom #{} ({})", idx + 1, tap.event_type)),
                auto_generated: Some(true),
            });
        }

        kfs.push(DirectorCameraKeyframe {
            id: "dir-auto-outro".into(),
            time_sec: (self.project.duration_sec - 2.0).max(1.0),
            duration_sec: 2.0,
            focus_x: 0.5,
            focus_y: 0.5,
            zoom: 1.0,
            rot_x_deg: 3.0,
            rot_y_deg: 0.0,
            rot_z_deg: 0.0,
            easing: "spring".into(),
            label: Some("Plano Cierre".into()),
            auto_generated: Some(true),
        });

        self.project.director_keyframes = kfs;
        self.show_toast("Modo Director: Auto-Zoom 3D generado desde los toques registrados");
    }

    fn apply_export_preset(&mut self, preset: ExportPresetId) {
        self.project.export_settings.preset_id = preset.clone();
        match preset {
            ExportPresetId::PlayStore16x9_4k => {
                self.project.export_settings.width = 3840;
                self.project.export_settings.height = 2160;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::H264;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::PlayStore16x9_1080p => {
                self.project.export_settings.width = 1920;
                self.project.export_settings.height = 1080;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::H264;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::PlayStoreShorts9x16 => {
                self.project.export_settings.width = 1080;
                self.project.export_settings.height = 1920;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::H264;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::PlayStoreTablet16x10 => {
                self.project.export_settings.width = 2560;
                self.project.export_settings.height = 1600;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::Hevc;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::AppStoreIphone6_9 => {
                self.project.export_settings.width = 886;
                self.project.export_settings.height = 1920;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::H264;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::AppStoreIpadPro => {
                self.project.export_settings.width = 1600;
                self.project.export_settings.height = 1200;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::H264;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::TransparentWebm => {
                self.project.export_settings.width = 1920;
                self.project.export_settings.height = 1080;
                self.project.export_settings.fps = 60;
                self.project.export_settings.codec = VideoCodec::Vp9Alpha;
                self.project.export_settings.transparent_background = true;
            }
            ExportPresetId::GifPreview => {
                self.project.export_settings.width = 960;
                self.project.export_settings.height = 540;
                self.project.export_settings.fps = 30;
                self.project.export_settings.codec = VideoCodec::Gif;
                self.project.export_settings.transparent_background = false;
            }
            ExportPresetId::Custom => {}
        }
    }
}

impl eframe::App for RecOrdingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_workers();

        // Advance timeline playback
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;

        if self.is_playing {
            self.current_time_sec += dt;
            if self.current_time_sec >= self.project.duration_sec {
                self.current_time_sec = 0.0;
            }
            ctx.request_repaint();
        } else if self.is_exporting || self.active_recording.is_some() {
            ctx.request_repaint();
        }

        // Keyboard shortcut Space = Play/Pause
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.is_playing = !self.is_playing;
        }

        // 1. TOP STUDIO HEADER
        egui::TopBottomPanel::top("top_studio_header")
            .exact_height(48.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.heading(
                        egui::RichText::new("🎬 rec-ording")
                            .color(Color32::from_rgb(165, 180, 252))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "100% Rust · Vulkan wgpu · {} Hilos Rayon",
                            rayon::current_num_threads()
                        ))
                        .size(11.0)
                        .color(Color32::from_rgb(52, 211, 153)),
                    );

                    ui.separator();
                    ui.add(
                        egui::TextEdit::singleline(&mut self.project.name)
                            .desired_width(190.0)
                            .hint_text("Nombre del proyecto"),
                    );

                    ui.separator();

                    let rec_btn_text = if self.active_recording.is_some() {
                        "⏹ Grabando... (Abrir)"
                    } else {
                        "● Grabar USB / PC"
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(rec_btn_text).color(Color32::WHITE).strong(),
                            )
                            .fill(Color32::from_rgb(225, 29, 72)),
                        )
                        .clicked()
                    {
                        self.show_recording_window = true;
                        spawn_adb_scan_worker(self.worker_tx.clone());
                    }

                    ui.separator();

                    // Quick 3D Mockup Selector Buttons
                    for (model, short_name) in [
                        (DeviceModelType::Pixel11Pro, "📱 Pixel 11 Pro"),
                        (DeviceModelType::GalaxyS26Ultra, "📱 Galaxy S26 Ultra"),
                        (DeviceModelType::GalaxyTabS10Ultra, "📟 Galaxy Tab Ultra"),
                    ] {
                        let active = self.project.device_3d.primary_model == model;
                        let btn = egui::Button::new(
                            egui::RichText::new(short_name).size(12.0).color(if active {
                                Color32::WHITE
                            } else {
                                Color32::from_rgb(203, 213, 225)
                            }),
                        )
                        .fill(if active {
                            Color32::from_rgb(79, 70, 229)
                        } else {
                            Color32::from_rgb(30, 41, 59)
                        });
                        if ui.add(btn).clicked() {
                            if model == DeviceModelType::GalaxyTabS10Ultra {
                                self.project.device_3d.orientation = Orientation::Landscape;
                            }
                            self.project.device_3d.primary_model = model;
                        }
                    }

                    let dual_active = self.project.device_3d.dual_layout != DualLayout::Single;
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("💻+📱 Dual PC+Android")
                                    .size(12.0)
                                    .color(Color32::WHITE),
                            )
                            .fill(if dual_active {
                                Color32::from_rgb(5, 150, 105)
                            } else {
                                Color32::from_rgb(30, 41, 59)
                            }),
                        )
                        .clicked()
                    {
                        self.project.device_3d.dual_layout = if dual_active {
                            DualLayout::Single
                        } else {
                            DualLayout::SideBySide
                        };
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("✦ Auto-Zoom")
                                    .size(12.0)
                                    .color(Color32::from_rgb(253, 224, 71)),
                            )
                            .fill(Color32::from_rgb(51, 39, 15)),
                        )
                        .on_hover_text("Generar cámara Director desde toques")
                        .clicked()
                    {
                        self.generate_auto_zoom_from_touches();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let exp_text = if self.is_exporting {
                            format!("⏳ Exportando {:.0}%", self.export_progress * 100.0)
                        } else {
                            format!(
                                "⬇ Exportar {}x{} @ {}fps",
                                self.project.export_settings.width,
                                self.project.export_settings.height,
                                self.project.export_settings.fps
                            )
                        };
                        if ui
                            .add_enabled(
                                !self.is_exporting,
                                egui::Button::new(
                                    egui::RichText::new(exp_text).color(Color32::WHITE).strong(),
                                )
                                .fill(Color32::from_rgb(16, 185, 129)),
                            )
                            .clicked()
                        {
                            self.is_exporting = true;
                            self.export_progress = 0.0;
                            self.inspector_tab = InspectorTab::ExportFfmpeg;
                            spawn_ffmpeg_export_worker(
                                self.project.clone(),
                                self.worker_tx.clone(),
                            );
                        }

                        if ui.button("💾 Guardar").clicked() {
                            if let Ok(p) = save_project(&self.project) {
                                self.show_toast(format!("Guardado en {}", p.display()));
                            }
                        }

                        if ui
                            .button("↻ Sync MCP")
                            .on_hover_text("Recargar cambios del servidor MCP en disco")
                            .clicked()
                        {
                            self.project = load_or_create_project();
                            self.show_toast("Proyecto recargado desde disco (MCP)");
                        }

                        let upd_label = if let Some(ref info) = self.update_info {
                            if info.update_available {
                                format!("⬆ Actualizar v{}", info.latest_version)
                            } else {
                                format!("✔ v{}", info.current_version)
                            }
                        } else {
                            format!("v{}", env!("CARGO_PKG_VERSION"))
                        };
                        if ui.button(upd_label).clicked() {
                            self.show_update_window = true;
                            spawn_update_check_worker(self.worker_tx.clone());
                        }
                    });
                });
            });

        // 2. BOTTOM MULTI-TRACK TIMELINE (4 Video, 4 Audio, 5 Text + Director + Privacy)
        egui::TopBottomPanel::bottom("bottom_multitrack_timeline")
            .resizable(true)
            .default_height(265.0)
            .min_height(190.0)
            .show(ctx, |ui| {
                self.render_timeline_panel(ui);
            });

        // 3. RIGHT INSPECTOR PANEL (5 TABS)
        egui::SidePanel::right("right_inspector_panel")
            .resizable(true)
            .default_width(390.0)
            .min_width(340.0)
            .show(ctx, |ui| {
                self.render_inspector_panel(ui, ctx);
            });

        // 4. CENTER 3D VIEWPORT & COMPOSITOR STAGE
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_center_viewport(ui, ctx);
        });

        // 5. FLOATING MODAL WINDOWS
        if self.show_recording_window {
            self.render_recording_modal(ctx);
        }
        if self.show_update_window {
            self.render_update_modal(ctx);
        }
    }
}

impl RecOrdingApp {
    fn render_center_viewport(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Top mini HUD bar inside viewport
        ui.horizontal(|ui| {
            let pose = evaluate_camera_pose(&self.project, self.current_time_sec, self.orbit_override);
            ui.label(
                egui::RichText::new(format!(
                    "🎥 Cámara Director: Zoom {:.2}x · Rot ({:.1}°, {:.1}°, {:.1}°)",
                    pose.zoom, pose.rot_x_deg, pose.rot_y_deg, pose.rot_z_deg
                ))
                .size(11.5)
                .color(Color32::from_rgb(148, 163, 184)),
            );

            if let Some((ref msg, inst)) = self.toast_message {
                if inst.elapsed().as_secs_f32() < 4.0 {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("✨ {}", msg))
                            .size(12.0)
                            .color(Color32::from_rgb(129, 140, 248))
                            .strong(),
                    );
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Resetear Órbita 3D").clicked() {
                    self.orbit_override = (0.0, 0.0, 1.0);
                }
                ui.checkbox(&mut self.show_safe_zones, "Guías Play Store");
            });
        });

        ui.separator();

        // Render 3D scene via Rayon multicore into RGBA buffer and upload to Vulkan wgpu texture
        let avail = ui.available_size();
        let target_aspect = (self.project.export_settings.width.max(1) as f32)
            / (self.project.export_settings.height.max(1) as f32);

        let mut draw_w = avail.x - 16.0;
        let mut draw_h = draw_w / target_aspect;
        if draw_h > avail.y - 16.0 {
            draw_h = (avail.y - 16.0).max(120.0);
            draw_w = draw_h * target_aspect;
        }

        let rw = 640usize;
        let rh = ((rw as f32 / target_aspect) as usize).clamp(240, 640);
        if self.render_res != (rw, rh) {
            self.render_res = (rw, rh);
            self.rgba_buf.resize(rw * rh * 4, 0);
        }

        render_frame_multicore(
            &self.project,
            self.current_time_sec,
            rw,
            rh,
            self.orbit_override,
            &mut self.rgba_buf,
        );

        let color_image = ColorImage::from_rgba_unmultiplied([rw, rh], &self.rgba_buf);
        if let Some(ref mut tex) = self.viewport_tex {
            tex.set(color_image, TextureOptions::LINEAR);
        } else {
            self.viewport_tex =
                Some(ctx.load_texture("viewport_3d_vulkan", color_image, TextureOptions::LINEAR));
        }

        let outer_rect = ui.available_rect_before_wrap();
        let canvas_rect = Rect::from_center_size(outer_rect.center(), Vec2::new(draw_w, draw_h));

        let response = ui.allocate_rect(canvas_rect, Sense::click_and_drag());
        if response.dragged() {
            let delta = response.drag_delta();
            self.orbit_override.1 += delta.x * 0.35;
            self.orbit_override.0 = (self.orbit_override.0 - delta.y * 0.35).clamp(-65.0, 65.0);
        }

        if let Some(ref tex) = self.viewport_tex {
            ui.painter().image(
                tex.id(),
                canvas_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Vector Overlays on Canvas: Animated Titles (T1..T5), Keystrokes HUD, and Karaoke Subtitles
        let painter = ui.painter_at(canvas_rect);

        // Border frame
        painter.rect_stroke(
            canvas_rect,
            Rounding::same(8.0),
            Stroke::new(1.5_f32, Color32::from_rgb(51, 65, 85)),
        );

        // Safe zones
        if self.show_safe_zones {
            let safe_rect = canvas_rect.shrink2(Vec2::new(draw_w * 0.08, draw_h * 0.08));
            painter.rect_stroke(
                safe_rect,
                Rounding::same(4.0),
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(56, 189, 248, 120)),
            );
        }

        // Animated Text Clips (T1..T5)
        for track in &self.project.tracks {
            if track.kind != "text" || track.muted {
                continue;
            }
            for clip in &track.clips {
                if self.current_time_sec >= clip.start_sec
                    && self.current_time_sec <= clip.start_sec + clip.duration_sec
                {
                    if let Some(ref title) = clip.text {
                        let px = canvas_rect.left() + clip.pos_x.unwrap_or(0.5) * draw_w;
                        let py = canvas_rect.top() + clip.pos_y.unwrap_or(0.12) * draw_h;
                        let box_rect =
                            Rect::from_center_size(Pos2::new(px, py), Vec2::new(draw_w * 0.62, 54.0));
                        painter.rect_filled(
                            box_rect,
                            Rounding::same(10.0),
                            Color32::from_rgba_unmultiplied(15, 23, 42, 215),
                        );
                        painter.rect_stroke(
                            box_rect,
                            Rounding::same(10.0),
                            Stroke::new(1.2_f32, Color32::from_rgb(99, 102, 241)),
                        );
                        painter.text(
                            Pos2::new(px, py - 8.0),
                            egui::Align2::CENTER_CENTER,
                            title,
                            FontId::proportional(18.0),
                            Color32::WHITE,
                        );
                        if let Some(ref sub) = clip.subtitle_text {
                            if !sub.is_empty() {
                                painter.text(
                                    Pos2::new(px, py + 12.0),
                                    egui::Align2::CENTER_CENTER,
                                    sub,
                                    FontId::proportional(12.0),
                                    Color32::from_rgb(165, 180, 252),
                                );
                            }
                        }
                    }
                }
            }
        }

        // On-Screen Keystrokes HUD
        if self.project.cursor_style.show_keystrokes_hud {
            let time_ms = (self.current_time_sec * 1000.0) as u64;
            for kev in &self.project.key_events {
                if time_ms >= kev.timestamp_ms && time_ms <= kev.timestamp_ms + kev.duration_ms {
                    let hud_rect = Rect::from_min_size(
                        Pos2::new(canvas_rect.right() - 230.0, canvas_rect.bottom() - 88.0),
                        Vec2::new(214.0, 32.0),
                    );
                    painter.rect_filled(
                        hud_rect,
                        Rounding::same(8.0),
                        Color32::from_rgba_unmultiplied(15, 23, 42, 230),
                    );
                    painter.rect_stroke(
                        hud_rect,
                        Rounding::same(8.0),
                        Stroke::new(1.0_f32, Color32::from_rgb(56, 189, 248)),
                    );
                    painter.text(
                        hud_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("⌨ {}", kev.key_combo),
                        FontId::monospace(12.0),
                        Color32::from_rgb(56, 189, 248),
                    );
                }
            }
        }

        // Subtitles (Word-by-Word Highlight)
        if self.project.subtitle_style.enabled {
            for seg in &self.project.subtitles {
                if self.current_time_sec >= seg.start_sec && self.current_time_sec <= seg.end_sec {
                    let sy = canvas_rect.top() + self.project.subtitle_style.position_y * draw_h;
                    let sub_rect = Rect::from_center_size(
                        Pos2::new(canvas_rect.center().x, sy),
                        Vec2::new(draw_w * 0.72, 38.0),
                    );
                    painter.rect_filled(
                        sub_rect,
                        Rounding::same(8.0),
                        Color32::from_rgba_unmultiplied(9, 13, 24, 220),
                    );

                    let active_rgb =
                        parse_hex_rgb(&self.project.subtitle_style.active_word_color, [56, 189, 248]);
                    let mut line_str = String::new();
                    for w in &seg.words {
                        let is_now =
                            self.current_time_sec >= w.start_sec && self.current_time_sec <= w.end_sec;
                        if is_now {
                            line_str.push_str(&format!("[{}] ", w.word));
                        } else {
                            line_str.push_str(&format!("{} ", w.word));
                        }
                    }

                    painter.text(
                        sub_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        line_str.trim(),
                        FontId::proportional(15.5),
                        Color32::from_rgb(active_rgb[0], active_rgb[1], active_rgb[2]),
                    );
                }
            }
        }
    }

    fn render_inspector_panel(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.horizontal_wrapped(|ui| {
            for (tab, title) in [
                (InspectorTab::Studio3D, "📱 3D & Estilo"),
                (InspectorTab::DirectorPrivacy, "🎥 Director"),
                (InspectorTab::Subtitles, "💬 Subtítulos"),
                (InspectorTab::McpAi, "🤖 MCP / IA"),
                (InspectorTab::ExportFfmpeg, "🚀 Exportar"),
            ] {
                let sel = self.inspector_tab == tab;
                if ui.selectable_label(sel, title).clicked() {
                    self.inspector_tab = tab;
                }
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| match self.inspector_tab {
            InspectorTab::Studio3D => {
                ui.heading("Maqueta 3D y Escenario");
                ui.add_space(4.0);

                egui::ComboBox::from_label("Dispositivo 3D")
                    .selected_text(self.project.device_3d.primary_model.label())
                    .show_ui(ui, |ui| {
                        for m in [
                            DeviceModelType::Pixel11Pro,
                            DeviceModelType::GalaxyS26Ultra,
                            DeviceModelType::GalaxyTabS10Ultra,
                            DeviceModelType::DesktopWindow,
                            DeviceModelType::CustomGlb,
                            DeviceModelType::None,
                        ] {
                            let label = m.label();
                            ui.selectable_value(&mut self.project.device_3d.primary_model, m, label);
                        }
                    });

                egui::ComboBox::from_label("Disposición Dual")
                    .selected_text(self.project.device_3d.dual_layout.label())
                    .show_ui(ui, |ui| {
                        for l in [
                            DualLayout::Single,
                            DualLayout::SideBySide,
                            DualLayout::PipFloating,
                            DualLayout::AngledShowcase,
                        ] {
                            let label = l.label();
                            ui.selectable_value(&mut self.project.device_3d.dual_layout, l, label);
                        }
                    });

                ui.horizontal(|ui| {
                    ui.label("Orientación:");
                    ui.selectable_value(
                        &mut self.project.device_3d.orientation,
                        Orientation::Portrait,
                        "Vertical",
                    );
                    ui.selectable_value(
                        &mut self.project.device_3d.orientation,
                        Orientation::Landscape,
                        "Horizontal",
                    );
                });

                if self.project.device_3d.primary_model == DeviceModelType::CustomGlb {
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Cargador Nativo .GLB / .GLTF (crate gltf)").strong());
                        let mut glb_path = self
                            .project
                            .device_3d
                            .custom_glb_path
                            .clone()
                            .unwrap_or_default();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut glb_path)
                                    .hint_text("/ruta/a/modelo_telefono.glb"),
                            )
                            .changed()
                        {
                            self.glb_meta = inspect_glb_file(&glb_path);
                            self.project.device_3d.custom_glb_path = Some(glb_path);
                        }
                        if let Some(ref meta) = self.glb_meta {
                            ui.label(
                                egui::RichText::new(format!(
                                    "✔ GLB cargado: {} mallas · {} vértices ({})",
                                    meta.mesh_count, meta.vertex_count, meta.primary_mesh_name
                                ))
                                .color(Color32::from_rgb(52, 211, 153))
                                .size(11.5),
                            );
                        }
                    });
                }

                ui.add(
                    egui::Slider::new(&mut self.project.device_3d.bezel_width, 0.01..=0.09)
                        .text("Grosor de Marco"),
                );
                ui.checkbox(
                    &mut self.project.device_3d.floating_animation,
                    "Animación 3D Flotante Suave",
                );
                ui.checkbox(&mut self.project.device_3d.show_shadow, "Sombra 3D Proyectada");
                ui.add(
                    egui::Slider::new(&mut self.project.device_3d.shadow_intensity, 0.0..=1.0)
                        .text("Intensidad de Sombra"),
                );

                ui.separator();
                ui.heading("Acabado Visual & Cursor");
                ui.checkbox(
                    &mut self.project.background.motion_blur_enabled,
                    "Desenfoque de Movimiento (Motion Blur)",
                );
                ui.checkbox(
                    &mut self.project.cursor_style.smooth_cursor,
                    "Cursor Suavizado (Spring)",
                );
                ui.checkbox(
                    &mut self.project.cursor_style.show_click_ripples,
                    "Ondas Táctiles en Clics y Toques",
                );
                ui.checkbox(
                    &mut self.project.cursor_style.show_touch_trails,
                    "Estelas de Gestos Swipe Android",
                );
                ui.checkbox(
                    &mut self.project.cursor_style.show_keystrokes_hud,
                    "Mostrar Teclas Pulsadas en Pantalla (HUD)",
                );

                ui.separator();
                ui.heading("Fondo y Colores Hex");
                ui.horizontal(|ui| {
                    ui.label("Color A:");
                    ui.text_edit_singleline(&mut self.project.background.color_a);
                });
                ui.horizontal(|ui| {
                    ui.label("Color B:");
                    ui.text_edit_singleline(&mut self.project.background.color_b);
                });
                ui.horizontal(|ui| {
                    ui.label("Chasis 3D:");
                    ui.text_edit_singleline(&mut self.project.device_3d.body_color);
                });
            }

            InspectorTab::DirectorPrivacy => {
                ui.heading("Planificador de Cámara (Modo Director)");
                ui.horizontal(|ui| {
                    if ui.button("✦ Auto-Zoom desde Toques").clicked() {
                        self.generate_auto_zoom_from_touches();
                    }
                    if ui.button("+ Añadir en Cabezal").clicked() {
                        self.project.director_keyframes.push(DirectorCameraKeyframe {
                            id: format!("dir-kf-{}", self.project.director_keyframes.len() + 1),
                            time_sec: self.current_time_sec,
                            duration_sec: 2.0,
                            focus_x: 0.5,
                            focus_y: 0.5,
                            zoom: 1.45,
                            rot_x_deg: 6.0,
                            rot_y_deg: -12.0,
                            rot_z_deg: 0.0,
                            easing: "spring".into(),
                            label: Some(format!("Cámara @ {:.1}s", self.current_time_sec)),
                            auto_generated: Some(false),
                        });
                    }
                });

                ui.add_space(6.0);
                let mut remove_kf_idx = None;
                for (i, kf) in self.project.director_keyframes.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(kf.label.as_deref().unwrap_or("Keyframe Cámara"));
                            if ui.small_button("Ir").clicked() {
                                self.current_time_sec = kf.time_sec;
                            }
                            if ui.small_button("🗑").clicked() {
                                remove_kf_idx = Some(i);
                            }
                        });
                        ui.add(
                            egui::Slider::new(&mut kf.time_sec, 0.0..=self.project.duration_sec)
                                .text("Tiempo (s)"),
                        );
                        ui.add(egui::Slider::new(&mut kf.zoom, 0.7..=2.5).text("Zoom"));
                        ui.add(egui::Slider::new(&mut kf.rot_x_deg, -35.0..=35.0).text("Rot X°"));
                        ui.add(egui::Slider::new(&mut kf.rot_y_deg, -45.0..=45.0).text("Rot Y°"));
                    });
                }
                if let Some(idx) = remove_kf_idx {
                    self.project.director_keyframes.remove(idx);
                }

                ui.separator();
                ui.heading("Zonas Sensibles (Difuminado / Tapado)");
                if ui.button("+ Añadir Zona Difuminada").clicked() {
                    self.project.privacy_masks.push(PrivacyMask {
                        id: format!("mask-{}", self.project.privacy_masks.len() + 1),
                        label: "Nueva Zona Sensible".into(),
                        start_sec: self.current_time_sec,
                        end_sec: (self.current_time_sec + 4.0).min(self.project.duration_sec),
                        x: 0.35,
                        y: 0.25,
                        width: 0.30,
                        height: 0.10,
                        mode: PrivacyMaskMode::Blur,
                        intensity: 16,
                        color: Some("#0f172a".into()),
                    });
                }

                let mut remove_mask_idx = None;
                for (i, mask) in self.project.privacy_masks.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut mask.label);
                            if ui.small_button("🗑").clicked() {
                                remove_mask_idx = Some(i);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut mask.mode, PrivacyMaskMode::Blur, "Blur");
                            ui.selectable_value(&mut mask.mode, PrivacyMaskMode::Pixelate, "Pixelado");
                            ui.selectable_value(&mut mask.mode, PrivacyMaskMode::Solid, "Tapado Sólido");
                        });
                        ui.add(
                            egui::Slider::new(&mut mask.start_sec, 0.0..=self.project.duration_sec)
                                .text("Inicio (s)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut mask.end_sec, 0.0..=self.project.duration_sec)
                                .text("Fin (s)"),
                        );
                        ui.add(egui::Slider::new(&mut mask.x, 0.0..=0.9).text("Pos X"));
                        ui.add(egui::Slider::new(&mut mask.y, 0.0..=0.9).text("Pos Y"));
                        ui.add(egui::Slider::new(&mut mask.width, 0.05..=0.8).text("Ancho"));
                        ui.add(egui::Slider::new(&mut mask.height, 0.04..=0.6).text("Alto"));
                    });
                }
                if let Some(idx) = remove_mask_idx {
                    self.project.privacy_masks.remove(idx);
                }
            }

            InspectorTab::Subtitles => {
                ui.heading("Subtítulos Locales (Palabra por Palabra)");
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("🎙 Transcribir Audio en este PC (Hilo Rust)")
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(79, 70, 229)),
                    )
                    .clicked()
                {
                    spawn_whisper_worker(self.project.duration_sec, self.worker_tx.clone());
                }

                ui.checkbox(
                    &mut self.project.subtitle_style.enabled,
                    "Mostrar Subtítulos en Vídeo",
                );
                ui.add(
                    egui::Slider::new(&mut self.project.subtitle_style.position_y, 0.15..=0.95)
                        .text("Posición Vertical Y"),
                );

                ui.separator();
                for seg in &mut self.project.subtitles {
                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Segmento {:.2}s - {:.2}s",
                                seg.start_sec, seg.end_sec
                            ))
                            .strong(),
                        );
                        for w in &mut seg.words {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut w.word).desired_width(95.0));
                                ui.add(
                                    egui::DragValue::new(&mut w.start_sec)
                                        .speed(0.05)
                                        .suffix("s"),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut w.end_sec)
                                        .speed(0.05)
                                        .suffix("s"),
                                );
                                let mut hl = w.highlight.unwrap_or(false);
                                if ui.checkbox(&mut hl, "★").changed() {
                                    w.highlight = Some(hl);
                                }
                            });
                        }
                    });
                }
            }

            InspectorTab::McpAi => {
                ui.heading("Servidor MCP Nativo en Rust (Cero Node.js)");
                ui.label(
                    "El servidor MCP está compilado dentro del propio binario nativo `/usr/bin/rec-ording`.",
                );
                ui.add_space(6.0);
                let mcp_json = r#"{
  "mcpServers": {
    "rec-ording": {
      "command": "/usr/bin/rec-ording",
      "args": ["--mcp"]
    }
  }
}"#;
                ui.code(mcp_json);
                if ui.button("📋 Copiar Configuración MCP (Claude / Cursor / Codex)").clicked() {
                    ui.ctx().copy_text(mcp_json.to_string());
                    self.show_toast("Configuración MCP copiada al portapapeles");
                }

                ui.separator();
                ui.strong("9 Herramientas MCP integradas en Rust:");
                for tool in [
                    "• get_project_summary",
                    "• import_recording_clip",
                    "• edit_clip (recorte, velocidad, congelado)",
                    "• set_3d_mockup (Pixel 11 Pro, S26 Ultra, Galaxy Tab, .GLB)",
                    "• plan_director_camera (auto-zoom 3D)",
                    "• add_privacy_mask (difuminado zonas sensibles)",
                    "• add_animated_title (pistas T1..T5)",
                    "• update_subtitles (palabra por palabra)",
                    "• render_export_ffmpeg (render multihilo Rayon + FFmpeg)",
                ] {
                    ui.label(egui::RichText::new(tool).size(12.0));
                }
            }

            InspectorTab::ExportFfmpeg => {
                ui.heading("Exportación FFmpeg Multihilo (Rayon)");
                ui.label(format!(
                    "Renderiza en paralelo usando los {} hilos de tu CPU y envía vídeo RGBA crudo por pipe a FFmpeg.",
                    rayon::current_num_threads()
                ));

                ui.add_space(4.0);
                ui.label("Presets para Tiendas de Apps:");
                for (preset, name) in [
                    (ExportPresetId::PlayStore16x9_4k, "Play Store 4K (3840×2160 @ 60fps)"),
                    (ExportPresetId::PlayStore16x9_1080p, "Play Store HD (1920×1080 @ 60fps)"),
                    (ExportPresetId::PlayStoreShorts9x16, "Play Store Vertical / Shorts (1080×1920)"),
                    (ExportPresetId::PlayStoreTablet16x10, "Play Store Tablet 16:10 (2560×1600 HEVC)"),
                    (ExportPresetId::AppStoreIphone6_9, "App Store 6.9\" (886×1920 @ 60fps)"),
                    (ExportPresetId::TransparentWebm, "Transparencia Alpha (WebM VP9 Alpha)"),
                    (ExportPresetId::GifPreview, "GIF Animado Optimizado (2 pasos)"),
                ] {
                    let active = self.project.export_settings.preset_id == preset;
                    if ui.selectable_label(active, name).clicked() {
                        self.apply_export_preset(preset);
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Resolución:");
                    ui.add(egui::DragValue::new(&mut self.project.export_settings.width).range(320..=3840));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut self.project.export_settings.height).range(240..=2160));
                });

                ui.horizontal(|ui| {
                    ui.label("FPS:");
                    ui.selectable_value(&mut self.project.export_settings.fps, 30, "30 fps");
                    ui.selectable_value(&mut self.project.export_settings.fps, 60, "60 fps");
                });

                egui::ComboBox::from_label("Códec")
                    .selected_text(self.project.export_settings.codec.label())
                    .show_ui(ui, |ui| {
                        for c in [
                            VideoCodec::H264,
                            VideoCodec::Hevc,
                            VideoCodec::Av1,
                            VideoCodec::Vp9Alpha,
                            VideoCodec::ProresAlpha,
                            VideoCodec::Gif,
                            VideoCodec::PngSequence,
                        ] {
                            let l = c.label();
                            ui.selectable_value(&mut self.project.export_settings.codec, c, l);
                        }
                    });

                ui.add(
                    egui::Slider::new(&mut self.project.export_settings.crf, 14..=32)
                        .text("Calidad CRF (menor = mejor)"),
                );
                ui.checkbox(
                    &mut self.project.export_settings.transparent_background,
                    "Fondo Transparente (Canal Alpha)",
                );

                ui.horizontal(|ui| {
                    ui.label("Archivo:");
                    ui.text_edit_singleline(&mut self.project.export_settings.output_path);
                });

                ui.add_space(6.0);
                ui.label("Tubería nativa Rust → FFmpeg:");
                ui.code(build_ffmpeg_cli_preview(&self.project));

                if self.is_exporting {
                    ui.add(
                        egui::ProgressBar::new(self.export_progress)
                            .text(&self.export_status_msg)
                            .animate(true),
                    );
                } else if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("🚀 Renderizar y Exportar con FFmpeg Multihilo")
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(16, 185, 129)),
                    )
                    .clicked()
                {
                    self.is_exporting = true;
                    self.export_progress = 0.0;
                    spawn_ffmpeg_export_worker(self.project.clone(), self.worker_tx.clone());
                }

                if !self.export_log.is_empty() {
                    ui.separator();
                    ui.code(&self.export_log);
                }
            }
        });
    }

    fn render_timeline_panel(&mut self, ui: &mut egui::Ui) {
        // Timeline top control bar
        ui.horizontal(|ui| {
            let play_label = if self.is_playing { "⏸ Pausar" } else { "▶ Reproducir" };
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(play_label).color(Color32::WHITE).strong())
                        .fill(Color32::from_rgb(79, 70, 229)),
                )
                .clicked()
            {
                self.is_playing = !self.is_playing;
            }

            if ui.button("⏮ 00:00").clicked() {
                self.current_time_sec = 0.0;
            }

            ui.label(
                egui::RichText::new(format!(
                    "{:05.2}s / {:05.2}s",
                    self.current_time_sec, self.project.duration_sec
                ))
                .monospace()
                .strong()
                .color(Color32::from_rgb(56, 189, 248)),
            );

            ui.separator();

            for (flt, label) in [
                (TrackFilter::All, "Todas (13)"),
                (TrackFilter::Video, "Vídeo (4)"),
                (TrackFilter::Audio, "Audio (4)"),
                (TrackFilter::Text, "Texto (5)"),
            ] {
                if ui.selectable_label(self.track_filter == flt, label).clicked() {
                    self.track_filter = flt;
                }
            }

            ui.separator();

            // Selected clip operations: Split, Speed, Freeze Frame
            if let Some(ref sel_id) = self.selected_clip_id.clone() {
                if ui.button("✂ Dividir en Cabezal").clicked() {
                    let t = self.current_time_sec;
                    for track in &mut self.project.tracks {
                        if let Some(pos) = track.clips.iter().position(|c| &c.id == sel_id) {
                            let clip = track.clips[pos].clone();
                            if t > clip.start_sec + 0.2 && t < clip.start_sec + clip.duration_sec - 0.2 {
                                let first_dur = t - clip.start_sec;
                                let second_dur = clip.duration_sec - first_dur;
                                track.clips[pos].duration_sec = first_dur;
                                let mut second = clip.clone();
                                second.id = format!("{}-part2", clip.id);
                                second.start_sec = t;
                                second.duration_sec = second_dur;
                                track.clips.push(second);
                                break;
                            }
                        }
                    }
                }

                if ui.button("❄ Congelar +1.5s").clicked() {
                    for track in &mut self.project.tracks {
                        if let Some(clip) = track.clips.iter_mut().find(|c| &c.id == sel_id) {
                            clip.duration_sec += 1.5;
                            clip.freeze_frames.push(FreezeFrameSegment {
                                id: format!("ff-{}", clip.freeze_frames.len() + 1),
                                source_time_sec: self.current_time_sec,
                                timeline_start_sec: self.current_time_sec,
                                duration_sec: 1.5,
                            });
                        }
                    }
                }

                for spd in [0.5f32, 1.0, 1.5, 2.0] {
                    if ui.small_button(format!("{}x", spd)).clicked() {
                        for track in &mut self.project.tracks {
                            if let Some(clip) = track.clips.iter_mut().find(|c| &c.id == sel_id) {
                                clip.speed = spd;
                            }
                        }
                    }
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Slider::new(&mut self.timeline_zoom, 0.5..=3.0)
                        .text("Zoom Pistas")
                        .show_value(false),
                );
            });
        });

        ui.separator();

        // Scrubbing slider across full duration
        ui.spacing_mut().slider_width = ui.available_width() - 120.0;
        ui.add(
            egui::Slider::new(&mut self.current_time_sec, 0.0..=self.project.duration_sec)
                .text("Cabezal (s)"),
        );

        // Multi-track lanes scroll area
        egui::ScrollArea::vertical().show(ui, |ui| {
            let header_w = 210.0;
            let track_h = 26.0;
            let avail_w = (ui.available_width() - header_w - 20.0).max(300.0) * self.timeline_zoom;
            let dur = self.project.duration_sec.max(1.0);

            // Director Camera Lane
            ui.horizontal(|ui| {
                ui.allocate_ui(Vec2::new(header_w, track_h), |ui| {
                    ui.label(
                        egui::RichText::new("🎥 Director · Cámara 3D")
                            .size(11.5)
                            .color(Color32::from_rgb(251, 191, 36)),
                    );
                });
                let (lane_rect, lane_resp) =
                    ui.allocate_exact_size(Vec2::new(avail_w, track_h), Sense::click());
                if lane_resp.clicked() {
                    if let Some(pos) = lane_resp.interact_pointer_pos() {
                        self.current_time_sec =
                            (((pos.x - lane_rect.left()) / avail_w) * dur).clamp(0.0, dur);
                    }
                }
                let p = ui.painter_at(lane_rect);
                p.rect_filled(lane_rect, Rounding::same(4.0), Color32::from_rgb(20, 26, 42));
                for kf in &self.project.director_keyframes {
                    let x = lane_rect.left() + (kf.time_sec / dur) * avail_w;
                    let w = ((kf.duration_sec / dur) * avail_w).max(18.0);
                    let r = Rect::from_min_size(
                        Pos2::new(x, lane_rect.top() + 3.0),
                        Vec2::new(w, track_h - 6.0),
                    );
                    p.rect_filled(
                        r,
                        Rounding::same(4.0),
                        Color32::from_rgba_unmultiplied(245, 158, 11, 180),
                    );
                    p.text(
                        r.left_center() + Vec2::new(4.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{:.1}x", kf.zoom),
                        FontId::proportional(10.5),
                        Color32::BLACK,
                    );
                }
                let ph_x = lane_rect.left() + (self.current_time_sec / dur) * avail_w;
                p.line_segment(
                    [Pos2::new(ph_x, lane_rect.top()), Pos2::new(ph_x, lane_rect.bottom())],
                    Stroke::new(2.0_f32, Color32::from_rgb(244, 63, 94)),
                );
            });

            // 13 Tracks (V1..V4, A1..A4, T1..T5)
            for track in &mut self.project.tracks {
                let show = match self.track_filter {
                    TrackFilter::All => true,
                    TrackFilter::Video => track.kind == "video",
                    TrackFilter::Audio => track.kind == "audio",
                    TrackFilter::Text => track.kind == "text",
                };
                if !show {
                    continue;
                }

                ui.horizontal(|ui| {
                    ui.allocate_ui(Vec2::new(header_w, track_h), |ui| {
                        ui.horizontal_centered(|ui| {
                            let m_col = if track.muted {
                                Color32::from_rgb(239, 68, 68)
                            } else {
                                Color32::from_rgb(148, 163, 184)
                            };
                            if ui
                                .add(egui::Button::new(egui::RichText::new("M").size(10.0).color(m_col)))
                                .clicked()
                            {
                                track.muted = !track.muted;
                            }
                            ui.label(egui::RichText::new(&track.name).size(11.5));
                        });
                    });

                    let (lane_rect, lane_resp) =
                        ui.allocate_exact_size(Vec2::new(avail_w, track_h), Sense::click_and_drag());
                    if lane_resp.clicked() {
                        if let Some(pos) = lane_resp.interact_pointer_pos() {
                            self.current_time_sec =
                                (((pos.x - lane_rect.left()) / avail_w) * dur).clamp(0.0, dur);
                        }
                    }

                    let p = ui.painter_at(lane_rect);
                    p.rect_filled(lane_rect, Rounding::same(4.0), Color32::from_rgb(15, 21, 35));

                    let base_col = match track.kind.as_str() {
                        "video" => Color32::from_rgb(79, 70, 229),
                        "audio" => Color32::from_rgb(16, 185, 129),
                        _ => Color32::from_rgb(14, 165, 233),
                    };

                    for clip in &mut track.clips {
                        let cx = lane_rect.left() + (clip.start_sec / dur) * avail_w;
                        let cw = ((clip.duration_sec / dur) * avail_w).max(24.0);
                        let clip_rect = Rect::from_min_size(
                            Pos2::new(cx, lane_rect.top() + 2.0),
                            Vec2::new(cw, track_h - 4.0),
                        );

                        let is_sel = self.selected_clip_id.as_deref() == Some(&clip.id);
                        p.rect_filled(clip_rect, Rounding::same(4.0), base_col);
                        if is_sel {
                            p.rect_stroke(
                                clip_rect,
                                Rounding::same(4.0),
                                Stroke::new(1.8_f32, Color32::WHITE),
                            );
                        }
                        p.text(
                            clip_rect.left_center() + Vec2::new(6.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{} ({:.1}x)", clip.name, clip.speed),
                            FontId::proportional(10.5),
                            Color32::WHITE,
                        );

                        if lane_resp.clicked() {
                            if let Some(pos) = lane_resp.interact_pointer_pos() {
                                if clip_rect.contains(pos) {
                                    self.selected_clip_id = Some(clip.id.clone());
                                }
                            }
                        }
                    }

                    let ph_x = lane_rect.left() + (self.current_time_sec / dur) * avail_w;
                    p.line_segment(
                        [Pos2::new(ph_x, lane_rect.top()), Pos2::new(ph_x, lane_rect.bottom())],
                        Stroke::new(2.0_f32, Color32::from_rgb(244, 63, 94)),
                    );
                });
            }
        });
    }

    fn render_recording_modal(&mut self, ctx: &egui::Context) {
        let mut open = self.show_recording_window;
        egui::Window::new("🔴 Estudio de Captura Multihilo (ADB USB + PC + Telemetría)")
            .open(&mut open)
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label("Captura simultánea en hilos dedicados de Rust (FFmpeg x11grab + ADB screenrecord + getevent).");
                ui.separator();

                ui.checkbox(
                    &mut self.rec_capture_android,
                    "📱 Grabar Teléfono / Tablet Android por USB (ADB)",
                );
                if self.rec_capture_android {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_label("Dispositivo USB")
                            .selected_text(&self.selected_adb_serial)
                            .show_ui(ui, |ui| {
                                for dev in &self.adb_devices {
                                    ui.selectable_value(
                                        &mut self.selected_adb_serial,
                                        dev.serial.clone(),
                                        format!(
                                            "{} · {} ({})",
                                            dev.model,
                                            dev.serial,
                                            dev.resolution.as_deref().unwrap_or("1080x2400")
                                        ),
                                    );
                                }
                            });
                        if ui.button("🔄 Refrescar ADB").clicked() {
                            spawn_adb_scan_worker(self.worker_tx.clone());
                        }
                    });
                }

                ui.checkbox(
                    &mut self.rec_capture_desktop,
                    "💻 Grabar Pantalla de PC simultáneamente (FFmpeg 60fps)",
                );
                ui.checkbox(&mut self.rec_capture_mic, "🎙 Grabar Micrófono (Pista A1)");
                ui.checkbox(
                    &mut self.rec_auto_zoom_on_stop,
                    "✦ Generar Auto-Zoom Director al finalizar desde los toques",
                );

                ui.separator();
                if self.active_recording.is_none() {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("● Iniciar Grabación Multihilo")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(225, 29, 72)),
                        )
                        .clicked()
                    {
                        let adb_opt = if self.rec_capture_android {
                            Some(self.selected_adb_serial.clone())
                        } else {
                            None
                        };
                        self.active_recording = Some(start_multithreaded_recording(
                            self.rec_capture_desktop,
                            adb_opt,
                            self.rec_capture_mic,
                        ));
                    }
                } else {
                    let elapsed = self
                        .active_recording
                        .as_ref()
                        .map(|h| h.started_at.elapsed().as_secs_f32())
                        .unwrap_or(0.0);
                    ui.label(
                        egui::RichText::new(format!("⏱ Grabando en vivo: {:.1}s", elapsed))
                            .color(Color32::from_rgb(248, 113, 113))
                            .strong(),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("⏹ Detener e Importar al Proyecto")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(16, 185, 129)),
                        )
                        .clicked()
                    {
                        if let Some(handle) = self.active_recording.take() {
                            stop_multithreaded_recording(handle, self.worker_tx.clone());
                        }
                        self.show_recording_window = false;
                    }
                }
            });
        if !open {
            self.show_recording_window = false;
        }
    }

    fn render_update_modal(&mut self, ctx: &egui::Context) {
        let mut open = self.show_update_window;
        egui::Window::new("🛡 Auto-Actualización · CachyOS / Arch Linux (blzkz/rec-ording)")
            .open(&mut open)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("Conectado al repositorio privado: github.com/blzkz/rec-ording");
                ui.checkbox(
                    &mut self.auto_check_updates,
                    "Comprobar actualizaciones automáticamente al iniciar",
                );
                ui.separator();

                if let Some(ref info) = self.update_info {
                    ui.horizontal(|ui| {
                        ui.label("Versión instalada:");
                        ui.strong(format!("v{}", info.current_version));
                        ui.separator();
                        ui.label("Última release en GitHub:");
                        ui.strong(format!("v{}", info.latest_version));
                    });
                    ui.label(&info.release_title);
                    ui.label(egui::RichText::new(&info.release_notes).size(11.5));
                }

                if !self.update_log.is_empty() {
                    ui.separator();
                    ui.code(&self.update_log);
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("🔄 Comprobar ahora").clicked() {
                        spawn_update_check_worker(self.worker_tx.clone());
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("⬇ Descargar e Instalar (.pkg.tar.zst)")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(79, 70, 229)),
                        )
                        .clicked()
                    {
                        self.update_log =
                            "Descargando paquete .pkg.tar.zst en hilo secundario...".into();
                        spawn_update_install_worker(self.worker_tx.clone());
                    }
                });
            });
        if !open {
            self.show_update_window = false;
        }
    }
}
