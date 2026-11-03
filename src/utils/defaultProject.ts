import {
  ExportPresetId,
  ExportSettings,
  ProjectFile,
  TimelineTrack,
} from '../types/project';

export interface ExportPresetDefinition {
  id: ExportPresetId;
  name: string;
  badge: string;
  description: string;
  settings: Partial<ExportSettings>;
}

export const EXPORT_PRESETS: ExportPresetDefinition[] = [
  {
    id: 'play-store-16-9-4k',
    name: 'Google Play Store Promo (4K 60fps)',
    badge: 'Play Store 4K',
    description: 'Vídeo promocional horizontal 3840×2160 a 60 fps para ficha de Play Store / YouTube.',
    settings: {
      width: 3840,
      height: 2160,
      fps: 60,
      codec: 'h264',
      crf: 18,
      transparentBackground: false,
    },
  },
  {
    id: 'play-store-16-9-1080p',
    name: 'Google Play Store Promo (1080p 60fps)',
    badge: 'Play Store HD',
    description: 'Estándar rápido 1920×1080 a 60 fps en H.264 alta compatibilidad.',
    settings: {
      width: 1920,
      height: 1080,
      fps: 60,
      codec: 'h264',
      crf: 20,
      transparentBackground: false,
    },
  },
  {
    id: 'play-store-shorts-9-16',
    name: 'Play Store Vertical / Shorts (9:16)',
    badge: 'Vertical 9:16',
    description: 'Formato vertical 1080×1920 a 60 fps para demos móviles y redes.',
    settings: {
      width: 1080,
      height: 1920,
      fps: 60,
      codec: 'h264',
      crf: 19,
      transparentBackground: false,
    },
  },
  {
    id: 'play-store-tablet-16-10',
    name: 'Play Store Tablet Showcase (2560×1600)',
    badge: 'Tablet 16:10',
    description: 'Relación 16:10 optimizada para capturas y vídeo de Galaxy Tab / Pixel Tablet.',
    settings: {
      width: 2560,
      height: 1600,
      fps: 60,
      codec: 'hevc',
      crf: 20,
      transparentBackground: false,
    },
  },
  {
    id: 'app-store-iphone-6-9',
    name: 'App Store Preview 6.9" (886×1920)',
    badge: 'App Store',
    description: 'Dimensiones exactas exigidas por App Store Connect con audio estéreo.',
    settings: {
      width: 886,
      height: 1920,
      fps: 60,
      codec: 'h264',
      crf: 18,
      transparentBackground: false,
    },
  },
  {
    id: 'app-store-ipad-pro',
    name: 'App Store iPad Pro 13" (1600×1200)',
    badge: 'iPad Store',
    description: 'Formato 4:3 para previsualizaciones de tablet en tiendas de aplicaciones.',
    settings: {
      width: 1600,
      height: 1200,
      fps: 60,
      codec: 'h264',
      crf: 19,
      transparentBackground: false,
    },
  },
  {
    id: 'transparent-webm',
    name: 'Vídeo con Transparencia Alpha (VP9 / ProRes)',
    badge: 'Alpha Canal',
    description: 'Exporta solo la maqueta 3D con sombra y fondo transparente para webs o landing pages.',
    settings: {
      width: 1920,
      height: 1080,
      fps: 60,
      codec: 'vp9-alpha',
      crf: 20,
      transparentBackground: true,
    },
  },
  {
    id: 'gif-preview',
    name: 'GIF Animado Optimizado (GitHub / README)',
    badge: 'GIF Paleta',
    description: 'Generación en 2 pasos con palettegen + paletteuse para máxima calidad en READMEs.',
    settings: {
      width: 960,
      height: 540,
      fps: 30,
      codec: 'gif',
      crf: 22,
      transparentBackground: false,
    },
  },
];

