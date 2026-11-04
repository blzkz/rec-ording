//! The timeline: where each clip plays, what the camera and the device do at any instant, and
//! every editing operation. The app and the MCP server both edit projects through these
//! functions, so an agent can do exactly what the user can.

use crate::compositor::Layout;
use crate::model::*;
use crate::model3d::ModelMeta;
use crate::text;

// ---------- Video track ----------

/// A clip with its place on the timeline.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub index: usize,
    pub clip: Clip,
    pub start: f32,
    pub end: f32,
}

pub fn placed(p: &Project) -> Vec<Placed> {
    let mut t = 0.0;
    p.clips
        .iter()
        .enumerate()
        .map(|(index, c)| {
            let start = t;
            t += c.duration();
            Placed { index, clip: *c, start, end: t }
        })
        .collect()
}

pub fn duration(p: &Project) -> f32 {
    p.clips.iter().map(|c| c.duration()).sum()
}

/// The clip under timeline second `t` (the last one at the very end).
pub fn locate(p: &Project, t: f32) -> Option<Placed> {
    let all = placed(p);
    all.iter().find(|c| t >= c.start && t < c.end).or(all.last().filter(|c| t >= c.end)).copied()
}

/// Recording second shown at timeline second `t`.
pub fn source_time(c: &Placed, t: f32) -> f32 {
    (c.clip.source_in + (t - c.start)).clamp(c.clip.source_in, c.clip.source_out)
}

/// Every touch of the clips on the timeline, in timeline seconds.
pub fn timeline_touches(p: &Project) -> Vec<Touch> {
    let mut out = Vec::new();
    for c in placed(p) {
        let Some(m) = p.media(c.clip.media) else { continue };
        for t in &m.touches {
            if t.time >= c.clip.source_in && t.time < c.clip.source_out {
                out.push(Touch { time: c.start + t.time - c.clip.source_in, ..*t });
            }
        }
    }
    out
}

// ---------- Evaluation ----------

/// Progress 0..1 between two keyframes, shaped by the easing of the one being reached.
pub fn ease(e: Ease, f: f32) -> f32 {
    let f = f.clamp(0.0, 1.0);
    match e {
        Ease::Spring => {
            // Critically damped spring, normalized to land exactly on the keyframe.
            let k = 5.0f32;
            (1.0 - (1.0 + k * f) * (-k * f).exp()) / (1.0 - (1.0 + k) * (-k).exp())
        }
        Ease::Smooth => {
            if f < 0.5 {
                4.0 * f * f * f
            } else {
                1.0 - (-2.0 * f + 2.0).powi(3) / 2.0
            }
        }
        Ease::Linear => f,
        Ease::Hold => {
            if f >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// What the camera does at an instant: zoom and focus (as before) plus angles added to the
/// device's resting angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraValue {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
    pub tilt: f32,
    pub turn: f32,
    pub roll: f32,
}

impl CameraValue {
    pub const REST: CameraValue = CameraValue { zoom: 1.0, x: 0.5, y: 0.5, tilt: 0.0, turn: 0.0, roll: 0.0 };

    fn of(k: &CamKey) -> Self {
        Self { zoom: k.zoom, x: k.x, y: k.y, tilt: k.tilt, turn: k.turn, roll: k.roll }
    }

    fn lerp(a: Self, b: Self, f: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * f;
        Self { zoom: l(a.zoom, b.zoom), x: l(a.x, b.x), y: l(a.y, b.y), tilt: l(a.tilt, b.tilt), turn: l(a.turn, b.turn), roll: l(a.roll, b.roll) }
    }
}

/// Device motion at an instant: offset (fraction of the short side), scale and angles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoseValue {
    pub dx: f32,
    pub dy: f32,
    pub scale: f32,
    pub tilt: f32,
    pub turn: f32,
    pub roll: f32,
}

impl PoseValue {
    pub const REST: PoseValue = PoseValue { dx: 0.0, dy: 0.0, scale: 1.0, tilt: 0.0, turn: 0.0, roll: 0.0 };

    fn of(k: &PoseKey) -> Self {
        Self { dx: k.dx, dy: k.dy, scale: k.scale, tilt: k.tilt, turn: k.turn, roll: k.roll }
    }

    fn lerp(a: Self, b: Self, f: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * f;
        Self { dx: l(a.dx, b.dx), dy: l(a.dy, b.dy), scale: l(a.scale, b.scale), tilt: l(a.tilt, b.tilt), turn: l(a.turn, b.turn), roll: l(a.roll, b.roll) }
    }
}

/// Interpolates keyframes (sorted by `t`) at clip-relative time `u`.
fn sample<K, V: Copy>(keys: &[K], u: f32, t: impl Fn(&K) -> f32, e: impl Fn(&K) -> Ease, val: impl Fn(&K) -> V, lerp: impl Fn(V, V, f32) -> V) -> Option<V> {
    let first = keys.first()?;
    if u <= t(first) {
        return Some(val(first));
    }
    for w in keys.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if u < t(b) {
            let span = (t(b) - t(a)).max(1e-6);
            return Some(lerp(val(a), val(b), ease(e(b), (u - t(a)) / span)));
        }
    }
    keys.last().map(val)
}

pub fn camera_at(p: &Project, t: f32) -> CameraValue {
    // Clips do not overlap; if they ever do, the one that starts last wins.
    p.camera
        .iter()
        .filter(|c| t >= c.start && t <= c.end())
        .max_by(|a, b| a.start.total_cmp(&b.start))
        .and_then(|c| sample(&c.keys, t - c.start, |k| k.t, |k| k.ease, CameraValue::of, CameraValue::lerp))
        .unwrap_or(CameraValue::REST)
}

