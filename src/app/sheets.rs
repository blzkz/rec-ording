//! Sheets for projects and for connecting an assistant over MCP.

use super::*;

impl RecOrdingApp {
    pub(super) fn projects_sheet(&mut self, ctx: &egui::Context) {
        let mut open: Option<PathBuf> = None;
        let mut create = false;
        let mut close = false;
        let current = self.project_path.clone();
        Self::sheet_window(ctx, "projects", 440.0, |ui| {
            ui.label(RichText::new("Proyectos").font(medium(20.0)).color(TEXT));
            ui.label(RichText::new("Cada proyecto guarda sus vídeos, movimientos y estilo.").color(TEXT_2));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for e in &self.projects {
                    let is_open = e.path == current;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                    let h = ui.ctx().animate_bool(resp.id, resp.hovered());
                    let p = ui.painter();
                    p.rect_filled(rect, Rounding::same(9.0), lerp_color(CARD, Color32::from_rgb(56, 56, 61), h));
                    if is_open {
                        p.rect_stroke(rect, Rounding::same(9.0), Stroke::new(2.0_f32, ACCENT));
                    }
                    p.text(Pos2::new(rect.left() + 14.0, rect.top() + 8.0), Align2::LEFT_TOP, &e.name, medium(14.0), TEXT);
                    let when = e
                        .modified
                        .elapsed()
                        .map(|d| match d.as_secs() {
                            s if s < 60 => "Ahora mismo".to_string(),
                            s if s < 3600 => format!("Hace {} min", s / 60),
                            s if s < 86400 => format!("Hace {} h", s / 3600),
                            s => format!("Hace {} días", s / 86400),
                        })
                        .unwrap_or_default();
                    let sub = if is_open { format!("Abierto · {when}") } else { when };
                    p.text(Pos2::new(rect.left() + 14.0, rect.top() + 26.0), Align2::LEFT_TOP, sub, regular(11.5), TEXT_3);
                    if resp.clicked() && !is_open {
                        open = Some(e.path.clone());
                    }
                    ui.add_space(2.0);
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Proyecto nuevo").font(medium(12.5)).color(TEXT_2));
            ui.horizontal(|ui| {
                let resp = ui.add(egui::TextEdit::singleline(&mut self.new_project_name).hint_text("Nombre, p. ej. «Mi app · Play Store»").desired_width(270.0));
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let ok = !self.new_project_name.trim().is_empty();
                if (primary_button(ui, "Crear", ok).clicked() || enter) && ok {
                    create = true;
                }
            });
            ui.label(RichText::new("Empieza vacío, con el mismo estilo que el proyecto abierto.").color(TEXT_3).font(regular(11.5)));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, "Cerrar").clicked() {
                    close = true;
                }
            });
        });
        if let Some(p) = open {
            self.open_project(p);
            self.sheet = Sheet::None;
        } else if create {
            let name = self.new_project_name.trim().to_string();
            self.new_project(&name);
            self.sheet = Sheet::None;
        } else if close {
            self.sheet = Sheet::None;
        }
    }

    pub(super) fn assistant_sheet(&mut self, ctx: &egui::Context) {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "/usr/bin/rec-ording".into());
        // Escaped as JSON, since Windows paths have backslashes.
        let command = serde_json::to_string(&exe).unwrap_or_default();
        let json = format!("{{\n  \"mcpServers\": {{\n    \"rec-ording\": {{\n      \"command\": {command},\n      \"args\": [\"--mcp\"]\n    }}\n  }}\n}}");
        let quoted = if exe.contains(char::is_whitespace) { format!("\"{exe}\"") } else { exe.clone() };
        let claude_code = format!("claude mcp add rec-ording -- {quoted} --mcp");
        let mut close = false;
        let mut copied: Option<&str> = None;
        Self::sheet_window(ctx, "assistant", 480.0, |ui| {
            ui.label(RichText::new("Asistente (MCP)").font(medium(20.0)).color(TEXT));
            ui.label(
                RichText::new("Un agente como Claude, Cursor o Codex puede montar el vídeo por ti: importar y recortar grabaciones, añadir zooms y movimientos, usar el Director, cambiar el estilo, ver fotogramas y exportar. Edita el proyecto abierto, y aquí verás sus cambios al momento; los puedes deshacer con Ctrl+Z.")
                    .color(TEXT_2),
            );
            ui.add_space(10.0);
            ui.label(RichText::new("Claude Desktop, Cursor y otros (configuración MCP)").font(medium(12.5)).color(TEXT_2));
            egui::Frame::none().fill(Color32::from_rgb(30, 30, 33)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(10.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&json).monospace().color(TEXT));
            });
            if secondary_button(ui, "Copiar configuración").clicked() {
                ui.ctx().copy_text(json.clone());
                copied = Some("Configuración copiada");
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Claude Code").font(medium(12.5)).color(TEXT_2));
            egui::Frame::none().fill(Color32::from_rgb(30, 30, 33)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(10.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&claude_code).monospace().color(TEXT));
            });
            if secondary_button(ui, "Copiar comando").clicked() {
                ui.ctx().copy_text(claude_code.clone());
                copied = Some("Comando copiado");
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if primary_button(ui, "Hecho", true).clicked() {
                    close = true;
                }
            });
        });
        if let Some(m) = copied {
            self.toast(m);
        }
        if close {
            self.sheet = Sheet::None;
        }
    }

    pub(super) fn update_sheet(&mut self, ctx: &egui::Context) {
        let Some(r) = self.update.clone() else {
            self.sheet = Sheet::None;
            return;
        };
        enum Act {
            Install,
            Skip,
            Close,
            Restart,
            Page,
        }
        let how = update::method();
        let can_install = how != update::Method::Manual && r.asset.is_some();
        let busy = self.recording.is_some() || self.saving_recording || matches!(self.export_ui, ExportUi::Running { .. });
        let mut act: Option<Act> = None;
        Self::sheet_window(ctx, "update", 440.0, |ui| {
            ui.label(RichText::new(format!("rec-ording {}", r.version)).font(medium(20.0)).color(TEXT));
            ui.label(RichText::new(format!("Hay una versión nueva. Tienes la {}.", update::CURRENT)).color(TEXT_2));
            if !r.notes.is_empty() {
                ui.add_space(8.0);
                egui::Frame::none().fill(Color32::from_rgb(30, 30, 33)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(12.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        ui.label(RichText::new(&r.notes).color(TEXT));
                    });
                });
            }
            ui.add_space(10.0);
            match &self.update_ui {
                UpdateUi::Idle => {
                    let note = match (&how, r.asset.is_some()) {
                        (_, false) => "Esta versión no tiene paquete para tu sistema. Puedes descargarla desde su página.",
                        (update::Method::Pacman, true) => "Se instalará el paquete y el sistema te pedirá tu contraseña.",
                        (update::Method::Folder(_), true) => "Se reemplazarán los archivos de la carpeta de rec-ording.",
                        (update::Method::Manual, true) => "Esta copia no se instaló con el paquete. Descarga la versión nueva desde su página.",
                    };
                    ui.label(RichText::new(note).color(TEXT_3).font(regular(11.5)));
                    ui.add_space(10.0);
                    button_row(ui, |ui| {
                        let main = if can_install { "Actualizar" } else { "Abrir la página" };
                        if primary_button(ui, main, true).clicked() {
                            act = Some(if can_install { Act::Install } else { Act::Page });
                        }
                        if secondary_button(ui, "Ahora no").clicked() {
                            act = Some(Act::Close);
                        }
                        if secondary_button(ui, "Omitir esta versión").clicked() {
                            act = Some(Act::Skip);
                        }
                    });
                }
                UpdateUi::Installing => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(16.0).color(TEXT_2));
                        ui.label(RichText::new("Descargando e instalando…").color(TEXT_2));
                    });
                    ui.add_space(10.0);
                    button_row(ui, |ui| {
                        if secondary_button(ui, "Ocultar").clicked() {
                            act = Some(Act::Close);
                        }
                    });
                }
                UpdateUi::Installed => {
                    ui.label(RichText::new("Listo. Reinicia rec-ording para usar la versión nueva.").color(GREEN));
                    if busy {
                        ui.label(RichText::new("Termina la grabación o la exportación antes de reiniciar.").color(TEXT_3).font(regular(11.5)));
                    }
                    ui.add_space(10.0);
                    button_row(ui, |ui| {
                        if primary_button(ui, "Reiniciar", !busy).clicked() && !busy {
                            act = Some(Act::Restart);
                        }
                        if secondary_button(ui, "Más tarde").clicked() {
                            act = Some(Act::Close);
                        }
                    });
                }
                UpdateUi::Failed(e) => {
                    ui.label(RichText::new(e).color(RED));
                    ui.add_space(10.0);
                    button_row(ui, |ui| {
                        if primary_button(ui, "Reintentar", true).clicked() {
                            act = Some(Act::Install);
                        }
                        if secondary_button(ui, "Abrir la página").clicked() {
                            act = Some(Act::Page);
                        }
                        if secondary_button(ui, "Cerrar").clicked() {
                            act = Some(Act::Close);
                        }
                    });
                }
            }
        });
        match act {
            Some(Act::Install) => self.install_update(),
            Some(Act::Skip) => {
                update::skip(&r.version);
                self.update = None;
                self.sheet = Sheet::None;
            }
            Some(Act::Close) => {
                if matches!(self.update_ui, UpdateUi::Failed(_)) {
                    self.update_ui = UpdateUi::Idle;
                }
                self.sheet = Sheet::None;
            }
            Some(Act::Restart) => {
                self.save_now();
                update::restart();
            }
            Some(Act::Page) => crate::platform::open_path(std::path::Path::new(&r.page)),
            None => {}
        }
    }
}

/// A row of buttons aligned to the right, one button tall.
fn button_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 30.0), egui::Layout::right_to_left(Align::Center), add);
}
