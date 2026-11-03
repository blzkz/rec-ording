use crate::model::{Touch, TouchKind, ZoomSegment};

/// Camera at a given instant: zoom factor and the point of the screen it focuses on (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamState {
    pub scale: f32,
    pub fx: f32,
    pub fy: f32,
}

impl CamState {
    pub const IDENTITY: CamState = CamState { scale: 1.0, fx: 0.5, fy: 0.5 };
}

/// Camera motion pre-simulated over the clip, so that any instant can be sampled
/// deterministically (scrubbing and export always agree).
pub struct CameraTrack {
    t0: f32,
    dt: f32,
    samples: Vec<CamState>,
}

const SIM_HZ: f32 = 240.0;
/// Angular frequency of the spring; ~0.6 s to settle, close to the feel of iOS animations.
const OMEGA: f32 = 7.5;

impl CameraTrack {
    pub fn empty() -> Self {
        Self { t0: 0.0, dt: 1.0, samples: vec![CamState::IDENTITY] }
    }

    pub fn build(zooms: &[ZoomSegment], start: f32, end: f32) -> Self {
        let dt = 1.0 / SIM_HZ;
        let steps = (((end - start).max(0.0)) / dt).ceil() as usize + 2;
        let mut samples = Vec::with_capacity(steps);

        let mut pos = [1.0f32, 0.5, 0.5];
        let mut vel = [0.0f32; 3];
        // A segment that starts at the very beginning should already be framed.
        if let Some(z) = active_zoom(zooms, start) {
            pos = [z.scale, z.x, z.y];
        }

        for i in 0..steps {
            let t = start + i as f32 * dt;
            samples.push(CamState { scale: pos[0], fx: pos[1], fy: pos[2] });
            let target = match active_zoom(zooms, t) {
                Some(z) => [z.scale, z.x, z.y],
                None => [1.0, 0.5, 0.5],
            };
            for k in 0..3 {
                // Critically damped spring (semi-implicit Euler).
                let acc = OMEGA * OMEGA * (target[k] - pos[k]) - 2.0 * OMEGA * vel[k];
                vel[k] += acc * dt;
                pos[k] += vel[k] * dt;
            }
        }
        Self { t0: start, dt, samples }
    }

    pub fn sample(&self, t: f32) -> CamState {
        let f = ((t - self.t0) / self.dt).max(0.0);
        let i = (f.floor() as usize).min(self.samples.len() - 1);
        let j = (i + 1).min(self.samples.len() - 1);
        let k = f - f.floor();
        let (a, b) = (self.samples[i], self.samples[j]);
        CamState {
            scale: a.scale + (b.scale - a.scale) * k,
            fx: a.fx + (b.fx - a.fx) * k,
            fy: a.fy + (b.fy - a.fy) * k,
        }
    }
}

fn active_zoom(zooms: &[ZoomSegment], t: f32) -> Option<&ZoomSegment> {
    zooms.iter().filter(|z| t >= z.start && t < z.end).last()
}

/// Turns recorded touches into zoom segments: zoom in slightly before each tap, follow
/// consecutive taps, and zoom back out once the action pauses.
pub fn auto_zooms(touches: &[Touch], scale: f32, clip_start: f32, clip_end: f32, first_id: u64) -> Vec<ZoomSegment> {
    const LEAD: f32 = 0.45;
    const HOLD: f32 = 1.6;
    const MERGE_GAP: f32 = 0.9;
    const SAME_SPOT: f32 = 0.18;

    let mut sorted: Vec<&Touch> = touches
        .iter()
        .filter(|t| t.time >= clip_start && t.time <= clip_end)
        .collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));

    let mut out: Vec<ZoomSegment> = Vec::new();
    for t in sorted {
        let (fx, fy, s) = match t.kind {
            TouchKind::Tap => (t.x, t.y, scale),
            // Scrolling reads better with a wider shot.
            TouchKind::Swipe => ((t.x + t.end_x) * 0.5, (t.y + t.end_y) * 0.5, (scale * 0.8).max(1.15)),
        };
        let start = (t.time - LEAD).max(clip_start);
        let end = (t.time + t.duration + HOLD).min(clip_end);

        if let Some(last) = out.last_mut() {
            if start <= last.end + MERGE_GAP {
                let dist = ((last.x - fx).powi(2) + (last.y - fy).powi(2)).sqrt();
                if dist < SAME_SPOT {
                    // Same area: keep the shot and extend it.
                    last.end = last.end.max(end);
                    last.x = (last.x + fx) * 0.5;
                    last.y = (last.y + fy) * 0.5;
                    continue;
                }
                // Different area: cut the previous shot so the camera pans instead of zooming out.
                last.end = start.max(last.start + 0.3);
            }
        }
        out.push(ZoomSegment {
            id: first_id + out.len() as u64,
            start,
            end,
            scale: s,
            x: fx,
            y: fy,
            auto: true,
        });
    }
    out.retain(|z| z.end - z.start >= 0.3);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(time: f32, x: f32, y: f32) -> Touch {
        Touch { time, duration: 0.1, x, y, end_x: x, end_y: y, kind: TouchKind::Tap }
    }

    #[test]
    fn nearby_taps_merge_and_distant_taps_pan() {
        let touches = [tap(1.0, 0.5, 0.3), tap(1.8, 0.52, 0.32), tap(3.0, 0.5, 0.9), tap(9.0, 0.2, 0.2)];
        let z = auto_zooms(&touches, 2.0, 0.0, 12.0, 1);
        assert_eq!(z.len(), 3, "{z:?}");
        // The first two taps share a shot; the third cuts straight to a new spot.
        assert!((z[0].end - z[1].start).abs() < 1e-4);
        assert!(z[2].start > z[1].end, "a long pause zooms back out");
        assert!(z.iter().all(|s| s.auto && s.scale == 2.0));
    }

    #[test]
    fn camera_settles_on_target_and_returns() {
        let zooms = [ZoomSegment { id: 1, start: 1.0, end: 3.0, scale: 2.0, x: 0.3, y: 0.7, auto: false }];
        let track = CameraTrack::build(&zooms, 0.0, 6.0);
        assert_eq!(track.sample(0.5), CamState::IDENTITY);
        let mid = track.sample(2.9);
        assert!((mid.scale - 2.0).abs() < 0.02 && (mid.fx - 0.3).abs() < 0.01, "{mid:?}");
        let end = track.sample(5.9);
        assert!((end.scale - 1.0).abs() < 0.02, "{end:?}");
        assert!(track.sample(1.3).scale > 1.0 && track.sample(1.3).scale < 2.0, "eases in");
    }
}