export function createInitialTracks(): TimelineTrack[] {
  const videoTracks: TimelineTrack[] = [1, 2, 3, 4].map((idx) => ({
    id: `video-track-${idx}`,
    name:
      idx === 1
        ? 'V1 · Android Principal'
        : idx === 2
        ? 'V2 · Pantalla PC / Secundaria'
        : idx === 3
        ? 'V3 · Webcam / PiP'
        : 'V4 · B-Roll / Overlay',
    kind: 'video',
    index: idx,
    muted: false,
    solo: false,
    locked: false,
    volume: 1.0,
    clips:
      idx === 1
        ? [
            {
              id: 'clip-demo-android',
              trackId: 'video-track-1',
              name: 'Demo_App_Android_PlayStore.mp4 (Simulación interactiva)',
              type: 'video',
              mediaPath: 'builtin://android-demo',
              sourceRole: 'android-main',
              startSec: 0,
              durationSec: 12,
              trimInSec: 0,
              trimOutSec: 12,
              speed: 1.0,
              volume: 1.0,
              muted: false,
              fadeInSec: 0.3,
              fadeOutSec: 0.5,
              freezeFrames: [],
              opacityKeyframes: [
                { id: 'kf-op-1', timeSec: 0, value: 1, easing: 'linear' },
              ],
              scaleKeyframes: [
                { id: 'kf-sc-1', timeSec: 0, value: 1, easing: 'spring' },
              ],
            },
          ]
        : idx === 2
        ? [
            {
              id: 'clip-demo-pc',
              trackId: 'video-track-2',
              name: 'AndroidStudio_Logcat_Backend.mp4',
              type: 'video',
              mediaPath: 'builtin://pc-demo',
              sourceRole: 'pc-screen',
              startSec: 0,
              durationSec: 12,
              trimInSec: 0,
              trimOutSec: 12,
              speed: 1.0,
              volume: 0.5,
              muted: false,
              fadeInSec: 0,
              fadeOutSec: 0,
              freezeFrames: [],
              opacityKeyframes: [],
              scaleKeyframes: [],
            },
          ]
        : [],
  }));

  const audioTracks: TimelineTrack[] = [1, 2, 3, 4].map((idx) => ({
    id: `audio-track-${idx}`,
    name:
      idx === 1
        ? 'A1 · Micrófono Voz en Off'
        : idx === 2
        ? 'A2 · Audio Sistema / App'
        : idx === 3
        ? 'A3 · Música de Fondo'
        : 'A4 · Efectos SFX (Toques/Whoosh)',
    kind: 'audio',
    index: idx,
    muted: false,
    solo: false,
    locked: false,
    volume: idx === 3 ? 0.35 : 1.0,
    clips:
      idx === 1
        ? [
            {
              id: 'clip-voiceover-1',
              trackId: 'audio-track-1',
              name: 'Locución_Demo_PlayStore.wav',
              type: 'audio',
              mediaPath: 'builtin://voiceover-demo',
              startSec: 0.4,
              durationSec: 11.2,
              trimInSec: 0,
              trimOutSec: 11.2,
              speed: 1.0,
              volume: 1.1,
              muted: false,
              fadeInSec: 0.2,
              fadeOutSec: 0.4,
              freezeFrames: [],
              opacityKeyframes: [],
              scaleKeyframes: [],
            },
          ]
        : [],
  }));

  const textTracks: TimelineTrack[] = [1, 2, 3, 4, 5].map((idx) => ({
    id: `text-track-${idx}`,
    name: `T${idx} · ${
      idx === 1
        ? 'Títulos Principales'
        : idx === 2
        ? 'Badges de Función'
        : idx === 3
        ? 'Callouts / Flechas'
        : idx === 4
        ? 'Banner Inferior'
        : 'Marca de Agua / CTA'
    }`,
    kind: 'text',
    index: idx,
    muted: false,
    solo: false,
    locked: false,
    volume: 1.0,
    clips:
      idx === 1
        ? [
            {
              id: 'clip-title-intro',
              trackId: 'text-track-1',
              name: 'Título: Nueva App en Google Play',
              type: 'text',
              startSec: 0.2,
              durationSec: 3.6,
              trimInSec: 0,
              trimOutSec: 3.6,
              speed: 1,
              volume: 1,
              muted: false,
              fadeInSec: 0.3,
              fadeOutSec: 0.3,
              freezeFrames: [],
              opacityKeyframes: [],
              scaleKeyframes: [],
              text: 'Diseñada para velocidad extrema',
              subtitleText: 'Disponible ahora en Google Play Store · 120 Hz nativos',
              titlePreset: 'pop-elastic',
              fontSize: 44,
              textColor: '#ffffff',
              bgColor: 'rgba(15, 23, 42, 0.78)',
              posX: 0.5,
              posY: 0.12,
            },
            {
              id: 'clip-title-mid',
              trackId: 'text-track-1',
              name: 'Título: Sincronización Instantánea',
              type: 'text',
              startSec: 4.5,
              durationSec: 3.8,
              trimInSec: 0,
              trimOutSec: 3.8,
              speed: 1,
              volume: 1,
              muted: false,
              fadeInSec: 0.3,
              fadeOutSec: 0.3,
              freezeFrames: [],
              opacityKeyframes: [],
              scaleKeyframes: [],
              text: 'Gestos fluidos & IA en tu bolsillo',
              subtitleText: 'Captura real desde USB ADB + renderizado 3D',
              titlePreset: 'glass-badge',
              fontSize: 40,
              textColor: '#f8fafc',
              bgColor: 'rgba(79, 70, 229, 0.75)',
              posX: 0.5,
              posY: 0.12,
            },
          ]
        : idx === 2
        ? [
            {
              id: 'clip-title-cta',
              trackId: 'text-track-2',
              name: 'CTA: Descárgala en Play Store',
              type: 'text',
              startSec: 8.8,
              durationSec: 3.0,
              trimInSec: 0,
              trimOutSec: 3.0,
              speed: 1,
              volume: 1,
              muted: false,
              fadeInSec: 0.25,
              fadeOutSec: 0.35,
              freezeFrames: [],
              opacityKeyframes: [],
              scaleKeyframes: [],
              text: 'Descárgala hoy en Google Play',
              subtitleText: '★ 4.9 Valoración · Gratis',
              titlePreset: 'neon-glow',
              fontSize: 42,
              textColor: '#34d399',
              bgColor: 'rgba(6, 78, 59, 0.85)',
              posX: 0.5,
              posY: 0.14,
            },
          ]
        : [],
  }));

  return [...videoTracks, ...audioTracks, ...textTracks];
}

