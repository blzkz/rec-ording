#!/usr/bin/env node
/**
 * rec-ording — Standalone Model Context Protocol (MCP) Server
 * Zero-external-dependency JSON-RPC 2.0 server over stdio.
 * Compatible with Claude Desktop, Cursor, Codex, and MCP Inspector.
 */

import fs from 'node:fs';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const execFileAsync = promisify(execFile);

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT_DIR = path.resolve(__dirname, '..');
const PROJECTS_DIR = path.join(ROOT_DIR, 'projects');
const EXPORTS_DIR = path.join(ROOT_DIR, 'exports');
const ACTIVE_PROJECT_PATH = path.join(PROJECTS_DIR, 'active-project.recproj.json');

function createDefaultProjectData() {
  const now = new Date().toISOString();
  return {
    version: '1.0.0',
    id: 'proj-default-playstore',
    name: 'Demo Play Store 3D — Mi App Android',
    createdAt: now,
    updatedAt: now,
    durationSec: 12,
    tracks: [
      {
        id: 'video-track-1',
        name: 'V1 · Android Principal',
        kind: 'video',
        index: 1,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [
          {
            id: 'clip-demo-android',
            trackId: 'video-track-1',
            name: 'Demo_App_Android_PlayStore.mp4',
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
            opacityKeyframes: [{ id: 'kf-op-1', timeSec: 0, value: 1, easing: 'linear' }],
            scaleKeyframes: [{ id: 'kf-sc-1', timeSec: 0, value: 1, easing: 'spring' }],
          },
        ],
      },
      {
        id: 'video-track-2',
        name: 'V2 · Pantalla PC / Secundaria',
        kind: 'video',
        index: 2,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [
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
        ],
      },
      {
        id: 'video-track-3',
        name: 'V3 · Webcam / PiP',
        kind: 'video',
        index: 3,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'video-track-4',
        name: 'V4 · B-Roll / Overlay',
        kind: 'video',
        index: 4,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'audio-track-1',
        name: 'A1 · Micrófono Voz en Off',
        kind: 'audio',
        index: 1,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [
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
        ],
      },
      {
        id: 'audio-track-2',
        name: 'A2 · Audio Sistema / App',
        kind: 'audio',
        index: 2,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'audio-track-3',
        name: 'A3 · Música de Fondo',
        kind: 'audio',
        index: 3,
        muted: false,
        solo: false,
        locked: false,
        volume: 0.35,
        clips: [],
      },
      {
        id: 'audio-track-4',
        name: 'A4 · Efectos SFX (Toques/Whoosh)',
        kind: 'audio',
        index: 4,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'text-track-1',
        name: 'T1 · Títulos Principales',
        kind: 'text',
        index: 1,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [
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
        ],
      },
      {
        id: 'text-track-2',
        name: 'T2 · Badges de Función',
        kind: 'text',
        index: 2,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'text-track-3',
        name: 'T3 · Callouts / Flechas',
        kind: 'text',
        index: 3,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'text-track-4',
        name: 'T4 · Banner Inferior',
        kind: 'text',
        index: 4,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
      {
        id: 'text-track-5',
        name: 'T5 · Marca de Agua / CTA',
        kind: 'text',
        index: 5,
        muted: false,
        solo: false,
        locked: false,
        volume: 1.0,
        clips: [],
      },
    ],
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
    ],
    keyEvents: [],
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
          { id: 'w-5', word: 'en', startSec: 2.05, endSec: 2.3, confidence: 0.99 },
          { id: 'w-6', word: '3D.', startSec: 2.3, endSec: 3.9, confidence: 0.99, highlight: true },
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

function ensureProjectFile() {
  if (!fs.existsSync(PROJECTS_DIR)) {
    fs.mkdirSync(PROJECTS_DIR, { recursive: true });
  }
  if (!fs.existsSync(EXPORTS_DIR)) {
    fs.mkdirSync(EXPORTS_DIR, { recursive: true });
  }
  if (!fs.existsSync(ACTIVE_PROJECT_PATH)) {
    const initial = createDefaultProjectData();
    fs.writeFileSync(ACTIVE_PROJECT_PATH, JSON.stringify(initial, null, 2), 'utf-8');
    return initial;
  }
  try {
    return JSON.parse(fs.readFileSync(ACTIVE_PROJECT_PATH, 'utf-8'));
  } catch {
    const fallback = createDefaultProjectData();
    fs.writeFileSync(ACTIVE_PROJECT_PATH, JSON.stringify(fallback, null, 2), 'utf-8');
    return fallback;
  }
}

function saveProjectFile(project) {
  if (!fs.existsSync(PROJECTS_DIR)) {
    fs.mkdirSync(PROJECTS_DIR, { recursive: true });
  }
  project.updatedAt = new Date().toISOString();
  fs.writeFileSync(ACTIVE_PROJECT_PATH, JSON.stringify(project, null, 2), 'utf-8');
  return project;
}

function resolveTrackId(codeOrId) {
  const upper = String(codeOrId || 'V1').trim().toUpperCase();
  if (/^V[1-4]$/.test(upper)) {
    return `video-track-${upper[1]}`;
  }
  if (/^A[1-4]$/.test(upper)) {
    return `audio-track-${upper[1]}`;
  }
  if (/^T[1-5]$/.test(upper)) {
    return `text-track-${upper[1]}`;
  }
  return String(codeOrId);
}

const MCP_TOOLS = [
  {
    name: 'get_project_summary',
    description:
      'Returns the active rec-ording project summary including tracks, clips, 3D device settings, director camera keyframes, privacy masks, subtitles, and export configuration.',
    inputSchema: {
      type: 'object',
      properties: {},
      additionalProperties: false,
    },
  },
  {
    name: 'import_recording_clip',
    description:
      'Adds a video or audio recording clip to a specified track (V1..V4 or A1..A4).',
    inputSchema: {
      type: 'object',
      properties: {
        track: {
          type: 'string',
          description: 'Target track identifier: V1..V4 or A1..A4',
        },
        name: { type: 'string', description: 'Clip display name' },
        mediaPath: {
          type: 'string',
          description: 'Local file path or builtin:// URL',
        },
        sourceRole: {
          type: 'string',
          enum: ['android-main', 'pc-screen', 'webcam', 'overlay'],
        },
        startSec: { type: 'number', default: 0 },
        durationSec: { type: 'number', default: 10 },
      },
      required: ['track', 'name', 'mediaPath'],
    },
  },
  {
    name: 'edit_clip',
    description:
      'Trims (trimInSec, trimOutSec), changes speed (0.25..4.0), sets volume, or adds a freezeFrame on any timeline clip.',
    inputSchema: {
      type: 'object',
      properties: {
        clipId: { type: 'string', description: 'ID of the clip to edit' },
        trimInSec: { type: 'number' },
        trimOutSec: { type: 'number' },
        startSec: { type: 'number' },
        speed: { type: 'number', minimum: 0.25, maximum: 4.0 },
        volume: { type: 'number', minimum: 0.0, maximum: 2.0 },
        freezeFrame: {
          type: 'object',
          properties: {
            sourceTimeSec: { type: 'number' },
            timelineStartSec: { type: 'number' },
            durationSec: { type: 'number' },
          },
          required: ['sourceTimeSec', 'timelineStartSec', 'durationSec'],
        },
      },
      required: ['clipId'],
    },
  },
  {
    name: 'set_3d_mockup',
    description:
      'Configures the 3D device mockup model, dual layout, orientation, and motion blur settings.',
    inputSchema: {
      type: 'object',
      properties: {
        primaryModel: {
          type: 'string',
          enum: [
            'pixel-11-pro',
            'galaxy-s26-ultra',
            'galaxy-tab-s10-ultra',
            'desktop-window',
            'custom-glb',
            'none',
          ],
        },
        secondaryModel: {
          type: 'string',
          enum: [
            'pixel-11-pro',
            'galaxy-s26-ultra',
            'galaxy-tab-s10-ultra',
            'desktop-window',
            'custom-glb',
            'none',
          ],
        },
        dualLayout: {
          type: 'string',
          enum: ['single', 'side-by-side', 'pip-floating', 'angled-showcase'],
        },
        orientation: {
          type: 'string',
          enum: ['portrait', 'landscape'],
        },
        motionBlurEnabled: { type: 'boolean' },
        customGlbPath: { type: 'string' },
      },
    },
  },
  {
    name: 'plan_director_camera',
    description:
      'Adds or replaces 3D director camera keyframes, or auto-generates camera zoom keyframes from touch/pointer telemetry events.',
    inputSchema: {
      type: 'object',
      properties: {
        autoGenerateFromTouches: {
          type: 'boolean',
          description: 'Automatically generate spring camera zooms on pointer/touch events',
        },
        replaceExisting: { type: 'boolean', default: false },
        keyframes: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              timeSec: { type: 'number' },
              durationSec: { type: 'number' },
              focusX: { type: 'number' },
              focusY: { type: 'number' },
              zoom: { type: 'number' },
              rotXDeg: { type: 'number' },
              rotYDeg: { type: 'number' },
              rotZDeg: { type: 'number' },
              easing: {
                type: 'string',
                enum: ['spring', 'easeInOutCubic', 'linear'],
              },
              label: { type: 'string' },
            },
            required: ['timeSec', 'focusX', 'focusY', 'zoom'],
          },
        },
      },
    },
  },
  {
    name: 'add_privacy_mask',
    description:
      'Adds a blur, pixelate, or solid privacy mask over sensitive coordinates (x, y, width, height in 0..1) for a specified time range.',
    inputSchema: {
      type: 'object',
      properties: {
        label: { type: 'string' },
        startSec: { type: 'number' },
        endSec: { type: 'number' },
        x: { type: 'number', minimum: 0, maximum: 1 },
        y: { type: 'number', minimum: 0, maximum: 1 },
        width: { type: 'number', minimum: 0.01, maximum: 1 },
        height: { type: 'number', minimum: 0.01, maximum: 1 },
        mode: { type: 'string', enum: ['blur', 'pixelate', 'solid'] },
        intensity: { type: 'number', default: 20 },
      },
      required: ['label', 'startSec', 'endSec', 'x', 'y', 'width', 'height'],
    },
  },
  {
    name: 'add_animated_title',
    description:
      'Adds an animated text title clip to tracks T1..T5 with a visual preset (pop-elastic, glass-badge, neon-glow, typewriter, slide-up-blur, minimal-fade).',
    inputSchema: {
      type: 'object',
      properties: {
        track: { type: 'string', default: 'T1', description: 'T1..T5' },
        text: { type: 'string' },
        subtitleText: { type: 'string' },
        preset: {
          type: 'string',
          enum: [
            'pop-elastic',
            'glass-badge',
            'neon-glow',
            'typewriter',
            'slide-up-blur',
            'minimal-fade',
          ],
          default: 'pop-elastic',
        },
        startSec: { type: 'number', default: 0 },
        durationSec: { type: 'number', default: 3.5 },
        posX: { type: 'number', default: 0.5 },
        posY: { type: 'number', default: 0.14 },
      },
      required: ['text'],
    },
  },
  {
    name: 'update_subtitles',
    description: 'Updates word-by-word subtitle segments on the project.',
    inputSchema: {
      type: 'object',
      properties: {
        segments: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              startSec: { type: 'number' },
              endSec: { type: 'number' },
              speaker: { type: 'string' },
              words: {
                type: 'array',
                items: {
                  type: 'object',
                  properties: {
                    word: { type: 'string' },
                    startSec: { type: 'number' },
                    endSec: { type: 'number' },
                    highlight: { type: 'boolean' },
                  },
                  required: ['word', 'startSec', 'endSec'],
                },
              },
            },
            required: ['startSec', 'endSec', 'words'],
          },
        },
      },
      required: ['segments'],
    },
  },
  {
    name: 'render_export_ffmpeg',
    description:
      'Builds and executes a real /usr/bin/ffmpeg command to render the project to MP4, WebM, GIF, or PNG.',
    inputSchema: {
      type: 'object',
      properties: {
        outputFilename: { type: 'string' },
        width: { type: 'number' },
        height: { type: 'number' },
        fps: { type: 'number', enum: [30, 60] },
        codec: {
          type: 'string',
          enum: ['h264', 'hevc', 'av1', 'vp9-alpha', 'gif', 'png-sequence'],
        },
        durationSec: {
          type: 'number',
          description: 'Optional render duration override in seconds',
        },
      },
    },
  },
];

