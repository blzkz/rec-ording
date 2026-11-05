//! The start screen: create or open a project, and the latest ones with a picture of each.

use super::*;

/// How many recent projects the start screen shows.
const RECENT: usize = 5;

pub(super) fn ago(modified: std::time::SystemTime) -> String {
    modified
        .elapsed()
        .map(|d| match d.as_secs() {
            s if s < 60 => "Ahora mismo".to_string(),
            s if s < 3600 => format!("Hace {} min", s / 60),
            s if s < 86400 => format!("Hace {} h", s / 3600),
            s if s < 2 * 86400 => "Ayer".to_string(),
            s => format!("Hace {} días", s / 86400),
        })
        .unwrap_or_default()
}

impl RecOrdingApp {
    /// Closes the open project (saved) and shows the start screen.
    pub(super) fn go_home(&mut self) {
        if self.recording.is_some() || self.saving_recording || matches!(self.export_ui, ExportUi::Running { .. }) {
            self.toast("Espera a que termine la grabación o la exportación");
            return;
        }
        if self.playing {
            self.toggle_play();
        }
        self.save_now();
        self.home = true;
        self.project = Project::default();
        self.last_saved = self.project.clone();
        self.committed = self.project.clone();
        self.undo.clear();
        self.redo.clear();
        self.selection = Selection::None;
        self.director_mode = false;
        self.sheet = Sheet::None;
        self.refresh_recent();
    }

    /// Opens a project from the start screen.
    pub(super) fn open_from_home(&mut self, path: PathBuf) {
        if let Err(e) = load_project(&path) {
            self.toast(e);
            return;
        }
        // Still on the start screen while switching, so the empty placeholder project is
        // never saved over the previous one.
        self.open_project(path);
        self.home = false;
    }