pub fn pose_at(p: &Project, t: f32) -> PoseValue {
    let mut out = PoseValue::REST;
    for c in p.device.iter().filter(|c| t >= c.start && t <= c.end()) {
        if let Some(v) = sample(&c.keys, t - c.start, |k| k.t, |k| k.ease, PoseValue::of, PoseValue::lerp) {
            out.dx += v.dx;
            out.dy += v.dy;
            out.scale *= v.scale;
            out.tilt += v.tilt;
            out.turn += v.turn;
            out.roll += v.roll;
        }
    }
    // Titles push the device aside while they are on screen. Titles in a row on the same side
    // count as one, so the device stays put between them; it comes back once they have gone.
    let mut spans: Vec<(f32, f32, TextPlace)> = Vec::new();
    for c in p.texts.iter().filter(|c| c.make_room && !text::is_blank(c)) {
        match spans.last_mut() {
            Some(s) if s.2 == c.place && c.start <= s.1 + 1.0 => s.1 = s.1.max(c.end()),
            _ => spans.push((c.start, c.end(), c.place)),
        }
    }
    let (mut room_x, mut room_y, mut room_s) = (0.0f32, 0.0f32, 1.0f32);
    for &(a, b, place) in spans.iter().filter(|s| t >= s.0 && t < s.1 + text::ROOM_AFTER) {
        let v = text::smooth((t - a) / text::ROOM_IN).min(text::smooth((b + text::ROOM_AFTER - t) / text::ROOM_AFTER));
        let (dx, dy, scale) = text::room(place, v, p.style.aspect.ratio());
        if dx.abs() > room_x.abs() {
            room_x = dx;
        }
        if dy.abs() > room_y.abs() {
            room_y = dy;
        }
        room_s = room_s.min(scale);
    }
    out.dx += room_x;
    out.dy += room_y;
    out.scale *= room_s;
    out
}

/// Instants to average into one frame at `t` for motion blur (a half-frame shutter at `fps`).
/// Empty when the device and the camera barely move during the frame.
pub fn shutter(p: &Project, t: f32, fps: f32) -> Vec<(f32, CameraValue, PoseValue)> {
    let half = 0.25 / fps.max(1.0);
    let (c0, p0) = (camera_at(p, t - half), pose_at(p, t - half));
    let (c1, p1) = (camera_at(p, t + half), pose_at(p, t + half));
    let degrees = [p1.turn - p0.turn, p1.tilt - p0.tilt, p1.roll - p0.roll, c1.turn - c0.turn, c1.tilt - c0.tilt, c1.roll - c0.roll]
        .iter()
        .fold(0.0f32, |m, d| m.max(d.abs()));
    // Travel in percent of the frame's short side.
    let travel = (p1.dx - p0.dx).hypot(p1.dy - p0.dy) * 100.0;
    let n = (degrees / 1.2).max(travel / 0.6).ceil() as usize;
    if n < 2 {
        return Vec::new();
    }
    let n = n.min(12);
    (0..n)
        .map(|i| {
            let s = t - half + (i as f32 + 0.5) / n as f32 * 2.0 * half;
            (s, camera_at(p, s), pose_at(p, s))
        })
        .collect()
}

// ---------- Titles ----------

pub fn texts_at(p: &Project, t: f32) -> Vec<text::Shown<'_>> {
    texts_in(&p.texts, t)
}

pub fn texts_in(texts: &[TextClip], t: f32) -> Vec<text::Shown<'_>> {
    texts.iter().filter(|c| t >= c.start && t < c.end()).map(|c| text::Shown { clip: c, local: t - c.start }).collect()
}

/// Adds a title at `t`, or right after the one already there: 3 seconds, or up to the next
/// title or the end of the video. Returns its id.
pub fn add_text(p: &mut Project, t: f32, title: &str) -> Result<u64, String> {
    let total = duration(p);
    let end_of = if total > 0.0 { total } else { f32::MAX };
    let mut start = t.clamp(0.0, (end_of - 1.5).max(0.0));
    if let Some(c) = p.texts.iter().find(|c| start >= c.start && start < c.end()) {
        start = c.end();
    }
    let next = p.texts.iter().map(|c| c.start).filter(|&s| s >= start).fold(end_of, f32::min);
    let len = 3.0f32.min(next - start);
    if len < 0.8 {
        return Err("No queda sitio para un texto aquí".into());
    }
    let id = p.next_id();
    p.texts.push(TextClip { id, start, duration: len, title: title.into(), ..TextClip::default() });
    p.texts.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(id)
}

pub fn delete_text(p: &mut Project, id: u64) -> Result<(), String> {
    let before = p.texts.len();
    p.texts.retain(|c| c.id != id);
    if p.texts.len() == before {
        return Err(format!("No hay ningún texto {id}"));
    }
    Ok(())
}

/// Sets when a title starts and ends (at least 0,8 s long).
pub fn place_text(p: &mut Project, id: u64, start: f32, end: f32) -> Result<(), String> {
    let c = p.texts.iter_mut().find(|c| c.id == id).ok_or(format!("No hay ningún texto {id}"))?;
    let start = start.max(0.0);
    c.start = start;
    c.duration = (end - start).max(0.8);
    p.texts.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(())
}

// ---------- Camera clips ----------

/// Time the camera takes to reach a zoom.
pub const RAMP: f32 = 0.6;

/// A zoom on one spot: reaches `zoom` at `start + RAMP`, holds until `end`, then returns.
pub fn zoom_clip(id: u64, start: f32, end: f32, zoom: f32, x: f32, y: f32, kind: CameraKind) -> CameraClip {
    let hold = (end - start).max(RAMP + 0.1);
    let shot = |t| CamKey { t, zoom, x, y, tilt: 0.0, turn: 0.0, roll: 0.0, ease: Ease::Spring };
    CameraClip {
        id,
        start,
        kind,
        keys: vec![CamKey::rest(0.0, Ease::Spring), shot(RAMP), CamKey { ease: Ease::Linear, ..shot(hold) }, CamKey::rest(hold + RAMP, Ease::Spring)],
    }
}