async function handleToolCall(name, args = {}) {
  const project = ensureProjectFile();

  switch (name) {
    case 'get_project_summary': {
      return {
        id: project.id,
        name: project.name,
        updatedAt: project.updatedAt,
        durationSec: project.durationSec,
        device3D: project.device3D,
        exportSettings: project.exportSettings,
        tracks: project.tracks.map((t) => ({
          id: t.id,
          name: t.name,
          kind: t.kind,
          muted: t.muted,
          clipCount: t.clips.length,
          clips: t.clips.map((c) => ({
            id: c.id,
            name: c.name,
            type: c.type,
            mediaPath: c.mediaPath,
            startSec: c.startSec,
            durationSec: c.durationSec,
            speed: c.speed,
            text: c.text,
          })),
        })),
        directorKeyframes: project.directorKeyframes,
        privacyMasks: project.privacyMasks,
        subtitleSegmentsCount: project.subtitles.length,
      };
    }

    case 'import_recording_clip': {
      const trackId = resolveTrackId(args.track);
      const track = project.tracks.find((t) => t.id === trackId);
      if (!track) {
        throw new Error(`Track not found: ${args.track}`);
      }
      const durationSec = Number(args.durationSec ?? 10);
      const startSec = Number(args.startSec ?? 0);
      const newClip = {
        id: `clip-mcp-${Date.now()}`,
        trackId: track.id,
        name: String(args.name),
        type: track.kind === 'audio' ? 'audio' : 'video',
        mediaPath: String(args.mediaPath),
        sourceRole:
          args.sourceRole ||
          (track.index === 1
            ? 'android-main'
            : track.index === 2
            ? 'pc-screen'
            : 'overlay'),
        startSec,
        durationSec,
        trimInSec: 0,
        trimOutSec: durationSec,
        speed: 1.0,
        volume: 1.0,
        muted: false,
        fadeInSec: 0.2,
        fadeOutSec: 0.2,
        freezeFrames: [],
        opacityKeyframes: [],
        scaleKeyframes: [],
      };
      track.clips.push(newClip);
      project.durationSec = Math.max(project.durationSec, startSec + durationSec);
      saveProjectFile(project);
      return { addedClip: newClip, projectDurationSec: project.durationSec };
    }

    case 'edit_clip': {
      let foundClip = null;
      for (const track of project.tracks) {
        const clip = track.clips.find((c) => c.id === args.clipId);
        if (clip) {
          foundClip = clip;
          break;
        }
      }
      if (!foundClip) {
        throw new Error(`Clip not found: ${args.clipId}`);
      }
      if (typeof args.trimInSec === 'number') foundClip.trimInSec = args.trimInSec;
      if (typeof args.trimOutSec === 'number') foundClip.trimOutSec = args.trimOutSec;
      if (typeof args.startSec === 'number') foundClip.startSec = args.startSec;
      if (typeof args.speed === 'number') {
        foundClip.speed = Math.min(4.0, Math.max(0.25, args.speed));
      }
      if (typeof args.volume === 'number') {
        foundClip.volume = Math.min(2.0, Math.max(0.0, args.volume));
      }
      const activeTrimSpan = Math.max(0.2, foundClip.trimOutSec - foundClip.trimInSec);
      foundClip.durationSec = Number((activeTrimSpan / (foundClip.speed || 1)).toFixed(2));

      if (args.freezeFrame) {
        foundClip.freezeFrames = foundClip.freezeFrames || [];
        foundClip.freezeFrames.push({
          id: `ff-${Date.now()}`,
          sourceTimeSec: Number(args.freezeFrame.sourceTimeSec),
          timelineStartSec: Number(args.freezeFrame.timelineStartSec),
          durationSec: Number(args.freezeFrame.durationSec),
        });
        foundClip.durationSec += Number(args.freezeFrame.durationSec);
      }

      saveProjectFile(project);
      return { updatedClip: foundClip };
    }

    case 'set_3d_mockup': {
      if (args.primaryModel) project.device3D.primaryModel = args.primaryModel;
      if (args.secondaryModel) project.device3D.secondaryModel = args.secondaryModel;
      if (args.dualLayout) project.device3D.dualLayout = args.dualLayout;
      if (args.orientation) project.device3D.orientation = args.orientation;
      if (args.customGlbPath) project.device3D.customGlbPath = args.customGlbPath;
      if (typeof args.motionBlurEnabled === 'boolean') {
        project.background.motionBlurEnabled = args.motionBlurEnabled;
      }
      saveProjectFile(project);
      return { device3D: project.device3D, background: project.background };
    }

    case 'plan_director_camera': {
      let generated = [];
      if (args.autoGenerateFromTouches) {
        generated = (project.pointerEvents || []).map((pt, idx) => ({
          id: `dir-auto-${Date.now()}-${idx}`,
          timeSec: Number((pt.timestampMs / 1000).toFixed(2)),
          durationSec: 1.8,
          focusX: pt.x,
          focusY: pt.y,
          zoom: pt.type === 'touch_swipe' ? 1.35 : 1.7,
          rotXDeg: (0.5 - pt.y) * 16,
          rotYDeg: (pt.x - 0.5) * 24,
          rotZDeg: 0,
          easing: 'spring',
          label: `Auto-Zoom (${pt.type})`,
          autoGenerated: true,
        }));
      }
      const custom = (args.keyframes || []).map((kf, idx) => ({
        id: `dir-mcp-${Date.now()}-${idx}`,
        timeSec: Number(kf.timeSec),
        durationSec: Number(kf.durationSec ?? 2.0),
        focusX: Number(kf.focusX),
        focusY: Number(kf.focusY),
        zoom: Number(kf.zoom),
        rotXDeg: Number(kf.rotXDeg ?? 0),
        rotYDeg: Number(kf.rotYDeg ?? 0),
        rotZDeg: Number(kf.rotZDeg ?? 0),
        easing: kf.easing || 'spring',
        label: kf.label || 'Keyframe MCP',
      }));

      if (args.replaceExisting) {
        project.directorKeyframes = [...generated, ...custom];
      } else {
        project.directorKeyframes.push(...generated, ...custom);
      }
      project.directorKeyframes.sort((a, b) => a.timeSec - b.timeSec);
      saveProjectFile(project);
      return { directorKeyframes: project.directorKeyframes };
    }

    case 'add_privacy_mask': {
      const mask = {
        id: `mask-mcp-${Date.now()}`,
        label: String(args.label),
        startSec: Number(args.startSec),
        endSec: Number(args.endSec),
        x: Number(args.x),
        y: Number(args.y),
        width: Number(args.width),
        height: Number(args.height),
        mode: args.mode || 'blur',
        intensity: Number(args.intensity ?? 20),
        color: '#0f172a',
      };
      project.privacyMasks.push(mask);
      saveProjectFile(project);
      return { addedMask: mask, totalMasks: project.privacyMasks.length };
    }

    case 'add_animated_title': {
      const trackId = resolveTrackId(args.track || 'T1');
      const track =
        project.tracks.find((t) => t.id === trackId) ||
        project.tracks.find((t) => t.kind === 'text');
      if (!track) {
        throw new Error(`Text track not found: ${args.track}`);
      }
      const startSec = Number(args.startSec ?? 0);
      const durationSec = Number(args.durationSec ?? 3.5);
      const titleClip = {
        id: `clip-title-mcp-${Date.now()}`,
        trackId: track.id,
        name: `Título: ${args.text}`,
        type: 'text',
        startSec,
        durationSec,
        trimInSec: 0,
        trimOutSec: durationSec,
        speed: 1,
        volume: 1,
        muted: false,
        fadeInSec: 0.25,
        fadeOutSec: 0.25,
        freezeFrames: [],
        opacityKeyframes: [],
        scaleKeyframes: [],
        text: String(args.text),
        subtitleText: args.subtitleText ? String(args.subtitleText) : '',
        titlePreset: args.preset || 'pop-elastic',
        fontSize: 42,
        textColor: '#ffffff',
        bgColor: 'rgba(15, 23, 42, 0.82)',
        posX: Number(args.posX ?? 0.5),
        posY: Number(args.posY ?? 0.14),
      };
      track.clips.push(titleClip);
      saveProjectFile(project);
      return { addedTitleClip: titleClip };
    }

    case 'update_subtitles': {
      project.subtitles = (args.segments || []).map((seg, sIdx) => ({
        id: seg.id || `sub-mcp-${Date.now()}-${sIdx}`,
        startSec: Number(seg.startSec),
        endSec: Number(seg.endSec),
        speaker: seg.speaker || 'Locutor',
        words: (seg.words || []).map((w, wIdx) => ({
          id: `w-mcp-${sIdx}-${wIdx}`,
          word: String(w.word),
          startSec: Number(w.startSec),
          endSec: Number(w.endSec),
          confidence: 0.99,
          highlight: Boolean(w.highlight),
        })),
      }));
      saveProjectFile(project);
      return { updatedSubtitlesCount: project.subtitles.length };
    }

    case 'render_export_ffmpeg': {
      if (args.width) project.exportSettings.width = Number(args.width);
      if (args.height) project.exportSettings.height = Number(args.height);
      if (args.fps) project.exportSettings.fps = Number(args.fps);
      if (args.codec) project.exportSettings.codec = args.codec;
      if (args.outputFilename) {
        project.exportSettings.outputPath = String(args.outputFilename);
      }
      saveProjectFile(project);

      const width = Math.max(16, Math.round(project.exportSettings.width / 2) * 2);
      const height = Math.max(16, Math.round(project.exportSettings.height / 2) * 2);
      const fps = project.exportSettings.fps || 30;
      const duration = Math.min(
        30,
        Math.max(1, Number(args.durationSec ?? Math.min(3, project.durationSec || 3)))
      );

      let outName = project.exportSettings.outputPath || 'demo_playstore_export.mp4';
      const codec = project.exportSettings.codec || 'h264';
      if (codec === 'vp9-alpha' && !outName.endsWith('.webm')) {
        outName = outName.replace(/\.[a-z0-9]+$/i, '') + '.webm';
      } else if (codec === 'gif' && !outName.endsWith('.gif')) {
        outName = outName.replace(/\.[a-z0-9]+$/i, '') + '.gif';
      } else if (codec === 'png-sequence') {
        outName = 'frame_preview.png';
      }

      const outputPath = path.isAbsolute(outName)
        ? outName
        : path.join(EXPORTS_DIR, outName);

      const primaryVideoClip = project.tracks
        .filter((t) => t.kind === 'video' && !t.muted)
        .flatMap((t) => t.clips)
        .find((c) => !c.muted);

      const hasRealVideoFile =
        primaryVideoClip?.mediaPath &&
        !primaryVideoClip.mediaPath.startsWith('builtin://') &&
        fs.existsSync(primaryVideoClip.mediaPath);

      const ffmpegArgs = ['-y'];
      if (hasRealVideoFile) {
        ffmpegArgs.push('-i', primaryVideoClip.mediaPath);
      } else {
        const bgHex = (project.background?.colorA || '#0f172a').replace('#', '0x');
        ffmpegArgs.push(
          '-f',
          'lavfi',
          '-i',
          `color=c=${bgHex}:s=${width}x${height}:r=${fps}:d=${duration}`,
          '-f',
          'lavfi',
          '-i',
          `testsrc2=s=${Math.max(64, Math.round((width * 0.42) / 2) * 2)}x${Math.max(64, Math.round((height * 0.78) / 2) * 2)}:r=${fps}:d=${duration}`
        );
      }

      const maskFilters = (project.privacyMasks || []).map((m) => {
        const mx = Math.max(0, Math.round(m.x * width));
        const my = Math.max(0, Math.round(m.y * height));
        const mw = Math.max(8, Math.round(m.width * width));
        const mh = Math.max(8, Math.round(m.height * height));
        return `drawbox=x=${mx}:y=${my}:w=${mw}:h=${mh}:color=0x1e293b@0.92:t=fill:enable='between(t,${m.startSec},${m.endSec})'`;
      });

      const postFilters = [
        `scale=${width}:${height}:flags=lanczos`,
        ...maskFilters,
      ].join(',');

      if (!hasRealVideoFile) {
        ffmpegArgs.push(
          '-filter_complex',
          `[0:v][1:v]overlay=(W-w)/2:(H-h)/2,${postFilters}[vout]`,
          '-map',
          '[vout]'
        );
      } else {
        ffmpegArgs.push('-vf', postFilters);
      }

      if (codec === 'gif') {
        ffmpegArgs.push('-t', String(duration), outputPath);
      } else if (codec === 'png-sequence') {
        ffmpegArgs.push('-frames:v', '1', outputPath);
      } else if (codec === 'vp9-alpha') {
        ffmpegArgs.push(
          '-c:v',
          'libvpx-vp9',
          '-b:v',
          '0',
          '-crf',
          String(project.exportSettings.crf || 24),
          '-pix_fmt',
          'yuva420p',
          '-t',
          String(duration),
          outputPath
        );
      } else if (codec === 'hevc') {
        ffmpegArgs.push(
          '-c:v',
          'libx265',
          '-preset',
          'ultrafast',
          '-crf',
          String(project.exportSettings.crf || 24),
          '-pix_fmt',
          'yuv420p',
          '-t',
          String(duration),
          outputPath
        );
      } else {
        ffmpegArgs.push(
          '-c:v',
          'libx264',
          '-preset',
          'ultrafast',
          '-crf',
          String(project.exportSettings.crf || 22),
          '-pix_fmt',
          'yuv420p',
          '-movflags',
          '+faststart',
          '-t',
          String(duration),
          outputPath
        );
      }

      const { stderr } = await execFileAsync('/usr/bin/ffmpeg', ffmpegArgs);
      const stat = fs.statSync(outputPath);

      return {
        success: true,
        outputPath,
        bytesWritten: stat.size,
        commandUsed: `/usr/bin/ffmpeg ${ffmpegArgs.join(' ')}`,
        ffmpegLogTail: stderr.split('\n').slice(-6).join('\n'),
      };
    }

    default:
      throw new Error(`Unknown tool: ${name}`);
  }
}

