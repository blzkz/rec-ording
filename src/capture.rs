//! Recording an Android device over USB: video with scrcpy (or `adb screenrecord` as a fallback)
//! and finger touches from the kernel input events, so zooms can follow what you tap.

use crate::model::{timestamp_for_filename, videos_dir, Touch, TouchKind};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use crate::platform::command;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tools {
    pub adb: bool,
    pub scrcpy: bool,
    pub ffmpeg: bool,
}

fn runs(cmd: &str, arg: &str) -> bool {
    command(cmd)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn detect_tools() -> Tools {
    Tools { adb: runs("adb", "version"), scrcpy: runs("scrcpy", "--version"), ffmpeg: runs("ffmpeg", "-version") }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Ready,
    Unauthorized,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub model: String,
    pub state: DeviceState,
}

pub fn list_devices() -> Vec<Device> {
    let Ok(out) = command("adb").args(["devices", "-l"]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = match parts.next()? {
                "device" => DeviceState::Ready,
                "unauthorized" => DeviceState::Unauthorized,
                _ => DeviceState::Offline,
            };
            let model = parts
                .find_map(|p| p.strip_prefix("model:"))
                .map(|m| m.replace('_', " "))
                .unwrap_or_else(|| "Dispositivo Android".into());
            Some(Device { serial, model, state })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Scrcpy,
    ScreenRecord,
}

pub struct RecordOptions {
    pub audio: bool,
    pub mirror: bool,
    /// Android's demo mode while recording: fixed clock, full battery, no notifications.
    pub clean_status_bar: bool,
}

/// A gesture played on the phone with `adb shell input`. Coordinates are 0..1 of the screen as
/// it is recorded (0,0 top left).
#[derive(Debug, Clone, PartialEq)]
pub enum Gesture {
    Tap { x: f32, y: f32 },
    /// Also a long press, when it starts and ends in the same place.
    Swipe { x0: f32, y0: f32, x1: f32, y1: f32, duration: f32 },
    Text(String),
    Key(String),
    Wait(f32),
}

/// A gesture played during the recording, timed on the host clock.
#[derive(Debug, Clone, Copy)]
struct Played {
    at: Instant,
    duration: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

/// A touch in the touch panel's own coordinates, timed with the device's kernel clock.
#[derive(Debug, Clone, Copy)]
struct RawTouch {
    t: f64,
    duration: f64,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

#[derive(Default)]
struct TouchLog {
    touches: Vec<RawTouch>,
    /// min(host receive time - kernel time): maps kernel timestamps onto the host clock.
    clock_offset: Option<f64>,
}

pub struct Recording {
    pub path: PathBuf,
    pub method: Method,
    pub started: Instant,
    pub touch_count: Arc<AtomicUsize>,
    serial: String,
    recorder: Child,
    getevent: Option<Child>,
    base: Instant,
    rec_started: Arc<Mutex<Option<Instant>>>,
    log: Arc<Mutex<Vec<String>>>,
    touch_log: Arc<Mutex<TouchLog>>,
    played: Mutex<Vec<Played>>,
    rotation: u8,
    demo: bool,
}

pub enum CaptureResult {
    Done { path: PathBuf, touches: Vec<Touch> },
    Failed(String),
}

fn adb(serial: &str) -> Command {
    let mut c = command("adb");
    c.args(["-s", serial]);
    c
}

fn adb_output(serial: &str, args: &[&str]) -> String {
    adb(serial)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Finds the touchscreen input device and its coordinate range.
fn touch_device(serial: &str) -> Option<(String, f32, f32)> {
    let out = adb_output(serial, &["shell", "getevent", "-lp"]);
    let mut current: Option<String> = None;
    let (mut mx, mut my) = (None, None);
    let max_of = |line: &str| -> Option<f32> {
        let rest = line.split("max").nth(1)?;
        rest.trim().split(|c: char| c == ',' || c.is_whitespace()).next()?.parse().ok()
    };
    for line in out.lines() {
        if let Some(dev) = line.trim().strip_prefix("add device") {
            if let (Some(d), Some(x), Some(y)) = (&current, mx, my) {
                return Some((d.clone(), x, y));
            }
            current = dev.split(':').nth(1).map(|s| s.trim().to_string());
            mx = None;
            my = None;
        } else if line.contains("ABS_MT_POSITION_X") {
            mx = max_of(line);
        } else if line.contains("ABS_MT_POSITION_Y") {
            my = max_of(line);
        }
    }
    match (current, mx, my) {
        (Some(d), Some(x), Some(y)) => Some((d, x, y)),
        _ => None,
    }
}

/// Display rotation (0..3) at the start of the recording.
fn display_rotation(serial: &str) -> u8 {
    let out = adb_output(serial, &["shell", "dumpsys", "input"]);
    for line in out.lines() {
        if let Some(v) = line.split("SurfaceOrientation:").nth(1) {
            if let Ok(r) = v.trim().parse::<u8>() {
                return r % 4;
            }
        }
        if line.contains("Viewport") && line.contains("INTERNAL") {
            if let Some(v) = line.split("orientation=").nth(1) {
                if let Some(r) = v.chars().next().and_then(|c| c.to_digit(10)) {
                    return (r % 4) as u8;
                }
            }
        }
    }
    0
}

/// Turns a stream of `getevent -lt` lines into finger gestures (first finger only).
struct GestureParser {
    max_x: f32,
    max_y: f32,
    slot: i32,
    primary: Option<i32>,
    uses_tracking: bool,
    x: f32,
    y: f32,
    start: Option<(f64, f32, f32)>,
    pending_down: bool,
    pending_up: bool,
}

impl GestureParser {
    fn new(max_x: f32, max_y: f32) -> Self {
        Self {
            max_x: max_x.max(1.0),
            max_y: max_y.max(1.0),
            slot: 0,
            primary: None,
            uses_tracking: false,
            x: 0.5,
            y: 0.5,
            start: None,
            pending_down: false,
            pending_up: false,
        }
    }

    fn feed(&mut self, t: f64, code: &str, value: &str) -> Option<RawTouch> {
        let num = || i64::from_str_radix(value, 16).ok();
        match code {
            "ABS_MT_SLOT" => self.slot = num().unwrap_or(0) as i32,
            "ABS_MT_TRACKING_ID" => {
                self.uses_tracking = true;
                if value == "ffffffff" {
                    if self.primary == Some(self.slot) {
                        self.primary = None;
                        self.pending_up = true;
                    }
                } else if self.primary.is_none() {
                    self.primary = Some(self.slot);
                    self.pending_down = true;
                }
            }
            "ABS_MT_POSITION_X" if self.primary.map_or(true, |p| p == self.slot) => {
                self.x = num().unwrap_or(0) as f32 / self.max_x;
            }
            "ABS_MT_POSITION_Y" if self.primary.map_or(true, |p| p == self.slot) => {
                self.y = num().unwrap_or(0) as f32 / self.max_y;
            }
            "BTN_TOUCH" if !self.uses_tracking => {
                if value == "DOWN" {
                    self.pending_down = true;
                } else if value == "UP" {
                    self.pending_up = true;
                }
            }
            "SYN_REPORT" => {
                if self.pending_down && self.start.is_none() {
                    self.start = Some((t, self.x, self.y));
                }
                self.pending_down = false;
                if self.pending_up {
                    self.pending_up = false;
                    if let Some((t0, x0, y0)) = self.start.take() {
                        return Some(RawTouch { t: t0, duration: t - t0, x0, y0, x1: self.x, y1: self.y });
                    }
                }
            }
            _ => {}
        }
        None
    }
}

fn spawn_touch_reader(
    serial: &str,
    base: Instant,
    touch_log: Arc<Mutex<TouchLog>>,
    count: Arc<AtomicUsize>,
) -> Option<Child> {
    let (dev, max_x, max_y) = touch_device(serial)?;
    // `-tt` gives getevent a pseudo-terminal so it flushes every line.
    let mut child = adb(serial)
        .args(["shell", "-tt", "getevent", "-lt", &dev])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    thread::spawn(move || {
        let mut parser = GestureParser::new(max_x, max_y);
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let host = base.elapsed().as_secs_f64();
            let line = line.trim();
            let Some(rest) = line.strip_prefix('[') else { continue };
            let Some((ts, fields)) = rest.split_once(']') else { continue };
            let Ok(kernel_t) = ts.trim().parse::<f64>() else { continue };
            let mut f = fields.split_whitespace();
            let (Some(_ty), Some(code), Some(value)) = (f.next(), f.next(), f.next()) else { continue };

            let mut log = touch_log.lock().unwrap();
            let off = host - kernel_t;
            log.clock_offset = Some(log.clock_offset.map_or(off, |o: f64| o.min(off)));
            if let Some(t) = parser.feed(kernel_t, code, value) {
                log.touches.push(t);
                count.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    Some(child)
}

fn pipe_lines<R: std::io::Read + Send + 'static>(
    r: R,
    log: Arc<Mutex<Vec<String>>>,
    rec_started: Arc<Mutex<Option<Instant>>>,
) {
    thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            if line.to_lowercase().contains("recording started") {
                rec_started.lock().unwrap().get_or_insert_with(Instant::now);
            }
            let mut l = log.lock().unwrap();
            l.push(line);
            if l.len() > 200 {
                l.remove(0);
            }
        }
    });
}

pub fn start(serial: &str, opts: &RecordOptions, tools: Tools) -> Result<Recording, String> {
    // Windows can't ask scrcpy to stop cleanly (see `finish`), so there it records to MKV, which
    // stays playable when cut off.
    let format = if cfg!(windows) { "mkv" } else { "mp4" };
    let path = videos_dir().join(format!("Grabación {}.{format}", timestamp_for_filename()));
    let base = Instant::now();
    let log = Arc::new(Mutex::new(Vec::new()));
    let rec_started = Arc::new(Mutex::new(None));
    let rotation = display_rotation(serial);
    if opts.clean_status_bar {
        demo_mode(serial, true);
    }

    let (method, mut recorder) = if tools.scrcpy {
        // scrcpy logs to stdout, which is block-buffered on a pipe; stdbuf makes it line-buffered
        // so "Recording started" (our sync point for touches) arrives on time.
        let mut cmd = if runs("stdbuf", "--version") {
            let mut c = command("stdbuf");
            c.args(["-oL", "-eL", "scrcpy"]);
            c
        } else {
            command("scrcpy")
        };
        cmd.args(["-s", serial])
            .arg(format!("--record={}", path.display()))
            .arg(format!("--record-format={format}"))
            .args(["--video-bit-rate=20M", "--max-fps=60", "--stay-awake"]);
        if !opts.audio {
            cmd.arg("--no-audio");
        }
        if opts.mirror {
            cmd.arg("--window-title=rec·ording — grabando");
        } else {
            cmd.arg("--no-window");
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("No se pudo iniciar scrcpy: {e}"))?;
        (Method::Scrcpy, child)
    } else {
        let child = adb(serial)
            .args(["shell", "screenrecord", "--bit-rate", "20000000", "/sdcard/rec-ording.mp4"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("No se pudo iniciar adb screenrecord: {e}"))?;
        (Method::ScreenRecord, child)
    };
    if let Some(o) = recorder.stdout.take() {
        pipe_lines(o, log.clone(), rec_started.clone());
    }
    if let Some(e) = recorder.stderr.take() {
        pipe_lines(e, log.clone(), rec_started.clone());
    }

    let touch_log = Arc::new(Mutex::new(TouchLog::default()));
    let touch_count = Arc::new(AtomicUsize::new(0));
    let getevent = spawn_touch_reader(serial, base, touch_log.clone(), touch_count.clone());

    Ok(Recording {
        path,
        method,
        started: Instant::now(),
        touch_count,
        serial: serial.to_string(),
        recorder,
        getevent,
        base,
        rec_started,
        log,
        touch_log,
        played: Mutex::new(Vec::new()),
        rotation,
        demo: opts.clean_status_bar,
    })
}

/// Turns Android's demo mode on (fixed 9:41, full battery and signal, no notifications) or off.
pub fn demo_mode(serial: &str, on: bool) {
    let shell = |cmd: &str| {
        let _ = adb(serial).args(["shell", cmd]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    };
    let demo = |args: &str| shell(&format!("am broadcast -a com.android.systemui.demo -e command {args}"));
    if on {
        shell("settings put global sysui_demo_allowed 1");
        demo("enter");
        demo("clock -e hhmm 0941");
        demo("battery -e level 100 -e plugged false -e powersave false");
        demo("network -e wifi show -e level 4 -e mobile show -e datatype none -e level 4");
        demo("notifications -e visible false");
        demo("status -e bluetooth hide -e alarm hide -e volume hide -e location hide -e mute hide -e speakerphone hide -e zen hide -e sync hide -e tty hide -e eri hide");
    } else {
        demo("exit");
    }
}

/// The screen size in pixels as it is shown now (width and height swap when turned).
fn screen_size(serial: &str) -> Option<(f32, f32)> {
    let out = adb_output(serial, &["shell", "wm", "size"]);
    let size = |line: &str| -> Option<(f32, f32)> {
        let (w, h) = line.split(':').nth(1)?.trim().split_once('x')?;
        Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
    };
    let (w, h) = out.lines().filter(|l| l.contains("Override size")).find_map(size).or_else(|| out.lines().find_map(size))?;
    Some(if display_rotation(serial) % 2 == 1 { (h, w) } else { (w, h) })
}

/// Quotes a string for the phone's shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn keycode(key: &str) -> String {
    let k = key.trim();
    let name = match k.to_lowercase().as_str() {
        "back" | "atrás" => "BACK",
        "home" | "inicio" => "HOME",
        "enter" | "intro" => "ENTER",
        "delete" | "backspace" | "borrar" => "DEL",
        "tab" => "TAB",
        "recents" | "app_switch" | "recientes" => "APP_SWITCH",
        "menu" => "MENU",
        "search" => "SEARCH",
        "volume_up" => "VOLUME_UP",
        "volume_down" => "VOLUME_DOWN",
        "power" => "POWER",
        "up" => "DPAD_UP",
        "down" => "DPAD_DOWN",
        "left" => "DPAD_LEFT",
        "right" => "DPAD_RIGHT",
        _ => "",
    };
    if !name.is_empty() {
        format!("KEYCODE_{name}")
    } else if k.chars().all(|c| c.is_ascii_digit()) || k.starts_with("KEYCODE_") {
        k.to_string()
    } else {
        format!("KEYCODE_{}", k.to_uppercase())
    }
}

/// Plays gestures on a phone one after another. `touched` gets each tap and swipe as it lands
/// on the screen. `input` takes a moment to start on the phone, so a gesture lands at the end
/// of its command: that is when it is timed.
pub fn play(serial: &str, gestures: &[Gesture], mut touched: impl FnMut(Instant, f32, [f32; 4])) -> Result<usize, String> {
    let needs_size = gestures.iter().any(|g| matches!(g, Gesture::Tap { .. } | Gesture::Swipe { .. }));
    let (w, h) = if needs_size { screen_size(serial).ok_or("No se pudo leer el tamaño de la pantalla del móvil")? } else { (1.0, 1.0) };
    let px = |x: f32, y: f32| ((x.clamp(0.0, 1.0) * (w - 1.0)).round() as i32, (y.clamp(0.0, 1.0) * (h - 1.0)).round() as i32);
    let run = |cmd: String| -> Result<Instant, String> {
        let out = adb(serial).args(["shell", &cmd]).output().map_err(|e| format!("adb: {e}"))?;
        let err = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() || err.contains("Exception") || err.contains("Error") {
            return Err(format!("El móvil no aceptó «{cmd}»: {}", err.trim()));
        }
        Ok(Instant::now())
    };
    let mut done = 0;
    for g in gestures {
        match g {
            Gesture::Tap { x, y } => {
                let (a, b) = px(*x, *y);
                let end = run(format!("input tap {a} {b}"))?;
                touched(end - Duration::from_millis(80), 0.1, [*x, *y, *x, *y]);
            }
            Gesture::Swipe { x0, y0, x1, y1, duration } => {
                let d = duration.clamp(0.05, 10.0);
                let ((a, b), (c, e)) = (px(*x0, *y0), px(*x1, *y1));
                let end = run(format!("input swipe {a} {b} {c} {e} {}", (d * 1000.0) as u32))?;
                touched(end - Duration::from_secs_f32(d + 0.05), d, [*x0, *y0, *x1, *y1]);
            }
            Gesture::Text(t) => {
                // `input text` takes %s for spaces.
                run(format!("input text {}", shell_quote(&t.replace(' ', "%s"))))?;
            }
            Gesture::Key(k) => {
                run(format!("input keyevent {}", keycode(k)))?;
            }
            Gesture::Wait(s) => thread::sleep(Duration::from_secs_f32(s.clamp(0.0, 30.0))),
        }
        done += 1;
    }
    Ok(done)
}

impl Recording {
    /// Plays gestures on the phone being recorded and keeps them as touches of the recording.
    pub fn play(&self, gestures: &[Gesture]) -> Result<usize, String> {
        play(&self.serial, gestures, |at, duration, [x0, y0, x1, y1]| {
            self.played.lock().unwrap().push(Played { at, duration, x0, y0, x1, y1 });
            self.touch_count.fetch_add(1, Ordering::Relaxed);
        })
    }

    pub fn tracks_touches(&self) -> bool {
        self.getevent.is_some()
    }

    /// If the recorder died on its own (cable unplugged, permission refused…), why.
    pub fn check_exited(&mut self) -> Option<String> {
        match self.recorder.try_wait() {
            Ok(Some(_)) => Some(self.log_tail()),
            _ => None,
        }
    }

    fn log_tail(&self) -> String {
        let l = self.log.lock().unwrap();
        let tail: Vec<&String> = l.iter().rev().take(6).collect();
        tail.into_iter().rev().cloned().collect::<Vec<_>>().join("\n")
    }

    /// Stops recording in the background; the result arrives on `done`.
    pub fn stop(mut self, done: impl FnOnce(CaptureResult) + Send + 'static) {
        thread::spawn(move || {
            let result = self.finish();
            done(result);
        });
    }

    /// Stops and waits (used when the app closes mid-recording).
    pub fn stop_blocking(mut self) -> CaptureResult {
        self.finish()
    }

    fn finish(&mut self) -> CaptureResult {
        let pid = self.recorder.id().to_string();
        match self.method {
            Method::Scrcpy => {
                // SIGINT lets scrcpy write the end of the MP4 file properly. Windows has no
                // such signal for another process; the MKV it records there survives a kill.
                if cfg!(windows) {
                    let _ = self.recorder.kill();
                } else {
                    let _ = command("kill").args(["-INT", &pid]).status();
                }
            }
            Method::ScreenRecord => {
                let _ = adb(&self.serial).args(["shell", "pkill", "-INT", "screenrecord"]).status();
            }
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.recorder.try_wait() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.recorder.kill();
        let _ = self.recorder.wait();
        if self.demo {
            demo_mode(&self.serial, false);
        }
        if let Some(mut g) = self.getevent.take() {
            let _ = g.kill();
            let _ = g.wait();
            // Killing the local adb client does not always end getevent on the phone.
            let _ = adb(&self.serial).args(["shell", "pkill", "getevent"]).status();
        }

        if self.method == Method::ScreenRecord {
            thread::sleep(Duration::from_millis(600));
            let pulled = adb(&self.serial)
                .args(["pull", "/sdcard/rec-ording.mp4"])
                .arg(&self.path)
                .output();
            let _ = adb(&self.serial).args(["shell", "rm", "-f", "/sdcard/rec-ording.mp4"]).status();
            if !matches!(pulled, Ok(ref o) if o.status.success()) {
                return CaptureResult::Failed("No se pudo copiar el vídeo desde el móvil.".into());
            }
        }

        let ok = std::fs::metadata(&self.path).map(|m| m.len() > 1024).unwrap_or(false);
        if !ok {
            let tail = self.log_tail();
            return CaptureResult::Failed(if tail.is_empty() {
                "La grabación no se guardó.".into()
            } else {
                format!("La grabación no se guardó.\n\n{tail}")
            });
        }

        // Video time 0 is when the recorder started writing frames.
        let fallback = Duration::from_millis(if self.method == Method::Scrcpy { 900 } else { 400 });
        let video_start = self
            .rec_started
            .lock()
            .unwrap()
            .map(|i| i.duration_since(self.base))
            .unwrap_or(self.started.duration_since(self.base) + fallback)
            .as_secs_f64();

        let log = self.touch_log.lock().unwrap();
        let offset = log.clock_offset.unwrap_or(0.0);
        let rot = self.rotation;
        let map = |x: f32, y: f32| -> (f32, f32) {
            let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            match rot {
                1 => (y, 1.0 - x),
                2 => (1.0 - x, 1.0 - y),
                3 => (1.0 - y, x),
                _ => (x, y),
            }
        };
        let touches = log
            .touches
            .iter()
            .filter_map(|r| {
                let time = (r.t + offset - video_start) as f32;
                if time < -0.3 {
                    return None;
                }
                let (x, y) = map(r.x0, r.y0);
                let (end_x, end_y) = map(r.x1, r.y1);
                let moved = ((end_x - x).powi(2) + (end_y - y).powi(2)).sqrt();
                Some(Touch {
                    time: time.max(0.0),
                    duration: (r.duration as f32).clamp(0.05, 3.0),
                    x,
                    y,
                    end_x,
                    end_y,
                    kind: if moved > 0.04 { TouchKind::Swipe } else { TouchKind::Tap },
                })
            })
            .collect::<Vec<_>>();
        let mut touches = touches;
        for g in self.played.lock().unwrap().iter() {
            let time = (g.at.duration_since(self.base).as_secs_f64() - video_start) as f32;
            if time < -0.3 {
                continue;
            }
            let moved = ((g.x1 - g.x0).powi(2) + (g.y1 - g.y0).powi(2)).sqrt();
            touches.push(Touch {
                time: time.max(0.0),
                duration: g.duration.clamp(0.05, 3.0),
                x: g.x0,
                y: g.y0,
                end_x: g.x1,
                end_y: g.y1,
                kind: if moved > 0.04 { TouchKind::Swipe } else { TouchKind::Tap },
            });
        }
        touches.sort_by(|a, b| a.time.total_cmp(&b.time));

        CaptureResult::Done { path: self.path.clone(), touches }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &str) -> Vec<RawTouch> {
        let mut p = GestureParser::new(1079.0, 2399.0);
        lines
            .lines()
            .filter_map(|l| {
                let rest = l.trim().strip_prefix('[')?;
                let (ts, fields) = rest.split_once(']')?;
                let mut f = fields.split_whitespace();
                let (_, code, value) = (f.next()?, f.next()?, f.next()?);
                p.feed(ts.trim().parse().ok()?, code, value)
            })
            .collect()
    }

    #[test]
    fn parses_tap_and_swipe() {
        let out = run("
[  100.000000] EV_ABS       ABS_MT_TRACKING_ID   00000010
[  100.000000] EV_ABS       ABS_MT_POSITION_X    0000021c
[  100.000000] EV_ABS       ABS_MT_POSITION_Y    000004b0
[  100.000000] EV_KEY       BTN_TOUCH            DOWN
[  100.000000] EV_SYN       SYN_REPORT           00000000
[  100.080000] EV_ABS       ABS_MT_TRACKING_ID   ffffffff
[  100.080000] EV_KEY       BTN_TOUCH            UP
[  100.080000] EV_SYN       SYN_REPORT           00000000
[  101.000000] EV_ABS       ABS_MT_TRACKING_ID   00000011
[  101.000000] EV_ABS       ABS_MT_POSITION_X    0000021c
[  101.000000] EV_ABS       ABS_MT_POSITION_Y    00000800
[  101.000000] EV_SYN       SYN_REPORT           00000000
[  101.100000] EV_ABS       ABS_MT_POSITION_Y    00000400
[  101.100000] EV_SYN       SYN_REPORT           00000000
[  101.300000] EV_ABS       ABS_MT_TRACKING_ID   ffffffff
[  101.300000] EV_SYN       SYN_REPORT           00000000
");
        assert_eq!(out.len(), 2);
        let tap = out[0];
        assert!((tap.t - 100.0).abs() < 1e-6 && (tap.duration - 0.08).abs() < 1e-6);
        assert!((tap.x0 - 540.0 / 1079.0).abs() < 1e-4 && (tap.y0 - 1200.0 / 2399.0).abs() < 1e-4);
        let swipe = out[1];
        assert!(swipe.y0 > 0.8 && swipe.y1 < 0.45, "{swipe:?}");
    }

    /// Real device. Run with: REC_TEST_SERIAL=<serial> cargo test records_from_device -- --ignored
    #[test]
    #[ignore]
    fn records_from_device() {
        let Ok(serial) = std::env::var("REC_TEST_SERIAL") else { return };
        let rec = start(&serial, &RecordOptions { audio: false, mirror: false, clean_status_bar: false }, detect_tools()).expect("start");
        assert!(rec.tracks_touches(), "touchscreen not found");
        thread::sleep(Duration::from_secs(4));
        assert!(rec.rec_started.lock().unwrap().is_some(), "recorder never reported it started");
        match rec.stop_blocking() {
            CaptureResult::Done { path, .. } => {
                let info = crate::media::probe(&path.to_string_lossy()).expect("valid mp4");
                assert!(info.duration > 2.0, "{info:?}");
                println!("{} {}x{} {:.1}s", path.display(), info.width, info.height, info.duration);
                std::fs::remove_file(path).unwrap();
            }
            CaptureResult::Failed(e) => panic!("{e}"),
        }
    }

    #[test]
    fn gesture_commands() {
        assert_eq!(keycode("back"), "KEYCODE_BACK");
        assert_eq!(keycode("Borrar"), "KEYCODE_DEL");
        assert_eq!(keycode("66"), "66");
        assert_eq!(keycode("volume_mute"), "KEYCODE_VOLUME_MUTE");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    /// Needs a phone: sends a key that does nothing and reads the screen size.
    #[test]
    #[ignore]
    fn plays_on_device() {
        let serial = list_devices().into_iter().find(|d| d.state == DeviceState::Ready).expect("no device").serial;
        assert!(screen_size(&serial).is_some());
        let mut n = 0;
        assert_eq!(play(&serial, &[Gesture::Key("unknown".into()), Gesture::Wait(0.1)], |_, _, _| n += 1), Ok(2));
        assert_eq!(n, 0);
    }

    #[test]
    fn second_finger_does_not_end_gesture() {
        let out = run("
[  1.0] EV_ABS ABS_MT_SLOT 00000000
[  1.0] EV_ABS ABS_MT_TRACKING_ID 00000001
[  1.0] EV_ABS ABS_MT_POSITION_X 00000100
[  1.0] EV_ABS ABS_MT_POSITION_Y 00000100
[  1.0] EV_SYN SYN_REPORT 00000000
[  1.1] EV_ABS ABS_MT_SLOT 00000001
[  1.1] EV_ABS ABS_MT_TRACKING_ID 00000002
[  1.1] EV_SYN SYN_REPORT 00000000
[  1.2] EV_ABS ABS_MT_TRACKING_ID ffffffff
[  1.2] EV_SYN SYN_REPORT 00000000
[  1.5] EV_ABS ABS_MT_SLOT 00000000
[  1.5] EV_ABS ABS_MT_TRACKING_ID ffffffff
[  1.5] EV_SYN SYN_REPORT 00000000
");
        assert_eq!(out.len(), 1);
        assert!((out[0].duration - 0.5).abs() < 1e-6);
    }
}
