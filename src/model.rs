use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceModelType {
    #[serde(rename = "pixel-11-pro")]
    Pixel11Pro,
    #[serde(rename = "galaxy-s26-ultra")]
    GalaxyS26Ultra,
    #[serde(rename = "galaxy-tab-s10-ultra")]
    GalaxyTabS10Ultra,
    #[serde(rename = "desktop-window")]
    DesktopWindow,
    #[serde(rename = "custom-glb")]
    CustomGlb,
    #[serde(rename = "none")]
    None,
}

impl DeviceModelType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pixel11Pro => "Google Pixel 11 Pro",
            Self::GalaxyS26Ultra => "Samsung Galaxy S26 Ultra",
            Self::GalaxyTabS10Ultra => "Samsung Galaxy Tab S10 Ultra",
            Self::DesktopWindow => "Ventana PC 3D",
            Self::CustomGlb => "Modelo .GLB Personalizado",
            Self::None => "Sin Marco (Pantalla Plana)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DualLayout {
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "side-by-side")]
    SideBySide,
    #[serde(rename = "pip-floating")]
    PipFloating,
    #[serde(rename = "angled-showcase")]
    AngledShowcase,
}

impl DualLayout {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Single => "Un dispositivo",
            Self::SideBySide => "Lado a Lado (PC + Android)",
            Self::PipFloating => "PiP Flotante 3D",
            Self::AngledShowcase => "Escaparate en Ángulo",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    #[serde(rename = "portrait")]
    Portrait,
    #[serde(rename = "landscape")]
    Landscape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoCodec {
    #[serde(rename = "h264")]
    H264,
    #[serde(rename = "hevc")]
    Hevc,
    #[serde(rename = "av1")]
    Av1,
    #[serde(rename = "vp9-alpha")]
    Vp9Alpha,
    #[serde(rename = "prores-alpha")]
    ProresAlpha,
    #[serde(rename = "gif")]
    Gif,
    #[serde(rename = "png-sequence")]
    PngSequence,
}

impl VideoCodec {
    pub fn label(&self) -> &'static str {
        match self {
            Self::H264 => "H.264 (libx264 · Máxima compatibilidad)",
            Self::Hevc => "HEVC / H.265 (libx265 · Alta eficiencia)",
            Self::Av1 => "AV1 (libsvtav1 · Nueva generación)",
            Self::Vp9Alpha => "WebM VP9 con Transparencia Alpha",
            Self::ProresAlpha => "Apple ProRes 4444 con Canal Alpha",
            Self::Gif => "GIF Animado (2 pasos palettegen)",
            Self::PngSequence => "Secuencia / Captura PNG Sin Pérdida",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportPresetId {
    #[serde(rename = "play-store-16-9-4k")]
    PlayStore16x9_4k,
    #[serde(rename = "play-store-16-9-1080p")]
    PlayStore16x9_1080p,
    #[serde(rename = "play-store-shorts-9-16")]
    PlayStoreShorts9x16,
    #[serde(rename = "play-store-tablet-16-10")]
    PlayStoreTablet16x10,
    #[serde(rename = "app-store-iphone-6-9")]
    AppStoreIphone6_9,
    #[serde(rename = "app-store-ipad-pro")]
    AppStoreIpadPro,
    #[serde(rename = "transparent-webm")]
    TransparentWebm,
    #[serde(rename = "gif-preview")]
    GifPreview,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    pub preset_id: ExportPresetId,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: VideoCodec,
    pub crf: u32,
    pub transparent_background: bool,
    pub include_audio: bool,
    pub burn_subtitles: bool,
    pub motion_blur_samples: u32,
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryPointerEvent {
    pub id: String,
    pub timestamp_ms: u64,
    pub source: String, // "desktop" | "android"
    #[serde(rename = "type")]
    pub event_type: String, // "move" | "click" | "touch_tap" | "touch_swipe"
    pub x: f32,
    pub y: f32,
    pub end_x: Option<f32>,
    pub end_y: Option<f32>,
    pub duration_ms: Option<u64>,
    pub button: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryKeyEvent {
    pub id: String,
    pub timestamp_ms: u64,
    pub key_combo: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyframeF32 {
    pub id: String,
    pub time_sec: f32,
    pub value: f32,
    pub easing: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorCameraKeyframe {
    pub id: String,
    pub time_sec: f32,
    pub duration_sec: f32,
    pub focus_x: f32,
    pub focus_y: f32,
    pub zoom: f32,
    pub rot_x_deg: f32,
    pub rot_y_deg: f32,
    pub rot_z_deg: f32,
    pub easing: String,
    pub label: Option<String>,
    pub auto_generated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreezeFrameSegment {
    pub id: String,
    pub source_time_sec: f32,
    pub timeline_start_sec: f32,
    pub duration_sec: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivacyMaskMode {
    #[serde(rename = "blur")]
    Blur,
    #[serde(rename = "pixelate")]
    Pixelate,
    #[serde(rename = "solid")]
    Solid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyMask {
    pub id: String,
    pub label: String,
    pub start_sec: f32,
    pub end_sec: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub mode: PrivacyMaskMode,
    pub intensity: u32,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub id: String,
    pub track_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub clip_type: String, // "video" | "audio" | "text"
    pub media_path: Option<String>,
    pub source_role: Option<String>,
    pub start_sec: f32,
    pub duration_sec: f32,
    pub trim_in_sec: f32,
    pub trim_out_sec: f32,
    pub speed: f32,
    pub volume: f32,
    pub muted: bool,
    pub fade_in_sec: f32,
    pub fade_out_sec: f32,
    #[serde(default)]
    pub freeze_frames: Vec<FreezeFrameSegment>,
    #[serde(default)]
    pub opacity_keyframes: Vec<KeyframeF32>,
    #[serde(default)]
    pub scale_keyframes: Vec<KeyframeF32>,
    pub text: Option<String>,
    pub subtitle_text: Option<String>,
    pub title_preset: Option<String>,
    pub font_size: Option<f32>,
    pub text_color: Option<String>,
    pub bg_color: Option<String>,
    pub pos_x: Option<f32>,
    pub pos_y: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTrack {
    pub id: String,
    pub name: String,
    pub kind: String, // "video" | "audio" | "text"
    pub index: usize,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub volume: f32,
    pub clips: Vec<TimelineClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleWord {
    pub id: String,
    pub word: String,
    pub start_sec: f32,
    pub end_sec: f32,
    pub confidence: f32,
    pub highlight: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSegment {
    pub id: String,
    pub start_sec: f32,
    pub end_sec: f32,
    pub speaker: Option<String>,
    pub words: Vec<SubtitleWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStyle {
    pub enabled: bool,
    pub position_y: f32,
    pub font_size: f32,
    pub font_family: String,
    pub text_color: String,
    pub active_word_color: String,
    pub background_color: String,
    pub word_by_word_highlight: bool,
    pub max_words_per_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device3DConfig {
    pub primary_model: DeviceModelType,
    pub secondary_model: DeviceModelType,
    pub dual_layout: DualLayout,
    pub custom_glb_path: Option<String>,
    pub custom_glb_screen_mesh_name: Option<String>,
    pub body_color: String,
    pub frame_metalness: f32,
    pub frame_roughness: f32,
    pub screen_corner_radius: f32,
    pub bezel_width: f32,
    pub show_shadow: bool,
    pub shadow_intensity: f32,
    pub floating_animation: bool,
    pub floating_amplitude: f32,
    pub orientation: Orientation,
    pub ambient_light_intensity: f32,
    pub directional_light_intensity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAndTouchStyle {
    pub smooth_cursor: bool,
    pub smoothing_factor: f32,
    pub cursor_size: f32,
    pub cursor_style: String,
    pub cursor_color: String,
    pub show_click_ripples: bool,
    pub click_ripple_color: String,
    pub show_touch_trails: bool,
    pub hide_idle_cursor_after_ms: u64,
    pub show_keystrokes_hud: bool,
    pub keystroke_position: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundStyle {
    #[serde(rename = "type")]
    pub bg_type: String,
    pub color_a: String,
    pub color_b: String,
    pub color_c: String,
    pub gradient_angle: f32,
    pub padding_ratio: f32,
    pub motion_blur_enabled: bool,
    pub motion_blur_intensity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub version: String,
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub duration_sec: f32,
    pub tracks: Vec<TimelineTrack>,
    pub director_keyframes: Vec<DirectorCameraKeyframe>,
    pub privacy_masks: Vec<PrivacyMask>,
    pub pointer_events: Vec<TelemetryPointerEvent>,
    pub key_events: Vec<TelemetryKeyEvent>,
    pub subtitles: Vec<SubtitleSegment>,
    pub subtitle_style: SubtitleStyle,
    #[serde(rename = "device3D")]
    pub device_3d: Device3DConfig,
    pub cursor_style: CursorAndTouchStyle,
    pub background: BackgroundStyle,
    pub export_settings: ExportSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDeviceInfo {
    pub serial: String,
    pub state: String,
    pub model: String,
    pub product: String,
    pub device: String,
    pub transport_id: String,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub repo: String,
    pub release_title: String,
    pub release_notes: String,
    pub published_at: String,
    pub asset_name: Option<String>,
    pub release_url: String,
}

pub fn default_project_path() -> PathBuf {
    let local_workspace = PathBuf::from("/home/blzkz/Documentos/pet-projects/rec-ordingApp/projects/active-project.recproj.json");
    if local_workspace.parent().map(|p| p.exists()).unwrap_or(false) {
        return local_workspace;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(home).join(".local/share/rec-ording/projects");
    let _ = fs::create_dir_all(&dir);
    dir.join("active-project.recproj.json")
}

pub fn default_exports_dir() -> PathBuf {
    let local_exports = PathBuf::from("/home/blzkz/Documentos/pet-projects/rec-ordingApp/exports");
    if local_exports.parent().map(|p| p.exists()).unwrap_or(false) {
        let _ = fs::create_dir_all(&local_exports);
        return local_exports;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(home).join("Videos/rec-ording");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn create_default_project() -> ProjectFile {
    let mut tracks = Vec::new();

    for idx in 1..=4 {
        let clips = if idx == 1 {
            vec![TimelineClip {
                id: "clip-demo-android".into(),
                track_id: "video-track-1".into(),
                name: "Demo_App_Android_PlayStore.mp4".into(),
                clip_type: "video".into(),
                media_path: Some("builtin://android-demo".into()),
                source_role: Some("android-main".into()),
                start_sec: 0.0,
                duration_sec: 12.0,
                trim_in_sec: 0.0,
                trim_out_sec: 12.0,
                speed: 1.0,
                volume: 1.0,
                muted: false,
                fade_in_sec: 0.3,
                fade_out_sec: 0.5,
                freeze_frames: vec![],
                opacity_keyframes: vec![KeyframeF32 {
                    id: "kf-op-1".into(),
                    time_sec: 0.0,
                    value: 1.0,
                    easing: "linear".into(),
                }],
                scale_keyframes: vec![KeyframeF32 {
                    id: "kf-sc-1".into(),
                    time_sec: 0.0,
                    value: 1.0,
                    easing: "spring".into(),
                }],
                text: None,
                subtitle_text: None,
                title_preset: None,
                font_size: None,
                text_color: None,
                bg_color: None,
                pos_x: None,
                pos_y: None,
            }]
        } else if idx == 2 {
            vec![TimelineClip {
                id: "clip-demo-pc".into(),
                track_id: "video-track-2".into(),
                name: "Rust_ADB_Logcat_IDE.mp4".into(),
                clip_type: "video".into(),
                media_path: Some("builtin://pc-demo".into()),
                source_role: Some("pc-screen".into()),
                start_sec: 0.0,
                duration_sec: 12.0,
                trim_in_sec: 0.0,
                trim_out_sec: 12.0,
                speed: 1.0,
                volume: 0.6,
                muted: false,
                fade_in_sec: 0.0,
                fade_out_sec: 0.0,
                freeze_frames: vec![],
                opacity_keyframes: vec![],
                scale_keyframes: vec![],
                text: None,
                subtitle_text: None,
                title_preset: None,
                font_size: None,
                text_color: None,
                bg_color: None,
                pos_x: None,
                pos_y: None,
            }]
        } else {
            vec![]
        };

        tracks.push(TimelineTrack {
            id: format!("video-track-{}", idx),
            name: match idx {
                1 => "V1 · Android Principal".into(),
                2 => "V2 · Pantalla PC / Secundaria".into(),
                3 => "V3 · Webcam / PiP".into(),
                _ => "V4 · B-Roll / Overlay".into(),
            },
            kind: "video".into(),
            index: idx,
            muted: false,
            solo: false,
            locked: false,
            volume: 1.0,
            clips,
        });
    }

    for idx in 1..=4 {
        let clips = if idx == 1 {
            vec![TimelineClip {
                id: "clip-voiceover-1".into(),
                track_id: "audio-track-1".into(),
                name: "Locucion_Demo_PlayStore.wav".into(),
                clip_type: "audio".into(),
                media_path: Some("builtin://voiceover-demo".into()),
                source_role: None,
                start_sec: 0.4,
                duration_sec: 11.2,
                trim_in_sec: 0.0,
                trim_out_sec: 11.2,
                speed: 1.0,
                volume: 1.1,
                muted: false,
                fade_in_sec: 0.2,
                fade_out_sec: 0.4,
                freeze_frames: vec![],
                opacity_keyframes: vec![],
                scale_keyframes: vec![],
                text: None,
                subtitle_text: None,
                title_preset: None,
                font_size: None,
                text_color: None,
                bg_color: None,
                pos_x: None,
                pos_y: None,
            }]
        } else {
            vec![]
        };

        tracks.push(TimelineTrack {
            id: format!("audio-track-{}", idx),
            name: match idx {
                1 => "A1 · Micrófono Voz en Off".into(),
                2 => "A2 · Audio Sistema / App".into(),
                3 => "A3 · Música de Fondo".into(),
                _ => "A4 · Efectos SFX (Toques/Whoosh)".into(),
            },
            kind: "audio".into(),
            index: idx,
            muted: false,
            solo: false,
            locked: false,
            volume: if idx == 3 { 0.35 } else { 1.0 },
            clips,
        });
    }

    for idx in 1..=5 {
        let clips = if idx == 1 {
            vec![
                TimelineClip {
                    id: "clip-title-intro".into(),
                    track_id: "text-track-1".into(),
                    name: "Título: Nueva App en Google Play".into(),
                    clip_type: "text".into(),
                    media_path: None,
                    source_role: None,
                    start_sec: 0.2,
                    duration_sec: 3.6,
                    trim_in_sec: 0.0,
                    trim_out_sec: 3.6,
                    speed: 1.0,
                    volume: 1.0,
                    muted: false,
                    fade_in_sec: 0.3,
                    fade_out_sec: 0.3,
                    freeze_frames: vec![],
                    opacity_keyframes: vec![],
                    scale_keyframes: vec![],
                    text: Some("Diseñada para velocidad extrema".into()),
                    subtitle_text: Some("Disponible en Google Play Store · 120 Hz nativos".into()),
                    title_preset: Some("pop-elastic".into()),
                    font_size: Some(42.0),
                    text_color: Some("#ffffff".into()),
                    bg_color: Some("#0f172a".into()),
                    pos_x: Some(0.5),
                    pos_y: Some(0.11),
                },
                TimelineClip {
                    id: "clip-title-mid".into(),
                    track_id: "text-track-1".into(),
                    name: "Título: Captura USB + Vulkan 3D".into(),
                    clip_type: "text".into(),
                    media_path: None,
                    source_role: None,
                    start_sec: 4.4,
                    duration_sec: 3.8,
                    trim_in_sec: 0.0,
                    trim_out_sec: 3.8,
                    speed: 1.0,
                    volume: 1.0,
                    muted: false,
                    fade_in_sec: 0.3,
                    fade_out_sec: 0.3,
                    freeze_frames: vec![],
                    opacity_keyframes: vec![],
                    scale_keyframes: vec![],
                    text: Some("Motor 100% Rust + Vulkan Multihilo".into()),
                    subtitle_text: Some("Auto-zoom en cada toque + FFmpeg directo".into()),
                    title_preset: Some("glass-badge".into()),
                    font_size: Some(40.0),
                    text_color: Some("#f8fafc".into()),
                    bg_color: Some("#312e81".into()),
                    pos_x: Some(0.5),
                    pos_y: Some(0.11),
                },
            ]
        } else if idx == 2 {
            vec![TimelineClip {
                id: "clip-title-cta".into(),
                track_id: "text-track-2".into(),
                name: "CTA: Descárgala en Play Store".into(),
                clip_type: "text".into(),
                media_path: None,
                source_role: None,
                start_sec: 8.6,
                duration_sec: 3.2,
                trim_in_sec: 0.0,
                trim_out_sec: 3.2,
                speed: 1.0,
                volume: 1.0,
                muted: false,
                fade_in_sec: 0.25,
                fade_out_sec: 0.35,
                freeze_frames: vec![],
                opacity_keyframes: vec![],
                scale_keyframes: vec![],
                text: Some("Descárgala hoy en Google Play".into()),
                subtitle_text: Some("★ 4.9 Valoración · Gratis".into()),
                title_preset: Some("neon-glow".into()),
                font_size: Some(42.0),
                text_color: Some("#34d399".into()),
                bg_color: Some("#064e3b".into()),
                pos_x: Some(0.5),
                pos_y: Some(0.12),
            }]
        } else {
            vec![]
        };

        tracks.push(TimelineTrack {
            id: format!("text-track-{}", idx),
            name: match idx {
                1 => "T1 · Títulos Principales".into(),
                2 => "T2 · Badges de Función".into(),
                3 => "T3 · Callouts / Flechas".into(),
                4 => "T4 · Banner Inferior".into(),
                _ => "T5 · Marca de Agua / CTA".into(),
            },
            kind: "text".into(),
            index: idx,
            muted: false,
            solo: false,
            locked: false,
            volume: 1.0,
            clips,
        });
    }

    ProjectFile {
        version: "2.0.0".into(),
        id: "proj-default-rust-vulkan".into(),
        name: "Demo Play Store 3D — Mi App Android".into(),
        created_at: "2026-11-03T22:30:00Z".into(),
        updated_at: "2026-11-03T22:30:00Z".into(),
        duration_sec: 12.0,
        tracks,
        director_keyframes: vec![
            DirectorCameraKeyframe {
                id: "dir-kf-1".into(),
                time_sec: 0.0,
                duration_sec: 2.2,
                focus_x: 0.5,
                focus_y: 0.5,
                zoom: 1.05,
                rot_x_deg: 8.0,
                rot_y_deg: -18.0,
                rot_z_deg: 2.0,
                easing: "spring".into(),
                label: Some("Plano Héroe 3D Inicial".into()),
                auto_generated: Some(false),
            },
            DirectorCameraKeyframe {
                id: "dir-kf-2".into(),
                time_sec: 2.5,
                duration_sec: 2.4,
                focus_x: 0.52,
                focus_y: 0.32,
                zoom: 1.48,
                rot_x_deg: -4.0,
                rot_y_deg: 12.0,
                rot_z_deg: -1.5,
                easing: "spring".into(),
                label: Some("Auto-Zoom: Toque en Tarjeta Principal".into()),
                auto_generated: Some(true),
            },
            DirectorCameraKeyframe {
                id: "dir-kf-3".into(),
                time_sec: 5.5,
                duration_sec: 2.5,
                focus_x: 0.48,
                focus_y: 0.72,
                zoom: 1.38,
                rot_x_deg: 10.0,
                rot_y_deg: -14.0,
                rot_z_deg: 2.5,
                easing: "easeInOutCubic".into(),
                label: Some("Enfoque en Barra de Acciones y Gestos".into()),
                auto_generated: Some(false),
            },
            DirectorCameraKeyframe {
                id: "dir-kf-4".into(),
                time_sec: 8.6,
                duration_sec: 3.4,
                focus_x: 0.5,
                focus_y: 0.5,
                zoom: 1.0,
                rot_x_deg: 4.0,
                rot_y_deg: 0.0,
                rot_z_deg: 0.0,
                easing: "spring".into(),
                label: Some("Plano Cierre Play Store".into()),
                auto_generated: Some(false),
            },
        ],
        privacy_masks: vec![PrivacyMask {
            id: "mask-api-key".into(),
            label: "Ocultar Token / Email Sensible".into(),
            start_sec: 2.0,
            end_sec: 5.5,
            x: 0.36,
            y: 0.25,
            width: 0.28,
            height: 0.08,
            mode: PrivacyMaskMode::Blur,
            intensity: 16,
            color: Some("#0f172a".into()),
        }],
        pointer_events: vec![
            TelemetryPointerEvent {
                id: "pt-1".into(),
                timestamp_ms: 1200,
                source: "android".into(),
                event_type: "touch_tap".into(),
                x: 0.50,
                y: 0.36,
                end_x: None,
                end_y: None,
                duration_ms: Some(220),
                button: Some("finger".into()),
            },
            TelemetryPointerEvent {
                id: "pt-2".into(),
                timestamp_ms: 2600,
                source: "android".into(),
                event_type: "touch_tap".into(),
                x: 0.52,
                y: 0.32,
                end_x: None,
                end_y: None,
                duration_ms: Some(240),
                button: Some("finger".into()),
            },
            TelemetryPointerEvent {
                id: "pt-3".into(),
                timestamp_ms: 4200,
                source: "android".into(),
                event_type: "touch_swipe".into(),
                x: 0.50,
                y: 0.72,
                end_x: Some(0.50),
                end_y: Some(0.30),
                duration_ms: Some(520),
                button: Some("finger".into()),
            },
            TelemetryPointerEvent {
                id: "pt-4".into(),
                timestamp_ms: 6100,
                source: "android".into(),
                event_type: "touch_tap".into(),
                x: 0.48,
                y: 0.70,
                end_x: None,
                end_y: None,
                duration_ms: Some(220),
                button: Some("finger".into()),
            },
            TelemetryPointerEvent {
                id: "pt-5".into(),
                timestamp_ms: 8200,
                source: "android".into(),
                event_type: "touch_swipe".into(),
                x: 0.35,
                y: 0.50,
                end_x: Some(0.65),
                end_y: Some(0.50),
                duration_ms: Some(450),
                button: Some("finger".into()),
            },
        ],
        key_events: vec![
            TelemetryKeyEvent {
                id: "key-1".into(),
                timestamp_ms: 1400,
                key_combo: "ADB · Tap (540, 720)".into(),
                duration_ms: 1200,
            },
            TelemetryKeyEvent {
                id: "key-2".into(),
                timestamp_ms: 4200,
                key_combo: "Swipe Up · 120Hz Scroll".into(),
                duration_ms: 1400,
            },
            TelemetryKeyEvent {
                id: "key-3".into(),
                timestamp_ms: 6800,
                key_combo: "Ctrl + Shift + R · Release Build".into(),
                duration_ms: 1500,
            },
        ],
        subtitles: vec![
            SubtitleSegment {
                id: "sub-1".into(),
                start_sec: 0.4,
                end_sec: 3.9,
                speaker: Some("Locutor".into()),
                words: vec![
                    SubtitleWord { id: "w-1".into(), word: "Presenta".into(), start_sec: 0.4, end_sec: 0.9, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-2".into(), word: "tu".into(), start_sec: 0.9, end_sec: 1.15, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-3".into(), word: "app".into(), start_sec: 1.15, end_sec: 1.5, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "w-4".into(), word: "Android".into(), start_sec: 1.5, end_sec: 2.05, confidence: 0.98, highlight: Some(true) },
                    SubtitleWord { id: "w-5".into(), word: "con".into(), start_sec: 2.05, end_sec: 2.3, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-6".into(), word: "acabado".into(), start_sec: 2.3, end_sec: 2.9, confidence: 0.97, highlight: Some(false) },
                    SubtitleWord { id: "w-7".into(), word: "3D".into(), start_sec: 2.9, end_sec: 3.35, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "w-8".into(), word: "en".into(), start_sec: 3.35, end_sec: 3.55, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-9".into(), word: "Rust.".into(), start_sec: 3.55, end_sec: 3.9, confidence: 0.99, highlight: Some(true) },
                ],
            },
            SubtitleSegment {
                id: "sub-2".into(),
                start_sec: 4.3,
                end_sec: 8.2,
                speaker: Some("Locutor".into()),
                words: vec![
                    SubtitleWord { id: "w-10".into(), word: "Graba".into(), start_sec: 4.3, end_sec: 4.75, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-11".into(), word: "por".into(), start_sec: 4.75, end_sec: 5.0, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-12".into(), word: "USB,".into(), start_sec: 5.0, end_sec: 5.45, confidence: 0.98, highlight: Some(true) },
                    SubtitleWord { id: "w-13".into(), word: "aplica".into(), start_sec: 5.45, end_sec: 5.9, confidence: 0.97, highlight: Some(false) },
                    SubtitleWord { id: "w-14".into(), word: "zoom".into(), start_sec: 5.9, end_sec: 6.3, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "w-15".into(), word: "automático".into(), start_sec: 6.3, end_sec: 7.0, confidence: 0.96, highlight: Some(true) },
                    SubtitleWord { id: "w-16".into(), word: "en".into(), start_sec: 7.0, end_sec: 7.2, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-17".into(), word: "cada".into(), start_sec: 7.2, end_sec: 7.55, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-18".into(), word: "toque.".into(), start_sec: 7.55, end_sec: 8.2, confidence: 0.99, highlight: Some(false) },
                ],
            },
            SubtitleSegment {
                id: "sub-3".into(),
                start_sec: 8.5,
                end_sec: 11.7,
                speaker: Some("Locutor".into()),
                words: vec![
                    SubtitleWord { id: "w-19".into(), word: "Exporta".into(), start_sec: 8.5, end_sec: 9.1, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-20".into(), word: "en".into(), start_sec: 9.1, end_sec: 9.3, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-21".into(), word: "4K".into(), start_sec: 9.3, end_sec: 9.75, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "w-22".into(), word: "60fps".into(), start_sec: 9.75, end_sec: 10.35, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "w-23".into(), word: "multihilo".into(), start_sec: 10.35, end_sec: 10.95, confidence: 0.99, highlight: Some(true) },
                    SubtitleWord { id: "w-24".into(), word: "con".into(), start_sec: 10.95, end_sec: 11.15, confidence: 0.99, highlight: Some(false) },
                    SubtitleWord { id: "w-25".into(), word: "FFmpeg.".into(), start_sec: 11.15, end_sec: 11.7, confidence: 0.99, highlight: Some(true) },
                ],
            },
        ],
        subtitle_style: SubtitleStyle {
            enabled: true,
            position_y: 0.88,
            font_size: 28.0,
            font_family: "Sans".into(),
            text_color: "#f8fafc".into(),
            active_word_color: "#38bdf8".into(),
            background_color: "#0f172a".into(),
            word_by_word_highlight: true,
            max_words_per_line: 6,
        },
        device_3d: Device3DConfig {
            primary_model: DeviceModelType::Pixel11Pro,
            secondary_model: DeviceModelType::DesktopWindow,
            dual_layout: DualLayout::Single,
            custom_glb_path: None,
            custom_glb_screen_mesh_name: None,
            body_color: "#1e293b".into(),
            frame_metalness: 0.85,
            frame_roughness: 0.22,
            screen_corner_radius: 0.08,
            bezel_width: 0.035,
            show_shadow: true,
            shadow_intensity: 0.65,
            floating_animation: true,
            floating_amplitude: 0.25,
            orientation: Orientation::Portrait,
            ambient_light_intensity: 1.1,
            directional_light_intensity: 1.8,
        },
        cursor_style: CursorAndTouchStyle {
            smooth_cursor: true,
            smoothing_factor: 0.22,
            cursor_size: 1.25,
            cursor_style: "touch-orb".into(),
            cursor_color: "#38bdf8".into(),
            show_click_ripples: true,
            click_ripple_color: "#818cf8".into(),
            show_touch_trails: true,
            hide_idle_cursor_after_ms: 2500,
            show_keystrokes_hud: true,
            keystroke_position: "bottom-right".into(),
        },
        background: BackgroundStyle {
            bg_type: "mesh-gradient".into(),
            color_a: "#0f172a".into(),
            color_b: "#1e1b4b".into(),
            color_c: "#042f2e".into(),
            gradient_angle: 135.0,
            padding_ratio: 0.08,
            motion_blur_enabled: true,
            motion_blur_intensity: 0.45,
        },
        export_settings: ExportSettings {
            preset_id: ExportPresetId::PlayStore16x9_1080p,
            width: 1920,
            height: 1080,
            fps: 60,
            codec: VideoCodec::H264,
            crf: 19,
            transparent_background: false,
            include_audio: true,
            burn_subtitles: true,
            motion_blur_samples: 8,
            output_path: "demo_playstore_export.mp4".into(),
        },
    }
}

pub fn load_or_create_project() -> ProjectFile {
    let path = default_project_path();
    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(proj) = serde_json::from_str::<ProjectFile>(&raw) {
                return proj;
            }
        }
    }
    let proj = create_default_project();
    let _ = save_project(&proj);
    proj
}

pub fn save_project(project: &ProjectFile) -> Result<PathBuf, String> {
    let path = default_project_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path)
}