    /// Re-reads the latest projects and draws their pictures in the background.
    pub(super) fn refresh_recent(&mut self) {
        self.recent = list_projects().into_iter().take(RECENT).collect();
        let paths: Vec<PathBuf> = self.recent.iter().map(|e| e.path.clone()).collect();
        self.recent_thumbs.retain(|p, _| paths.contains(p));
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            for path in paths {
                let thumb = crate::thumbs::project(&path).map(|t| (t.size, t.rgba));
                let _ = tx.send(AppEvent::ProjectThumb { path, thumb });
                ctx.request_repaint();
            }
        });
    }

    pub(super) fn create_from_home(&mut self, name: &str) {
        // A new project keeps the look of the latest one.
        let template = self.recent.first().and_then(|e| load_project(&e.path).ok());
        match create_project(name, template.as_ref()) {
            Ok((path, _)) => {
                self.open_project(path);
                self.home = false;
                self.toast(format!("Proyecto «{}» creado", name.trim()));
            }
            Err(e) => self.toast(e),
        }
    }

    pub(super) fn pick_project_file(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            match crate::platform::pick_file("Abrir proyecto", "Proyectos de rec·ording", &["json"], &projects_dir()) {
                Ok(Some(p)) => {
                    let _ = tx.send(AppEvent::ProjectPicked(p));
                }
                Ok(None) => {}
                Err(()) => {
                    let _ = tx.send(AppEvent::PickerUnavailable);
                }
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn home(&mut self, ctx: &egui::Context) {
        let mut open: Option<PathBuf> = None;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(VIEWER)).show(ctx, |ui| {
            let r = ui.max_rect();
            let width = (r.width() - 96.0).clamp(420.0, 900.0);
            // Centred on the window, a little above the middle.
            let pic_h = ((width - 14.0 * (RECENT as f32 - 1.0)) / RECENT as f32 * 0.82).round();
            let content_h = 300.0 + pic_h;
            let top = r.top() + ((r.height() - content_h) * 0.42).max(24.0);
            let col = Rect::from_min_size(Pos2::new(r.center().x - width / 2.0, top), Vec2::new(width, r.bottom() - top));
            let mut c = ui.new_child(UiBuilder::new().max_rect(col).layout(egui::Layout::top_down(Align::Min)));
            c.vertical_centered(|ui| {
                wordmark_centered(ui, 34.0);
                ui.add_space(4.0);
                ui.label(RichText::new("Crea el vídeo de tu app").font(regular(16.0)).color(TEXT_2));
            });
            c.add_space(30.0);

            let gap = 16.0;
            let (row, _) = c.allocate_exact_size(Vec2::new(width, 96.0), Sense::hover());
            let half = (width - gap) / 2.0;
            let a = Rect::from_min_size(row.min, Vec2::new(half, row.height()));
            let b = Rect::from_min_size(Pos2::new(row.left() + half + gap, row.top()), Vec2::new(half, row.height()));
            if action_card(&mut c, a, "new", Icon::Plus, ACCENT, "Nuevo proyecto", "Graba el móvil o importa un vídeo.").clicked() {
                self.new_project_name.clear();
                self.sheet = Sheet::NewProject;
            }
            if action_card(&mut c, b, "open", Icon::Folder, PURPLE, "Abrir proyecto", "Elige uno de tus proyectos o un archivo.").clicked() {
                self.projects = list_projects();
                self.sheet = Sheet::Projects;
            }

            c.add_space(34.0);
            c.horizontal(|ui| {
                ui.label(RichText::new("Recientes").font(medium(15.0)).color(TEXT));
                if !self.recent.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui.link(RichText::new("Ver todos").font(regular(13.0)).color(ACCENT)).clicked() {
                            self.projects = list_projects();
                            self.sheet = Sheet::Projects;
                        }
                    });
                }
            });
            c.add_space(8.0);
            if self.recent.is_empty() {
                c.label(RichText::new("Aquí aparecerán los proyectos en los que trabajes.").color(TEXT_3));
            } else {
                let gap = 14.0;
                let card_w = ((width - gap * (RECENT as f32 - 1.0)) / RECENT as f32).floor();
                let pic_h = (card_w * 0.82).round();
                let (row, _) = c.allocate_exact_size(Vec2::new(width, pic_h + 46.0), Sense::hover());
                for (i, e) in self.recent.iter().enumerate() {
                    let rect = Rect::from_min_size(Pos2::new(row.left() + i as f32 * (card_w + gap), row.top()), Vec2::new(card_w, row.height()));
                    let thumb = self.recent_thumbs.get(&e.path);
                    if recent_card(&mut c, rect, pic_h, e, thumb).clicked() {
                        open = Some(e.path.clone());
                    }
                }
            }

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
                c.add_space(28.0);
                c.label(
                    RichText::new(format!("Para usar todas las funciones instala:\n{}", crate::platform::install_hint(&missing)))
                        .font(regular(12.5))
                        .color(YELLOW),
                );
            }
        });
        if let Some(p) = open {
            self.open_from_home(p);
        }
    }

    pub(super) fn new_project_sheet(&mut self, ctx: &egui::Context) {
        let mut create = false;
        let mut close = false;
        Self::sheet_window(ctx, "new-project", 420.0, |ui| {
            ui.label(RichText::new("Proyecto nuevo").font(medium(20.0)).color(TEXT));
            ui.label(RichText::new("Ponle un nombre para encontrarlo después.").color(TEXT_2));
            ui.add_space(10.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.new_project_name)
                    .hint_text("p. ej. «Mi app · Play Store»")
                    .desired_width(f32::INFINITY)
                    .font(regular(14.0)),
            );
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            let ok = !self.new_project_name.trim().is_empty();
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && ok {
                create = true;
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Min), |ui| {
                if primary_button(ui, "Crear", ok).clicked() {
                    create = true;
                }
                if secondary_button(ui, "Cancelar").clicked() {
                    close = true;
                }
            });
        });
        if create {
            self.sheet = Sheet::None;
            let name = self.new_project_name.trim().to_string();
            self.create_from_home(&name);
        } else if close {
            self.sheet = Sheet::None;
        }
    }
}

