//! Pictures and sounds: adding them, their waveforms and thumbnails, their settings, placing
//! pictures on the viewer, and playing the mix while the preview plays.

use super::*;
use crate::model::{AudioTrack, ImageClip};

pub(super) const SOUND_EXTS: [&str; 9] = ["mp3", "wav", "m4a", "aac", "ogg", "opus", "flac", "aiff", "wma"];
pub(super) const IMAGE_EXTS: [&str; 6] = ["png", "webp", "jpg", "jpeg", "gif", "bmp"];

pub(super) fn has_ext(path: &std::path::Path, exts: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e.to_lowercase().as_str()))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

impl RecOrdingApp {
    // ---------- Adding ----------

    /// Asks for a sound file for `track` (any track when `None`: long files go to music).
    pub(super) fn pick_sound(&self, track: Option<AudioTrack>, at: f32) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let title = match track {
            Some(t) => format!("Añadir a {}", t.label().to_lowercase()),
            None => "Añadir sonido".into(),
        };
        thread::spawn(move || {
            match crate::platform::pick_file(&title, "Sonidos", &SOUND_EXTS, &crate::platform::home()) {
                Ok(Some(p)) => Self::read_sound(&tx, track, p, at),
                Ok(None) => {}
                Err(()) => {
                    let _ = tx.send(AppEvent::PickerUnavailable);
                }
            }
            ctx.request_repaint();
        });
    }

    /// Reads a sound file's length on this thread and hands it to the app.
    fn read_sound(tx: &Sender<AppEvent>, track: Option<AudioTrack>, path: PathBuf, at: f32) {
        let path = path.to_string_lossy().into_owned();
        let ev = match crate::audio::length(&path) {
            Ok(length) => {
                let track = track.unwrap_or(if length > 15.0 { AudioTrack::Music } else { AudioTrack::Effects });
                AppEvent::SoundReady { track, path, length, at }
            }
            Err(e) => AppEvent::MediaFailed(e),
        };
        let _ = tx.send(ev);
    }

    pub(super) fn pick_picture(&self, at: f32) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            match crate::platform::pick_file("Añadir imagen", "Imágenes", &IMAGE_EXTS, &crate::platform::home()) {
                Ok(Some(p)) => Self::read_picture(&tx, p, at),
                Ok(None) => {}
                Err(()) => {
                    let _ = tx.send(AppEvent::PickerUnavailable);
                }
            }
            ctx.request_repaint();
        });
    }

    fn read_picture(tx: &Sender<AppEvent>, path: PathBuf, at: f32) {
        let path = path.to_string_lossy().into_owned();
        let ev = match crate::images::size(&path) {
            Ok((w, h)) => AppEvent::PictureReady { path, aspect: w as f32 / h as f32, at },
            Err(e) => AppEvent::MediaFailed(e),
        };
        let _ = tx.send(ev);
    }

    /// A sound or picture file dropped on the window, added at the cursor.
    pub(super) fn drop_layer_file(&mut self, path: PathBuf) {
        let (tx, ctx, at) = (self.tx.clone(), self.ctx.clone(), self.time);
        let picture = has_ext(&path, &IMAGE_EXTS);
        thread::spawn(move || {
            if picture {
                Self::read_picture(&tx, path, at);
            } else {
                Self::read_sound(&tx, None, path, at);
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn sound_ready(&mut self, track: AudioTrack, path: String, length: f32, at: f32) {
        match timeline::add_audio(&mut self.project, track, &path, length, at) {
            Ok(id) => {
                self.selection = Selection::Audio(id);
                self.toast(format!("«{}» añadido a {}", file_name(&path), track.label().to_lowercase()));
            }
            Err(e) => self.toast(e),
        }
    }

    pub(super) fn picture_ready(&mut self, path: String, aspect: f32, at: f32) {
        match timeline::add_image(&mut self.project, &path, aspect, at) {
            Ok(id) => {
                self.selection = Selection::Image(id);
                self.toast("Imagen añadida: arrástrala en la vista previa para colocarla");
            }
            Err(e) => self.toast(e),
        }
    }

    // ---------- Waveforms and thumbnails ----------

    /// Starts reading the waveform of new sound files and the thumbnail of new pictures.
    pub(super) fn prepare_layers(&mut self) {
        for c in &self.project.audio {
            if !self.peaks.contains_key(&c.path) {
                self.peaks.insert(c.path.clone(), None);
                let (tx, ctx, path) = (self.tx.clone(), self.ctx.clone(), c.path.clone());
                thread::spawn(move || {
                    let peaks = crate::audio::peaks(&path);
                    let _ = tx.send(AppEvent::Peaks { path, peaks });
                    ctx.request_repaint();
                });
            }
        }
        for c in &self.project.images {
            if !self.image_thumbs.contains_key(&c.path) {
                self.image_thumbs.insert(c.path.clone(), None);
                let (tx, ctx, path) = (self.tx.clone(), self.ctx.clone(), c.path.clone());
                thread::spawn(move || {
                    let thumb = crate::images::load(&path).map(|pic| {
                        let k = (96.0 / pic.w.max(pic.h) as f32).min(1.0);
                        let (w, h) = (((pic.w as f32 * k) as usize).max(1), ((pic.h as f32 * k) as usize).max(1));
                        let small = crate::images::scale(&pic, w, h);
                        ((w, h), small.data)
                    });
                    let _ = tx.send(AppEvent::PictureThumb { path, thumb });
                    ctx.request_repaint();
                });
            }
        }
    }

    pub(super) fn picture_thumb(&mut self, path: String, thumb: Option<((usize, usize), Vec<u8>)>) {
        let tex = thumb.map(|((w, h), rgba)| {
            let img = ColorImage::from_rgba_premultiplied([w, h], &rgba);
            self.ctx.load_texture(format!("picture-{path}"), img, TextureOptions::LINEAR)
        });
        self.image_thumbs.insert(path, tex);
    }

    // ---------- Playing the sound ----------

    /// What the mix depends on, to render it again when something changes.
    fn mix_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let p = &self.project;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let sources: Vec<(&str, bool)> = p.media.iter().map(|m| (m.info.path.as_str(), m.info.has_audio)).collect();
        format!("{:?}{:?}{:?}{:?}{}{}", p.clips, sources, p.audio, p.mix, p.style.finish.fade_in, p.style.finish.fade_out).hash(&mut h);
        h.finish()
    }

    /// Keeps the sound in step with the preview: renders the mix again after edits (once the
    /// mouse is up) and plays it from the playhead while playing.
    pub(super) fn sync_sound(&mut self, ctx: &egui::Context) {
        let Some(player) = &self.player else { return };
        if self.home {
            player.stop();
            return;
        }
        if self.mix_busy.is_none() && !ctx.input(|i| i.pointer.any_down()) && self.last_mix_check.elapsed() > Duration::from_millis(250) {
            self.last_mix_check = Instant::now();
            let key = self.mix_key();
            if key != self.mix_have {
                self.mix_busy = Some(key);
                let (tx, ctx, project, rate) = (self.tx.clone(), self.ctx.clone(), self.project.clone(), player.rate);
                thread::spawn(move || {
                    let samples = crate::audio::render(&project, rate);
                    let _ = tx.send(AppEvent::MixRendered { key, samples });
                    ctx.request_repaint();
                });
            }
        }
        if self.playing {
            if !player.is_playing() || (player.position() - self.time).abs() > 0.15 {
                player.play_from(self.time);
            }
        } else if player.is_playing() {
            player.stop();
        }
    }

    pub(super) fn mix_rendered(&mut self, key: u64, samples: Result<Vec<f32>, String>) {
        self.mix_busy = None;
        self.mix_have = key;
        match samples {
            Ok(s) => {
                if let Some(p) = &self.player {
                    p.set_mix(s);
                }
            }
            Err(e) => eprintln!("rec·ording: {e}"),
        }
    }

    // ---------- Settings ----------

    pub(super) fn audio_inspector(&mut self, ui: &mut Ui, id: u64) {
        let Some(c) = self.project.audio.iter().find(|c| c.id == id).cloned() else {
            self.selection = Selection::None;
            return;
        };
        let mut delete = false;
        let mut looped = None;
        section_with(
            ui,
            c.track.label(),
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar el sonido", true, true).clicked() {
                    delete = true;
                }
            },
            |ui| {
                ui.add(egui::Label::new(RichText::new(file_name(&c.path)).color(TEXT)).truncate());
                ui.label(
                    RichText::new(format!("De {} a {} · {}", fmt_time(c.start), fmt_time(c.end()), fmt_secs(c.duration)))
                        .color(TEXT_2)
                        .font(regular(12.0)),
                );
                let ac = self.project.audio.iter_mut().find(|x| x.id == id).unwrap();
                let opts: Vec<(AudioTrack, &str)> = AudioTrack::ALL.iter().map(|t| (*t, t.label())).collect();
                segmented_row(ui, "Pista", &mut ac.track, &opts);
                slider_row(ui, "Volumen", &mut ac.volume, 0.0..=2.0, |v| format!("{:.0} %", v * 100.0));
                let secs = |v: f32| if v <= 0.0 { "No".to_string() } else { fmt_secs(v) };
                let most = (ac.duration / 2.0).max(0.1);
                slider_row(ui, "Entrada", &mut ac.fade_in, 0.0..=most.min(5.0), secs);
                slider_row(ui, "Salida", &mut ac.fade_out, 0.0..=most.min(5.0), secs);
                let mut l = ac.looped;
                toggle_row(ui, "Repetir en bucle", "Vuelve a empezar al acabar el archivo", &mut l);
                if l != ac.looped {
                    looped = Some(l);
                }
                ui.label(RichText::new("Arrastra sus bordes en la línea de tiempo para recortarlo, o el clip para moverlo (también a otra pista de sonido).").color(TEXT_3).font(regular(11.5)));
            },
        );
        if let Some(l) = looped {
            let _ = timeline::set_audio_loop(&mut self.project, id, l);
        }
        if delete {
            let _ = timeline::delete_audio(&mut self.project, id);
            self.selection = Selection::None;
        }
        ui.add_space(6.0);
    }

    pub(super) fn image_inspector(&mut self, ui: &mut Ui, id: u64) {
        let Some(c) = self.project.images.iter().find(|c| c.id == id).cloned() else {
            self.selection = Selection::None;
            return;
        };
        let mut delete = false;
        let mut length = None;
        section_with(
            ui,
            "Imagen",
            |ui| {
                if small_icon_button(ui, Icon::Trash, "Eliminar la imagen", true, true).clicked() {
                    delete = true;
                }
            },
            |ui| {
                ui.add(egui::Label::new(RichText::new(file_name(&c.path)).color(TEXT)).truncate());
                ui.label(RichText::new(format!("De {} a {}", fmt_time(c.start), fmt_time(c.end()))).color(TEXT_2).font(regular(12.0)));
                let ic = self.project.images.iter_mut().find(|x| x.id == id).unwrap();
                let pct = |v: f32| format!("{:.0} %", v * 100.0);
                slider_row(ui, "Tamaño", &mut ic.width, 0.03..=1.0, pct);
                slider_row(ui, "Horizontal", &mut ic.x, 0.0..=1.0, pct);
                slider_row(ui, "Vertical", &mut ic.y, 0.0..=1.0, pct);
                slider_row(ui, "Opacidad", &mut ic.opacity, 0.0..=1.0, pct);
                let secs = |v: f32| if v <= 0.0 { "No".to_string() } else { fmt_secs(v) };
                let most = (ic.duration / 2.0).max(0.1).min(3.0);
                slider_row(ui, "Entrada", &mut ic.fade_in, 0.0..=most, secs);
                slider_row(ui, "Salida", &mut ic.fade_out, 0.0..=most, secs);
                let mut d = ic.duration;
                if slider_row(ui, "Duración", &mut d, 0.2..=30.0, fmt_secs) {
                    length = Some(d);
                }
                ui.label(RichText::new("Arrástrala en la vista previa para colocarla. Se ve por encima del vídeo y por debajo de los textos.").color(TEXT_3).font(regular(11.5)));
            },
        );
        if let Some(d) = length {
            let _ = timeline::place_image(&mut self.project, id, c.start, c.start + d);
        }
        if delete {
            let _ = timeline::delete_image(&mut self.project, id);
            self.selection = Selection::None;
        }
        ui.add_space(6.0);
    }

    /// Volume of each source of sound.
    pub(super) fn mix_inspector(&mut self, ui: &mut Ui) {
        let has_voice = self.project.audio.iter().any(|c| c.track == AudioTrack::Voice);
        let has_music = self.project.audio.iter().any(|c| c.track == AudioTrack::Music);
        let m = &mut self.project.mix;
        section(ui, "Sonido", |ui| {
            let pct = |v: f32| format!("{:.0} %", v * 100.0);
            slider_row(ui, "Grabación", &mut m.recording, 0.0..=2.0, pct);
            slider_row(ui, "Efectos", &mut m.effects, 0.0..=2.0, pct);
            slider_row(ui, "Voz en off", &mut m.voice, 0.0..=2.0, pct);
            slider_row(ui, "Música", &mut m.music, 0.0..=2.0, pct);
            if has_voice && has_music {
                toggle_row(ui, "Bajar la música con la voz", "Se oye más baja mientras habla la voz en off", &mut m.duck_music);
            }
            if self.player.is_none() {
                ui.label(RichText::new("No hay salida de sonido: la vista previa se reproduce en silencio (la exportación lleva el sonido).").color(TEXT_3).font(regular(11.5)));
            }
        });
    }

    // ---------- Placing pictures on the viewer ----------

    /// Selects a picture by clicking it, and moves the selected one by dragging it. `canvas` is
    /// where the frame is shown.
    pub(super) fn pictures_on_canvas(&mut self, ui: &Ui, resp: &egui::Response, canvas: Rect) {
        if self.playing {
            return;
        }
        let rect_of = |c: &ImageClip| {
            let w = canvas.width() * c.width.max(0.01);
            let h = w / c.aspect.max(0.01);
            Rect::from_center_size(Pos2::new(canvas.left() + c.x * canvas.width(), canvas.top() + c.y * canvas.height()), Vec2::new(w, h))
        };
        let at = self.time;
        let under = |app: &Self, pos: Pos2| app.project.images.iter().rev().find(|c| at >= c.start && at < c.end() && rect_of(c).contains(pos)).map(|c| c.id);

        if resp.clicked() {
            if let Some(id) = resp.interact_pointer_pos().and_then(|p| under(self, p)) {
                self.selection = Selection::Image(id);
            }
        }
        if resp.drag_started() {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                let id = match self.selection {
                    Selection::Image(id) if self.project.images.iter().any(|c| c.id == id && rect_of(c).contains(origin)) => Some(id),
                    _ => under(self, origin),
                };
                if let Some(c) = id.and_then(|id| self.project.images.iter().find(|c| c.id == id)) {
                    self.selection = Selection::Image(c.id);
                    self.picture_grab = Some(origin - rect_of(c).center());
                }
            }
        }
        if resp.drag_stopped() {
            self.picture_grab = None;
        }
        let Selection::Image(id) = self.selection else { return };
        if let (Some(grab), Some(pos)) = (self.picture_grab, resp.interact_pointer_pos()) {
            if resp.dragged() {
                if let Some(c) = self.project.images.iter_mut().find(|c| c.id == id) {
                    let centre = pos - grab;
                    c.x = ((centre.x - canvas.left()) / canvas.width()).clamp(-0.2, 1.2);
                    c.y = ((centre.y - canvas.top()) / canvas.height()).clamp(-0.2, 1.2);
                }
            }
        }
        if let Some(c) = self.project.images.iter().find(|c| c.id == id) {
            if at >= c.start && at < c.end() {
                let r = rect_of(c);
                let p = ui.painter_at(canvas);
                p.rect_stroke(r, Rounding::same(2.0), Stroke::new(1.5_f32, YELLOW));
                for corner in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
                    p.circle_filled(corner, 3.5, YELLOW);
                }
                if resp.hover_pos().is_some_and(|h| r.contains(h)) {
                    ui.ctx().set_cursor_icon(if self.picture_grab.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab });
                }
            }
        }
    }
}