/// Zooms that follow the touches: zoom in slightly before each tap, move between nearby
/// taps without zooming out, and zoom back out once the action pauses.
pub fn auto_zoom_clips(p: &Project, scale: f32, first_id: u64) -> Vec<CameraClip> {
    const LEAD: f32 = 0.45;
    const HOLD: f32 = 1.6;
    const MERGE_GAP: f32 = 0.9;
    const SAME_SPOT: f32 = 0.18;
    let end_of_video = duration(p);

    struct Shot {
        begin: f32,
        end: f32,
        x: f32,
        y: f32,
        zoom: f32,
    }
    let mut touches = timeline_touches(p);
    touches.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut shots: Vec<Shot> = Vec::new();
    for t in touches {
        let (x, y, zoom) = match t.kind {
            TouchKind::Tap => (t.x, t.y, scale),
            // Scrolling reads better with a wider shot.
            TouchKind::Swipe => ((t.x + t.end_x) * 0.5, (t.y + t.end_y) * 0.5, (scale * 0.8).max(1.15)),
        };
        let begin = (t.time - LEAD).max(0.0);
        let end = (t.time + t.duration + HOLD).min(end_of_video);
        if let Some(last) = shots.last_mut() {
            let near = ((last.x - x).powi(2) + (last.y - y).powi(2)).sqrt() < SAME_SPOT;
            if near && begin <= last.end + MERGE_GAP {
                last.end = last.end.max(end);
                continue;
            }
        }
        shots.push(Shot { begin, end, x, y, zoom });
    }

    // Shots close in time share a clip: the camera pans from one to the next.
    let mut clips: Vec<CameraClip> = Vec::new();
    let mut i = 0;
    while i < shots.len() {
        let start = shots[i].begin;
        let key = |s: &Shot, t: f32, ease| CamKey { t: t - start, zoom: s.zoom, x: s.x, y: s.y, tilt: 0.0, turn: 0.0, roll: 0.0, ease };
        let mut keys = vec![CamKey::rest(0.0, Ease::Spring), key(&shots[i], start + RAMP, Ease::Spring)];
        let mut j = i;
        while j + 1 < shots.len() && shots[j + 1].begin <= shots[j].end + MERGE_GAP {
            let next_begin = shots[j + 1].begin.max(start + keys.last().unwrap().t + 0.05);
            keys.push(key(&shots[j], next_begin, Ease::Linear));
            keys.push(key(&shots[j + 1], next_begin + RAMP, Ease::Spring));
            j += 1;
        }
        let hold_end = shots[j].end.max(start + keys.last().unwrap().t + 0.05);
        keys.push(key(&shots[j], hold_end, Ease::Linear));
        keys.push(CamKey { t: hold_end - start + RAMP, ..CamKey::rest(0.0, Ease::Spring) });
        clips.push(CameraClip { id: first_id + clips.len() as u64, start, kind: CameraKind::Auto, keys });
        i = j + 1;
    }
    clips
}

/// Removes or shortens camera clips so that `[start, end]` is free.
fn make_room(p: &mut Project, start: f32, end: f32) {
    p.camera.retain(|c| c.end() <= start || c.start >= end);
}

/// Replaces the automatic zooms with new ones from the touches (zooms made by hand, moves and
/// Director shots are kept; automatic zooms that would overlap them are dropped).
pub fn regenerate_auto_zooms(p: &mut Project, scale: f32) -> usize {
    p.camera.retain(|c| c.kind != CameraKind::Auto);
    let first = p.next_id();
    let fresh = auto_zoom_clips(p, scale, first);
    let mut added = 0;
    for c in fresh {
        if p.camera.iter().all(|o| c.end() <= o.start || c.start >= o.end()) {
            p.camera.push(c);
            added += 1;
        }
    }
    sort_camera(p);
    added
}

fn sort_camera(p: &mut Project) {
    p.camera.sort_by(|a, b| a.start.total_cmp(&b.start));
}

/// A zoom on (x, y) from `start` to `end`, replacing whatever camera clips were there.
pub fn add_zoom(p: &mut Project, start: f32, end: f32, zoom: f32, x: f32, y: f32) -> Result<u64, String> {
    let total = duration(p);
    if end - start < 0.3 || start < 0.0 || start >= total {
        return Err("El zoom debe durar al menos 0,3 s y caer dentro del vídeo".into());
    }
    let id = p.next_id();
    let clip = zoom_clip(id, start, end.min(total), zoom.clamp(1.0, 5.0), x.clamp(0.0, 1.0), y.clamp(0.0, 1.0), CameraKind::Zoom);
    make_room(p, clip.start, clip.end());
    p.camera.push(clip);
    sort_camera(p);
    Ok(id)
}

/// A camera move around `t`: the device turns to an angle and comes back. Its keyframes are
/// meant to be edited.
pub fn add_move(p: &mut Project, t: f32) -> Result<u64, String> {
    let (start, end) = free_span(p, t, 0.8, 3.0)?;
    let len = end - start;
    let id = p.next_id();
    let shot = |t| CamKey { t, zoom: 1.0, x: 0.5, y: 0.5, tilt: 8.0, turn: 25.0, roll: 0.0, ease: Ease::Smooth };
    let clip = CameraClip {
        id,
        start,
        kind: CameraKind::Move,
        keys: vec![CamKey::rest(0.0, Ease::Smooth), shot(len * 0.27), CamKey { ease: Ease::Linear, ..shot(len * 0.73) }, CamKey::rest(len, Ease::Smooth)],
    };
    p.camera.push(clip);
    sort_camera(p);
    Ok(id)
}

