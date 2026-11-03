//! Final render: decode the recording at full size, compose every frame and encode with ffmpeg.

use crate::camera::CameraTrack;
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
    let media = project.media.as_ref().ok_or("No hay ningún vídeo en el proyecto")?;
    let settings = &project.export;
    let (ow, oh) = settings.dimensions(project.style.aspect);
    let (ow, oh) = (ow as usize, oh as usize);
    let fps = if settings.format == ExportFormat::Gif { settings.fps.min(25) } else { settings.fps };
    let start = project.trim_start;
    let dur = project.trimmed_duration().max(0.1);
    let total = ((dur * fps as f32).round() as usize).max(1);
    let (vw, vh) = (media.width as usize, media.height as usize);

    let ss = format!("{start:.3}");
    let tt = format!("{dur:.3}");

    // Decoder: full-size RGBA frames at the output frame rate.
    let mut decoder = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-ss", &ss, "-t", &tt, "-i", &media.path])
        .args(["-vf", &format!("scale={vw}:{vh},fps={fps}"), "-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("No se pudo iniciar ffmpeg: {e}"))?;
    let dec_err = collect_stderr(&mut decoder);

    // Encoder
    let mut enc = Command::new("ffmpeg");
    enc.args(["-nostdin", "-y", "-loglevel", "error"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{ow}x{oh}"), "-r", &fps.to_string(), "-i", "-"]);
    let audio = settings.include_audio && media.has_audio && settings.format != ExportFormat::Gif;
    if audio {
        enc.args(["-ss", &ss, "-t", &tt, "-i", &media.path, "-map", "0:v", "-map", "1:a?"]);
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

    // Pipeline: reader thread -> compose (here, parallel per row) -> writer thread.
    let (frame_tx, frame_rx) = bounded::<Frame>(3);
    let mut dec_out = decoder.stdout.take().ok_or("ffmpeg sin salida")?;
    let reader = thread::spawn(move || loop {
        let mut data = vec![0u8; vw * vh * 4];
        if dec_out.read_exact(&mut data).is_err() {
            break;
        }
        if frame_tx.send(Frame { w: vw, h: vh, data }).is_err() {
            break;
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

    let track = CameraTrack::build(&project.zooms, 0.0, media.duration);
    let model = match (&project.style.frame, &project.style.model) {
        (crate::model::FrameStyle::Custom, Some(cm)) => {
            let src = crate::model3d::load(&cm.path)?;
            let part = cm.screen(src.guess, src.parts.len()).ok_or("No se encontró la pantalla del modelo: elígela en «Pantalla»")?;
            Some(crate::model3d::prepare(&src, part, cm.quarter_turns)?)
        }
        _ => None,
    };
    let began = Instant::now();
    let mut written = 0usize;
    let mut last_mips: Option<Mips> = None;
    let mut failed = None;

    for i in 0..total {
        if cancel.load(Ordering::Relaxed) {
            failed = Some("cancelado".to_string());
            break;
        }
        // Reuse the last frame if the source runs a few frames short.
        if let Ok(f) = frame_rx.recv() {
            last_mips = Some(Mips::build(f));
        }
        let Some(mips) = last_mips.as_ref() else {
            failed = Some("No se pudo decodificar el vídeo".to_string());
            break;
        };
        let t = start + i as f32 / fps as f32;
        let mut buf = vec![0u8; ow * oh * 4];
        compose(
            &mut buf,
            ow,
            oh,
            &ComposeInput {
                style: &project.style,
                screen: Some(mips),
                video_w: vw as f32,
                video_h: vh as f32,
                cam: track.sample(t),
                touches: &project.touches,
                time: t,
                since_start: t - start,
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
    drop(frame_rx);
    let wrote_all = writer.join().unwrap_or(false);
    let _ = decoder.kill();
    let _ = decoder.wait();
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
        let mut project = Project::default().with_new_media(info, vec![]);
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
        project.zooms.push(ZoomSegment { id: 1, start: 1.0, end: 4.0, scale: 2.0, x: 0.5, y: 0.2, auto: false });
        project.touches.push(Touch { time: 1.5, duration: 0.1, x: 0.5, y: 0.2, end_x: 0.5, end_y: 0.2, kind: TouchKind::Tap });
        let out = output_path(&project);
        let (tx, _rx) = crossbeam_channel::unbounded();
        run(&project, &out, &AtomicBool::new(false), &tx).expect("export");
        let probed = crate::media::probe(&out.to_string_lossy()).expect("output readable");
        assert_eq!((probed.width, probed.height), (1280, 720));
        assert!(probed.has_audio);
        assert!((probed.duration - project.trimmed_duration()).abs() < 0.3);
        println!("{}", out.display());
    }
}
