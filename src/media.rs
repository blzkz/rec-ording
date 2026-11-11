//! Reading videos with ffprobe/ffmpeg: metadata, a light preview proxy and a background decoder.

use crate::compositor::{Frame, Mips};
use crate::model::{cache_dir, MediaInfo};
use crossbeam_channel::{bounded, unbounded, Receiver, SendTimeoutError, Sender};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use crate::platform::command;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use crate::i18n::lz;

pub const PREVIEW_FPS: f32 = 30.0;
const PREVIEW_MAX_SIDE: f32 = 1280.0;

pub fn probe(path: &str) -> Result<MediaInfo, String> {
    let out = command("ffprobe")
        .args(["-v", "error", "-show_streams", "-show_format", "-of", "json", path])
        .output()
        .map_err(|e| tr!("No se pudo ejecutar ffprobe ({}). ¿Está instalado ffmpeg?", e))?;
    if !out.status.success() {
        return Err(tr!(
            "No se pudo leer el vídeo: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let v = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or(lz("El archivo no contiene vídeo"))?;

    let mut width = v["width"].as_u64().unwrap_or(0) as u32;
    let mut height = v["height"].as_u64().unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err(lz("No se pudo leer el tamaño del vídeo").into());
    }

    let rotation = v["side_data_list"]
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()))
        .or_else(|| v["tags"]["rotate"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0) as i32;
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }

    let parse_rate = |s: &str| -> Option<f32> {
        let (n, d) = s.split_once('/')?;
        let (n, d): (f32, f32) = (n.parse().ok()?, d.parse().ok()?);
        (d > 0.0 && n > 0.0).then(|| n / d)
    };
    let fps = v["avg_frame_rate"]
        .as_str()
        .and_then(parse_rate)
        .or_else(|| v["r_frame_rate"].as_str().and_then(parse_rate))
        .filter(|f| *f > 1.0 && *f < 241.0)
        .unwrap_or(30.0);

    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f32>().ok())
        .or_else(|| v["duration"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0);
    if duration <= 0.05 {
        return Err(lz("El vídeo está vacío o dañado").into());
    }

    Ok(MediaInfo {
        path: path.to_string(),
        width,
        height,
        duration,
        fps,
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
    })
}

/// Size the preview works at: the recording scaled down so its longest side is at most 1280 px.
/// Moments where the picture changes a lot (scrolling, the keyboard coming up, a dialog…), in
/// seconds of the video, at least 0,4 s apart. Reads a small, fast version of the frames.
pub fn screen_changes(path: &str) -> Vec<f32> {
    let Ok(out) = command("ffmpeg")
        .args(["-nostdin", "-i", path, "-vf", "scale=180:-2,select='gt(scene,0.05)',showinfo", "-an", "-f", "null", "-"])
        .stdin(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let log = String::from_utf8_lossy(&out.stderr);
    let mut marks: Vec<f32> = Vec::new();
    for t in log.split("pts_time:").skip(1).filter_map(|rest| rest.split_whitespace().next()?.parse::<f32>().ok()) {
        if marks.last().is_none_or(|&m| t - m >= 0.4) {
            marks.push(t);
        }
    }
    marks
}

/// Start time and keyframes (seconds) of a video's first video stream, cached per file.
fn keyframes(path: &str) -> (f64, Vec<f64>) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, (f64, Vec<f64>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(k) = cache.lock().unwrap().get(path) {
        return k.clone();
    }
    let out = command("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=start_time:packet=pts_time,flags", "-of", "json", path])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok());
    let num = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok());
    let start = out.as_ref().and_then(|v| num(&v["streams"][0]["start_time"])).unwrap_or(0.0);
    let mut keys: Vec<f64> = out
        .as_ref()
        .and_then(|v| v["packets"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|p| p["flags"].as_str().is_some_and(|f| f.starts_with('K')))
        .filter_map(|p| num(&p["pts_time"]))
        .collect();
    keys.sort_by(f64::total_cmp);
    let k = (start, keys);
    cache.lock().unwrap().insert(path.to_string(), k.clone());
    k
}