/// Room for a new camera clip around `t`: about `before` seconds before it and `len` in total,
/// shrunk to fit between the neighbouring camera clips (they are never removed).
fn free_span(p: &Project, t: f32, before: f32, len: f32) -> Result<(f32, f32), String> {
    let total = duration(p);
    if p.camera.iter().any(|c| t > c.start + 1e-4 && t < c.end() - 1e-4) {
        return Err("Ya hay un movimiento de cámara en ese momento: selecciónalo y añade ahí el fotograma clave".into());
    }
    let prev_end = p.camera.iter().map(|c| c.end()).filter(|e| *e <= t + 1e-4).fold(0.0f32, f32::max);
    let next_start = p.camera.iter().map(|c| c.start).filter(|s| *s >= t - 1e-4).fold(total.max(t), f32::min);
    let start = (t - before).max(prev_end).max(0.0);
    let end = (start + len).min(next_start).min(total.max(start + 1.0));
    if end - start < 1.0 || t < start || t > end {
        return Err("No hay hueco libre en la pista de cámara: elige un movimiento existente y añade el fotograma clave en él".into());
    }
    Ok((start, end))
}

/// Adds a keyframe at timeline second `t`: inside the camera clip there, or a new free move
/// clip around it. Values not given keep what the camera does at that instant.
pub fn add_camera_key(p: &mut Project, t: f32, set: impl FnOnce(&mut CamKey)) -> Result<(u64, usize), String> {
    let now = camera_at(p, t);
    let mut key = CamKey { t: 0.0, zoom: now.zoom, x: now.x, y: now.y, tilt: now.tilt, turn: now.turn, roll: now.roll, ease: Ease::Smooth };
    set(&mut key);
    if let Some(c) = p.camera.iter_mut().find(|c| t >= c.start && t <= c.end()) {
        key.t = t - c.start;
        if c.keys.iter().any(|k| (k.t - key.t).abs() < 0.02) {
            let i = c.keys.iter().position(|k| (k.t - key.t).abs() < 0.02).unwrap();
            c.keys[i] = key;
            return Ok((c.id, i));
        }
        c.keys.push(key);
        c.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        if c.kind == CameraKind::Auto {
            c.kind = CameraKind::Move;
        }
        let i = c.keys.iter().position(|k| (k.t - key.t).abs() < 1e-6).unwrap();
        return Ok((c.id, i));
    }
    // A new move in the free space around `t`: rest → the keyframe → hold → rest.
    let (start, end) = free_span(p, t, 0.8, 2.4)?;
    let id = p.next_id();
    key.t = t - start;
    let len = end - start;
    let hold = (key.t + 0.8).min(len - 0.3).max(key.t + 0.05);
    let mut keys = vec![key, CamKey { t: hold, ease: Ease::Linear, ..key }, CamKey::rest(len, Ease::Smooth)];
    if key.t > 0.05 {
        keys.insert(0, CamKey::rest(0.0, Ease::Smooth));
    }
    let index = keys.iter().position(|k| (k.t - key.t).abs() < 1e-6).unwrap_or(0);
    let clip = CameraClip { id, start, kind: CameraKind::Move, keys };
    p.camera.push(clip);
    sort_camera(p);
    Ok((id, index))
}

/// Sorts a camera clip's keyframes and moves the clip so the first one is at its start.
pub fn normalize_camera_clip(c: &mut CameraClip) {
    c.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    if let Some(first) = c.keys.first().map(|k| k.t) {
        c.start += first;
        for k in &mut c.keys {
            k.t -= first;
        }
    }
}

pub fn delete_camera_clip(p: &mut Project, id: u64) -> Result<(), String> {
    let before = p.camera.len();
    p.camera.retain(|c| c.id != id);
    if p.camera.len() == before {
        return Err(format!("No hay ningún clip de cámara {id}"));
    }
    Ok(())
}

pub fn delete_camera_key(p: &mut Project, id: u64, index: usize) -> Result<(), String> {
    let c = p.camera.iter_mut().find(|c| c.id == id).ok_or(format!("No hay ningún clip de cámara {id}"))?;
    if index >= c.keys.len() {
        return Err("Ese fotograma clave no existe".into());
    }
    c.keys.remove(index);
    if c.keys.len() < 2 {
        p.camera.retain(|c| c.id != id);
    } else if c.keys[0].t > 0.0 {
        // Keep keyframe times relative to the clip start.
        let shift = c.keys[0].t;
        c.start += shift;
        for k in &mut c.keys {
            k.t -= shift;
        }
    }
    Ok(())
}

/// Moves a camera clip (keeping its length), never over another one.
pub fn move_camera_clip(p: &mut Project, id: u64, start: f32) -> Result<(), String> {
    let total = duration(p);
    let others: Vec<(f32, f32)> = p.camera.iter().filter(|c| c.id != id).map(|c| (c.start, c.end())).collect();
    let c = p.camera.iter_mut().find(|c| c.id == id).ok_or(format!("No hay ningún clip de cámara {id}"))?;
    let len = c.duration();
    let mut s = start.clamp(0.0, (total - len).max(0.0));
    for (a, b) in others {
        if s < b && s + len > a {
            // Snap next to the neighbour on the side we came from.
            s = if c.start < a { a - len } else { b };
        }
    }
    c.start = s.max(0.0);
    sort_camera(p);
    Ok(())
}

/// Stretches a camera clip to a new start/end. Zooms keep their in and out ramps; other clips
/// scale all their keyframes.
pub fn resize_camera_clip(p: &mut Project, id: u64, start: f32, end: f32) -> Result<(), String> {
    let c = p.camera.iter_mut().find(|c| c.id == id).ok_or(format!("No hay ningún clip de cámara {id}"))?;
    let len = (end - start).max(RAMP * 2.0 + 0.1);
    let old = c.duration().max(1e-3);
    if matches!(c.kind, CameraKind::Zoom | CameraKind::Auto) && c.keys.len() == 4 {
        c.keys[2].t = len - RAMP;
        c.keys[3].t = len;
    } else {
        for k in &mut c.keys {
            k.t *= len / old;
        }
    }
    c.start = start.max(0.0);
    Ok(())
}

// ---------- Device clips ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePreset {
    SpinIn,
    DropIn,
    FullTurn,
    Flip,
    RiseUp,
    Exit,
    Hop,
}

