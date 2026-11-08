//! Sound: the recordings' own sound mixed with the effects, voice-over and music tracks. One
//! ffmpeg filter graph does the mix for the export and for the preview, which plays it on the
//! computer's sound output while the video plays.

use crate::model::{AudioTrack, Project};
use crate::platform::command;
use crate::timeline;
use std::io::Read;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

/// Inputs and filters that mix the project's sound into `[aout]`.
pub struct Graph {
    pub args: Vec<String>,
    pub filter: String,
}

/// `,atempo=…` steps for a speed (each step goes from ×0,5 to ×2).
pub fn atempo(speed: f32) -> String {
    let mut s = speed.clamp(0.25, 4.0);
    let mut out = String::new();
    while (s - 1.0).abs() > 1e-3 {
        let step = s.clamp(0.5, 2.0);
        out.push_str(&format!(",atempo={step:.4}"));
        s /= step;
    }
    out
}

fn fades(fade_in: f32, fade_out: f32, len: f32) -> String {
    let mut f = String::new();
    if fade_in > 0.0 {
        f.push_str(&format!(",afade=t=in:d={:.3}", fade_in.min(len)));
    }
    if fade_out > 0.0 {
        f.push_str(&format!(",afade=t=out:st={:.3}:d={:.3}", (len - fade_out).max(0.0), fade_out.min(len)));
    }
    f
}

