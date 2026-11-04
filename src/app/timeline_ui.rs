//! The timeline: video clips (trim, reorder, split), camera clips with keyframes, device
//! motion, titles, Director markers and the playhead.

use super::*;

const LABEL_W: f32 = 64.0;
const MOVE_COLOR: Color32 = Color32::from_rgb(255, 159, 10);
const DIRECTOR_COLOR: Color32 = Color32::from_rgb(90, 200, 250);
const DEVICE_COLOR: Color32 = Color32::from_rgb(52, 170, 90);
const TEXT_COLOR: Color32 = Color32::from_rgb(235, 68, 120);

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Drag {
    Scrub,
    TrimIn { id: u64, orig: f32, grab: f32 },
    TrimOut { id: u64, orig: f32, grab: f32 },
    ClipMove { id: u64 },
    CamMove { id: u64, grab: f32 },
    CamStart { id: u64, end: f32 },
    CamEnd { id: u64, start: f32 },
    CamKey { id: u64, index: usize },
    DevMove { id: u64, grab: f32 },
    TextMove { id: u64, grab: f32 },
    TextStart { id: u64, end: f32 },
    TextEnd { id: u64, start: f32 },
    Marker { id: u64 },
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Ruler,
    Marker(u64),
    ClipIn(u64),
    ClipOut(u64),
    ClipBody(u64),
    VideoEmpty,
    CamStart(u64),
    CamEnd(u64),
    CamKey(u64, usize),
    CamBody(u64, f32),
    CamEmpty,
    DevKey(u64, usize),
    DevBody(u64, f32),
    DevEmpty,
    TextStart(u64),
    TextEnd(u64),
    TextBody(u64, f32),
    TextEmpty,
    None,
}

/// Timeline geometry: maps seconds to x and back.
struct Tl {
    x0: f32,
    x1: f32,
    dur: f32,
    ruler: Rect,
    video: Rect,
    camera: Rect,
    device: Rect,
    text: Rect,
}

impl Tl {
    fn x(&self, t: f32) -> f32 {
        self.x0 + (t / self.dur) * (self.x1 - self.x0)
    }
    fn t(&self, x: f32) -> f32 {
        ((x - self.x0) / (self.x1 - self.x0) * self.dur).clamp(0.0, self.dur)
    }
}

fn camera_color(kind: CameraKind) -> Color32 {
    match kind {
        CameraKind::Auto | CameraKind::Zoom => PURPLE,
        CameraKind::Move => MOVE_COLOR,
        CameraKind::Director => DIRECTOR_COLOR,
    }
}

fn diamond(p: &egui::Painter, c: Pos2, r: f32, fill: Color32, stroke: Color32) {
    let pts = vec![Pos2::new(c.x, c.y - r), Pos2::new(c.x + r, c.y), Pos2::new(c.x, c.y + r), Pos2::new(c.x - r, c.y)];
    p.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.0_f32, stroke)));
}

