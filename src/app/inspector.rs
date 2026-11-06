//! The inspector: settings of what is selected (clip, camera clip or keyframe, device motion,
//! Director target), the Director, then the device and look of the video.

use super::*;

impl RecOrdingApp {
    pub(super) fn inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector")
            .default_width(360.0)
            .width_range(330.0..=560.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(SIDEBAR)
                    // The scroll bar takes the rest of the right margin (see below).
                    .inner_margin(Margin { left: 14.0, right: 6.0, top: 12.0, bottom: 12.0 })
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(18, 18, 20))),
            )
            .show(ctx, |ui| {
                // A scroll bar with its own lane, so it never covers the controls.
                ui.spacing_mut().scroll = egui::style::ScrollStyle {
                    bar_width: 9.0,
                    bar_inner_margin: 9.0,
                    dormant_handle_opacity: 0.3,
                    ..egui::style::ScrollStyle::solid()
                };
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        if self.director_mode {
                            self.director_inspector(ui);
                        }
                        self.selection_inspector(ui);
                        self.style_inspector(ui);
                    });
            });
    }

    fn director_inspector(&mut self, ui: &mut Ui) {
        let mut generate = false;
        let mut retake_all = false;
        let mut clear = false;
        let n = self.project.director.targets.len();
        section(ui, "Director", |ui| {
            ui.label(
                RichText::new("Mueve el cursor a cada momento importante y arrastra sobre la vista previa lo que debe verse. Cada recuadro será un plano de cámara.")
                    .color(TEXT_2)
                    .font(regular(12.0)),
            );
            segmented_row(ui, "Movimiento", &mut self.project.director.motion, &[(DirectorMotion::ThreeD, "3D"), (DirectorMotion::TwoD, "2D")]);
            if self.project.director.motion == DirectorMotion::ThreeD {
                slider_row(ui, "Intensidad", &mut self.project.director.intensity, 0.0..=1.0, |v| {
                    match v {
                        v if v < 0.34 => "Suave",
                        v if v < 0.67 => "Media",
                        _ => "Marcada",
                    }
                    .to_string()
                });
            }
            ui.label(RichText::new(format!("{n} objetivos")).color(TEXT_3).font(regular(12.0)));
            ui.horizontal(|ui| {
                if primary_button(ui, "Generar planos", n > 0).on_hover_text("Sustituye los movimientos de cámara que se solapen").clicked() {
                    generate = true;
                }
                if ui.add_enabled(n > 0, egui::Button::new("Otra toma").min_size(Vec2::new(0.0, 28.0))).clicked() {
                    retake_all = true;
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(n > 0, egui::Button::new(RichText::new("Borrar todo").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    clear = true;
                }
                if secondary_button(ui, "Salir del Director").clicked() {
                    self.director_mode = false;
                    self.selection = Selection::None;
                }
            });
        });
        if retake_all {
            timeline::retake(&mut self.project, None);
            generate = true;
        }
        if generate {
            self.director_generate();
        }
        if clear {
            self.project.director.targets.clear();
            self.project.camera.retain(|c| c.kind != CameraKind::Director);
            self.selection = Selection::None;
        }
        ui.add_space(6.0);
    }

    pub(super) fn director_generate(&mut self) {
        let (w, h) = self.project.export.dimensions(self.project.style.aspect);
        let meta = self.active_model().map(|m| m.meta);
        match timeline::director_generate(&mut self.project, w as f32, h as f32, meta.as_ref()) {
            Ok((n, replaced)) => {
                self.director_mode = false;
                self.selection = Selection::None;
                self.seek(self.project.director.targets.first().map(|t| (t.time - 1.0).max(0.0)).unwrap_or(0.0));
                let extra = match replaced {
                    0 => String::new(),
                    1 => " Sustituyen 1 movimiento que se solapaba (Ctrl+Z para recuperarlo).".into(),
                    n => format!(" Sustituyen {n} movimientos que se solapaban (Ctrl+Z para recuperarlos)."),
                };
                self.toast(format!("{n} planos generados. Pulsa Espacio para verlos.{extra}"));
            }
            Err(e) => self.toast(e),
        }
    }

    fn selection_inspector(&mut self, ui: &mut Ui) {
        match self.selection {
            Selection::None => {}
            Selection::Clip(id) => self.clip_inspector(ui, id),
            Selection::Camera { id, key } => self.camera_inspector(ui, id, key),
            Selection::Device { id, key } => self.device_inspector(ui, id, key),
            Selection::Text(id) => self.text_inspector(ui, id),
            Selection::Target(id) => self.target_inspector(ui, id),
            Selection::Audio(id) => self.audio_inspector(ui, id),
            Selection::Image(id) => self.image_inspector(ui, id),
        }
    }

    fn clip_inspector(&mut self, ui: &mut Ui, id: u64) {
        let Some(c) = timeline::placed(&self.project).into_iter().find(|c| c.clip.id == id) else {
            self.selection = Selection::None;
            return;
        };
        let name = self
            .project
            .media(c.clip.media)
            .and_then(|m| std::path::Path::new(&m.info.path).file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let mut split = false;
        let mut delete = false;
        let mut freeze = false;
        let mut replace = false;
        let mut speed = None;
        let mut still_len = None;
        let title = if c.clip.freeze.is_some() { "Fotograma congelado" } else { "Clip de vídeo" };
        section_with(
            ui,
            title,
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar el clip", true, true).clicked() {
                    delete = true;
                }
            },
            |ui| {
                ui.add(egui::Label::new(RichText::new(name).color(TEXT)).truncate());
                let detail = match c.clip.freeze {
                    Some(_) => format!("El fotograma de {} de la grabación · {}", fmt_time(c.clip.source_in), fmt_secs(c.clip.duration())),
                    None => format!("De {} a {} de la grabación · {}", fmt_time(c.clip.source_in), fmt_time(c.clip.source_out), fmt_secs(c.clip.duration())),
                };
                ui.label(RichText::new(detail).color(TEXT_2).font(regular(12.0)));
                if let Some(secs) = c.clip.freeze {
                    let mut s = secs;
                    if slider_row(ui, "Duración", &mut s, 0.2..=10.0, fmt_secs) {
                        still_len = Some(s);
                    }
                } else {
                    let mut v = [0.5, 1.0, 1.5, 2.0, 4.0].into_iter().find(|v| (c.clip.speed - v).abs() < 1e-3).unwrap_or(c.clip.speed);
                    if segmented_row(ui, "Velocidad", &mut v, &[(0.5, "0,5×"), (1.0, "1×"), (1.5, "1,5×"), (2.0, "2×"), (4.0, "4×")]) {
                        speed = Some(v);
                    }
                    ui.label(RichText::new("Arrastra sus bordes para recortarlo y el clip para cambiarlo de orden.").color(TEXT_3).font(regular(11.5)));
                }
                ui.add_space(2.0);
                tool_group(ui, |ui| {
                    if icon_button(ui, Icon::Split, "Dividir en el cursor", "Ctrl+B", false, true).clicked() {
                        split = true;
                    }
                    if icon_button(ui, Icon::Freeze, "Congelar el fotograma del cursor 2 s (lo demás se desplaza)", "", false, true).clicked() {
                        freeze = true;
                    }
                    if icon_button(ui, Icon::Swap, "Cambiar el vídeo del clip (mantiene el recorte)", "", false, true).clicked() {
                        replace = true;
                    }
                });
            },
        );
        if let Some(v) = speed {
            if let Err(e) = timeline::set_clip_speed(&mut self.project, id, v) {
                self.toast(e);
            }
            self.requested_time = None;
        }
        if let Some(s) = still_len {
            let _ = timeline::set_freeze_duration(&mut self.project, id, s);
        }
        if freeze {
            match timeline::freeze_frame(&mut self.project, self.time, 2.0) {
                Ok(still) => self.selection = Selection::Clip(still),
                Err(e) => self.toast(e),
            }
            self.requested_time = None;
        }
        if replace {
            self.replacing = Some(id);
            self.pick_video();
        }
        if split {
            self.split_here();
        }
        if delete {
            self.delete_selection();
        }
        ui.add_space(6.0);
    }

    fn camera_inspector(&mut self, ui: &mut Ui, id: u64, key: Option<usize>) {
        let Some(c) = self.project.camera.iter().find(|c| c.id == id).cloned() else {
            self.selection = Selection::None;
            return;
        };
        let mut action: Option<&str> = None;
        let mut new_key: Option<usize> = key;
        let time = self.time;
        let title = format!("Cámara · {}", c.kind.label());
        let mut delete_clip = false;
        section_with(
            ui,
            &title,
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar el movimiento", true, true).clicked() {
                    delete_clip = true;
                }
            },
            |ui| {
                ui.label(
                    RichText::new(format!("De {} a {} · {}", fmt_time(c.start), fmt_time(c.end()), fmt_secs(c.duration())))
                        .color(TEXT_2)
                        .font(regular(12.0)),
                );
                let cm = self.project.camera.iter_mut().find(|x| x.id == id).unwrap();
                if matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) && c.keys.len() == 4 {
                    let mut zoom = c.keys[1].zoom;
                    if slider_row(ui, "Nivel", &mut zoom, 1.1..=4.0, |v| format!("{v:.1}×").replace('.', ",")) {
                        cm.keys[1].zoom = zoom;
                        cm.keys[2].zoom = zoom;
                        cm.kind = CameraKind::Zoom;
                    }
                    ui.label(RichText::new("Haz clic en la vista previa para elegir dónde enfocar.").color(TEXT_3).font(regular(11.5)));
                } else {
                    // Keyframes, as numbered chips.
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for i in 0..c.keys.len() {
                            if chip(ui, &format!("{}", i + 1), key == Some(i)).on_hover_text(format!("Fotograma clave en {}", fmt_time(c.start + c.keys[i].t))).clicked() {
                                new_key = Some(i);
                                action = Some("seek-key");
                            }
                        }
                        if chip(ui, "+ en el cursor", false).on_hover_text("Añade un fotograma clave con la cámara de este momento").clicked() {
                            action = Some("add-key");
                        }
                    });
                    if let Some(k) = key.and_then(|i| cm.keys.get_mut(i)) {
                        ui.add_space(2.0);
                        let deg = |v: f32| format!("{v:.0}°");
                        slider_row(ui, "Zoom", &mut k.zoom, 1.0..=4.0, |v| format!("{v:.1}×").replace('.', ","));
                        slider_row(ui, "Inclinación", &mut k.tilt, -60.0..=60.0, deg);
                        slider_row(ui, "Giro", &mut k.turn, -180.0..=180.0, deg);
                        slider_row(ui, "Rotación", &mut k.roll, -45.0..=45.0, deg);
                        let opts: Vec<(Ease, &str)> = Ease::ALL.iter().map(|e| (*e, e.label())).collect();
                        segmented_row(ui, "Llegada", &mut k.ease, &opts);
                        if k.zoom > 1.02 {
                            ui.label(RichText::new("Haz clic en la vista previa para elegir el encuadre.").color(TEXT_3).font(regular(11.5)));
                        }
                        if ui.add(egui::Button::new(RichText::new("Eliminar fotograma").color(RED)).min_size(Vec2::new(0.0, 26.0))).clicked() {
                            action = Some("del-key");
                        }
                    } else {
                        ui.label(RichText::new("Elige un fotograma clave (los rombos de la línea de tiempo) para cambiar su zoom y sus ángulos.").color(TEXT_3).font(regular(11.5)));
                    }
                }
            },
        );
        self.selection = Selection::Camera { id, key: new_key };
        if delete_clip {
            action = Some("del-clip");
        }
        match action {
            Some("seek-key") => {
                if let Some(i) = new_key {
                    self.seek(c.start + c.keys[i].t);
                }
            }
            Some("add-key") => match timeline::add_camera_key(&mut self.project, time, |_| {}) {
                Ok((cid, i)) => self.selection = Selection::Camera { id: cid, key: Some(i) },
                Err(e) => self.toast(e),
            },
            Some("del-key") => {
                if let Some(i) = key {
                    let _ = timeline::delete_camera_key(&mut self.project, id, i);
                    let still = self.project.camera.iter().any(|x| x.id == id);
                    self.selection = if still { Selection::Camera { id, key: None } } else { Selection::None };
                }
            }
            Some("del-clip") => {
                let _ = timeline::delete_camera_clip(&mut self.project, id);
                self.selection = Selection::None;
            }
            _ => {}
        }
        ui.add_space(6.0);
    }

    fn device_inspector(&mut self, ui: &mut Ui, id: u64, key: Option<usize>) {
        let Some(c) = self.project.device.iter().find(|c| c.id == id).cloned() else {
            self.selection = Selection::None;
            return;
        };
        let mut new_key = key;
        let mut delete = false;
        let mut seek_to = None;
        section_with(
            ui,
            &format!("Móvil · {}", c.name),
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar el movimiento", true, true).clicked() {
                    delete = true;
                }
            },
            |ui| {
                ui.label(RichText::new(format!("De {} a {}", fmt_time(c.start), fmt_time(c.end()))).color(TEXT_2).font(regular(12.0)));
                {
                    let dc = self.project.device.iter_mut().find(|x| x.id == id).unwrap();
                    let mut secs = dc.duration();
                    if slider_row(ui, "Duración", &mut secs, 0.3..=6.0, fmt_secs) {
                        timeline::set_device_duration(dc, secs);
                    }
                    if let Some((axis, degrees)) = timeline::spin_of(dc) {
                        let turns = (degrees.abs() / 360.0).round().max(1.0) as u32;
                        let sign = degrees.signum();
                        let mut new_spin = None;
                        ui.horizontal(|ui| {
                            fixed_label(ui, 78.0, RichText::new("Vueltas").color(TEXT), Align::Min);
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for n in 1..=3u32 {
                                if chip(ui, &n.to_string(), n == turns).clicked() {
                                    new_spin = Some(sign * 360.0 * n as f32);
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            fixed_label(ui, 78.0, RichText::new("Sentido").color(TEXT), Align::Min);
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (a, b) = if axis == 1 { ("Hacia atrás", "Hacia delante") } else { ("Izquierda", "Derecha") };
                            if chip(ui, a, sign < 0.0).clicked() {
                                new_spin = Some(-degrees.abs());
                            }
                            if chip(ui, b, sign > 0.0).clicked() {
                                new_spin = Some(degrees.abs());
                            }
                        });
                        if let Some(d) = new_spin {
                            timeline::set_spin(dc, d);
                        }
                        let axis = ["sobre su eje vertical", "sobre su eje horizontal", "en el plano"][axis];
                        ui.label(RichText::new(format!("Gira {axis}. Junto a un cambio de clip, el corte queda oculto mientras se ve la trasera.")).color(TEXT_3).font(regular(11.5)));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    for i in 0..c.keys.len() {
                        if chip(ui, &format!("{}", i + 1), key == Some(i)).clicked() {
                            new_key = Some(i);
                            seek_to = Some(c.start + c.keys[i].t);
                        }
                    }
                });
                let dc = self.project.device.iter_mut().find(|x| x.id == id).unwrap();
                if let Some(k) = key.and_then(|i| dc.keys.get_mut(i)) {
                    let deg = |v: f32| format!("{v:.0}°");
                    let pct = |v: f32| format!("{:.0} %", v * 100.0);
                    slider_row(ui, "Horizontal", &mut k.dx, -1.0..=1.0, pct);
                    slider_row(ui, "Vertical", &mut k.dy, -1.0..=1.0, pct);
                    slider_row(ui, "Escala", &mut k.scale, 0.3..=2.0, pct);
                    slider_row(ui, "Inclinación", &mut k.tilt, -360.0..=360.0, deg);
                    slider_row(ui, "Giro", &mut k.turn, -1080.0..=1080.0, deg);
                    slider_row(ui, "Rotación", &mut k.roll, -360.0..=360.0, deg);
                    let opts: Vec<(Ease, &str)> = Ease::ALL.iter().map(|e| (*e, e.label())).collect();
                    segmented_row(ui, "Llegada", &mut k.ease, &opts);
                } else {
                    ui.label(RichText::new("Elige un fotograma clave para ajustar la posición, la escala y el giro.").color(TEXT_3).font(regular(11.5)));
                }
            },
        );
        self.selection = Selection::Device { id, key: new_key };
        if let Some(t) = seek_to {
            self.seek(t);
        }
        if delete {
            let _ = timeline::delete_device_clip(&mut self.project, id);
            self.selection = Selection::None;
        }
        ui.add_space(6.0);
    }

    fn text_inspector(&mut self, ui: &mut Ui, id: u64) {
        let Some(c) = self.project.texts.iter().find(|c| c.id == id).cloned() else {
            self.selection = Selection::None;
            return;
        };
        let mut delete = false;
        let mut place: Option<(f32, f32)> = None;
        let focus = std::mem::take(&mut self.focus_title);
        section_with(
            ui,
            "Texto",
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar el texto", true, true).clicked() {
                    delete = true;
                }
            },
            |ui| {
                ui.label(RichText::new(format!("De {} a {}", fmt_time(c.start), fmt_time(c.end()))).color(TEXT_2).font(regular(12.0)));
                let tc = self.project.texts.iter_mut().find(|x| x.id == id).unwrap();
                ui.label(RichText::new("Titular").color(TEXT));
                let title = ui.add(
                    egui::TextEdit::multiline(&mut tc.title)
                        .hint_text("Camina.\n(una línea por salto de línea)")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                if focus {
                    title.request_focus();
                }
                ui.label(RichText::new("Debajo, más pequeño").color(TEXT));
                ui.add(egui::TextEdit::singleline(&mut tc.subtitle).hint_text("Opcional").desired_width(f32::INFINITY));
                ui.add_space(4.0);
                fn chips<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, all: &[T], name: impl Fn(T) -> &'static str) {
                    ui.label(RichText::new(label).color(TEXT_2).font(regular(11.5)));
                    ui.add_space(-4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for &v in all {
                            if chip(ui, name(v), *value == v).clicked() {
                                *value = v;
                            }
                        }
                    });
                }
                chips(ui, "Posición", &mut tc.place, &TextPlace::ALL, TextPlace::label);
                chips(ui, "Tamaño", &mut tc.size, &TextSize::ALL, TextSize::label);
                chips(ui, "Entrada", &mut tc.reveal, &TextReveal::ALL, TextReveal::label);
                chips(ui, "Color", &mut tc.color, &TextColor::ALL, TextColor::label);
                if tc.place != TextPlace::Center {
                    toggle_row(ui, "Apartar el móvil", "Le deja sitio al texto mientras se ve", &mut tc.make_room);
                    if tc.make_room {
                        toggle_row(ui, "Mirar al texto", "El móvil gira un poco hacia él", &mut tc.face_text);
                        slider_row(ui, "Movimiento", &mut tc.move_time, 0.2..=2.0, fmt_secs);
                    }
                }
                let mut secs = tc.duration;
                if slider_row(ui, "Duración", &mut secs, 0.8..=10.0, fmt_secs) {
                    place = Some((tc.start, tc.start + secs));
                }
            },
        );
        if let Some((a, b)) = place {
            let _ = timeline::place_text(&mut self.project, id, a, b);
        }
        if delete {
            let _ = timeline::delete_text(&mut self.project, id);
            self.selection = Selection::None;
        }
        ui.add_space(6.0);
    }

    fn target_inspector(&mut self, ui: &mut Ui, id: u64) {
        let Some((n, t)) = self.project.director.targets.iter().enumerate().find(|(_, t)| t.id == id).map(|(n, t)| (n, *t)) else {
            self.selection = Selection::None;
            return;
        };
        let mut retake = false;
        let mut delete = false;
        section_with(
            ui,
            &format!("Objetivo {}", n + 1),
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar el objetivo", true, true).clicked() {
                    delete = true;
                }
            },
            |ui| {
                ui.label(RichText::new(format!("En {}", fmt_time(t.time))).color(TEXT_2).font(regular(12.0)));
                ui.label(RichText::new("Arrastra su marca en la regla para cambiar el momento.").color(TEXT_3).font(regular(11.5)));
                if secondary_button(ui, "Otra toma").on_hover_text("Otro movimiento de cámara para este plano").clicked() {
                    retake = true;
                }
            },
        );
        if retake {
            timeline::retake(&mut self.project, Some(id));
            if self.project.camera.iter().any(|c| c.kind == CameraKind::Director) {
                self.director_generate();
                self.director_mode = true;
                self.selection = Selection::Target(id);
            } else {
                self.toast("Se usará otro movimiento al generar los planos");
            }
        }
        if delete {
            let _ = timeline::remove_target(&mut self.project, id);
            self.selection = Selection::None;
        }
        ui.add_space(6.0);
    }
}
