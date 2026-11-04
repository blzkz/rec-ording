//! The viewer: the rendered frame, framing a zoom or keyframe by clicking, drawing Director
//! targets, and the transport controls.

use super::*;

/// What the viewer lets the user point at, besides watching.
enum Pointing {
    /// Choose where a zoom (or keyframe) looks: the zoom level and focus being edited.
    Focus { zoom: f32, x: f32, y: f32 },
    /// Draw Director targets.
    Director,
    Nothing,
}

impl RecOrdingApp {
    pub(super) fn viewer(&mut self, ctx: &egui::Context) {
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

    /// What clicking on the viewer does right now.
    fn pointing(&self) -> Pointing {
        if self.playing {
            return Pointing::Nothing;
        }
        if self.director_mode {
            return Pointing::Director;
        }
        match self.selection {
            Selection::Camera { id, key } => {
                let Some(c) = self.project.camera.iter().find(|c| c.id == id) else { return Pointing::Nothing };
                let k = match (c.kind, key) {
                    (CameraKind::Zoom | CameraKind::Auto, _) if c.keys.len() == 4 => c.keys[1],
                    (_, Some(i)) => match c.keys.get(i) {
                        Some(k) => *k,
                        None => return Pointing::Nothing,
                    },
                    _ => return Pointing::Nothing,
                };
                if k.zoom > 1.02 || matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) {
                    Pointing::Focus { zoom: k.zoom, x: k.x, y: k.y }
                } else {
                    Pointing::Nothing
                }
            }
            _ => Pointing::Nothing,
        }
    }