impl RecOrdingApp {
    pub(super) fn timeline(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("timeline")
            .exact_height(256.0)
            .frame(
                egui::Frame::none()
                    .fill(WINDOW)
                    .inner_margin(Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 12.0 })
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(16, 16, 18))),
            )
            .show(ctx, |ui| {
                self.timeline_header(ui);
                ui.add_space(4.0);
                let body = ui.available_rect_before_wrap();
                self.timeline_body(ui, body);
            });
    }

    fn timeline_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if secondary_button(ui, "+ Zoom").on_hover_text("Zoom en el cursor (Z)").clicked() {
                self.add_zoom_at(self.time);
            }
            if secondary_button(ui, "+ Movimiento").on_hover_text("Un giro de cámara en el cursor; edita sus fotogramas clave").clicked() {
                match timeline::add_move(&mut self.project, self.time) {
                    Ok(id) => self.selection = Selection::Camera { id, key: Some(1) },
                    Err(e) => self.toast(e),
                }
            }
            if secondary_button(ui, "+ Texto").on_hover_text("Un título junto al móvil, en el cursor (T)").clicked() {
                self.add_text_at(self.time);
            }
            ui.menu_button(RichText::new("Móvil…").color(TEXT), |ui| {
                ui.label(RichText::new("Movimiento del móvil en el cursor").color(TEXT_3).font(regular(11.5)));
                ui.label(RichText::new("Los giros junto a un cambio de clip se centran en él").color(TEXT_3).font(regular(11.5)));
                for preset in DevicePreset::ALL {
                    if ui.button(preset.label()).clicked() {
                        let id = timeline::add_device_preset(&mut self.project, preset, self.time);
                        self.selection = Selection::Device { id, key: None };
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text("Añadir un movimiento del dispositivo");
            let director = egui::Button::new(RichText::new("Director").color(if self.director_mode { Color32::WHITE } else { TEXT }))
                .fill(if self.director_mode { ACCENT } else { CONTROL })
                .min_size(Vec2::new(0.0, 28.0));
            if ui.add(director).on_hover_text("Marca lo que debe verse en cada momento y se generan los planos de cámara").clicked() {
                self.director_mode = !self.director_mode;
                self.selection = Selection::None;
                if self.playing {
                    self.toggle_play();
                }
            }
            if secondary_button(ui, "Dividir").on_hover_text("Divide el clip en el cursor (Ctrl+B)").clicked() {
                self.split_here();
            }
            if secondary_button(ui, "Añadir vídeo").on_hover_text("Añade un vídeo al final").clicked() {
                self.pick_video();
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("Duración {}", fmt_time(timeline::duration(&self.project)))).color(TEXT_2).font(regular(12.5)));
                ui.add_space(8.0);
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Rehacer").min_size(Vec2::new(0.0, 28.0))).on_hover_text("Ctrl+Mayús+Z").clicked() {
                    self.undo_redo(false);
                }
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Deshacer").min_size(Vec2::new(0.0, 28.0))).on_hover_text("Ctrl+Z").clicked() {
                    self.undo_redo(true);
                }
            });
        });
    }

    fn hit(&self, tl: &Tl, pos: Pos2) -> Hit {
        const EDGE: f32 = 6.0;
        if tl.ruler.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            for t in &self.project.director.targets {
                if (pos.x - tl.x(t.time)).abs() <= 7.0 {
                    return Hit::Marker(t.id);
                }
            }
            return Hit::Ruler;
        }
        if tl.video.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            for c in timeline::placed(&self.project) {
                let (a, b) = (tl.x(c.start), tl.x(c.end));
                if (pos.x - a).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::ClipIn(c.clip.id);
                }
                if (pos.x - b).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::ClipOut(c.clip.id);
                }
                if pos.x >= a && pos.x <= b {
                    return Hit::ClipBody(c.clip.id);
                }
            }
            return Hit::VideoEmpty;
        }
        if tl.camera.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            for c in self.project.camera.iter().rev() {
                let (a, b) = (tl.x(c.start), tl.x(c.end()));
                if pos.x < a - EDGE || pos.x > b + EDGE {
                    continue;
                }
                // Inner keyframes; the first and last sit on the edges, which resize the clip
                // (clicking an edge still selects that keyframe).
                if !matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) {
                    let last = c.keys.len().saturating_sub(1);
                    for (i, k) in c.keys.iter().enumerate().take(last).skip(1) {
                        if (pos.x - tl.x(c.start + k.t)).abs() <= 5.0 {
                            return Hit::CamKey(c.id, i);
                        }
                    }
                }
                if (pos.x - b).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::CamEnd(c.id);
                }
                if (pos.x - a).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::CamStart(c.id);
                }
                if pos.x >= a && pos.x <= b {
                    return Hit::CamBody(c.id, tl.t(pos.x) - c.start);
                }
            }
            return Hit::CamEmpty;
        }
        if tl.device.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            for c in self.project.device.iter().rev() {
                let (a, b) = (tl.x(c.start), tl.x(c.end()));
                for (i, k) in c.keys.iter().enumerate() {
                    if (pos.x - tl.x(c.start + k.t)).abs() <= 5.0 {
                        return Hit::DevKey(c.id, i);
                    }
                }
                if pos.x >= a && pos.x <= b {
                    return Hit::DevBody(c.id, tl.t(pos.x) - c.start);
                }
            }
            return Hit::DevEmpty;
        }
        if tl.text.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
            for c in self.project.texts.iter().rev() {
                let (a, b) = (tl.x(c.start), tl.x(c.end()));
                if pos.x < a - EDGE || pos.x > b + EDGE {
                    continue;
                }
                if (pos.x - b).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::TextEnd(c.id);
                }
                if (pos.x - a).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::TextStart(c.id);
                }
                if pos.x >= a && pos.x <= b {
                    return Hit::TextBody(c.id, tl.t(pos.x) - c.start);
                }
            }
            return Hit::TextEmpty;
        }
        Hit::None
    }

    fn timeline_body(&mut self, ui: &mut Ui, body: Rect) {
        let dur = timeline::duration(&self.project).max(0.1);
        let ruler = Rect::from_min_size(body.min, Vec2::new(body.width(), 18.0));
        let video = Rect::from_min_size(Pos2::new(body.left(), ruler.bottom() + 6.0), Vec2::new(body.width(), 58.0));
        let camera = Rect::from_min_size(Pos2::new(body.left(), video.bottom() + 8.0), Vec2::new(body.width(), 30.0));
        let device = Rect::from_min_size(Pos2::new(body.left(), camera.bottom() + 6.0), Vec2::new(body.width(), 24.0));
        let text = Rect::from_min_size(Pos2::new(body.left(), device.bottom() + 6.0), Vec2::new(body.width(), 24.0));
        let tl = Tl { x0: body.left() + LABEL_W, x1: body.right() - 2.0, dur, ruler, video, camera, device, text };

        let resp = ui.interact(Rect::from_min_max(ruler.min, text.max), Id::new("timeline"), Sense::click_and_drag());
        let hover_hit = resp.hover_pos().map(|p| self.hit(&tl, p)).unwrap_or(Hit::None);
        match self.drag {
            Some(Drag::TrimIn { .. } | Drag::TrimOut { .. } | Drag::CamStart { .. } | Drag::CamEnd { .. } | Drag::TextStart { .. } | Drag::TextEnd { .. }) => {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
            }
            Some(Drag::CamMove { .. } | Drag::DevMove { .. } | Drag::TextMove { .. } | Drag::ClipMove { .. } | Drag::CamKey { .. } | Drag::Marker { .. }) => {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing)
            }
            _ => match hover_hit {
                Hit::ClipIn(_) | Hit::ClipOut(_) | Hit::CamStart(_) | Hit::CamEnd(_) | Hit::TextStart(_) | Hit::TextEnd(_) => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
                }
                Hit::CamBody(..) | Hit::DevBody(..) | Hit::TextBody(..) | Hit::CamKey(..) | Hit::DevKey(..) | Hit::Marker(_) => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grab)
                }
                _ => {}
            },
        }

        if resp.drag_started() {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                let t = tl.t(origin.x);
                self.drag = match self.hit(&tl, origin) {
                    Hit::ClipIn(id) => self.project.clips.iter().find(|c| c.id == id).map(|c| Drag::TrimIn { id, orig: c.source_in, grab: t }),
                    Hit::ClipOut(id) => self.project.clips.iter().find(|c| c.id == id).map(|c| Drag::TrimOut { id, orig: c.source_out, grab: t }),
                    Hit::ClipBody(id) if self.project.clips.len() > 1 => Some(Drag::ClipMove { id }),
                    Hit::CamBody(id, grab) => Some(Drag::CamMove { id, grab }),
                    Hit::CamStart(id) => self.project.camera.iter().find(|c| c.id == id).map(|c| Drag::CamStart { id, end: c.end() }),
                    Hit::CamEnd(id) => self.project.camera.iter().find(|c| c.id == id).map(|c| Drag::CamEnd { id, start: c.start }),
                    Hit::CamKey(id, index) => Some(Drag::CamKey { id, index }),
                    Hit::DevBody(id, grab) => Some(Drag::DevMove { id, grab }),
                    Hit::DevKey(id, _) => {
                        let start = self.project.device.iter().find(|d| d.id == id).map(|d| d.start).unwrap_or(0.0);
                        Some(Drag::DevMove { id, grab: t - start })
                    }
                    Hit::TextBody(id, grab) => Some(Drag::TextMove { id, grab }),
                    Hit::TextStart(id) => self.project.texts.iter().find(|c| c.id == id).map(|c| Drag::TextStart { id, end: c.end() }),
                    Hit::TextEnd(id) => self.project.texts.iter().find(|c| c.id == id).map(|c| Drag::TextEnd { id, start: c.start }),
                    Hit::Marker(id) => Some(Drag::Marker { id }),
                    Hit::None => None,
                    _ => Some(Drag::Scrub),
                };
                match self.drag {
                    Some(Drag::TrimIn { id, .. } | Drag::TrimOut { id, .. } | Drag::ClipMove { id }) => self.selection = Selection::Clip(id),
                    Some(Drag::CamMove { id, .. } | Drag::CamStart { id, .. } | Drag::CamEnd { id, .. }) => self.selection = Selection::Camera { id, key: None },
                    Some(Drag::CamKey { id, index }) => self.selection = Selection::Camera { id, key: Some(index) },
                    Some(Drag::DevMove { id, .. }) => self.selection = Selection::Device { id, key: None },
                    Some(Drag::TextMove { id, .. } | Drag::TextStart { id, .. } | Drag::TextEnd { id, .. }) => self.selection = Selection::Text(id),
                    Some(Drag::Marker { id }) => self.selection = Selection::Target(id),
                    _ => {}
                }
                if self.playing {
                    self.toggle_play();
                }
            }
        }
        if resp.dragged() {
            if let (Some(d), Some(pos)) = (self.drag, resp.interact_pointer_pos()) {
                self.apply_drag(d, tl.t(pos.x));
            }
        }
        if resp.drag_stopped() {
            if let (Some(Drag::ClipMove { id }), Some(pos)) = (self.drag, resp.interact_pointer_pos()) {
                let index = self.drop_index(id, tl.t(pos.x));
                let _ = timeline::move_clip(&mut self.project, id, index);
                self.requested_time = None;
            }
            self.drag = None;
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = tl.t(pos.x);
                match self.hit(&tl, pos) {
                    Hit::Ruler => self.seek(t),
                    Hit::Marker(id) => {
                        self.selection = Selection::Target(id);
                        self.director_mode = true;
                        if let Some(tg) = self.project.director.targets.iter().find(|x| x.id == id) {
                            self.seek(tg.time);
                        }
                    }
                    Hit::ClipIn(id) | Hit::ClipOut(id) | Hit::ClipBody(id) => {
                        self.selection = Selection::Clip(id);
                        self.seek(t);
                    }
                    Hit::TextStart(id) | Hit::TextEnd(id) | Hit::TextBody(id, _) => {
                        self.selection = Selection::Text(id);
                        self.seek(t);
                    }
                    Hit::VideoEmpty | Hit::CamEmpty | Hit::DevEmpty | Hit::TextEmpty => {
                        self.selection = Selection::None;
                        self.seek(t);
                    }
                    Hit::CamStart(id) | Hit::CamEnd(id) | Hit::CamBody(id, _) => {
                        let edge_key = self.project.camera.iter().find(|c| c.id == id).and_then(|c| {
                            if matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) {
                                return None;
                            }
                            match self.hit(&tl, pos) {
                                Hit::CamStart(_) => Some(0),
                                Hit::CamEnd(_) => Some(c.keys.len() - 1),
                                _ => None,
                            }
                        });
                        self.selection = Selection::Camera { id, key: edge_key };
                        self.seek(t);
                    }
                    Hit::CamKey(id, i) => {
                        self.selection = Selection::Camera { id, key: Some(i) };
                        if let Some(c) = self.project.camera.iter().find(|c| c.id == id) {
                            self.seek(c.start + c.keys[i].t);
                        }
                    }
                    Hit::DevBody(id, _) => {
                        self.selection = Selection::Device { id, key: None };
                        self.seek(t);
                    }
                    Hit::DevKey(id, i) => {
                        self.selection = Selection::Device { id, key: Some(i) };
                        if let Some(c) = self.project.device.iter().find(|c| c.id == id) {
                            self.seek(c.start + c.keys[i].t);
                        }
                    }
                    Hit::None => {}
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                match self.hit(&tl, pos) {
                    Hit::CamEmpty => self.add_zoom_at(tl.t(pos.x)),
                    Hit::TextEmpty => self.add_text_at(tl.t(pos.x)),
                    _ => {}
                }
            }
        }

        let drop_x = match (self.drag, resp.interact_pointer_pos()) {
            (Some(Drag::ClipMove { id }), Some(pos)) => {
                let index = self.drop_index(id, tl.t(pos.x));
                let others: Vec<timeline::Placed> = timeline::placed(&self.project).into_iter().filter(|c| c.clip.id != id).collect();
                let mut t = 0.0;
                for c in others.iter().take(index) {
                    t += c.clip.duration();
                }
                Some(tl.x(t))
            }
            _ => None,
        };
        self.paint_timeline(ui, &tl, hover_hit, drop_x);
    }

    /// Where a dragged clip lands: before the first other clip whose middle is past `t`.
    fn drop_index(&self, id: u64, t: f32) -> usize {
        timeline::placed(&self.project)
            .into_iter()
            .filter(|c| c.clip.id != id)
            .filter(|c| (c.start + c.end) * 0.5 < t)
            .count()
    }

    fn apply_drag(&mut self, d: Drag, t: f32) {
        let total = timeline::duration(&self.project);
        match d {
            Drag::Scrub => self.seek(t),
            Drag::TrimIn { id, orig, grab } => {
                if timeline::trim_clip(&mut self.project, id, Some(orig + (t - grab)), None).is_ok() {
                    if let Some(c) = timeline::placed(&self.project).into_iter().find(|c| c.clip.id == id) {
                        self.seek(c.start);
                    }
                }
            }
            Drag::TrimOut { id, orig, grab } => {
                if timeline::trim_clip(&mut self.project, id, None, Some(orig + (t - grab))).is_ok() {
                    if let Some(c) = timeline::placed(&self.project).into_iter().find(|c| c.clip.id == id) {
                        self.seek(c.end - 0.001);
                    }
                }
            }
            Drag::ClipMove { .. } => {}
            Drag::CamMove { id, grab } => {
                let _ = timeline::move_camera_clip(&mut self.project, id, t - grab);
            }
            Drag::CamStart { id, end } => {
                let _ = timeline::resize_camera_clip(&mut self.project, id, t.min(end - 0.5), end);
            }
            Drag::CamEnd { id, start } => {
                let _ = timeline::resize_camera_clip(&mut self.project, id, start, t.max(start + 0.5).min(total + 1.0));
            }
            Drag::CamKey { id, index } => {
                if let Some(c) = self.project.camera.iter_mut().find(|c| c.id == id) {
                    let lo = c.keys.get(index.wrapping_sub(1)).map(|k| k.t + 0.05).unwrap_or(0.0);
                    let hi = c.keys.get(index + 1).map(|k| k.t - 0.05).unwrap_or(f32::MAX);
                    if let Some(k) = c.keys.get_mut(index) {
                        k.t = (t - c.start).clamp(lo, hi.max(lo));
                        self.time = c.start + k.t;
                        self.requested_time = None;
                    }
                }
            }
            Drag::DevMove { id, grab } => {
                if let Some(c) = self.project.device.iter_mut().find(|c| c.id == id) {
                    c.start = (t - grab).clamp(0.0, (total - c.duration()).max(0.0));
                }
            }
            Drag::TextMove { id, grab } => {
                if let Some(len) = self.project.texts.iter().find(|c| c.id == id).map(|c| c.duration) {
                    let start = (t - grab).clamp(0.0, (total - len).max(0.0));
                    let _ = timeline::place_text(&mut self.project, id, start, start + len);
                }
            }
            Drag::TextStart { id, end } => {
                let _ = timeline::place_text(&mut self.project, id, t.min(end - 0.8), end);
            }
            Drag::TextEnd { id, start } => {
                let _ = timeline::place_text(&mut self.project, id, start, t.max(start + 0.8));
            }
            Drag::Marker { id } => {
                if let Some(tg) = self.project.director.targets.iter_mut().find(|x| x.id == id) {
                    tg.time = t;
                }
                self.project.director.targets.sort_by(|a, b| a.time.total_cmp(&b.time));
                self.seek(t);
            }
        }
    }

    fn paint_timeline(&self, ui: &Ui, tl: &Tl, hover: Hit, drop_x: Option<f32>) {
        let p = ui.painter_at(Rect::from_min_max(tl.ruler.min - Vec2::new(0.0, 2.0), tl.text.max + Vec2::new(0.0, 2.0)));
        let dur = tl.dur;

        // Row labels
        for (r, label) in [(tl.video, "Vídeo"), (tl.camera, "Cámara"), (tl.device, "Móvil"), (tl.text, "Texto")] {
            p.text(Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, label, regular(11.5), TEXT_3);
        }

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

        // Video clips with their filmstrip
        p.rect_filled(Rect::from_min_max(Pos2::new(tl.x0, tl.video.top()), Pos2::new(tl.x1, tl.video.bottom())), Rounding::same(7.0), TRACK);
        for c in timeline::placed(&self.project) {
            let r = Rect::from_min_max(Pos2::new(tl.x(c.start) + 1.0, tl.video.top()), Pos2::new(tl.x(c.end) - 1.0, tl.video.bottom()));
            if r.width() < 1.0 {
                continue;
            }
            let selected = self.selection == Selection::Clip(c.clip.id);
            p.rect_filled(r, Rounding::same(6.0), CLIP.linear_multiply(0.7));
            let media = self.project.media(c.clip.media);
            if let (Some(m), Some(view)) = (media, self.media_views.get(&c.clip.media)) {
                if !view.thumbs.is_empty() {
                    let tp = ui.painter_at(r.shrink(1.0));
                    let tile_w = (tl.video.height() * view.thumb_aspect).max(8.0);
                    let mut x = r.left();
                    while x < r.right() {
                        let src = c.clip.source_in + (tl.t(x + tile_w * 0.5) - c.start);
                        let idx = ((src / m.info.duration.max(0.1)) * view.thumbs.len() as f32).floor().max(0.0) as usize;
                        let tex = &view.thumbs[idx.min(view.thumbs.len() - 1)];
                        tp.image(tex.id(), Rect::from_min_size(Pos2::new(x, r.top()), Vec2::new(tile_w, r.height())), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        x += tile_w;
                    }
                }
            }
            p.rect_stroke(r.expand(1.0), Rounding::same(7.0), Stroke::new(2.0_f32, WINDOW));
            let edge_hover = matches!(hover, Hit::ClipIn(id) | Hit::ClipOut(id) if id == c.clip.id);
            if selected || edge_hover {
                p.rect_stroke(r, Rounding::same(6.0), Stroke::new(2.0_f32, YELLOW));
                for x in [r.left(), r.right()] {
                    p.rect_filled(Rect::from_center_size(Pos2::new(x, r.center().y), Vec2::new(5.0, 24.0)), Rounding::same(2.5), YELLOW);
                }
            } else {
                p.rect_stroke(r, Rounding::same(6.0), Stroke::new(1.0_f32, Color32::from_white_alpha(60)));
            }
            let name = media
                .and_then(|m| std::path::Path::new(&m.info.path).file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let g = p.layout_no_wrap(name, regular(11.0), Color32::WHITE);
            let label = Rect::from_min_size(r.min + Vec2::new(6.0, 5.0), g.size() + Vec2::new(10.0, 4.0));
            if label.right() < r.right() - 4.0 {
                p.rect_filled(label, Rounding::same(4.0), Color32::from_black_alpha(140));
                p.galley(label.min + Vec2::new(5.0, 2.0), g, Color32::WHITE);
            }
        }
        if let Some(x) = drop_x {
            p.line_segment([Pos2::new(x, tl.video.top() - 3.0), Pos2::new(x, tl.video.bottom() + 3.0)], Stroke::new(3.0_f32, ACCENT));
        }

        // Touches
        let ty = tl.video.bottom() - 9.0;
        for t in timeline::timeline_touches(&self.project) {
            let x = tl.x(t.time);
            if t.kind == TouchKind::Swipe {
                let x2 = tl.x(t.time + t.duration).max(x + 4.0);
                p.line_segment([Pos2::new(x, ty), Pos2::new(x2, ty)], Stroke::new(4.0_f32, Color32::from_white_alpha(170)));
            }
            p.circle_filled(Pos2::new(x, ty), 3.5, Color32::WHITE);
            p.circle_stroke(Pos2::new(x, ty), 3.5, Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
        }

        // Camera clips
        let cam_row = Rect::from_min_max(Pos2::new(tl.x0, tl.camera.top()), Pos2::new(tl.x1, tl.camera.bottom()));
        p.rect_filled(cam_row, Rounding::same(7.0), TRACK);
        if self.project.camera.is_empty() {
            p.text(cam_row.center(), Align2::CENTER_CENTER, "Sin movimientos · + Zoom, + Movimiento, Director o doble clic aquí", regular(12.0), TEXT_3);
        }
        for c in &self.project.camera {
            let r = Rect::from_min_max(Pos2::new(tl.x(c.start), tl.camera.top() + 2.0), Pos2::new(tl.x(c.end()), tl.camera.bottom() - 2.0));
            let selected = matches!(self.selection, Selection::Camera { id, .. } if id == c.id);
            let hovered = matches!(hover, Hit::CamBody(id, _) | Hit::CamStart(id) | Hit::CamEnd(id) | Hit::CamKey(id, _) if id == c.id);
            let base = camera_color(c.kind);
            p.rect_filled(r, Rounding::same(6.0), if selected || hovered { lerp_color(base, Color32::WHITE, 0.12) } else { base });
            if selected {
                p.rect_stroke(r, Rounding::same(6.0), Stroke::new(2.0_f32, YELLOW));
            }
            let label = match c.kind {
                CameraKind::Zoom | CameraKind::Auto => format!("{:.1}×", c.keys.iter().map(|k| k.zoom).fold(1.0, f32::max)).replace('.', ","),
                k => k.label().to_string(),
            };
            if r.width() > 34.0 {
                p.text(r.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, label, medium(11.0), Color32::WHITE);
            }
            if c.kind == CameraKind::Auto && r.width() > 80.0 {
                p.text(r.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, "auto", regular(10.5), Color32::from_white_alpha(150));
            }
            if !matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) {
                for (i, k) in c.keys.iter().enumerate() {
                    let sel = self.selection == Selection::Camera { id: c.id, key: Some(i) };
                    let fill = if sel { YELLOW } else { Color32::WHITE };
                    diamond(&p, Pos2::new(tl.x(c.start + k.t), r.center().y), if sel { 6.0 } else { 4.5 }, fill, Color32::from_black_alpha(140));
                }
            }
        }

        // Device motion
        let dev_row = Rect::from_min_max(Pos2::new(tl.x0, tl.device.top()), Pos2::new(tl.x1, tl.device.bottom()));
        p.rect_filled(dev_row, Rounding::same(6.0), TRACK);
        if self.project.device.is_empty() {
            p.text(dev_row.center(), Align2::CENTER_CENTER, "Sin movimientos del móvil · menú «Móvil»", regular(11.5), TEXT_3);
        }
        for c in &self.project.device {
            let r = Rect::from_min_max(Pos2::new(tl.x(c.start), tl.device.top() + 2.0), Pos2::new(tl.x(c.end()).max(tl.x(c.start) + 6.0), tl.device.bottom() - 2.0));
            let selected = matches!(self.selection, Selection::Device { id, .. } if id == c.id);
            p.rect_filled(r, Rounding::same(5.0), if selected { lerp_color(DEVICE_COLOR, Color32::WHITE, 0.15) } else { DEVICE_COLOR });
            if selected {
                p.rect_stroke(r, Rounding::same(5.0), Stroke::new(2.0_f32, YELLOW));
            }
            if r.width() > 50.0 {
                let g = p.layout_no_wrap(c.name.clone(), regular(10.5), Color32::WHITE);
                if g.size().x < r.width() - 12.0 {
                    p.galley(Pos2::new(r.left() + 7.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
                }
            }
            for (i, k) in c.keys.iter().enumerate() {
                let sel = self.selection == Selection::Device { id: c.id, key: Some(i) };
                diamond(&p, Pos2::new(tl.x(c.start + k.t), r.center().y), if sel { 5.5 } else { 3.5 }, if sel { YELLOW } else { Color32::WHITE }, Color32::from_black_alpha(140));
            }
        }

        // Titles
        let text_row = Rect::from_min_max(Pos2::new(tl.x0, tl.text.top()), Pos2::new(tl.x1, tl.text.bottom()));
        p.rect_filled(text_row, Rounding::same(6.0), TRACK);
        if self.project.texts.is_empty() {
            p.text(text_row.center(), Align2::CENTER_CENTER, "Sin textos · + Texto, T o doble clic aquí", regular(11.5), TEXT_3);
        }
        for c in &self.project.texts {
            let r = Rect::from_min_max(Pos2::new(tl.x(c.start), tl.text.top() + 2.0), Pos2::new(tl.x(c.end()).max(tl.x(c.start) + 6.0), tl.text.bottom() - 2.0));
            let selected = self.selection == Selection::Text(c.id);
            let hovered = matches!(hover, Hit::TextBody(id, _) | Hit::TextStart(id) | Hit::TextEnd(id) if id == c.id);
            p.rect_filled(r, Rounding::same(5.0), if selected || hovered { lerp_color(TEXT_COLOR, Color32::WHITE, 0.15) } else { TEXT_COLOR });
            if selected {
                p.rect_stroke(r, Rounding::same(5.0), Stroke::new(2.0_f32, YELLOW));
            }
            let first = c.title.lines().map(str::trim).find(|l| !l.is_empty()).or(c.subtitle.lines().map(str::trim).find(|l| !l.is_empty()));
            let (label, col) = match first {
                Some(l) => (l.to_string(), Color32::WHITE),
                None => ("Sin texto".to_string(), Color32::from_white_alpha(150)),
            };
            if r.width() > 30.0 {
                let tp = ui.painter_at(r.shrink2(Vec2::new(6.0, 0.0)));
                let g = tp.layout_no_wrap(label, regular(10.5), col);
                tp.galley(Pos2::new(r.left() + 7.0, r.center().y - g.size().y / 2.0), g, col);
            }
        }

        // Director markers
        for (n, t) in self.project.director.targets.iter().enumerate() {
            let c = Pos2::new(tl.x(t.time), tl.ruler.center().y + 1.0);
            let sel = self.selection == Selection::Target(t.id);
            p.circle_filled(c, 8.0, if sel { YELLOW } else { DIRECTOR_COLOR });
            p.text(c, Align2::CENTER_CENTER, format!("{}", n + 1), medium(10.0), Color32::BLACK);
        }

        // Playhead
        let x = tl.x(self.time);
        p.line_segment([Pos2::new(x, tl.ruler.top() + 4.0), Pos2::new(x, tl.text.bottom())], Stroke::new(1.5_f32, RED));
        let head = vec![
            Pos2::new(x - 5.0, tl.ruler.top()),
            Pos2::new(x + 5.0, tl.ruler.top()),
            Pos2::new(x + 5.0, tl.ruler.top() + 6.0),
            Pos2::new(x, tl.ruler.top() + 11.0),
            Pos2::new(x - 5.0, tl.ruler.top() + 6.0),
        ];
        p.add(egui::Shape::convex_polygon(head, RED, Stroke::NONE));
    }
}