/// The mix of everything that sounds, as ffmpeg inputs (numbered from `first`) and a filter
/// graph ending in `[aout]`, as long as the video. `fps` rounds the clips to whole frames, as
/// the export does. `None` when nothing sounds.
pub fn graph(p: &Project, first: usize, fps: Option<f32>) -> Option<Graph> {
    let at = |t: f32| match fps {
        Some(f) => (t * f).round() / f,
        None => t,
    };
    let total = at(timeline::duration(p)).max(0.1);
    let mix = &p.mix;
    let mut args: Vec<String> = Vec::new();
    let mut filter = String::new();
    let mut next = first;
    let mut input = |args: &mut Vec<String>, a: &[String]| {
        args.extend_from_slice(a);
        next += 1;
        next - 1
    };
    let s = |v: &str| v.to_string();

    // The recordings, clip after clip (silence for stills and clips without sound).
    let placed = timeline::placed(p);
    let sounds = |c: &timeline::Placed| c.clip.freeze.is_none() && p.media(c.clip.media).is_some_and(|m| m.info.has_audio);
    let recording = mix.recording > 0.001 && placed.iter().any(sounds);
    let clips: Vec<_> = p.audio.iter().filter(|c| c.start < total && c.duration > 0.0 && c.volume * mix.track(c.track) > 0.001).collect();
    if !recording && clips.is_empty() {
        return None;
    }

    let base = input(&mut args, &[s("-f"), s("lavfi"), s("-t"), format!("{total:.3}"), s("-i"), s("anullsrc=r=48000:cl=stereo")]);
    let mut layers = vec![format!("[{base}:a]")];

    if recording {
        for (k, c) in placed.iter().enumerate() {
            let shown = format!("{:.3}", at(c.end) - at(c.start));
            if sounds(c) {
                let path = &p.media(c.clip.media).unwrap().info.path;
                let len = format!("{:.3}", c.clip.source_out - c.clip.source_in);
                let i = input(&mut args, &[s("-ss"), format!("{:.3}", c.clip.source_in), s("-t"), len.clone(), s("-i"), path.clone()]);
                filter.push_str(&format!(
                    "[{i}:a]aresample=48000,aformat=channel_layouts=stereo,atrim=0:{len},asetpts=PTS-STARTPTS{tempo},atrim=0:{shown}[r{k}];",
                    tempo = atempo(c.clip.speed.max(0.01))
                ));
            } else {
                let i = input(&mut args, &[s("-f"), s("lavfi"), s("-t"), shown.clone(), s("-i"), s("anullsrc=r=48000:cl=stereo")]);
                filter.push_str(&format!("[{i}:a]atrim=0:{shown},asetpts=PTS-STARTPTS[r{k}];"));
            }
        }
        for k in 0..placed.len() {
            filter.push_str(&format!("[r{k}]"));
        }
        filter.push_str(&format!("concat=n={}:v=0:a=1,volume={:.3}[rec];", placed.len(), mix.recording));
        layers.push("[rec]".into());
    }

    // The tracks: each sound placed at its moment, then the sounds of a track together.
    let mut tracks: Vec<(AudioTrack, String)> = Vec::new();
    for track in AudioTrack::ALL {
        let mine: Vec<_> = clips.iter().filter(|c| c.track == track).collect();
        if mine.is_empty() {
            continue;
        }
        let mut labels = String::new();
        for c in &mine {
            let dur = format!("{:.3}", c.duration);
            let i = if c.looped {
                input(&mut args, &[s("-stream_loop"), s("-1"), s("-i"), c.path.clone()])
            } else {
                input(&mut args, &[s("-ss"), format!("{:.3}", c.offset), s("-t"), dur.clone(), s("-i"), c.path.clone()])
            };
            let trim = if c.looped { format!("atrim=start={:.3}:duration={dur}", c.offset) } else { format!("atrim=0:{dur}") };
            let delay = (c.start * 1000.0).round() as i64;
            filter.push_str(&format!(
                "[{i}:a]aresample=48000,aformat=channel_layouts=stereo,{trim},asetpts=PTS-STARTPTS,volume={:.3}{},adelay={delay}|{delay}[c{}];",
                c.volume * mix.track(track),
                fades(c.fade_in, c.fade_out, c.duration),
                c.id
            ));
            labels.push_str(&format!("[c{}]", c.id));
        }
        let name = format!("[{}]", track.key());
        if mine.len() == 1 {
            filter.push_str(&format!("{labels}anull{name};"));
        } else {
            filter.push_str(&format!("{labels}amix=inputs={}:normalize=0:duration=longest{name};", mine.len()));
        }
        tracks.push((track, name));
    }
    // The music steps back while the voice-over speaks.
    let has = |t: AudioTrack| tracks.iter().any(|(k, _)| *k == t);
    if mix.duck_music && has(AudioTrack::Voice) && has(AudioTrack::Music) {
        filter.push_str("[voice]asplit=2[voice_out][voice_key];[music][voice_key]sidechaincompress=threshold=0.02:ratio=8:attack=20:release=400[music_out];");
        for (_, name) in &mut tracks {
            if name == "[voice]" || name == "[music]" {
                *name = format!("{}_out]", name.trim_end_matches(']'));
            }
        }
    }
    layers.extend(tracks.into_iter().map(|(_, n)| n));

    // Everything together, as long as the video, with the picture's fades.
    let fin = &p.style.finish;
    filter.push_str(&format!(
        "{}amix=inputs={}:normalize=0:duration=first{}[aout]",
        layers.concat(),
        layers.len(),
        fades(fin.fade_in, fin.fade_out, total)
    ));
    Some(Graph { args, filter })
}

/// Length of a sound file in seconds; an error if it has no sound.
pub fn length(path: &str) -> Result<f32, String> {
    let out = command("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_type:format=duration", "-of", "default=nw=1"])
        .arg(path)
        .output()
        .map_err(|_| "No se encontró ffprobe".to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.contains("codec_type=audio") {
        return Err("Ese archivo no tiene sonido".into());
    }
    text.lines()
        .find_map(|l| l.strip_prefix("duration="))
        .and_then(|d| d.trim().parse::<f32>().ok())
        .filter(|d| *d > 0.0)
        .ok_or_else(|| "No se pudo saber cuánto dura ese sonido".into())
}