impl DevicePreset {
    pub const ALL: [DevicePreset; 7] =
        [DevicePreset::FullTurn, DevicePreset::Flip, DevicePreset::SpinIn, DevicePreset::DropIn, DevicePreset::RiseUp, DevicePreset::Hop, DevicePreset::Exit];

    pub fn label(self) -> &'static str {
        match self {
            DevicePreset::SpinIn => "Entrada girando",
            DevicePreset::DropIn => "Caída desde arriba",
            DevicePreset::FullTurn => "Giro 360°",
            DevicePreset::Flip => "Voltereta 360°",
            DevicePreset::RiseUp => "Asomarse desde abajo",
            DevicePreset::Hop => "Saltito",
            DevicePreset::Exit => "Salida",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            DevicePreset::SpinIn => "spin_in",
            DevicePreset::DropIn => "drop_in",
            DevicePreset::FullTurn => "full_turn",
            DevicePreset::Flip => "flip",
            DevicePreset::RiseUp => "rise_up",
            DevicePreset::Hop => "hop",
            DevicePreset::Exit => "exit",
        }
    }

    /// Spins turn the device all the way round; near a cut they centre on it, so the clip
    /// changes while the back of the device faces the camera.
    pub fn is_spin(self) -> bool {
        matches!(self, DevicePreset::FullTurn | DevicePreset::Flip)
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == k)
    }

    fn keys(self) -> Vec<PoseKey> {
        let r = PoseKey::rest;
        match self {
            DevicePreset::SpinIn => vec![PoseKey { dy: 0.6, turn: -40.0, tilt: 18.0, ..r(0.0, Ease::Smooth) }, r(1.4, Ease::Spring)],
            DevicePreset::DropIn => vec![PoseKey { dy: -1.1, tilt: -14.0, ..r(0.0, Ease::Smooth) }, r(0.9, Ease::Spring)],
            DevicePreset::FullTurn => vec![r(0.0, Ease::Smooth), PoseKey { turn: 360.0, ..r(1.2, Ease::Smooth) }],
            DevicePreset::Flip => vec![r(0.0, Ease::Smooth), PoseKey { tilt: 360.0, ..r(1.2, Ease::Smooth) }],
            DevicePreset::RiseUp => vec![PoseKey { dy: 0.9, ..r(0.0, Ease::Smooth) }, r(1.2, Ease::Spring)],
            DevicePreset::Hop => vec![r(0.0, Ease::Smooth), PoseKey { dy: -0.06, scale: 1.04, ..r(0.25, Ease::Smooth) }, r(0.6, Ease::Spring)],
            DevicePreset::Exit => vec![r(0.0, Ease::Smooth), PoseKey { dy: 0.9, turn: 30.0, ..r(1.0, Ease::Smooth) }],
        }
    }
}

/// Adds a device motion preset starting at `t` (the exit ends at `t` instead, so it can be
/// dropped at the end of the video).
pub fn add_device_preset(p: &mut Project, preset: DevicePreset, t: f32) -> u64 {
    let keys = preset.keys();
    let len = keys.last().map(|k| k.t).unwrap_or(0.0);
    let total = duration(p);
    let cut = placed(p).iter().rev().skip(1).map(|c| c.end).filter(|e| (e - t).abs() <= 1.5).min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()));
    let start = match (preset, cut) {
        (DevicePreset::Exit, _) => (t.min(total) - len).max(0.0),
        (pr, Some(cut)) if pr.is_spin() => (cut - len * 0.5).max(0.0),
        _ => t.clamp(0.0, (total - len).max(0.0)),
    };
    let id = p.next_id();
    p.device.push(DeviceClip { id, start, name: preset.label().into(), keys });
    p.device.sort_by(|a, b| a.start.total_cmp(&b.start));
    id
}

/// The angle a spin turns (`(axis, degrees)`, axis 0 = turn, 1 = tilt, 2 = roll), if the clip
/// turns the device all the way round.
pub fn spin_of(c: &DeviceClip) -> Option<(usize, f32)> {
    let last = c.keys.last()?;
    [last.turn, last.tilt, last.roll].into_iter().enumerate().find(|(_, v)| v.abs() >= 300.0)
}

fn axis_mut(k: &mut PoseKey, axis: usize) -> &mut f32 {
    match axis {
        0 => &mut k.turn,
        1 => &mut k.tilt,
        _ => &mut k.roll,
    }
}

/// Sets how far a spin turns, keeping the shape of its motion.
pub fn set_spin(c: &mut DeviceClip, degrees: f32) {
    if let Some((axis, current)) = spin_of(c) {
        let k = degrees / current;
        for key in &mut c.keys {
            *axis_mut(key, axis) *= k;
        }
    }
}

/// Stretches a device clip's keyframes to last `secs`.
pub fn set_device_duration(c: &mut DeviceClip, secs: f32) {
    let current = c.duration();
    if current > 0.0 {
        let k = secs.max(0.2) / current;
        for key in &mut c.keys {
            key.t *= k;
        }
    }
}

pub fn delete_device_clip(p: &mut Project, id: u64) -> Result<(), String> {
    let before = p.device.len();
    p.device.retain(|c| c.id != id);
    if p.device.len() == before {
        return Err(format!("No hay ningún movimiento del dispositivo {id}"));
    }
    Ok(())
}

// ---------- Video clips ----------

/// Adds a recording at the end of the timeline. Returns the new clip id.
pub fn append_media(p: &mut Project, info: MediaInfo, touches: Vec<Touch>) -> u64 {
    let dur = info.duration;
    // The same recording added again shares its media (and touches).
    let media = match p.media.iter().find(|m| m.info.path == info.path) {
        Some(m) => m.id,
        None => {
            let id = p.next_id();
            p.media.push(Media { id, info, touches });
            id
        }
    };
    let id = p.next_id();
    p.clips.push(Clip { id, media, source_in: 0.0, source_out: dur });
    id
}

