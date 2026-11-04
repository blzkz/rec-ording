//! Final render: decode the recording at full size, compose every frame and encode with ffmpeg.

use crate::timeline;
use crate::compositor::{compose, ComposeInput, Frame, Mips};
use crate::model::{ExportFormat, Project, Quality};
use crossbeam_channel::{bounded, Sender};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

pub enum ExportEvent {
    Progress { fraction: f32, eta_secs: Option<f32> },
    Done(PathBuf),
    Failed(String),
    Cancelled,
}

pub fn output_path(project: &Project) -> PathBuf {
    let clean: String = project
        .name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '-' } else { c })
        .collect();
    let name = if clean.trim().is_empty() { "rec-ording".into() } else { clean.trim().to_string() };
    PathBuf::from(&project.export.folder).join(format!(
        "{} {}.{}",
        name,
        crate::model::timestamp_for_filename(),
        project.export.format.extension()
    ))
}

pub fn start(project: Project, cancel: Arc<AtomicBool>, tx: Sender<ExportEvent>) {
    thread::spawn(move || {
        let out = output_path(&project);
        let ev = match run(&project, &out, &cancel, &tx) {
            Ok(()) => ExportEvent::Done(out),
            Err(_) if cancel.load(Ordering::Relaxed) => {
                let _ = std::fs::remove_file(&out);
                ExportEvent::Cancelled
            }
            Err(e) => {
                let _ = std::fs::remove_file(&out);
                ExportEvent::Failed(e)
            }
        };
        let _ = tx.send(ev);
    });
}

fn collect_stderr(child: &mut Child) -> Arc<Mutex<String>> {
    let buf = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let b = buf.clone();
        thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            *b.lock().unwrap() = s;
        });
    }
    buf
}