/// ffmpeg input options and the start of the video filter chain that read `path` from `from`
/// for `len` seconds at `fps`, with the frames where they really are in time. Recordings of a
/// still screen have gaps with no frames: plain seeking jumps to the next keyframe after the gap
/// and shifts everything, so this seeks to the keyframe before `from`, keeps the original
/// timestamps and lets `fps` hold the last frame through the gap.
/// `speed` plays it faster or slower (`len` is still seconds of the recording).
pub fn timed_input(path: &str, from: f32, len: f32, fps: f32, speed: f32) -> (Vec<String>, String) {
    let (start, keys) = keyframes(path);
    let at = start + from.max(0.0) as f64;
    let seek = keys.iter().rev().find(|&&k| k <= at + 1e-6).copied().unwrap_or(start);
    let input = vec!["-ss".into(), format!("{seek:.6}"), "-copyts".into(), "-i".into(), path.into()];
    // Sample the recording at the rate that gives `fps` once sped up.
    let speed = speed.max(0.01);
    let filter = format!("fps={:.6},trim=start={at:.6}:duration={len:.6},setpts=(PTS-STARTPTS)/{speed:.6}", fps / speed);
    (input, filter)
}

pub fn preview_size(info: &MediaInfo) -> (usize, usize) {
    let (w, h) = (info.width as f32, info.height as f32);
    let k = (PREVIEW_MAX_SIDE / w.max(h)).min(1.0);
    let even = |v: f32| (((v * k).round() as usize) / 2 * 2).max(2);
    (even(w), even(h))
}

fn proxy_path(info: &MediaInfo) -> PathBuf {
    let mut h = DefaultHasher::new();
    info.path.hash(&mut h);
    if let Ok(meta) = std::fs::metadata(&info.path) {
        meta.len().hash(&mut h);
        meta.modified().ok().hash(&mut h);
    }
    let dir = cache_dir().join("proxies");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{:016x}.mp4", h.finish()))
}

/// Transcodes the recording into a small, seek-friendly copy (short keyframe interval) so
/// scrubbing the timeline is instant. Cached across sessions.
pub fn build_proxy(info: &MediaInfo) -> Result<PathBuf, String> {
    let path = proxy_path(info);
    if path.exists() {
        return Ok(path);
    }
    let (w, h) = preview_size(info);
    let tmp = path.with_extension("tmp.mp4");
    let out = command("ffmpeg")
        .args(["-nostdin", "-y", "-loglevel", "error", "-i", &info.path])
        .args(["-vf", &format!("scale={w}:{h},fps={PREVIEW_FPS}")])
        .args(["-an", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "20", "-g", "10"])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&tmp)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Small RGBA stills spread evenly over the clip, for the timeline filmstrip.
pub fn thumbnails(path: &Path, duration: f32, aspect: f32, count: usize, height: usize) -> Option<((usize, usize), Vec<Vec<u8>>)> {
    let h = height.max(2);
    let w = (((h as f32 * aspect).round() as usize) / 2 * 2).max(2);
    let rate = count as f32 / duration.max(0.1);
    let out = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vf", &format!("fps={rate:.4},scale={w}:{h}"), "-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()
        .ok()?;
    let size = w * h * 4;
    let frames: Vec<Vec<u8>> = out.stdout.chunks_exact(size).map(|c| c.to_vec()).collect();
    (!frames.is_empty()).then_some(((w, h), frames))
}

/// A decoded preview frame. `gen` identifies the request it answers, so stale frames from
/// an earlier seek can be ignored.
pub struct DecodedFrame {
    pub gen: u64,
    pub time: f32,
    pub mips: Arc<Mips>,
}

struct Source {
    path: String,
    w: usize,
    h: usize,
}

enum Req {
    Source(Source),
    Seek { gen: u64, t: f32 },
    Play { gen: u64, t: f32, speed: f32 },
    Stop,
}

/// Decodes preview frames on a background thread: single frames while scrubbing, a stream
/// of frames while playing.
pub struct PreviewDecoder {
    tx: Sender<Req>,
    pub frames: Receiver<DecodedFrame>,
}

impl PreviewDecoder {
    /// `notify` is called whenever a frame is ready (to wake up the UI).
    pub fn new(notify: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = unbounded();
        let (ftx, frx) = bounded(6);
        thread::spawn(move || decoder_thread(rx, ftx, notify));
        Self { tx, frames: frx }
    }

    pub fn set_source(&self, path: &str, size: (usize, usize)) {
        let _ = self.tx.send(Req::Source(Source { path: path.into(), w: size.0, h: size.1 }));
    }

    pub fn seek(&self, gen: u64, t: f32) {
        let _ = self.tx.send(Req::Seek { gen, t });
    }

    /// Plays from recording second `t` at `speed` (frames report recording seconds).
    pub fn play(&self, gen: u64, t: f32, speed: f32) {
        let _ = self.tx.send(Req::Play { gen, t, speed });
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Req::Stop);
    }
}

