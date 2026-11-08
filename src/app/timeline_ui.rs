//! The timeline: video clips (trim, reorder, split), camera clips with keyframes, device
//! motion, titles, pictures, the three sound tracks, Director markers and the playhead.

use super::*;

/// Width of the track headers: name and, on the sound tracks, their volume.
const LABEL_W: f32 = 150.0;
const MOVE_COLOR: Color32 = Color32::from_rgb(255, 159, 10);
const DIRECTOR_COLOR: Color32 = Color32::from_rgb(90, 200, 250);
const DEVICE_COLOR: Color32 = Color32::from_rgb(52, 170, 90);
const TEXT_COLOR: Color32 = Color32::from_rgb(235, 68, 120);
/// Height of one lane of the pictures and sound tracks (overlapping clips stack in lanes).
const LANE_H: f32 = 22.0;

/// The tracks where clips may overlap: pictures and the three sound tracks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Layer {
    Images,
    Audio(AudioTrack),
}

const LAYERS: [Layer; 4] = [Layer::Images, Layer::Audio(AudioTrack::Effects), Layer::Audio(AudioTrack::Voice), Layer::Audio(AudioTrack::Music)];

impl Layer {
    fn label(self) -> &'static str {
        match self {
            Layer::Images => "Imágenes",
            Layer::Audio(AudioTrack::Effects) => "Efectos",
            Layer::Audio(AudioTrack::Voice) => "Voz",
            Layer::Audio(AudioTrack::Music) => "Música",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Layer::Images => Color32::from_rgb(94, 92, 230),
            Layer::Audio(AudioTrack::Effects) => Color32::from_rgb(214, 150, 0),
            Layer::Audio(AudioTrack::Voice) => Color32::from_rgb(0, 168, 160),
            Layer::Audio(AudioTrack::Music) => Color32::from_rgb(176, 80, 220),
        }
    }

    fn empty_hint(self) -> &'static str {
        match self {
            Layer::Images => "Sin imágenes · arrastra un PNG a la ventana o haz doble clic aquí",
            Layer::Audio(AudioTrack::Effects) => "Sin efectos de sonido · doble clic aquí",
            Layer::Audio(AudioTrack::Voice) => "Sin voz en off · doble clic aquí",
            Layer::Audio(AudioTrack::Music) => "Sin música · doble clic aquí",
        }
    }
}

/// One pictures or sound track laid out: its clips (id, start, end, lane).
struct Row {
    layer: Layer,
    rect: Rect,
    items: Vec<(u64, f32, f32, usize)>,
}

impl Row {
    fn lane_rect(&self, tl: &Tl, start: f32, end: f32, lane: usize) -> Rect {
        let top = self.rect.top() + 1.0 + lane as f32 * LANE_H;
        Rect::from_min_max(Pos2::new(tl.x(start), top + 1.0), Pos2::new(tl.x(end).max(tl.x(start) + 6.0), top + LANE_H - 1.0))
    }
}

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
    ItemMove { layer: Layer, id: u64, grab: f32 },
    ItemStart { layer: Layer, id: u64, end: f32 },
    ItemEnd { layer: Layer, id: u64, start: f32 },
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
    ItemStart(Layer, u64),
    ItemEnd(Layer, u64),
    ItemBody(Layer, u64, f32),
    LayerEmpty(Layer),
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
    rows: Vec<Row>,
    /// Bottom of the last track.
    bottom: f32,
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

/// The selection for a clip of a pictures or sound track.
fn select(layer: Layer, id: u64) -> Selection {
    match layer {
        Layer::Images => Selection::Image(id),
        Layer::Audio(_) => Selection::Audio(id),
    }
}

fn diamond(p: &egui::Painter, c: Pos2, r: f32, fill: Color32, stroke: Color32) {
    let pts = vec![Pos2::new(c.x, c.y - r), Pos2::new(c.x + r, c.y), Pos2::new(c.x, c.y + r), Pos2::new(c.x - r, c.y)];
    p.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.0_f32, stroke)));
}