/// The app's name, centred on its own line.
fn wordmark_centered(ui: &mut Ui, size: f32) {
    ui.horizontal(|ui| {
        let w = {
            let font = medium(size);
            let p = ui.painter();
            p.layout_no_wrap("rec".into(), font.clone(), TEXT).size().x + p.layout_no_wrap("ording".into(), font, TEXT).size().x + size * 0.76
        };
        ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
        wordmark(ui, size);
    });
}

/// A large button with an icon, a title and a line of explanation.
fn action_card(ui: &mut Ui, rect: Rect, id: &str, icon: Icon, tint: Color32, title: &str, body: &str) -> egui::Response {
    let resp = ui.interact(rect, Id::new(("home", id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(14.0), lerp_color(CARD, Color32::from_rgb(58, 58, 64), h));
    p.rect_stroke(rect, Rounding::same(14.0), Stroke::new(1.0_f32, lerp_color(Color32::from_rgb(62, 62, 68), tint.linear_multiply(0.7), h)));
    let ic = Pos2::new(rect.left() + 46.0, rect.center().y);
    p.circle_filled(ic, 24.0, lerp_color(CARD, tint, 0.25 + 0.1 * h));
    paint_icon(p, icon, ic, 22.0, tint);
    let x = rect.left() + 86.0;
    p.text(Pos2::new(x, rect.center().y - 4.0), Align2::LEFT_BOTTOM, title, medium(17.0), TEXT);
    let g = p.layout(body.to_string(), regular(13.0), TEXT_2, rect.right() - x - 16.0);
    p.galley(Pos2::new(x, rect.center().y + 2.0), g, TEXT_2);
    resp
}

/// A recent project: its picture, name and when it was last changed.
fn recent_card(ui: &mut Ui, rect: Rect, pic_h: f32, e: &ProjectEntry, thumb: Option<&Option<TextureHandle>>) -> egui::Response {
    let resp = ui
        .interact(rect, Id::new(("recent", &e.path)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(home_relative(&e.path.to_string_lossy()));
    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    let pic = Rect::from_min_size(rect.min, Vec2::new(rect.width(), pic_h));
    p.rect_filled(pic, Rounding::same(10.0), lerp_color(Color32::from_rgb(34, 34, 37), Color32::from_rgb(44, 44, 48), h));
    match thumb {
        Some(Some(t)) => {
            // Fitted inside the box, whatever the shape of the video.
            let [w, hh] = t.size();
            let s = (pic.width() / w as f32).min(pic.height() / hh as f32);
            let r = Rect::from_center_size(pic.center(), Vec2::new(w as f32 * s, hh as f32 * s));
            p.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        Some(None) => {
            paint_icon(p, Icon::AddVideo, pic.center(), 26.0, TEXT_3);
            p.text(pic.center() + Vec2::new(0.0, 26.0), Align2::CENTER_CENTER, "Sin vídeo", regular(11.5), TEXT_3);
        }
        None => {
            p.text(pic.center(), Align2::CENTER_CENTER, "…", regular(16.0), TEXT_3);
        }
    }
    // Round the corners of the picture.
    p.rect_stroke(pic.expand(1.0), Rounding::same(11.0), Stroke::new(2.5_f32, VIEWER));
    p.rect_stroke(pic, Rounding::same(10.0), Stroke::new(if h > 0.01 { 2.0_f32 } else { 1.0 }, lerp_color(Color32::from_rgb(58, 58, 63), ACCENT, h)));
    let name = p.layout(e.name.clone(), medium(13.0), TEXT, rect.width());
    let name = if name.rows.len() > 1 {
        // Cut short to one line.
        let mut job = egui::text::LayoutJob::simple_singleline(e.name.clone(), medium(13.0), TEXT);
        job.wrap = egui::text::TextWrapping { max_width: rect.width(), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        p.layout_job(job)
    } else {
        name
    };
    p.galley(Pos2::new(rect.left() + 2.0, pic.bottom() + 8.0), name, TEXT);
    p.text(Pos2::new(rect.left() + 2.0, pic.bottom() + 28.0), Align2::LEFT_TOP, ago(e.modified), regular(11.5), TEXT_3);
    resp
}