export function createDefaultProject(): ProjectFile {
  const now = new Date().toISOString();
  return {
    version: '1.0.0',
    id: 'proj-default-playstore',
    name: 'Demo Play Store 3D — Mi App Android',
    createdAt: now,
    updatedAt: now,
    durationSec: 12,
    tracks: createInitialTracks(),
    directorKeyframes: [
      {
        id: 'dir-kf-1',
        timeSec: 0.0,
        durationSec: 2.2,
        focusX: 0.5,
        focusY: 0.5,
        zoom: 1.05,
        rotXDeg: 8,
        rotYDeg: -18,
        rotZDeg: 2,
        easing: 'spring',
        label: 'Plano Héroe 3D Inicial',
      },
      {
        id: 'dir-kf-2',
        timeSec: 2.5,
        durationSec: 2.4,
        focusX: 0.52,
        focusY: 0.32,
        zoom: 1.65,
        rotXDeg: -4,
        rotYDeg: 12,
        rotZDeg: -1,
        easing: 'spring',
        label: 'Auto-Zoom: Toque en Tarjeta Principal',
        autoGenerated: true,
      },
      {
        id: 'dir-kf-3',
        timeSec: 5.5,
        durationSec: 2.5,
        focusX: 0.48,
        focusY: 0.72,
        zoom: 1.5,
        rotXDeg: 10,
        rotYDeg: -14,
        rotZDeg: 3,
        easing: 'easeInOutCubic',
        label: 'Enfoque en Barra de Acciones y Gestos',
      },
      {
        id: 'dir-kf-4',
        timeSec: 8.6,
        durationSec: 3.4,
        focusX: 0.5,
        focusY: 0.5,
        zoom: 0.98,
        rotXDeg: 4,
        rotYDeg: 0,
        rotZDeg: 0,
        easing: 'spring',
        label: 'Plano Cierre Play Store',
      },
    ],
    privacyMasks: [
      {
        id: 'mask-api-key',
        label: 'Ocultar Token / Email de Usuario',
        startSec: 2.2,
        endSec: 5.2,
        x: 0.18,
        y: 0.22,
        width: 0.64,
        height: 0.07,
        mode: 'blur',
        intensity: 18,
        color: '#0f172a',
      },
    ],
    pointerEvents: [
      {
        id: 'pt-1',
        timestampMs: 1200,
        source: 'android',
        type: 'touch_tap',
        x: 0.5,
        y: 0.34,
        durationMs: 180,
        button: 'finger',
      },
      {
        id: 'pt-2',
        timestampMs: 2600,
        source: 'android',
        type: 'touch_tap',
        x: 0.52,
        y: 0.32,
        durationMs: 210,
        button: 'finger',
      },
      {
        id: 'pt-3',
        timestampMs: 4200,
        source: 'android',
        type: 'touch_swipe',
        x: 0.5,
        y: 0.75,
        endX: 0.5,
        endY: 0.28,
        durationMs: 480,
        button: 'finger',
      },
      {
        id: 'pt-4',
        timestampMs: 6100,
        source: 'android',
        type: 'touch_tap',
        x: 0.48,
        y: 0.72,
        durationMs: 190,
        button: 'finger',
      },
      {
        id: 'pt-5',
        timestampMs: 8200,
        source: 'android',
        type: 'touch_swipe',
        x: 0.25,
        y: 0.5,
        endX: 0.78,
        endY: 0.5,
        durationMs: 420,
        button: 'finger',
      },
    ],
    keyEvents: [
      {
        id: 'key-1',
        timestampMs: 1500,
        keyCombo: 'ADB · Tap (540, 720)',
        durationMs: 1200,
      },
      {
        id: 'key-2',
        timestampMs: 4200,
        keyCombo: 'Swipe Up · 120Hz Scroll',
        durationMs: 1300,
      },
      {
        id: 'key-3',
        timestampMs: 6800,
        keyCombo: 'Ctrl + Shift + P · Run Release',
        durationMs: 1500,
      },
    ],
    subtitles: [
      {
        id: 'sub-1',
        startSec: 0.4,
        endSec: 3.9,
        words: [
          { id: 'w-1', word: 'Presenta', startSec: 0.4, endSec: 0.9, confidence: 0.99 },
          { id: 'w-2', word: 'tu', startSec: 0.9, endSec: 1.15, confidence: 0.99 },
          { id: 'w-3', word: 'app', startSec: 1.15, endSec: 1.5, confidence: 0.99, highlight: true },
          { id: 'w-4', word: 'Android', startSec: 1.5, endSec: 2.05, confidence: 0.98, highlight: true },
          { id: 'w-5', word: 'con', startSec: 2.05, endSec: 2.3, confidence: 0.99 },
          { id: 'w-6', word: 'acabado', startSec: 2.3, endSec: 2.9, confidence: 0.97 },
          { id: 'w-7', word: '3D', startSec: 2.9, endSec: 3.35, confidence: 0.99, highlight: true },
          { id: 'w-8', word: 'de', startSec: 3.35, endSec: 3.5, confidence: 0.99 },
          { id: 'w-9', word: 'estudio.', startSec: 3.5, endSec: 3.9, confidence: 0.98 },
        ],
      },
      {
        id: 'sub-2',
        startSec: 4.3,
        endSec: 8.2,
        words: [
          { id: 'w-10', word: 'Graba', startSec: 4.3, endSec: 4.75, confidence: 0.99 },
          { id: 'w-11', word: 'por', startSec: 4.75, endSec: 5.0, confidence: 0.99 },
          { id: 'w-12', word: 'USB,', startSec: 5.0, endSec: 5.45, confidence: 0.98, highlight: true },
          { id: 'w-13', word: 'aplica', startSec: 5.45, endSec: 5.9, confidence: 0.97 },
          { id: 'w-14', word: 'zoom', startSec: 5.9, endSec: 6.3, confidence: 0.99, highlight: true },
          { id: 'w-15', word: 'automático', startSec: 6.3, endSec: 7.0, confidence: 0.96, highlight: true },
          { id: 'w-16', word: 'en', startSec: 7.0, endSec: 7.2, confidence: 0.99 },
          { id: 'w-17', word: 'cada', startSec: 7.2, endSec: 7.55, confidence: 0.99 },
          { id: 'w-18', word: 'toque.', startSec: 7.55, endSec: 8.2, confidence: 0.99 },
        ],
      },
      {
        id: 'sub-3',
        startSec: 8.5,
        endSec: 11.7,
        words: [
          { id: 'w-19', word: 'Y', startSec: 8.5, endSec: 8.7, confidence: 0.99 },
          { id: 'w-20', word: 'exporta', startSec: 8.7, endSec: 9.2, confidence: 0.99 },
          { id: 'w-21', word: 'en', startSec: 9.2, endSec: 9.4, confidence: 0.99 },
          { id: 'w-22', word: '4K', startSec: 9.4, endSec: 9.85, confidence: 0.99, highlight: true },
          { id: 'w-23', word: 'a', startSec: 9.85, endSec: 10.0, confidence: 0.99 },
          { id: 'w-24', word: '60', startSec: 10.0, endSec: 10.35, confidence: 0.99, highlight: true },
          { id: 'w-25', word: 'fps', startSec: 10.35, endSec: 10.75, confidence: 0.99, highlight: true },
          { id: 'w-26', word: 'con', startSec: 10.75, endSec: 11.0, confidence: 0.99 },
          { id: 'w-27', word: 'FFmpeg.', startSec: 11.0, endSec: 11.7, confidence: 0.99, highlight: true },
        ],
      },
    ],
    subtitleStyle: {
      enabled: true,
      positionY: 0.87,
      fontSize: 32,
      fontFamily: 'Inter, system-ui, sans-serif',
      textColor: '#f8fafc',
      activeWordColor: '#38bdf8',
      backgroundColor: 'rgba(15, 23, 42, 0.82)',
      wordByWordHighlight: true,
      maxWordsPerLine: 6,
    },
    device3D: {
      primaryModel: 'pixel-11-pro',
      secondaryModel: 'desktop-window',
      dualLayout: 'single',
      bodyColor: '#1e293b',
      frameMetalness: 0.85,
      frameRoughness: 0.22,
      screenCornerRadius: 0.08,
      bezelWidth: 0.035,
      showShadow: true,
      shadowIntensity: 0.65,
      floatingAnimation: true,
      floatingAmplitude: 0.25,
      orientation: 'portrait',
      ambientLightIntensity: 1.1,
      directionalLightIntensity: 1.8,
    },
    cursorStyle: {
      smoothCursor: true,
      smoothingFactor: 0.22,
      cursorSize: 1.25,
      cursorStyle: 'touch-orb',
      cursorColor: '#38bdf8',
      showClickRipples: true,
      clickRippleColor: '#818cf8',
      showTouchTrails: true,
      hideIdleCursorAfterMs: 2500,
      showKeystrokesHUD: true,
      keystrokePosition: 'bottom-right',
    },
    background: {
      type: 'mesh-gradient',
      colorA: '#0f172a',
      colorB: '#1e1b4b',
      colorC: '#042f2e',
      gradientAngle: 135,
      paddingRatio: 0.08,
      motionBlurEnabled: true,
      motionBlurIntensity: 0.45,
    },
    exportSettings: {
      presetId: 'play-store-16-9-1080p',
      width: 1920,
      height: 1080,
      fps: 60,
      codec: 'h264',
      crf: 20,
      transparentBackground: false,
      includeAudio: true,
      burnSubtitles: true,
      motionBlurSamples: 8,
      outputPath: 'demo_playstore_export.mp4',
    },
  };
}