    fn set_focus(&mut self, x: f32, y: f32) {
        let Selection::Camera { id, key } = self.selection else { return };
        let Some(c) = self.project.camera.iter_mut().find(|c| c.id == id) else { return };
        if matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) && c.keys.len() == 4 {
            for k in &mut c.keys[1..3] {
                k.x = x;
                k.y = y;
            }
            c.kind = CameraKind::Zoom;
        } else if let Some(k) = key.and_then(|i| c.keys.get_mut(i)) {
            k.x = x;
            k.y = y;
        }
    }

    fn canvas(&mut self, ui: &mut Ui, canvas: Rect) {
        let Some(media) = self.current_media().map(|m| m.info.clone()) else { return };
        let ppp = ui.ctx().pixels_per_point();
        let mut ow = (canvas.width() * ppp).round().max(16.0);
        let mut oh = (canvas.height() * ppp).round().max(16.0);
        let k = (1600.0 / ow.max(oh)).min(1.0);
        ow = (ow * k).round();
        oh = (oh * k).round();
        let (ow, oh) = (ow as usize, oh as usize);

        let pointing = self.pointing();
        // While framing or drawing, show the device straight on so clicks land where they look.
        let flat = !matches!(pointing, Pointing::Nothing);
        let (cam, pose, style) = if flat {
            (CameraValue::REST, PoseValue::REST, self.project.style.flattened())
        } else {
            (timeline::camera_at(&self.project, self.time), timeline::pose_at(&self.project, self.time), self.project.style.clone())
        };
        let touches = timeline::timeline_touches(&self.project);
        let bits = |v: [f32; 6]| v.map(f32::to_bits);
        let key = RenderKey {
            time: self.time,
            frame: self.shown.as_ref().map(|f| Arc::as_ptr(&f.mips) as usize).unwrap_or(0),
            style: style.clone(),
            cam: bits([cam.zoom, cam.x, cam.y, cam.tilt, cam.turn, cam.roll]),
            pose: bits([pose.dx, pose.dy, pose.scale, pose.tilt, pose.turn, pose.roll]),
            size: (ow, oh),
            video: (media.width, media.height),
            touches: touches.len(),
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
                    pose,
                    touches: &touches,
                    time: self.time,
                    since_start: self.time,
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
        if !flat {
            return;
        }

        // Mapping between the viewer and the recording, with the device seen straight on.
        let layout = Layout::new(ow as f32, oh as f32, &style, media.width as f32, media.height as f32, self.active_model().map(|m| &m.meta));
        let to_pt = move |x: f32, y: f32| Pos2::new(canvas.left() + x / ow as f32 * canvas.width(), canvas.top() + y / oh as f32 * canvas.height());
        let v = layout.video;
        let video_pt = move |u: f32, w: f32| to_pt(v.x0 + u * v.w(), v.y0 + w * v.h());
        let to_video = move |pos: Pos2| {
            let x = (pos.x - canvas.left()) / canvas.width() * ow as f32;
            let y = (pos.y - canvas.top()) / canvas.height() * oh as f32;
            (((x - v.x0) / v.w()).clamp(0.0, 1.0), ((y - v.y0) / v.h()).clamp(0.0, 1.0))
        };
        let p = ui.painter_at(canvas);
        let chip = |p: &egui::Painter, text: String| {
            let g = p.layout_no_wrap(text, regular(12.0), Color32::WHITE);
            let r = Rect::from_center_size(Pos2::new(canvas.center().x, canvas.top() + 20.0), g.size() + Vec2::new(20.0, 10.0));
            p.rect_filled(r, Rounding::same(12.0), Color32::from_black_alpha(170));
            p.galley(r.min + Vec2::new(10.0, 5.0), g, Color32::WHITE);
        };

        match pointing {
            Pointing::Focus { zoom, x, y } => {
                let cam = layout.camera(CamState { scale: zoom, fx: x, fy: y });
                let (ax, ay) = layout.to_scene(cam, 0.0, 0.0);
                let (bx, by) = layout.to_scene(cam, ow as f32, oh as f32);
                let region = Rect::from_two_pos(to_pt(ax, ay), to_pt(bx, by)).intersect(canvas);
                let dim = Color32::from_black_alpha(120);
                p.rect_filled(Rect::from_min_max(canvas.min, Pos2::new(canvas.right(), region.top())), 0.0, dim);
                p.rect_filled(Rect::from_min_max(Pos2::new(canvas.left(), region.bottom()), canvas.max), 0.0, dim);
                p.rect_filled(Rect::from_min_max(Pos2::new(canvas.left(), region.top()), Pos2::new(region.left(), region.bottom())), 0.0, dim);
                p.rect_filled(Rect::from_min_max(Pos2::new(region.right(), region.top()), Pos2::new(canvas.right(), region.bottom())), 0.0, dim);
                p.rect_stroke(region, Rounding::same(4.0), Stroke::new(2.0_f32, YELLOW));
                let focus = video_pt(x, y);
                p.circle_filled(focus, 8.0, Color32::from_black_alpha(80));
                p.circle_filled(focus, 6.0, YELLOW);
                p.circle_stroke(focus, 6.0, Stroke::new(1.5_f32, Color32::WHITE));
                chip(&p, format!("Zoom {:.1}×  ·  Haz clic donde quieras enfocar  ·  Esc para ver el resultado", zoom).replace('.', ","));
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                if resp.clicked() || resp.dragged() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (u, w) = to_video(pos);
                        self.set_focus(u, w);
                    }
                }
            }
            Pointing::Director => {
                // Targets near this moment; the one at it, highlighted.
                for (n, t) in self.project.director.targets.iter().enumerate() {
                    let dt = (t.time - self.time).abs();
                    if dt > 0.6 {
                        continue;
                    }
                    let here = dt < 0.05;
                    let r = Rect::from_two_pos(video_pt(t.x0, t.y0), video_pt(t.x1, t.y1));
                    let selected = self.selection == Selection::Target(t.id);
                    let col = if selected { YELLOW } else if here { ACCENT } else { Color32::from_white_alpha(90) };
                    p.rect_filled(r, Rounding::same(6.0), col.linear_multiply(0.12));
                    p.rect_stroke(r, Rounding::same(6.0), Stroke::new(if here { 2.0_f32 } else { 1.0 }, col));
                    let badge = Pos2::new(r.left() + 12.0, r.top() + 12.0);
                    p.circle_filled(badge, 10.0, col);
                    p.text(badge, Align2::CENTER_CENTER, format!("{}", n + 1), medium(11.0), Color32::BLACK);
                }
                if let (Some((u0, v0)), Some(pos)) = (self.drawing_target, resp.interact_pointer_pos()) {
                    let (u1, v1) = to_video(pos);
                    let r = Rect::from_two_pos(video_pt(u0, v0), video_pt(u1, v1));
                    p.rect_stroke(r, Rounding::same(6.0), Stroke::new(2.0_f32, ACCENT));
                }
                chip(&p, "Director · Arrastra para marcar lo que debe verse en este momento".into());
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                if resp.drag_started() {
                    if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                        self.drawing_target = Some(to_video(origin));
                    }
                }
                if resp.drag_stopped() {
                    if let (Some((u0, v0)), Some(pos)) = (self.drawing_target.take(), resp.interact_pointer_pos()) {
                        let (u1, v1) = to_video(pos);
                        match timeline::add_target(&mut self.project, self.time, u0, v0, u1, v1) {
                            Ok(id) => self.selection = Selection::Target(id),
                            Err(e) => self.toast(e),
                        }
                    }
                }
                if resp.clicked() {
                    // Clicking a box selects its target.
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (u, w) = to_video(pos);
                        let hit = self
                            .project
                            .director
                            .targets
                            .iter()
                            .find(|t| (t.time - self.time).abs() < 0.6 && u >= t.x0 && u <= t.x1 && w >= t.y0 && w <= t.y1);
                        self.selection = hit.map(|t| Selection::Target(t.id)).unwrap_or(Selection::None);
                    }
                }
            }
            Pointing::Nothing => {}
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
            self.seek(0.0);
        }

        let p = ui.painter();
        p.text(
            Pos2::new(rect.left(), c.y),
            Align2::LEFT_CENTER,
            format!("{}  /  {}", fmt_time(self.time), fmt_time(timeline::duration(&self.project))),
            medium(13.0),
            TEXT,
        );
        let n_touches = timeline::timeline_touches(&self.project).len();
        let mut info = format!("{} clips  ·  {} movimientos de cámara", self.project.clips.len(), self.project.camera.len());
        if n_touches > 0 {
            info.push_str(&format!("  ·  {n_touches} toques"));
        }
        p.text(Pos2::new(rect.right(), c.y), Align2::RIGHT_CENTER, info, regular(12.5), TEXT_3);
    }
}