fn run(project: &Project, out: &PathBuf, cancel: &AtomicBool, tx: &Sender<ExportEvent>) -> Result<(), String> {
    let clips = timeline::placed(project);
    if clips.is_empty() {
        return Err("No hay ningún vídeo en el proyecto".into());
    }
    let settings = &project.export;
    let (ow, oh) = settings.dimensions(project.style.aspect);
    let (ow, oh) = (ow as usize, oh as usize);
    let fps = if settings.format == ExportFormat::Gif { settings.fps.min(25) } else { settings.fps };
    let dur = timeline::duration(project).max(0.1);
    let total = ((dur * fps as f32).round() as usize).max(1);

    // Each clip, with its recording and how many output frames it fills.
    struct Part {
        path: String,
        w: usize,
        h: usize,
        from: f32,
        len: f32,
        frames: usize,
        audio: bool,
    }
    let parts: Vec<Part> = clips
        .iter()
        .map(|c| {
            let m = project.media(c.clip.media).ok_or("Falta una grabación del proyecto")?;
            let frames = ((c.end * fps as f32).round() - (c.start * fps as f32).round()).max(0.0) as usize;
            Ok(Part {
                path: m.info.path.clone(),
                w: m.info.width as usize,
                h: m.info.height as usize,
                from: c.clip.source_in,
                len: c.clip.duration(),
                frames,
                audio: m.info.has_audio,
            })
        })
        .collect::<Result<_, String>>()?;

    // Encoder: input 0 is the rendered video, then one audio input per clip.
    let mut enc = Command::new("ffmpeg");
    enc.args(["-nostdin", "-y", "-loglevel", "error"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{ow}x{oh}"), "-r", &fps.to_string(), "-i", "-"]);
    let audio = settings.include_audio && parts.iter().any(|p| p.audio) && settings.format != ExportFormat::Gif;
    if audio {
        let mut filter = String::new();
        for (k, part) in parts.iter().enumerate() {
            let len = format!("{:.3}", part.len);
            if part.audio {
                enc.args(["-ss", &format!("{:.3}", part.from), "-t", &len, "-i", &part.path]);
            } else {
                // Silence for clips without sound, so the rest stays in sync.
                enc.args(["-f", "lavfi", "-t", &len, "-i", "anullsrc=r=48000:cl=stereo"]);
            }
            filter.push_str(&format!(
                "[{i}:a]aresample=48000,aformat=channel_layouts=stereo,atrim=0:{len},asetpts=PTS-STARTPTS[a{k}];",
                i = k + 1
            ));
        }
        for k in 0..parts.len() {
            filter.push_str(&format!("[a{k}]"));
        }
        filter.push_str(&format!("concat=n={}:v=0:a=1[aout]", parts.len()));
        enc.args(["-filter_complex", &filter, "-map", "0:v", "-map", "[aout]"]);
    }
    let crf = |base: u32| -> String {
        match settings.quality {
            Quality::Standard => base + 5,
            Quality::High => base,
            Quality::Max => base.saturating_sub(4),
        }
        .to_string()
    };
    match settings.format {
        ExportFormat::H264 => {
            enc.args(["-c:v", "libx264", "-preset", "medium", "-crf", &crf(18), "-pix_fmt", "yuv420p"]);
        }
        ExportFormat::Hevc => {
            enc.args(["-c:v", "libx265", "-preset", "medium", "-crf", &crf(21), "-pix_fmt", "yuv420p", "-tag:v", "hvc1"]);
        }
        ExportFormat::Gif => {
            let width = ow.min(800);
            enc.args([
                "-vf",
                &format!(
                    "scale={width}:-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4"
                ),
                "-loop",
                "0",
            ]);
        }
    }
    if audio {
        enc.args(["-c:a", "aac", "-b:a", "192k", "-shortest"]);
    }
    if settings.format != ExportFormat::Gif {
        enc.args(["-movflags", "+faststart"]);
    }
    let mut encoder = enc
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("No se pudo iniciar ffmpeg: {e}"))?;
    let enc_err = collect_stderr(&mut encoder);

    // Reader thread: decodes the clips one after another, giving exactly `frames` frames per
    // clip (repeating the last one if a recording runs a few frames short).
    let (frame_tx, frame_rx) = bounded::<Frame>(3);
    let dec_err = Arc::new(Mutex::new(String::new()));
    let reader_err = dec_err.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = stop.clone();
    let reader = thread::spawn(move || {
        for part in parts {
            if part.frames == 0 {
                continue;
            }
            let spawned = Command::new("ffmpeg")
                .args(["-nostdin", "-loglevel", "error", "-ss", &format!("{:.3}", part.from), "-t", &format!("{:.3}", part.len), "-i", &part.path])
                .args(["-vf", &format!("scale={}:{},fps={fps}", part.w, part.h), "-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            let Ok(mut dec) = spawned else {
                *reader_err.lock().unwrap() = "No se pudo iniciar ffmpeg".into();
                return;
            };
            let err = collect_stderr(&mut dec);
            let mut stdout = dec.stdout.take().unwrap();
            let mut last: Option<Vec<u8>> = None;
            for _ in 0..part.frames {
                if reader_stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut data = vec![0u8; part.w * part.h * 4];
                let data = match stdout.read_exact(&mut data) {
                    Ok(()) => {
                        last = Some(data.clone());
                        data
                    }
                    Err(_) => match &last {
                        Some(l) => l.clone(),
                        None => {
                            *reader_err.lock().unwrap() = format!("No se pudo leer {}: {}", part.path, err.lock().unwrap().trim());
                            let _ = dec.kill();
                            return;
                        }
                    },
                };
                if frame_tx.send(Frame { w: part.w, h: part.h, data }).is_err() {
                    let _ = dec.kill();
                    return;
                }
            }
            let _ = dec.kill();
            let _ = dec.wait();
        }
    });

    let (out_tx, out_rx) = bounded::<Vec<u8>>(3);
    let mut enc_in = encoder.stdin.take().ok_or("ffmpeg sin entrada")?;
    let writer = thread::spawn(move || -> bool {
        for buf in out_rx {
            if enc_in.write_all(&buf).is_err() {
                return false;
            }
        }
        true
    });

    let model = match (&project.style.frame, &project.style.model) {
        (crate::model::FrameStyle::Custom, Some(cm)) => {
            let src = crate::model3d::load(&cm.path)?;
            let part = cm.screen(src.guess, src.parts.len()).ok_or("No se encontró la pantalla del modelo: elígela en «Pantalla»")?;
            Some(crate::model3d::prepare(&src, part, cm.quarter_turns)?)
        }
        _ => None,
    };
    let touches = timeline::timeline_touches(project);
    let began = Instant::now();
    let mut written = 0usize;
    let mut last: Option<(Mips, usize, usize)> = None;
    let mut failed = None;

    for i in 0..total {
        if cancel.load(Ordering::Relaxed) {
            failed = Some("cancelado".to_string());
            break;
        }
        if let Ok(f) = frame_rx.recv() {
            let (w, h) = (f.w, f.h);
            last = Some((Mips::build(f), w, h));
        }
        let Some((mips, vw, vh)) = last.as_ref() else {
            let e = dec_err.lock().unwrap().clone();
            failed = Some(if e.is_empty() { "No se pudo decodificar el vídeo".to_string() } else { e });
            break;
        };
        let t = i as f32 / fps as f32;
        let mut buf = vec![0u8; ow * oh * 4];
        compose(
            &mut buf,
            ow,
            oh,
            &ComposeInput {
                style: &project.style,
                screen: Some(mips),
                video_w: *vw as f32,
                video_h: *vh as f32,
                cam: timeline::camera_at(project, t),
                pose: timeline::pose_at(project, t),
                touches: &touches,
                time: t,
                since_start: t,
                model: model.as_ref(),
                supersample: 2,
            },
        );
        if out_tx.send(buf).is_err() {
            break;
        }
        written += 1;
        if i % 4 == 0 {
            let frac = written as f32 / total as f32;
            let el = began.elapsed().as_secs_f32();
            let eta = (written > 8).then(|| el / frac - el);
            let _ = tx.send(ExportEvent::Progress { fraction: frac, eta_secs: eta });
        }
    }

    drop(out_tx);
    stop.store(true, Ordering::Relaxed);
    drop(frame_rx);
    let wrote_all = writer.join().unwrap_or(false);
    let _ = reader.join();

    if let Some(reason) = failed {
        let _ = encoder.kill();
        let _ = encoder.wait();
        return Err(reason);
    }
    let status = encoder.wait().map_err(|e| e.to_string())?;
    if !status.success() || !wrote_all {
        let msg = enc_err.lock().unwrap().clone();
        let dmsg = dec_err.lock().unwrap().clone();
        return Err(format!("ffmpeg no pudo crear el vídeo.\n{}{}", msg.trim(), dmsg.trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    /// Full pipeline with ffmpeg. Run with: REC_TEST_VIDEO=/path/video.mp4 cargo test -- --ignored
    #[test]
    #[ignore]
    fn exports_real_video() {
        let Ok(src) = std::env::var("REC_TEST_VIDEO") else { return };
        let info = crate::media::probe(&src).expect("probe");
        // Two clips: the recording from 1 s to 5 s, then the whole recording again.
        let mut project = Project::default();
        let tap = Touch { time: 1.5, duration: 0.1, x: 0.5, y: 0.2, end_x: 0.5, end_y: 0.2, kind: TouchKind::Tap };
        timeline::append_media(&mut project, info.clone(), vec![tap]);
        timeline::append_media(&mut project, info, vec![]);
        let first = project.clips[0].id;
        timeline::trim_clip(&mut project, first, Some(1.0), Some(5.0)).unwrap();
        project.name = "prueba".into();
        project.export.folder = std::env::temp_dir().to_string_lossy().into_owned();
        project.export.resolution = Resolution::P720;
        project.export.fps = 30;
        project.style.turn = -28.0;
        project.style.tilt = 6.0;
        project.style.intro = true;
        // Optionally through an imported model: REC_TEST_MODEL=/path/model.glb
        if let Ok(path) = std::env::var("REC_TEST_MODEL") {
            let src = crate::model3d::load(&path).expect("model");
            project.style.frame = FrameStyle::Custom;
            let animation = std::env::var("REC_TEST_ANIM").ok().and_then(|a| a.parse().ok());
            assert!(src.guess.is_some(), "screen");
            project.style.model = Some(CustomModel { path, animation, ..Default::default() });
        }
        timeline::add_zoom(&mut project, 0.0, 3.0, 2.0, 0.5, 0.2).unwrap();
        timeline::add_move(&mut project, 6.0).unwrap();
        timeline::add_device_preset(&mut project, timeline::DevicePreset::FullTurn, 9.0);
        let out = output_path(&project);
        let (tx, _rx) = crossbeam_channel::unbounded();
        run(&project, &out, &AtomicBool::new(false), &tx).expect("export");
        let probed = crate::media::probe(&out.to_string_lossy()).expect("output readable");
        assert_eq!((probed.width, probed.height), (1280, 720));
        assert!(probed.has_audio);
        assert!((probed.duration - timeline::duration(&project)).abs() < 0.3, "{} vs {}", probed.duration, timeline::duration(&project));
        println!("{}", out.display());
    }
}