impl RecOrdingApp {
    pub(super) fn timeline(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("timeline")
            .resizable(true)
            .default_height(400.0)
            .height_range(240.0..=760.0)
            .frame(
                egui::Frame::none()
                    .fill(WINDOW)
                    .inner_margin(Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 12.0 })
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(16, 16, 18))),
            )
            .show(ctx, |ui| {
                self.timeline_header(ui);
                ui.add_space(4.0);
                // The tracks scroll when they do not fit (many overlapping sounds or pictures).
                let width = ui.available_width() - 12.0;
                let height = self.tracks_height();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let (body, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                    self.timeline_body(ui, body);
                });
            });
    }

    /// The clips of a pictures or sound track: (id, start, end).
    fn layer_items(&self, layer: Layer) -> Vec<(u64, f32, f32)> {
        match layer {
            Layer::Images => self.project.images.iter().map(|c| (c.id, c.start, c.end())).collect(),
            Layer::Audio(t) => self.project.audio.iter().filter(|c| c.track == t).map(|c| (c.id, c.start, c.end())).collect(),
        }
    }

    /// The pictures and sound tracks laid out from `top`, with their lanes.
    fn layer_rows(&self, left: f32, width: f32, mut top: f32) -> Vec<Row> {
        LAYERS
            .iter()
            .map(|&layer| {
                let items = self.layer_items(layer);
                let (lanes, n) = timeline::lanes(&items);
                let rect = Rect::from_min_size(Pos2::new(left, top + 6.0), Vec2::new(width, n as f32 * LANE_H + 2.0));
                top = rect.bottom();
                Row { layer, rect, items: items.iter().zip(lanes).map(|(&(id, a, b), l)| (id, a, b, l)).collect() }
            })
            .collect()
    }

    /// Height of all the tracks and the ruler.
    fn tracks_height(&self) -> f32 {
        let fixed = 18.0 + 6.0 + 58.0 + 8.0 + 30.0 + 6.0 + 24.0 + 6.0 + 24.0;
        self.layer_rows(0.0, 1.0, fixed).last().map(|r| r.rect.bottom()).unwrap_or(fixed) + 4.0
    }

    fn timeline_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // Adding things at the cursor.
            tool_group(ui, |ui| {
                if icon_button(ui, Icon::Zoom, "Añadir zoom en el cursor", "Z", false, true).clicked() {
                    self.add_zoom_at(self.time);
                }
                if icon_button(ui, Icon::Camera, "Añadir movimiento de cámara en el cursor", "", false, true).clicked() {
                    match timeline::add_move(&mut self.project, self.time) {
                        Ok(id) => self.selection = Selection::Camera { id, key: Some(1) },
                        Err(e) => self.toast(e),
                    }
                }
                if icon_button(ui, Icon::Text, "Añadir texto en el cursor", "T", false, true).clicked() {
                    self.add_text_at(self.time);
                }
                let phone = icon_button(ui, Icon::PhoneMove, "Añadir movimiento del móvil…", "", false, true);
                let popup = ui.make_persistent_id("device-presets");
                if phone.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(popup));
                }
                egui::popup::popup_above_or_below_widget(ui, popup, &phone, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(220.0);
                    ui.label(RichText::new("Movimiento del móvil en el cursor").color(TEXT_2).font(regular(12.0)));
                    ui.label(RichText::new("Los giros junto a un cambio de clip se centran en él").color(TEXT_3).font(regular(11.5)));
                    for preset in DevicePreset::ALL {
                        if ui.button(preset.label()).clicked() {
                            let id = timeline::add_device_preset(&mut self.project, preset, self.time);
                            self.selection = Selection::Device { id, key: None };
                        }
                    }
                });
            });
            tool_group(ui, |ui| {
                if icon_button(ui, Icon::Image, "Añadir una imagen en el cursor (PNG con transparencia)", "", false, true).clicked() {
                    self.pick_picture(self.time);
                }
                if icon_button(ui, Icon::Effect, "Añadir un efecto de sonido en el cursor", "", false, true).clicked() {
                    self.pick_sound(Some(AudioTrack::Effects), self.time);
                }
                if icon_button(ui, Icon::Mic, "Añadir voz en off en el cursor", "", false, true).clicked() {
                    self.pick_sound(Some(AudioTrack::Voice), self.time);
                }
                if icon_button(ui, Icon::Music, "Añadir música en el cursor", "", false, true).clicked() {
                    self.pick_sound(Some(AudioTrack::Music), self.time);
                }
            });
            tool_group(ui, |ui| {
                if icon_button(ui, Icon::Director, "Director: marca lo que debe verse y se generan los planos", "", self.director_mode, true).clicked() {
                    self.director_mode = !self.director_mode;
                    self.selection = Selection::None;
                    if self.playing {
                        self.toggle_play();
                    }
                }
            });
            // Editing the video clips.
            tool_group(ui, |ui| {
                if icon_button(ui, Icon::Split, "Dividir el clip en el cursor", "Ctrl+B", false, true).clicked() {
                    self.split_here();
                }
                if icon_button(ui, Icon::Freeze, "Congelar el fotograma del cursor 2 s", "", false, true).clicked() {
                    match timeline::freeze_frame(&mut self.project, self.time, 2.0) {
                        Ok(still) => self.selection = Selection::Clip(still),
                        Err(e) => self.toast(e),
                    }
                    self.requested_time = None;
                }
                if icon_button(ui, Icon::AddVideo, "Añadir un vídeo al final", "", false, true).clicked() {
                    self.pick_video();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("Duración {}", fmt_time(timeline::duration(&self.project)))).color(TEXT_2).font(regular(12.5)));
                ui.add_space(4.0);
                // Right to left here: Redo goes first to end up on the right.
                tool_group(ui, |ui| {
                    if icon_button(ui, Icon::Redo, "Rehacer", "Ctrl+Mayús+Z", false, !self.redo.is_empty()).clicked() {
                        self.undo_redo(false);
                    }
                    if icon_button(ui, Icon::Undo, "Deshacer", "Ctrl+Z", false, !self.undo.is_empty()).clicked() {
                        self.undo_redo(true);
                    }
                });
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
        for row in &tl.rows {
            if !row.rect.expand2(Vec2::new(0.0, 2.0)).contains(pos) {
                continue;
            }
            let lane = ((pos.y - row.rect.top() - 1.0) / LANE_H).floor().max(0.0) as usize;
            for &(id, start, end, l) in row.items.iter().rev().filter(|i| i.3 == lane) {
                let r = row.lane_rect(tl, start, end, l);
                let (a, b) = (r.left(), r.right());
                if pos.x < a - EDGE || pos.x > b + EDGE {
                    continue;
                }
                if (pos.x - b).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::ItemEnd(row.layer, id);
                }
                if (pos.x - a).abs() <= EDGE && b - a > 3.0 * EDGE {
                    return Hit::ItemStart(row.layer, id);
                }
                if pos.x >= a && pos.x <= b {
                    return Hit::ItemBody(row.layer, id, tl.t(pos.x) - start);
                }
            }
            return Hit::LayerEmpty(row.layer);
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
        let rows = self.layer_rows(body.left(), body.width(), text.bottom());
        let bottom = rows.last().map(|r| r.rect.bottom()).unwrap_or(text.bottom());
        let tl = Tl { x0: body.left() + LABEL_W, x1: body.right() - 2.0, dur, ruler, video, camera, device, text, rows, bottom };

        let resp = ui.interact(Rect::from_min_max(ruler.min, Pos2::new(body.right(), bottom)), Id::new("timeline"), Sense::click_and_drag());
        let hover_hit = resp.hover_pos().map(|p| self.hit(&tl, p)).unwrap_or(Hit::None);
        match self.drag {
            Some(Drag::TrimIn { .. } | Drag::TrimOut { .. } | Drag::CamStart { .. } | Drag::CamEnd { .. } | Drag::TextStart { .. } | Drag::TextEnd { .. } | Drag::ItemStart { .. } | Drag::ItemEnd { .. }) => {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
            }
            Some(Drag::CamMove { .. } | Drag::DevMove { .. } | Drag::TextMove { .. } | Drag::ItemMove { .. } | Drag::ClipMove { .. } | Drag::CamKey { .. } | Drag::Marker { .. }) => {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing)
            }
            _ => match hover_hit {
                Hit::ClipIn(_) | Hit::ClipOut(_) | Hit::CamStart(_) | Hit::CamEnd(_) | Hit::TextStart(_) | Hit::TextEnd(_) | Hit::ItemStart(..) | Hit::ItemEnd(..) => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
                }
                Hit::CamBody(..) | Hit::DevBody(..) | Hit::TextBody(..) | Hit::ItemBody(..) | Hit::CamKey(..) | Hit::DevKey(..) | Hit::Marker(_) => {
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
                    Hit::ItemBody(layer, id, grab) => Some(Drag::ItemMove { layer, id, grab }),
                    Hit::ItemStart(layer, id) => self.layer_items(layer).iter().find(|i| i.0 == id).map(|i| Drag::ItemStart { layer, id, end: i.2 }),
                    Hit::ItemEnd(layer, id) => self.layer_items(layer).iter().find(|i| i.0 == id).map(|i| Drag::ItemEnd { layer, id, start: i.1 }),
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
                    Some(Drag::ItemMove { layer, id, .. } | Drag::ItemStart { layer, id, .. } | Drag::ItemEnd { layer, id, .. }) => self.selection = select(layer, id),
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
                // A sound dragged onto another sound track moves to it.
                if let Drag::ItemMove { layer: Layer::Audio(from), id, grab } = d {
                    let over = tl.rows.iter().find(|r| pos.y >= r.rect.top() && pos.y < r.rect.bottom() + 6.0).map(|r| r.layer);
                    if let Some(Layer::Audio(to)) = over.filter(|l| *l != Layer::Audio(from)) {
                        if let Some(c) = self.project.audio.iter_mut().find(|c| c.id == id) {
                            c.track = to;
                            self.drag = Some(Drag::ItemMove { layer: Layer::Audio(to), id, grab });
                        }
                    }
                }
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
                    Hit::ItemStart(layer, id) | Hit::ItemEnd(layer, id) | Hit::ItemBody(layer, id, _) => {
                        self.selection = select(layer, id);
                        self.seek(t);
                    }
                    Hit::VideoEmpty | Hit::CamEmpty | Hit::DevEmpty | Hit::TextEmpty | Hit::LayerEmpty(_) => {
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
                    Hit::LayerEmpty(Layer::Images) => self.pick_picture(tl.t(pos.x)),
                    Hit::LayerEmpty(Layer::Audio(track)) => self.pick_sound(Some(track), tl.t(pos.x)),
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
        self.track_volumes(ui, &tl);
    }

    /// The volume of each sound track, in its header. Added after the timeline's own area, so
    /// the sliders get the mouse first.
    fn track_volumes(&mut self, ui: &mut Ui, tl: &Tl) {
        for row in &tl.rows {
            let Layer::Audio(track) = row.layer else { continue };
            let y = row.rect.top() + 1.0 + LANE_H / 2.0;
            let rect = Rect::from_min_size(Pos2::new(row.rect.left() + 56.0, y - 9.0), Vec2::new(54.0, 18.0));
            let volume = self.project.mix.track_mut(track);
            let builder = UiBuilder::new().id_salt(("track-volume", track)).max_rect(rect).layout(egui::Layout::left_to_right(Align::Center));
            let mut c = ui.new_child(builder);
            c.spacing_mut().slider_width = rect.width();
            c.spacing_mut().interact_size.y = 18.0;
            c.add(egui::Slider::new(volume, 0.0..=2.0).show_value(false)).on_hover_text(format!("Volumen de {}", track.label().to_lowercase()));
            let text = format!("{:.0} %", *volume * 100.0);
            ui.painter().text(Pos2::new(tl.x0 - 8.0, y), Align2::RIGHT_CENTER, text, regular(10.5), TEXT_3);
        }
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
                let start = timeline::snap_to_mark(&self.project, t - grab, 0.12);
                let _ = timeline::move_camera_clip(&mut self.project, id, start);
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
                if let Some(len) = self.project.device.iter().find(|c| c.id == id).map(|c| c.duration()) {
                    let start = timeline::snap_to_mark(&self.project, (t - grab).clamp(0.0, (total - len).max(0.0)), 0.12);
                    if let Some(c) = self.project.device.iter_mut().find(|c| c.id == id) {
                        c.start = start;
                    }
                }
            }
            Drag::TextMove { id, grab } => {
                if let Some(len) = self.project.texts.iter().find(|c| c.id == id).map(|c| c.duration) {
                    let start = timeline::snap_to_mark(&self.project, (t - grab).clamp(0.0, (total - len).max(0.0)), 0.12);
                    let _ = timeline::place_text(&mut self.project, id, start, start + len);
                }
            }
            Drag::TextStart { id, end } => {
                let _ = timeline::place_text(&mut self.project, id, t.min(end - 0.8), end);
            }
            Drag::TextEnd { id, start } => {
                let _ = timeline::place_text(&mut self.project, id, start, t.max(start + 0.8));
            }
            Drag::ItemMove { layer, id, grab } => {
                let start = timeline::snap_to_mark(&self.project, (t - grab).max(0.0), 0.12);
                match layer {
                    Layer::Images => {
                        if let Some(len) = self.project.images.iter().find(|c| c.id == id).map(|c| c.duration) {
                            let _ = timeline::place_image(&mut self.project, id, start, start + len);
                        }
                    }
                    Layer::Audio(_) => {
                        let _ = timeline::move_audio(&mut self.project, id, start);
                    }
                }
            }
            Drag::ItemStart { layer, id, end } => match layer {
                Layer::Images => {
                    let _ = timeline::place_image(&mut self.project, id, t.min(end - 0.2), end);
                }
                Layer::Audio(_) => {
                    let _ = timeline::trim_audio(&mut self.project, id, t.min(end - 0.1), end);
                }
            },
            Drag::ItemEnd { layer, id, start } => match layer {
                Layer::Images => {
                    let _ = timeline::place_image(&mut self.project, id, start, t.max(start + 0.2));
                }
                Layer::Audio(_) => {
                    let _ = timeline::trim_audio(&mut self.project, id, start, t.max(start + 0.1));
                }
            },
            Drag::Marker { id } => {
                if let Some(tg) = self.project.director.targets.iter_mut().find(|x| x.id == id) {
                    tg.time = t;
                }
                self.project.director.targets.sort_by(|a, b| a.time.total_cmp(&b.time));
                self.seek(t);
            }
        }
    }

    /// A pictures or sound track: its clips in their lanes, with the waveform or a thumbnail.
    fn paint_layer(&self, ui: &Ui, p: &egui::Painter, tl: &Tl, row: &Row, hover: Hit) {
        let track = Rect::from_min_max(Pos2::new(tl.x0, row.rect.top()), Pos2::new(tl.x1, row.rect.bottom()));
        p.rect_filled(track, Rounding::same(6.0), TRACK);
        if row.items.is_empty() {
            p.text(track.center(), Align2::CENTER_CENTER, row.layer.empty_hint(), regular(11.5), TEXT_3);
        }
        let base = row.layer.color();
        for &(id, start, end, lane) in &row.items {
            let r = row.lane_rect(tl, start, end, lane);
            let selected = self.selection == select(row.layer, id);
            let hovered = matches!(hover, Hit::ItemBody(l, i, _) | Hit::ItemStart(l, i) | Hit::ItemEnd(l, i) if l == row.layer && i == id);
            p.rect_filled(r, Rounding::same(5.0), if selected || hovered { lerp_color(base, Color32::WHITE, 0.15) } else { base });
            let inner = ui.painter_at(r.shrink(1.0));
            let mut text_x = r.left() + 7.0;
            let name = match row.layer {
                Layer::Images => {
                    let Some(c) = self.project.images.iter().find(|c| c.id == id) else { continue };
                    if let Some(Some(tex)) = self.image_thumbs.get(&c.path) {
                        let h = r.height() - 4.0;
                        let [tw, th] = tex.size();
                        let w = (h * tw as f32 / th.max(1) as f32).min(h * 2.0);
                        let pic = Rect::from_min_size(Pos2::new(r.left() + 4.0, r.top() + 2.0), Vec2::new(w, h));
                        inner.rect_filled(pic, Rounding::same(2.0), Color32::from_black_alpha(60));
                        inner.image(tex.id(), pic, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        text_x = pic.right() + 6.0;
                    }
                    std::path::Path::new(&c.path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                }
                Layer::Audio(_) => {
                    let Some(c) = self.project.audio.iter().find(|c| c.id == id) else { continue };
                    // The waveform, repeated when the sound loops.
                    if let Some(Some(peaks)) = self.peaks.get(&c.path) {
                        let per = crate::audio::PEAKS_PER_SECOND;
                        let mid = r.center().y;
                        let half = r.height() * 0.42;
                        let mut x = r.left().max(tl.x0);
                        while x < r.right() {
                            let mut at = c.offset + (tl.t(x) - c.start);
                            if c.looped && c.length > 0.0 {
                                at = at.rem_euclid(c.length);
                            }
                            let v = peaks.get((at * per) as usize).copied().unwrap_or(0.0) * (c.volume * self.project.mix.track(c.track)).clamp(0.0, 1.5) / 1.5;
                            let h = (v * half).max(0.5);
                            inner.line_segment([Pos2::new(x, mid - h), Pos2::new(x, mid + h)], Stroke::new(1.0_f32, Color32::from_white_alpha(110)));
                            x += 2.0;
                        }
                    }
                    if c.looped {
                        // Where the file starts over.
                        let mut k = c.start + (c.length - c.offset);
                        while k < c.end() && c.length > 0.05 {
                            let x = tl.x(k);
                            inner.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(1.0_f32, Color32::from_black_alpha(90)));
                            k += c.length;
                        }
                    }
                    c.name()
                }
            };
            if r.right() - text_x > 24.0 {
                let g = inner.layout_no_wrap(name, regular(10.5), Color32::WHITE);
                let label = Rect::from_min_size(Pos2::new(text_x - 3.0, r.center().y - g.size().y / 2.0 - 1.0), g.size() + Vec2::new(6.0, 2.0));
                inner.rect_filled(label, Rounding::same(3.0), Color32::from_black_alpha(90));
                inner.galley(Pos2::new(text_x, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
            }
            if selected {
                p.rect_stroke(r, Rounding::same(5.0), Stroke::new(2.0_f32, YELLOW));
            }
        }
    }

    fn paint_timeline(&self, ui: &Ui, tl: &Tl, hover: Hit, drop_x: Option<f32>) {
        let p = ui.painter_at(Rect::from_min_max(tl.ruler.min - Vec2::new(0.0, 2.0), Pos2::new(tl.text.right(), tl.bottom + 2.0)));
        let dur = tl.dur;

        // Row labels
        for (r, label) in [(tl.video, "Vídeo"), (tl.camera, "Cámara"), (tl.device, "Móvil"), (tl.text, "Texto")] {
            p.text(Pos2::new(r.left(), r.center().y), Align2::LEFT_CENTER, label, regular(11.5), TEXT_3);
        }
        for row in &tl.rows {
            p.text(Pos2::new(row.rect.left(), row.rect.top() + 1.0 + LANE_H / 2.0), Align2::LEFT_CENTER, row.layer.label(), regular(11.5), TEXT_3);
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
                        let src = timeline::source_time(&c, tl.t(x + tile_w * 0.5));
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

        // Screen changes (things snap to them)
        for m in timeline::timeline_marks(&self.project) {
            let x = tl.x(m);
            let tip = vec![Pos2::new(x - 4.0, tl.video.top() - 1.0), Pos2::new(x + 4.0, tl.video.top() - 1.0), Pos2::new(x, tl.video.top() + 5.0)];
            p.add(egui::Shape::convex_polygon(tip, Color32::from_white_alpha(210), Stroke::new(1.0_f32, Color32::from_black_alpha(120))));
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
            p.text(cam_row.center(), Align2::CENTER_CENTER, "Sin movimientos de cámara · usa los botones de arriba o haz doble clic aquí", regular(12.0), TEXT_3);
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
            p.text(dev_row.center(), Align2::CENTER_CENTER, "Sin movimientos del móvil · botón del móvil, arriba", regular(11.5), TEXT_3);
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
            p.text(text_row.center(), Align2::CENTER_CENTER, "Sin textos · pulsa T o haz doble clic aquí", regular(11.5), TEXT_3);
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

        // Pictures and sounds
        for row in &tl.rows {
            self.paint_layer(ui, &p, tl, row, hover);
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
        p.line_segment([Pos2::new(x, tl.ruler.top() + 4.0), Pos2::new(x, tl.bottom)], Stroke::new(1.5_f32, RED));
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
