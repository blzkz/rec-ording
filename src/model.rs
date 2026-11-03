use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROJECT_VERSION: u32 = 3;

/// A single editable video: one screen recording plus the styling applied on top of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub media: Option<MediaInfo>,
    pub trim_start: f32,
    pub trim_end: f32,
    pub touches: Vec<Touch>,
    pub zooms: Vec<ZoomSegment>,
    pub style: Style,
    pub export: ExportSettings,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            name: "Mi app".into(),
            media: None,
            trim_start: 0.0,
            trim_end: 0.0,
            touches: Vec::new(),
            zooms: Vec::new(),
            style: Style::default(),
            export: ExportSettings::default(),
        }
    }
}

impl Project {
    /// Replaces the clip, keeping the look (style, export settings, name) of the current project.
    pub fn with_new_media(&self, media: MediaInfo, touches: Vec<Touch>) -> Project {
        let mut p = Project {
            name: self.name.clone(),
            style: self.style.clone(),
            export: self.export.clone(),
            ..Default::default()
        };
        p.trim_start = 0.0;
        p.trim_end = media.duration;
        p.media = Some(media);
        p.touches = touches;
        p
    }

    pub fn duration(&self) -> f32 {
        self.media.as_ref().map(|m| m.duration).unwrap_or(0.0)
    }

    pub fn trimmed_duration(&self) -> f32 {
        (self.trim_end - self.trim_start).max(0.0)
    }

    pub fn next_zoom_id(&self) -> u64 {
        self.zooms.iter().map(|z| z.id).max().unwrap_or(0) + 1
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub path: String,
    /// Display size (after applying any rotation metadata).
    pub width: u32,
    pub height: u32,
    pub duration: f32,
    pub fps: f32,
    pub has_audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchKind {
    Tap,
    Swipe,
}

/// A finger interaction, in source-video seconds and normalized screen coordinates (0..1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Touch {
    pub time: f32,
    pub duration: f32,
    pub x: f32,
    pub y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub kind: TouchKind,
}

/// A camera move: between `start` and `end` the camera zooms to `scale` around (`x`, `y`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ZoomSegment {
    pub id: u64,
    pub start: f32,
    pub end: f32,
    pub scale: f32,
    pub x: f32,
    pub y: f32,
    /// Created by "zoom automático" (regenerating replaces only these).
    pub auto: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    Landscape,
    Portrait,
    Square,
    Feed,
}

impl Aspect {
    pub const ALL: [Aspect; 4] = [Aspect::Landscape, Aspect::Portrait, Aspect::Square, Aspect::Feed];