fn spawn_decoder(src: &Source, t: f32, single: bool, speed: f32) -> Option<Child> {
    let mut cmd = command("ffmpeg");
    cmd.args(["-nostdin", "-loglevel", "error", "-ss", &format!("{:.3}", t.max(0.0)), "-i", &src.path]);
    if single {
        cmd.args(["-frames:v", "1", "-vf", &format!("scale={}:{}", src.w, src.h)]);
    } else {
        cmd.args(["-vf", &format!("setpts=(PTS-STARTPTS)/{:.4},scale={}:{},fps={PREVIEW_FPS}", speed.max(0.01), src.w, src.h)]);
    }
    cmd.args(["-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

fn read_frame(child: &mut Child, w: usize, h: usize) -> Option<Frame> {
    let mut data = vec![0u8; w * h * 4];
    child.stdout.as_mut()?.read_exact(&mut data).ok()?;
    Some(Frame { w, h, data })
}

fn finish(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn decoder_thread(rx: Receiver<Req>, out: Sender<DecodedFrame>, notify: impl Fn()) {
    let mut src: Option<Source> = None;
    let mut pending: Option<Req> = None;

    loop {
        let first = match pending.take() {
            Some(r) => r,
            None => match rx.recv() {
                Ok(r) => r,
                Err(_) => return,
            },
        };
        // Only the newest command matters (fast scrubbing sends many).
        let mut cmd = None;
        let mut take = |r: Req| match r {
            Req::Source(s) => src = Some(s),
            other => cmd = Some(other),
        };
        take(first);
        while let Ok(r) = rx.try_recv() {
            take(r);
        }
        let (Some(cmd), Some(s)) = (cmd, src.as_ref()) else { continue };

        match cmd {
            Req::Seek { gen, t } => {
                let Some(mut child) = spawn_decoder(s, t, true, 1.0) else { continue };
                let frame = read_frame(&mut child, s.w, s.h);
                finish(child);
                if let Some(f) = frame {
                    let item = DecodedFrame { gen, time: t, mips: Arc::new(Mips::build(f)) };
                    if out.send(item).is_err() {
                        return;
                    }
                    notify();
                }
            }
            Req::Play { gen, t, speed } => {
                let Some(mut child) = spawn_decoder(s, t, false, speed) else { continue };
                let mut n = 0u32;
                'play: while let Some(f) = read_frame(&mut child, s.w, s.h) {
                    let mut item =
                        DecodedFrame { gen, time: t + n as f32 * speed / PREVIEW_FPS, mips: Arc::new(Mips::build(f)) };
                    n += 1;
                    loop {
                        if let Ok(r) = rx.try_recv() {
                            pending = Some(r);
                            break 'play;
                        }
                        match out.send_timeout(item, Duration::from_millis(15)) {
                            Ok(()) => {
                                notify();
                                break;
                            }
                            Err(SendTimeoutError::Timeout(back)) => item = back,
                            Err(SendTimeoutError::Disconnected(_)) => return,
                        }
                    }
                }
                finish(child);
            }
            Req::Stop | Req::Source(_) => {}
        }
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    /// A recording of a still screen: frames until 1 s, then nothing until 10,3 s.
    #[test]
    fn keeps_the_last_frame_through_gaps() {
        if command("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rec-ording-gap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("gap.mp4");
        let ok = command("ffmpeg")
            .args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=64x48:r=30:d=1", "-f", "lavfi", "-i", "color=c=blue:s=64x48:r=30:d=1"])
            .args(["-filter_complex", "[0]setpts=PTS-STARTPTS[a];[1]setpts=PTS-STARTPTS+9.3/TB[b];[a][b]concat=n=2:v=1[v]", "-map", "[v]"])
            .args(["-fps_mode", "passthrough", "-c:v", "libx264", "-g", "600"])
            .arg(&path)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok);
        let read = |from: f32, len: f32| {
            let (input, timed) = timed_input(&path.to_string_lossy(), from, len, 30.0, 1.0);
            let out = command("ffmpeg").args(["-nostdin", "-loglevel", "error"]).args(input).args(["-vf", &timed, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]).output().unwrap();
            let frames: Vec<[u8; 3]> = out.stdout.chunks(64 * 48 * 3).map(|f| [f[0], f[1], f[2]]).collect();
            frames
        };
        // In the gap: the red frame from before it, for the whole length.
        let gap = read(3.0, 4.0);
        assert_eq!(gap.len(), 120);
        assert!(gap.iter().all(|c| c[0] > 200 && c[2] < 50), "{:?}", &gap[..3]);
        // Across the end of the gap: red, then blue at 10,3 s.
        let across = read(10.0, 1.0);
        assert!(across[0][0] > 200 && across.last().unwrap()[2] > 200, "{:?} {:?}", across[0], across.last());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
