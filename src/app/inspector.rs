//! The inspector: settings of what is selected (clip, camera clip or keyframe, device motion,
//! Director target), the Director, then the device and look of the video.

use super::*;

impl RecOrdingApp {
    pub(super) fn inspector(&mut self, ctx: &egui::Context) {
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
            ui.horizontal(|ui| {
                fixed_label(ui, 78.0, RichText::new("Movimiento").color(TEXT), Align::Min);
                segmented(ui, &mut self.project.director.motion, &[(DirectorMotion::ThreeD, "3D"), (DirectorMotion::TwoD, "2D")], ui.available_width());
            });
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
            Selection::Target(id) => self.target_inspector(ui, id),
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
        section(ui, "Clip de vídeo", |ui| {
            ui.add(egui::Label::new(RichText::new(name).color(TEXT)).truncate());
            ui.label(
                RichText::new(format!(
                    "De {} a {} de la grabación · {}",
                    fmt_time(c.clip.source_in),
                    fmt_time(c.clip.source_out),
                    fmt_secs(c.clip.duration())
                ))
                .color(TEXT_2)
                .font(regular(12.0)),
            );
            ui.label(RichText::new("Arrastra sus bordes para recortarlo y el clip para cambiarlo de orden.").color(TEXT_3).font(regular(11.5)));
            ui.horizontal(|ui| {
                if secondary_button(ui, "Dividir en el cursor").on_hover_text("Ctrl+B").clicked() {
                    split = true;
                }
                if ui.add(egui::Button::new(RichText::new("Eliminar").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    delete = true;
                }
            });
        });
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
        section(ui, &title, |ui| {
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
                    ui.horizontal(|ui| {
                        fixed_label(ui, 78.0, RichText::new("Llegada").color(TEXT), Align::Min);
                        let opts: Vec<(Ease, &str)> = Ease::ALL.iter().map(|e| (*e, e.label())).collect();
                        segmented(ui, &mut k.ease, &opts, ui.available_width());
                    });
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
            ui.horizontal(|ui| {
                if secondary_button(ui, "Listo").clicked() {
                    action = Some("done");
                }
                if ui.add(egui::Button::new(RichText::new("Eliminar movimiento").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    action = Some("del-clip");
                }
            });
        });
        self.selection = Selection::Camera { id, key: new_key };
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
            Some("done") => self.selection = Selection::None,
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
        section(ui, &format!("Móvil · {}", c.name), |ui| {
            ui.label(RichText::new(format!("De {} a {}", fmt_time(c.start), fmt_time(c.end()))).color(TEXT_2).font(regular(12.0)));
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
                slider_row(ui, "Inclinación", &mut k.tilt, -60.0..=60.0, deg);
                slider_row(ui, "Giro", &mut k.turn, -360.0..=360.0, deg);
                slider_row(ui, "Rotación", &mut k.roll, -45.0..=45.0, deg);
                ui.horizontal(|ui| {
                    fixed_label(ui, 78.0, RichText::new("Llegada").color(TEXT), Align::Min);
                    let opts: Vec<(Ease, &str)> = Ease::ALL.iter().map(|e| (*e, e.label())).collect();
                    segmented(ui, &mut k.ease, &opts, ui.available_width());
                });
            } else {
                ui.label(RichText::new("Elige un fotograma clave para ajustar la posición, la escala y el giro.").color(TEXT_3).font(regular(11.5)));
            }
            if ui.add(egui::Button::new(RichText::new("Eliminar movimiento").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                delete = true;
            }
        });
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

    fn target_inspector(&mut self, ui: &mut Ui, id: u64) {
        let Some((n, t)) = self.project.director.targets.iter().enumerate().find(|(_, t)| t.id == id).map(|(n, t)| (n, *t)) else {
            self.selection = Selection::None;
            return;
        };
        let mut retake = false;
        let mut delete = false;
        section(ui, &format!("Objetivo {}", n + 1), |ui| {
            ui.label(RichText::new(format!("En {}", fmt_time(t.time))).color(TEXT_2).font(regular(12.0)));
            ui.label(RichText::new("Arrastra su marca en la regla para cambiar el momento.").color(TEXT_3).font(regular(11.5)));
            ui.horizontal(|ui| {
                if secondary_button(ui, "Otra toma").on_hover_text("Otro movimiento de cámara para este plano").clicked() {
                    retake = true;
                }
                if ui.add(egui::Button::new(RichText::new("Eliminar").color(RED)).min_size(Vec2::new(0.0, 28.0))).clicked() {
                    delete = true;
                }
            });
        });
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