/// Loudness of a sound file, `PEAKS_PER_SECOND` values per second (0..1), for drawing its wave.
pub const PEAKS_PER_SECOND: f32 = 50.0;

pub fn peaks(path: &str) -> Option<Vec<f32>> {
    let rate = 4000;
    let out = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", &rate.to_string(), "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let samples: Vec<f32> = out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    let per = (rate as f32 / PEAKS_PER_SECOND) as usize;
    let peaks: Vec<f32> = samples.chunks(per).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs()))).collect();
    let top = peaks.iter().cloned().fold(0.0f32, f32::max).max(1e-4);
    Some(peaks.into_iter().map(|v| (v / top).sqrt()).collect())
}

/// The whole mix as interleaved stereo samples at `rate`, for the preview.
pub fn render(p: &Project, rate: u32) -> Result<Vec<f32>, String> {
    let Some(g) = graph(p, 0, None) else { return Ok(Vec::new()) };
    let mut child = command("ffmpeg")
        .args(["-nostdin", "-loglevel", "error"])
        .args(&g.args)
        .args(["-filter_complex", &g.filter, "-map", "[aout]", "-ac", "2", "-ar", &rate.to_string(), "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "No se encontró ffmpeg".to_string())?;
    let mut raw = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if !ok {
        return Err(format!("No se pudo mezclar el sonido: {}", err.trim()));
    }
    Ok(raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// What the sound output reads: the mix and where playback is.
#[derive(Default)]
struct Shared {
    samples: Arc<Vec<f32>>,
    /// Next stereo frame to play.
    pos: usize,
    playing: bool,
}

/// Plays the mix on the default sound output, in step with the preview.
pub struct Player {
    shared: Arc<Mutex<Shared>>,
    pub rate: u32,
    _stream: cpal::Stream,
}

impl Player {
    /// The default output, or `None` when there is none (the preview then plays silently).
    pub fn new() -> Option<Player> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = config.channels.max(1) as usize;
        let shared = Arc::new(Mutex::new(Shared::default()));
        // Fills `n` frames, calling `put` with each frame's left and right samples.
        fn fill(shared: &Mutex<Shared>, frames: usize, mut put: impl FnMut(usize, f32, f32)) {
            let Ok(mut s) = shared.try_lock() else {
                (0..frames).for_each(|f| put(f, 0.0, 0.0));
                return;
            };
            for f in 0..frames {
                let i = s.pos * 2;
                if !s.playing {
                    put(f, 0.0, 0.0);
                    continue;
                }
                // The clock runs on past the end of the mix (or with no mix), in silence.
                match s.samples.get(i..i + 2) {
                    Some(lr) => put(f, lr[0], lr[1]),
                    None => put(f, 0.0, 0.0),
                }
                s.pos += 1;
            }
        }
        let err = |e| eprintln!("rec·ording: sonido: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => {
                let sh = shared.clone();
                device.build_output_stream(&config, move |data: &mut [f32], _| {
                    fill(&sh, data.len() / channels, |f, l, r| write(&mut data[f * channels..(f + 1) * channels], l, r, |v| v));
                }, err, None)
            }
            cpal::SampleFormat::I16 => {
                let sh = shared.clone();
                device.build_output_stream(&config, move |data: &mut [i16], _| {
                    fill(&sh, data.len() / channels, |f, l, r| write(&mut data[f * channels..(f + 1) * channels], l, r, |v| (v.clamp(-1.0, 1.0) * 32767.0) as i16));
                }, err, None)
            }
            cpal::SampleFormat::U16 => {
                let sh = shared.clone();
                device.build_output_stream(&config, move |data: &mut [u16], _| {
                    fill(&sh, data.len() / channels, |f, l, r| write(&mut data[f * channels..(f + 1) * channels], l, r, |v| ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16));
                }, err, None)
            }
            _ => return None,
        }
        .ok()?;
        stream.play().ok()?;
        Some(Player { shared, rate: config.sample_rate.0, _stream: stream })
    }

    /// Puts in a new mix (rendered at `self.rate`), keeping the playback position.
    pub fn set_mix(&self, samples: Vec<f32>) {
        self.shared.lock().unwrap().samples = Arc::new(samples);
    }

    /// Where playback is, in seconds.
    pub fn position(&self) -> f32 {
        self.shared.lock().unwrap().pos as f32 / self.rate as f32
    }

    pub fn is_playing(&self) -> bool {
        self.shared.lock().unwrap().playing
    }

    pub fn play_from(&self, t: f32) {
        let mut s = self.shared.lock().unwrap();
        s.pos = (t.max(0.0) * self.rate as f32) as usize;
        s.playing = true;
    }

    pub fn stop(&self) {
        self.shared.lock().unwrap().playing = false;
    }
}

/// One output frame: left and right on the first two channels (both on a mono output).
fn write<T: Copy + Default>(frame: &mut [T], l: f32, r: f32, conv: impl Fn(f32) -> T) {
    match frame.len() {
        1 => frame[0] = conv((l + r) * 0.5),
        _ => {
            frame[0] = conv(l);
            frame[1] = conv(r);
            for x in &mut frame[2..] {
                *x = T::default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioClip, Media, MediaInfo};

    /// A sine wave file of `secs` seconds at `freq`.
    fn tone(name: &str, secs: f32, freq: u32) -> String {
        let path = std::env::temp_dir().join(format!("rec-ording-{name}-{}.wav", std::process::id()));
        let ok = command("ffmpeg")
            .args(["-nostdin", "-y", "-loglevel", "error", "-f", "lavfi", "-i", &format!("sine=frequency={freq}:duration={secs}"), "-ac", "2"])
            .arg(&path)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg");
        path.to_string_lossy().into_owned()
    }

    /// Loudness (RMS) of the stereo mix between two seconds.
    fn rms(samples: &[f32], rate: u32, a: f32, b: f32) -> f32 {
        let (i, j) = ((a * rate as f32) as usize * 2, (b * rate as f32) as usize * 2);
        let part = &samples[i.min(samples.len())..j.min(samples.len())];
        (part.iter().map(|v| v * v).sum::<f32>() / part.len().max(1) as f32).sqrt()
    }

    #[test]
    fn mixes_the_tracks_at_their_moments() {
        let silent = MediaInfo { path: "/nada.mp4".into(), width: 1080, height: 1920, duration: 6.0, fps: 30.0, has_audio: false };
        let mut p = Project { media: vec![Media { id: 1, info: silent, touches: Vec::new(), marks: Vec::new() }], ..Default::default() };
        p.clips.push(crate::model::Clip::new(2, 1, 0.0, 6.0));
        assert!(graph(&p, 0, None).is_none(), "nothing sounds yet");

        let fx = tone("fx", 0.5, 880);
        let music = tone("music", 1.5, 220);
        let len = length(&music).unwrap();
        assert!((len - 1.5).abs() < 0.05, "{len}");
        assert!(length("/nada.mp4").is_err());

        // An effect at 1 s; music from 3 s, looping to the end; two effects overlapping at 4 s.
        timeline::add_audio(&mut p, AudioTrack::Effects, &fx, 0.5, 1.0).unwrap();
        let m = timeline::add_audio(&mut p, AudioTrack::Music, &music, len, 3.0).unwrap();
        let c = p.audio.iter().find(|c| c.id == m).unwrap();
        assert!(c.looped && (c.duration - 3.0).abs() < 1e-4, "{c:?}");
        p.audio.push(AudioClip { id: 90, path: fx.clone(), start: 4.0, duration: 0.5, length: 0.5, ..Default::default() });
        p.audio.push(AudioClip { id: 91, path: fx.clone(), start: 4.2, duration: 0.5, length: 0.5, ..Default::default() });
        let (lane, n) = timeline::lanes(&p.audio.iter().filter(|c| c.track == AudioTrack::Effects).map(|c| (c.id, c.start, c.end())).collect::<Vec<_>>());
        assert_eq!((lane, n), (vec![0, 0, 1], 2));
        p.style.finish.fade_in = 0.0;
        p.style.finish.fade_out = 0.0;

        let rate = 8000;
        let mix = render(&p, rate).unwrap();
        assert!((mix.len() as f32 / 2.0 / rate as f32 - 6.0).abs() < 0.05, "as long as the video: {}", mix.len());
        assert!(rms(&mix, rate, 0.2, 0.8) < 0.001, "silence before the effect");
        assert!(rms(&mix, rate, 1.1, 1.4) > 0.05, "the effect: {}", rms(&mix, rate, 1.1, 1.4));
        assert!(rms(&mix, rate, 2.0, 2.9) < 0.001, "silence between");
        assert!(rms(&mix, rate, 5.0, 5.9) > 0.03, "the music, looped past the end of its file: {}", rms(&mix, rate, 5.0, 5.9));

        // Quieter music.
        let loud = rms(&mix, rate, 5.0, 5.9);
        p.mix.music = 0.25;
        let quiet = rms(&render(&p, rate).unwrap(), rate, 5.0, 5.9);
        assert!((quiet / loud - 0.25 / 0.8).abs() < 0.05, "{quiet} vs {loud}");

        // Each track has its own volume: lowering one leaves the other as it was.
        p.mix.music = 0.8;
        let base = render(&p, rate).unwrap();
        p.mix.effects = 0.5;
        let softer = render(&p, rate).unwrap();
        assert!((rms(&softer, rate, 1.1, 1.4) / rms(&base, rate, 1.1, 1.4) - 0.5).abs() < 0.03, "effects at half");
        assert!((rms(&softer, rate, 3.2, 3.9) / rms(&base, rate, 3.2, 3.9) - 1.0).abs() < 0.01, "the music does not change");
        p.mix.effects = 1.0;

        // A voice-over over the music: the music steps back while it speaks.
        let voice = tone("voice", 1.0, 440);
        timeline::add_audio(&mut p, AudioTrack::Voice, &voice, 1.0, 4.8).unwrap();
        let ducked = render(&p, rate).unwrap();
        p.mix.duck_music = false;
        let plain = render(&p, rate).unwrap();
        assert!(rms(&ducked, rate, 5.2, 5.7) < rms(&plain, rate, 5.2, 5.7) * 0.9, "the music is lower under the voice");
        let _ = std::fs::remove_file(voice);
        for f in [fx, music] {
            let _ = std::fs::remove_file(f);
        }
    }

    /// Opens the sound output and plays silence: the clock moves on. Needs a sound output.
    #[test]
    #[ignore]
    fn plays_on_the_sound_output() {
        let player = Player::new().expect("no sound output");
        assert!(player.rate >= 8000);
        player.play_from(2.0);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let t = player.position();
        assert!(player.is_playing() && t > 2.1 && t < 2.8, "{t}");
        player.stop();
        assert!(!player.is_playing());
    }

    #[test]
    fn trimming_the_start_keeps_the_sound_in_place() {
        let mut p = Project::default();
        let id = timeline::add_audio(&mut p, AudioTrack::Voice, "/voz.wav", 10.0, 2.0).unwrap();
        timeline::trim_audio(&mut p, id, 3.0, 12.0).unwrap();
        let c = &p.audio[0];
        assert_eq!((c.start, c.offset, c.duration), (3.0, 1.0, 9.0));
        // Not before the file begins, nor past its end.
        timeline::trim_audio(&mut p, id, 0.0, 20.0).unwrap();
        let c = &p.audio[0];
        assert_eq!((c.start, c.offset, c.duration), (2.0, 0.0, 10.0));
    }
}