function sendJsonRpc(message) {
  process.stdout.write(JSON.stringify(message) + '\n');
}

async function handleJsonRpcMessage(msg) {
  if (!msg || typeof msg !== 'object') return;
  const { id, method, params } = msg;

  try {
    if (method === 'initialize') {
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        result: {
          protocolVersion: params?.protocolVersion || '2024-11-05',
          capabilities: {
            tools: { listChanged: false },
            resources: { subscribe: false, listChanged: false },
          },
          serverInfo: {
            name: 'rec-ording-mcp',
            version: '1.0.0',
          },
        },
      });
      return;
    }

    if (method === 'notifications/initialized') {
      return;
    }

    if (method === 'ping') {
      sendJsonRpc({ jsonrpc: '2.0', id, result: {} });
      return;
    }

    if (method === 'tools/list') {
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        result: {
          tools: MCP_TOOLS,
        },
      });
      return;
    }

    if (method === 'tools/call') {
      const toolName = params?.name;
      const toolArgs = params?.arguments || {};
      const output = await handleToolCall(toolName, toolArgs);
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        result: {
          content: [
            {
              type: 'text',
              text: JSON.stringify(output, null, 2),
            },
          ],
          isError: false,
        },
      });
      return;
    }

    if (method === 'resources/list') {
      ensureProjectFile();
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        result: {
          resources: [
            {
              uri: 'project://active',
              name: 'Active rec-ording Project (.recproj.json)',
              mimeType: 'application/json',
              description:
                'Full multi-track timeline, 3D device config, camera keyframes, privacy masks, and subtitles.',
            },
          ],
        },
      });
      return;
    }

    if (method === 'resources/read') {
      const project = ensureProjectFile();
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        result: {
          contents: [
            {
              uri: params?.uri || 'project://active',
              mimeType: 'application/json',
              text: JSON.stringify(project, null, 2),
            },
          ],
        },
      });
      return;
    }

    if (id !== undefined) {
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        error: {
          code: -32601,
          message: `Method not found: ${method}`,
        },
      });
    }
  } catch (err) {
    if (id !== undefined) {
      sendJsonRpc({
        jsonrpc: '2.0',
        id,
        result: {
          content: [
            {
              type: 'text',
              text: `Error: ${err instanceof Error ? err.message : String(err)}`,
            },
          ],
          isError: true,
        },
      });
    }
  }
}

ensureProjectFile();

let inputBuffer = '';
process.stdin.setEncoding('utf-8');
process.stdin.on('data', (chunk) => {
  inputBuffer += chunk;
  while (true) {
    const newlineIdx = inputBuffer.indexOf('\n');
    if (newlineIdx === -1) break;
    const rawLine = inputBuffer.slice(0, newlineIdx).trim();
    inputBuffer = inputBuffer.slice(newlineIdx + 1);
    if (!rawLine || rawLine.startsWith('Content-Length:')) continue;
    try {
      const parsed = JSON.parse(rawLine);
      handleJsonRpcMessage(parsed);
    } catch {
      // Ignore partial or non-JSON header lines
    }
  }
});