/// Splits the clip under `t` in two.
pub fn split_at(p: &mut Project, t: f32) -> Result<u64, String> {
    let c = locate(p, t).ok_or("No hay ningún clip en ese punto")?;
    let cut = source_time(&c, t);
    if cut - c.clip.source_in < 0.1 || c.clip.source_out - cut < 0.1 {
        return Err("Demasiado cerca del borde del clip para dividirlo".into());
    }
    let id = p.next_id();
    p.clips[c.index].source_out = cut;
    p.clips.insert(c.index + 1, Clip { id, media: c.clip.media, source_in: cut, source_out: c.clip.source_out });
    Ok(id)
}

/// Changes the part of the recording a clip plays (the clips after it move to follow).
pub fn trim_clip(p: &mut Project, id: u64, source_in: Option<f32>, source_out: Option<f32>) -> Result<(), String> {
    let i = p.clips.iter().position(|c| c.id == id).ok_or(format!("No hay ningún clip {id}"))?;
    let media_len = p.media(p.clips[i].media).map(|m| m.info.duration).unwrap_or(f32::MAX);
    let c = &mut p.clips[i];
    let a = source_in.unwrap_or(c.source_in).clamp(0.0, media_len);
    let b = source_out.unwrap_or(c.source_out).clamp(0.0, media_len);
    if b - a < 0.2 {
        return Err("El clip debe durar al menos 0,2 s".into());
    }
    c.source_in = a;
    c.source_out = b;
    Ok(())
}

pub fn move_clip(p: &mut Project, id: u64, index: usize) -> Result<(), String> {
    let i = p.clips.iter().position(|c| c.id == id).ok_or(format!("No hay ningún clip {id}"))?;
    let c = p.clips.remove(i);
    p.clips.insert(index.min(p.clips.len()), c);
    Ok(())
}

pub fn delete_clip(p: &mut Project, id: u64) -> Result<(), String> {
    let i = p.clips.iter().position(|c| c.id == id).ok_or(format!("No hay ningún clip {id}"))?;
    p.clips.remove(i);
    let used: Vec<u64> = p.clips.iter().map(|c| c.media).collect();
    p.media.retain(|m| used.contains(&m.id));
    let total = duration(p);
    p.camera.retain(|c| c.start < total);
    p.director.targets.retain(|t| t.time < total);
    Ok(())
}

// ---------- Director ----------