    pub fn ratio(self) -> f32 {
        match self {
            Aspect::Landscape => 16.0 / 9.0,
            Aspect::Portrait => 9.0 / 16.0,
            Aspect::Square => 1.0,
            Aspect::Feed => 4.0 / 5.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Landscape => "16:9",
            Aspect::Portrait => "9:16",
            Aspect::Square => "1:1",
            Aspect::Feed => "4:5",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Aspect::Landscape => "YouTube, web, ficha de Google Play",
            Aspect::Portrait => "Reels, Shorts, TikTok, Stories",
            Aspect::Square => "Instagram, X, LinkedIn",
            Aspect::Feed => "Feed de Instagram y Facebook",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameStyle {
    Rounded,
    Flat,
    Curved,
    Tablet,
    None,
    /// A .glb / .gltf model chosen by the user (see `Style::model`).
    Custom,
}

impl FrameStyle {
    pub const ALL: [FrameStyle; 6] = [
        FrameStyle::Rounded,
        FrameStyle::Flat,
        FrameStyle::Curved,
        FrameStyle::Tablet,
        FrameStyle::None,
        FrameStyle::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FrameStyle::Rounded => "Redondeado",
            FrameStyle::Flat => "Recto",
            FrameStyle::Curved => "Curvo",
            FrameStyle::Tablet => "Tablet",
            FrameStyle::None => "Sin marco",
            FrameStyle::Custom => "Modelo 3D",
        }
    }
}

/// A named device angle (degrees): `tilt` leans the top away, `turn` turns it sideways, `roll` spins it in the frame.
pub struct AnglePreset {
    pub name: &'static str,
    pub tilt: f32,
    pub turn: f32,
    pub roll: f32,
}

pub const ANGLE_PRESETS: &[AnglePreset] = &[
    AnglePreset { name: "Frente", tilt: 0.0, turn: 0.0, roll: 0.0 },
    AnglePreset { name: "Izquierda", tilt: 6.0, turn: -28.0, roll: 2.0 },
    AnglePreset { name: "Derecha", tilt: 6.0, turn: 28.0, roll: -2.0 },
    AnglePreset { name: "Tumbado", tilt: 38.0, turn: 0.0, roll: 0.0 },
    AnglePreset { name: "Dinámico", tilt: 18.0, turn: -24.0, roll: 9.0 },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameColor {
    Obsidian,
    Silver,
    Sand,
    Blue,
    White,
}

impl FrameColor {
    pub const ALL: [FrameColor; 5] = [
        FrameColor::Obsidian,
        FrameColor::Silver,
        FrameColor::Sand,
        FrameColor::Blue,
        FrameColor::White,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FrameColor::Obsidian => "Obsidiana",
            FrameColor::Silver => "Plata",
            FrameColor::Sand => "Arena",
            FrameColor::Blue => "Azul",
            FrameColor::White => "Porcelana",
        }
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            FrameColor::Obsidian => [44, 44, 48],
            FrameColor::Silver => [196, 198, 202],
            FrameColor::Sand => [214, 198, 176],
            FrameColor::Blue => [74, 92, 122],
            FrameColor::White => [236, 236, 234],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Background {
    pub a: [u8; 3],
    pub b: [u8; 3],
    /// Gradient direction in degrees (0 = left to right, 90 = top to bottom).
    pub angle: f32,
}

pub struct BackgroundPreset {
    pub name: &'static str,
    pub a: [u8; 3],
    pub b: [u8; 3],
    pub angle: f32,
}

pub const BACKGROUND_PRESETS: &[BackgroundPreset] = &[
    BackgroundPreset { name: "Medianoche", a: [24, 32, 56], b: [58, 40, 92], angle: 135.0 },
    BackgroundPreset { name: "Aurora", a: [92, 108, 214], b: [126, 214, 223], angle: 135.0 },
    BackgroundPreset { name: "Atardecer", a: [255, 138, 128], b: [255, 196, 140], angle: 120.0 },
    BackgroundPreset { name: "Lavanda", a: [176, 150, 230], b: [246, 190, 222], angle: 135.0 },
    BackgroundPreset { name: "Menta", a: [120, 210, 170], b: [208, 240, 160], angle: 135.0 },
    BackgroundPreset { name: "Océano", a: [20, 72, 140], b: [40, 170, 200], angle: 160.0 },
    BackgroundPreset { name: "Melocotón", a: [255, 226, 200], b: [250, 176, 150], angle: 135.0 },
    BackgroundPreset { name: "Bosque", a: [18, 52, 44], b: [64, 120, 84], angle: 135.0 },
    BackgroundPreset { name: "Grafito", a: [30, 30, 34], b: [70, 70, 78], angle: 90.0 },
    BackgroundPreset { name: "Nube", a: [244, 244, 246], b: [222, 224, 230], angle: 90.0 },
];

impl Default for Background {
    fn default() -> Self {
        let p = &BACKGROUND_PRESETS[1];
        Self { a: p.a, b: p.b, angle: p.angle }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub aspect: Aspect,
    pub frame: FrameStyle,
    pub frame_color: FrameColor,
    pub background: Background,
    /// Empty space around the device, as a fraction of the shortest canvas side.
    pub padding: f32,
    pub shadow: f32,
    pub show_touches: bool,
    pub touch_size: f32,
    /// Zoom level used when generating automatic zooms.
    pub auto_zoom_scale: f32,
    /// Device angle in degrees (see `AnglePreset`).
    pub tilt: f32,
    pub turn: f32,
    pub roll: f32,
    /// Face the camera while zoomed, so the app stays readable.
    pub straighten_on_zoom: bool,
    /// Slow idle sway.
    pub float_motion: bool,
    /// The device swings into place at the start of the video.
    pub intro: bool,
    /// Imported 3D model, used when `frame` is `Custom`.
    pub model: Option<CustomModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomModel {
    pub path: String,
    /// Index of the material that is the screen.
    pub screen_material: usize,
    /// Extra 90° turns of the screen, for models whose top was guessed wrong.
    pub quarter_turns: u8,
}

impl Style {
    /// Same look, seen straight on and without motion (used while picking a zoom's focus).
    pub fn flattened(&self) -> Style {
        Style { tilt: 0.0, turn: 0.0, roll: 0.0, float_motion: false, intro: false, ..self.clone() }
    }
}

impl Default for Style {
    fn default() -> Self {
        Self {
            aspect: Aspect::Landscape,
            frame: FrameStyle::Rounded,
            frame_color: FrameColor::Obsidian,
            background: Background::default(),
            padding: 0.08,
            shadow: 0.6,
            show_touches: true,
            touch_size: 1.0,
            auto_zoom_scale: 1.8,
            tilt: 0.0,
            turn: 0.0,
            roll: 0.0,
            straighten_on_zoom: true,
            float_motion: false,
            intro: false,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    P720,
    P1080,
    P2160,
}

impl Resolution {
    pub const ALL: [Resolution; 3] = [Resolution::P720, Resolution::P1080, Resolution::P2160];

    pub fn label(self) -> &'static str {
        match self {
            Resolution::P720 => "720p",
            Resolution::P1080 => "1080p",
            Resolution::P2160 => "4K",
        }
    }

    fn short_side(self) -> u32 {
        match self {
            Resolution::P720 => 720,
            Resolution::P1080 => 1080,
            Resolution::P2160 => 2160,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    H264,
    Hevc,
    Gif,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] = [ExportFormat::H264, ExportFormat::Hevc, ExportFormat::Gif];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::H264 => "MP4 · H.264",
            ExportFormat::Hevc => "MP4 · HEVC",
            ExportFormat::Gif => "GIF",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ExportFormat::H264 => "Funciona en todas partes",
            ExportFormat::Hevc => "Archivos más pequeños",
            ExportFormat::Gif => "Animación sin sonido",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Gif => "gif",
            _ => "mp4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Standard,
    High,
    Max,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Standard, Quality::High, Quality::Max];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Standard => "Estándar",
            Quality::High => "Alta",
            Quality::Max => "Máxima",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub resolution: Resolution,
    pub fps: u32,
    pub format: ExportFormat,
    pub quality: Quality,
    pub include_audio: bool,
    pub folder: String,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            resolution: Resolution::P1080,
            fps: 60,
            format: ExportFormat::H264,
            quality: Quality::High,
            include_audio: true,
            folder: videos_dir().to_string_lossy().into_owned(),
        }
    }
}

impl ExportSettings {
    /// Output size in pixels for the given canvas aspect (always even, as encoders require).
    pub fn dimensions(&self, aspect: Aspect) -> (u32, u32) {
        let short = self.resolution.short_side() as f32;
        let r = aspect.ratio();
        let (w, h) = if r >= 1.0 { (short * r, short) } else { (short, short / r) };
        let even = |v: f32| ((v.round() as u32) / 2 * 2).max(2);
        (even(w), even(h))
    }
}

// ---------- Locations ----------

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".local/share"));
    let dir = base.join("rec-ording");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".cache"));
    let dir = base.join("rec-ording");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// `~/Vídeos/rec-ording` (localized through xdg-user-dir), created on demand.
pub fn videos_dir() -> PathBuf {
    let base = Command::new("xdg-user-dir")
        .arg("VIDEOS")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty() && Path::new(s).is_dir())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("Videos"));
    let dir = base.join("rec-ording");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Local timestamp suitable for file names, e.g. `2026-11-03 14.32.10`.
pub fn timestamp_for_filename() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H.%M.%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            secs.to_string()
        })
}

fn autosave_path() -> PathBuf {
    data_dir().join("proyecto.json")
}

pub fn load_last_project() -> Project {
    let Ok(raw) = fs::read_to_string(autosave_path()) else {
        return Project::default();
    };
    let Ok(mut p) = serde_json::from_str::<Project>(&raw) else {
        return Project::default();
    };
    // The recording may have been moved or deleted since the last session.
    if let Some(m) = &p.media {
        if !Path::new(&m.path).exists() {
            p.media = None;
            p.touches.clear();
            p.zooms.clear();
        }
    }
    if !Path::new(&p.export.folder).is_dir() {
        p.export.folder = videos_dir().to_string_lossy().into_owned();
    }
    p
}

pub fn save_project(project: &Project) -> Result<(), String> {
    let json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    fs::write(autosave_path(), json).map_err(|e| e.to_string())
}