/// Puts a target at timeline second `t`, replacing the one already at that moment.
pub fn add_target(p: &mut Project, t: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Result<u64, String> {
    let (x0, x1) = (x0.min(x1).clamp(0.0, 1.0), x0.max(x1).clamp(0.0, 1.0));
    let (y0, y1) = (y0.min(y1).clamp(0.0, 1.0), y0.max(y1).clamp(0.0, 1.0));
    if x1 - x0 < 0.02 || y1 - y0 < 0.01 {
        return Err("El recuadro es demasiado pequeño".into());
    }
    if t < 0.0 || t > duration(p) {
        return Err("Ese momento está fuera del vídeo".into());
    }
    p.director.targets.retain(|o| (o.time - t).abs() >= 0.05);
    let id = p.next_id();
    p.director.targets.push(Target { id, time: t, x0, y0, x1, y1, seed: id as u32 });
    p.director.targets.sort_by(|a, b| a.time.total_cmp(&b.time));
    Ok(id)
}

pub fn remove_target(p: &mut Project, id: u64) -> Result<(), String> {
    let before = p.director.targets.len();
    p.director.targets.retain(|t| t.id != id);
    if before == p.director.targets.len() {
        return Err(format!("No hay ningún objetivo {id}"));
    }
    Ok(())
}

/// A different camera move for one target (or all of them) on the next generation.
pub fn retake(p: &mut Project, id: Option<u64>) {
    for t in p.director.targets.iter_mut().filter(|t| id.is_none_or(|i| i == t.id)) {
        t.seed = t.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    }
}

fn rand01(seed: u32, salt: u32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.0
}

/// How the Director frames one target: zoom so the box fills most of the frame, centred on
/// it, with an angle that gets gentler the closer the shot.
fn shot_for(t: &Target, layout: &Layout, motion: DirectorMotion, intensity: f32, index: usize) -> CamKey {
    let v = &layout.video;
    let (bw, bh) = ((t.x1 - t.x0) * v.w(), (t.y1 - t.y0) * v.h());
    let fill = 0.78;
    let zoom = (fill * layout.cw / bw.max(1.0)).min(fill * layout.ch / bh.max(1.0)).clamp(1.0, 4.0);
    let (x, y) = ((t.x0 + t.x1) * 0.5, (t.y0 + t.y1) * 0.5);
    let mut key = CamKey { t: 0.0, zoom, x, y, tilt: 0.0, turn: 0.0, roll: 0.0, ease: Ease::Smooth };
    if motion == DirectorMotion::ThreeD {
        let k = 2.0 * intensity.clamp(0.0, 1.0);
        let close = ((zoom - 1.0) / 2.0).clamp(0.0, 1.0);
        let side = if (index + t.seed as usize) % 2 == 0 { 1.0 } else { -1.0 };
        key.turn = side * (12.0 + 14.0 * rand01(t.seed, 1)) * (1.0 - 0.6 * close) * k;
        key.tilt = (4.0 + 9.0 * rand01(t.seed, 2)) * (1.0 - 0.5 * close) * k;
        key.roll = (rand01(t.seed, 3) - 0.5) * 6.0 * k;
    }
    key
}

/// Turns the targets into camera shots, replacing earlier Director shots and any camera clip
/// in the way. `layout` frames the shots (canvas and screen of the export). Returns how many
/// shots were made.
pub fn director_generate(p: &mut Project, cw: f32, ch: f32, model: Option<&ModelMeta>) -> Result<(usize, usize), String> {
    let mut targets = p.director.targets.clone();
    if targets.is_empty() {
        return Err("Dibuja al menos un objetivo".into());
    }
    targets.sort_by(|a, b| a.time.total_cmp(&b.time));
    let total = duration(p);
    let (vw, vh) = locate(p, targets[0].time)
        .and_then(|c| p.media(c.clip.media))
        .map(|m| (m.info.width as f32, m.info.height as f32))
        .unwrap_or((1080.0, 2400.0));
    let layout = Layout::new(cw, ch, &p.style.flattened(), vw, vh, model);

    let shots: Vec<CamKey> = targets.iter().enumerate().map(|(i, t)| shot_for(t, &layout, p.director.motion, p.director.intensity, i)).collect();
    // Each shot starts moving `travel` before its target; shots are back to back.
    let travel: Vec<f32> = targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let room = if i == 0 { t.time } else { t.time - targets[i - 1].time };
            (room * 0.5).clamp(0.3, 1.2).min(t.time.max(0.01))
        })
        .collect();
    let begins: Vec<f32> = targets.iter().zip(&travel).map(|(t, d)| t.time - d).collect();
    let last_end = (targets.last().unwrap().time + 2.0).min(total);

    p.camera.retain(|c| c.kind != CameraKind::Director);
    let span = (begins[0], (last_end + 1.0).min(total.max(last_end)));
    let before = p.camera.len();
    make_room(p, span.0, span.1);
    let replaced = before - p.camera.len();

    let first_id = p.next_id();
    for i in 0..targets.len() {
        let start = begins[i];
        let end = if i + 1 < targets.len() { begins[i + 1] } else { last_end };
        let from = if i == 0 { CamKey::rest(0.0, Ease::Smooth) } else { CamKey { t: 0.0, ..shots[i - 1] } };
        let arrive = CamKey { t: travel[i], ..shots[i] };
        let mut keys = vec![from, arrive];
        if end - start > travel[i] + 0.01 {
            keys.push(CamKey { t: end - start, ease: Ease::Linear, ..shots[i] });
        }
        if i + 1 == targets.len() {
            let back = keys.last().unwrap().t + 1.0;
            keys.push(CamKey::rest(back, Ease::Smooth));
        }
        p.camera.push(CameraClip { id: first_id + i as u64, start, kind: CameraKind::Director, keys });
    }
    sort_camera(p);
    Ok((targets.len(), replaced))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(durations: &[f32]) -> Project {
        let mut p = Project::default();
        for (i, d) in durations.iter().enumerate() {
            let info = MediaInfo { path: format!("/v{i}.mp4"), width: 1080, height: 2400, duration: *d, fps: 30.0, has_audio: false };
            let touches = vec![Touch { time: 1.0, duration: 0.1, x: 0.3, y: 0.2, end_x: 0.3, end_y: 0.2, kind: TouchKind::Tap }];
            append_media(&mut p, info, touches);
        }
        p
    }

    #[test]
    fn clips_play_back_to_back_and_split() {
        let mut p = project(&[5.0, 3.0]);
        assert_eq!(duration(&p), 8.0);
        let c = locate(&p, 6.0).unwrap();
        assert_eq!(c.index, 1);
        assert!((source_time(&c, 6.0) - 1.0).abs() < 1e-5);
        let touches = timeline_touches(&p);
        assert_eq!(touches.iter().map(|t| t.time).collect::<Vec<_>>(), vec![1.0, 6.0]);

        split_at(&mut p, 2.0).unwrap();
        assert_eq!(p.clips.len(), 3);
        assert_eq!(duration(&p), 8.0);
        let first = p.clips[0].id;
        trim_clip(&mut p, first, Some(0.5), None).unwrap();
        assert!((duration(&p) - 7.5).abs() < 1e-5);
        let last = p.clips[2].id;
        move_clip(&mut p, last, 0).unwrap();
        assert_eq!(p.clips[0].id, last);
        delete_clip(&mut p, last).unwrap();
        assert_eq!(p.media.len(), 1, "unused recordings are dropped");
    }

    #[test]
    fn new_moves_fit_between_neighbours() {
        let mut p = project(&[10.0]);
        add_zoom(&mut p, 1.0, 3.0, 2.0, 0.5, 0.5).unwrap(); // ends at 3.6
        add_zoom(&mut p, 6.0, 7.0, 2.0, 0.5, 0.5).unwrap();
        let (id, _) = add_camera_key(&mut p, 4.2, |k| k.turn = 30.0).unwrap();
        assert_eq!(p.camera.len(), 3, "neighbours are kept");
        let c = p.camera.iter().find(|c| c.id == id).unwrap();
        assert!(c.start >= 3.6 - 1e-4 && c.end() <= 6.0 + 1e-4, "{} {}", c.start, c.end());
        assert!((camera_at(&p, 4.2).turn - 30.0).abs() < 1e-3);
        assert!(add_move(&mut p, 6.5).is_err(), "no move on top of another clip");
    }

    #[test]
    fn keyframes_ease_and_hold() {
        let mut p = project(&[10.0]);
        add_move(&mut p, 3.0).unwrap();
        let c = &p.camera[0];
        assert_eq!(camera_at(&p, 0.5), CameraValue::REST);
        let mid = camera_at(&p, c.start + 1.5);
        assert!((mid.turn - 25.0).abs() < 1e-3 && (mid.tilt - 8.0).abs() < 1e-3, "{mid:?}");
        let ramp = camera_at(&p, c.start + 0.4);
        assert!(ramp.turn > 0.0 && ramp.turn < 25.0);
        assert_eq!(camera_at(&p, c.end() + 0.1), CameraValue::REST);
        assert!((ease(Ease::Spring, 1.0) - 1.0).abs() < 1e-5 && ease(Ease::Hold, 0.99) == 0.0);
    }

    #[test]
    fn auto_zoom_follows_timeline_touches() {
        let mut p = project(&[5.0, 5.0]);
        let n = regenerate_auto_zooms(&mut p, 2.0);
        assert_eq!(n, 2);
        let z = camera_at(&p, 6.2);
        assert!((z.zoom - 2.0).abs() < 0.05 && (z.x - 0.3).abs() < 1e-3, "{z:?}");
        // Manual zoom over an automatic one replaces it.
        add_zoom(&mut p, 5.5, 7.0, 3.0, 0.5, 0.5).unwrap();
        assert_eq!(p.camera.iter().filter(|c| c.kind == CameraKind::Auto).count(), 1);
    }

    #[test]
    fn director_frames_targets_in_order() {
        let mut p = project(&[12.0]);
        regenerate_auto_zooms(&mut p, 2.0);
        add_target(&mut p, 2.0, 0.1, 0.1, 0.4, 0.2).unwrap();
        add_target(&mut p, 6.0, 0.0, 0.0, 1.0, 1.0).unwrap();
        add_target(&mut p, 6.02, 0.5, 0.5, 0.9, 0.7).unwrap(); // replaces the one at 6.0
        assert_eq!(p.director.targets.len(), 2);
        let (n, replaced) = director_generate(&mut p, 1920.0, 1080.0, None).unwrap();
        assert_eq!(n, 2);
        assert!(replaced > 0, "the automatic zoom in the way is reported");
        assert!(p.camera.iter().all(|c| c.kind == CameraKind::Director), "auto zooms in the way are replaced");
        let a = camera_at(&p, 2.0);
        assert!(a.zoom > 1.5 && (a.x - 0.25).abs() < 1e-3 && a.turn.abs() > 1.0, "{a:?}");
        let b = camera_at(&p, 6.5);
        assert!((b.x - 0.7).abs() < 1e-3);
        // Shots are back to back: no jump where one clip hands over to the next.
        let edge = p.camera[1].start;
        let (l, r) = (camera_at(&p, edge - 0.001), camera_at(&p, edge + 0.001));
        assert!((l.zoom - r.zoom).abs() < 0.02 && (l.turn - r.turn).abs() < 0.2, "{l:?} {r:?}");
        p.director.motion = DirectorMotion::TwoD;
        director_generate(&mut p, 1920.0, 1080.0, None).unwrap();
        assert!(camera_at(&p, 2.0).turn.abs() < 1e-6);
        let seed = p.director.targets[0].seed;
        let target = p.director.targets[0].id;
        retake(&mut p, Some(target));
        assert_ne!(p.director.targets[0].seed, seed);
    }

    #[test]
    fn device_presets_add_up() {
        let mut p = project(&[10.0]);
        add_device_preset(&mut p, DevicePreset::FullTurn, 2.0);
        let half = pose_at(&p, 2.6);
        assert!(half.turn > 100.0 && half.turn < 260.0, "{half:?}");
        add_device_preset(&mut p, DevicePreset::Exit, 10.0);
        assert!((p.device.iter().find(|d| d.name == "Salida").unwrap().end() - 10.0).abs() < 1e-4);
        assert!(pose_at(&p, 10.0).dy > 0.8);
    }

    #[test]
    fn spins_hide_the_cut_and_can_turn_more() {
        let mut p = project(&[4.0, 4.0]);
        // Dropped near the cut at 4 s, the spin shows the back of the device right on it.
        let id = add_device_preset(&mut p, DevicePreset::FullTurn, 3.2);
        assert!((pose_at(&p, 4.0).turn - 180.0).abs() < 1.0, "{:?}", pose_at(&p, 4.0));
        // Away from cuts it starts where it is dropped.
        let flip = add_device_preset(&mut p, DevicePreset::Flip, 6.0);
        assert_eq!(p.device.iter().find(|d| d.id == flip).unwrap().start, 6.0);

        let c = p.device.iter_mut().find(|d| d.id == id).unwrap();
        assert_eq!(spin_of(c), Some((0, 360.0)));
        set_spin(c, -720.0);
        set_device_duration(c, 2.0);
        assert_eq!(spin_of(c), Some((0, -720.0)));
        assert!((c.duration() - 2.0).abs() < 1e-5);
    }

    #[test]
    fn fast_spins_get_motion_blur() {
        let mut p = project(&[6.0]);
        assert!(shutter(&p, 1.0, 30.0).is_empty());
        add_device_preset(&mut p, DevicePreset::FullTurn, 1.0);
        let s = shutter(&p, 1.6, 30.0);
        assert!(s.len() >= 3, "{}", s.len());
        assert!(s.windows(2).all(|w| w[1].2.turn > w[0].2.turn));
        assert!(shutter(&p, 4.0, 30.0).is_empty());
    }

    #[test]
    fn titles_move_the_device_aside() {
        let mut p = project(&[8.0]);
        let id = add_text(&mut p, 1.0, "Camina.").unwrap();
        let c = p.texts.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!((c.start, c.duration), (1.0, 3.0));
        assert_eq!(texts_at(&p, 2.0).len(), 1);
        assert!(pose_at(&p, 2.5).dx > 0.3, "{:?}", pose_at(&p, 2.5));
        assert_eq!(pose_at(&p, 5.0).dx, 0.0);
        // A second title right after keeps the device where it is.
        let second = add_text(&mut p, 4.2, "Corre.").unwrap();
        let dxs: Vec<f32> = (0..20).map(|i| pose_at(&p, 3.5 + i as f32 * 0.05).dx).collect();
        assert!(dxs.iter().all(|d| (d - dxs[0]).abs() < 0.01), "{dxs:?}");
        // Adding over a title puts the new one right after it, if there is room before the next.
        assert!(add_text(&mut p, 2.0, "").is_err());
        delete_text(&mut p, second).unwrap();
        let third = add_text(&mut p, 2.0, "").unwrap();
        let c = p.texts.iter().find(|c| c.id == third).unwrap();
        assert_eq!((c.start, c.duration), (4.0, 3.0));
        delete_text(&mut p, third).unwrap();
        place_text(&mut p, id, 6.0, 6.2).unwrap();
        let c = p.texts.iter().find(|c| c.id == id).unwrap();
        assert!((c.duration - 0.8).abs() < 1e-5);
        delete_text(&mut p, id).unwrap();
        assert!(delete_text(&mut p, id).is_err());
    }
}
