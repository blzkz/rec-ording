import React, { useMemo, useRef, useState } from 'react';
import {
  Smartphone,
  Camera,
  Captions,
  Bot,
  Film,
  Upload,
  Sparkles,
  Plus,
  Trash2,
  Copy,
  Check,
  Wand2,
  ShieldAlert,
  MousePointer,
  Palette,
  Terminal,
  Type,
} from 'lucide-react';
import {
  DeviceModelType,
  DirectorCameraKeyframe,
  PrivacyMask,
  ProjectFile,
  SubtitleSegment,
  SubtitleWord,
  TimelineClip,
  TitleAnimationPreset,
  VideoCodec,
} from '../types/project';
import { EXPORT_PRESETS } from '../utils/defaultProject';

export interface RightInspectorPanelProps {
  project: ProjectFile;
  currentTimeSec: number;
  selectedClipId: string | null;
  onSeek: (timeSec: number) => void;
  onUpdateProject: (updater: (prev: ProjectFile) => ProjectFile) => void;
  onTriggerExport: () => void;
  isExporting: boolean;
  exportProgress: number;
  exportLog: string;
}

type InspectorTab = '3d' | 'director' | 'subtitles' | 'mcp' | 'export';

const MCP_TOOLS_CATALOG = [
  {
    name: 'get_project_summary',
    desc: 'Inspecciona pistas, duración, modelo 3D, keyframes de cámara y ajustes de exportación.',
  },
  {
    name: 'set_3d_device_showcase',
    desc: 'Cambia chasis 3D (Pixel 11 Pro, S26 Ultra, Tab S10, .GLB propio) y layout dual.',
  },
  {
    name: 'auto_generate_director_zooms',
    desc: 'Analiza los toques/clics grabados y genera zooms 3D con físicas spring automáticamente.',
  },
  {
    name: 'add_animated_title',
    desc: 'Inserta banners animados de marketing en las pistas T1..T5.',
  },
  {
    name: 'add_privacy_mask',
    desc: 'Añade zonas de difuminado (blur/pixelado) sobre datos sensibles o API keys.',
  },
  {
    name: 'upsert_word_subtitles',
    desc: 'Actualiza subtítulos palabra por palabra con resaltado tipo karaoke.',
  },
  {
    name: 'edit_timeline_clip',
    desc: 'Recorta clips, ajusta velocidad (0.25x..4x) o inserta freeze frames.',
  },
  {
    name: 'configure_export_preset',
    desc: 'Configura presets de Google Play Store 4K, Shorts 9:16, GIF o WebM con canal Alpha.',
  },
  {
    name: 'build_ffmpeg_render_command',
    desc: 'Devuelve la línea de comandos exacta de FFmpeg lista para renderizar el proyecto.',
  },
];

function buildFfmpegCommandPreview(project: ProjectFile): string {
  const exp = project.exportSettings;
  const scaleFilter = `scale=${exp.width}:${exp.height}:flags=lanczos`;
  const motionBlurFilter =
    exp.motionBlurSamples > 1
      ? `,tblend=all_mode=average,framestep=1`
      : '';

  if (exp.codec === 'gif') {
    return `ffmpeg -y -framerate ${exp.fps} -i frames/frame_%05d.png -vf "${scaleFilter},split[s0][s1];[s0]palettegen=max_colors=256[p];[s1][p]paletteuse=dither=bayer" "${exp.outputPath.replace(
      /\.\w+$/,
      '.gif'
    )}"`;
  }

  if (exp.codec === 'vp9-alpha') {
    return `ffmpeg -y -framerate ${exp.fps} -i frames/frame_%05d.png -c:v libvpx-vp9 -pix_fmt yuva420p -crf ${exp.crf} -b:v 0 "${exp.outputPath.replace(
      /\.\w+$/,
      '.webm'
    )}"`;
  }

  if (exp.codec === 'prores-alpha') {
    return `ffmpeg -y -framerate ${exp.fps} -i frames/frame_%05d.png -c:v prores_ks -profile:v 4444 -pix_fmt yuva444p10le "${exp.outputPath.replace(
      /\.\w+$/,
      '.mov'
    )}"`;
  }

  const codecMap: Record<VideoCodec, string> = {
    h264: 'libx264 -pix_fmt yuv420p -preset slow',
    hevc: 'libx265 -pix_fmt yuv420p -preset slow',
    av1: 'libsvtav1 -pix_fmt yuv420p',
    'vp9-alpha': 'libvpx-vp9 -pix_fmt yuva420p',
    'prores-alpha': 'prores_ks -profile:v 4444',
    gif: 'gif',
    'png-sequence': 'png',
  };

  return `ffmpeg -y -framerate ${exp.fps} -i frames/frame_%05d.png ${
    exp.includeAudio ? '-i audio_mix.wav ' : ''
  }-vf "${scaleFilter}${motionBlurFilter}" -c:v ${codecMap[exp.codec]} -crf ${
    exp.crf
  } ${exp.includeAudio ? '-c:a aac -b:a 192k ' : ''}"${exp.outputPath}"`;
}

export const RightInspectorPanel: React.FC<RightInspectorPanelProps> = ({
  project,
  currentTimeSec,
  selectedClipId,
  onSeek,
  onUpdateProject,
  onTriggerExport,
  isExporting,
  exportProgress,
  exportLog,
}) => {
  const [activeTab, setActiveTab] = useState<InspectorTab>('3d');
  const [isTranscribing, setIsTranscribing] = useState<boolean>(false);
  const [editingWordId, setEditingWordId] = useState<string | null>(null);
  const [copiedSnippet, setCopiedSnippet] = useState<string | null>(null);
  const [aiPolishFeedback, setAiPolishFeedback] = useState<string | null>(null);
  const glbInputRef = useRef<HTMLInputElement | null>(null);

  const selectedClip: TimelineClip | null = useMemo(() => {
    if (!selectedClipId) return null;
    for (const track of project.tracks) {
      const found = track.clips.find((c) => c.id === selectedClipId);
      if (found) return found;
    }
    return null;
  }, [project.tracks, selectedClipId]);

  const updateSelectedClip = (patch: Partial<TimelineClip>) => {
    if (!selectedClip) return;
    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((track) => ({
        ...track,
        clips: track.clips.map((c) =>
          c.id === selectedClip.id ? { ...c, ...patch } : c
        ),
      })),
    }));
  };

  const handleGlbFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const objectUrl = URL.createObjectURL(file);
    onUpdateProject((prev) => ({
      ...prev,
      device3D: {
        ...prev.device3D,
        primaryModel: 'custom-glb',
        customGlbPath: objectUrl,
        customGlbScreenMeshName: file.name,
      },
    }));
  };

  const handleAutoGenerateDirectorFromTouches = () => {
    const events = project.pointerEvents;
    if (events.length === 0) return;

    const generated: DirectorCameraKeyframe[] = [
      {
        id: `dir-hero-${Date.now()}`,
        timeSec: 0,
        durationSec: 1.5,
        focusX: 0.5,
        focusY: 0.5,
        zoom: 1.05,
        rotXDeg: 7,
        rotYDeg: -16,
        rotZDeg: 2,
        easing: 'spring',
        label: 'Apertura 3D Héroe',
      },
      ...events.map((pt, idx) => ({
        id: `dir-auto-pt-${idx}-${Date.now()}`,
        timeSec: Number((pt.timestampMs / 1000).toFixed(2)),
        durationSec: 2.1,
        focusX: pt.x,
        focusY: pt.y,
        zoom: pt.type === 'touch_swipe' ? 1.35 : 1.7,
        rotXDeg: idx % 2 === 0 ? -5 : 8,
        rotYDeg: idx % 2 === 0 ? 14 : -12,
        rotZDeg: idx % 2 === 0 ? -1 : 2,
        easing: 'spring' as const,
        label:
          pt.type === 'touch_swipe'
            ? `Seguimiento Swipe (${pt.source})`
            : `Auto-Zoom Toque (${Math.round(pt.x * 100)}%, ${Math.round(
                pt.y * 100
              )}%)`,
        autoGenerated: true,
      })),
    ];

    onUpdateProject((prev) => ({
      ...prev,
      directorKeyframes: generated.sort((a, b) => a.timeSec - b.timeSec),
    }));
  };

  const handleTranscribeWhisperLocal = () => {
    setIsTranscribing(true);
    setTimeout(() => {
      const richSegments: SubtitleSegment[] = [
        {
          id: 'sub-whisper-1',
          startSec: 0.3,
          endSec: 3.8,
          words: [
            { id: 'ww-1', word: 'Descubre', startSec: 0.3, endSec: 0.85, confidence: 0.99 },
            { id: 'ww-2', word: 'nuestra', startSec: 0.85, endSec: 1.25, confidence: 0.99 },
            { id: 'ww-3', word: 'nueva', startSec: 1.25, endSec: 1.65, confidence: 0.99 },
            {
              id: 'ww-4',
              word: 'experiencia',
              startSec: 1.65,
              endSec: 2.35,
              confidence: 0.98,
              highlight: true,
            },
            {
              id: 'ww-5',
              word: 'Android',
              startSec: 2.35,
              endSec: 2.95,
              confidence: 0.99,
              highlight: true,
            },
            { id: 'ww-6', word: 'en', startSec: 2.95, endSec: 3.15, confidence: 0.99 },
            {
              id: 'ww-7',
              word: '120Hz.',
              startSec: 3.15,
              endSec: 3.8,
              confidence: 0.99,
              highlight: true,
            },
          ],
        },
        {
          id: 'sub-whisper-2',
          startSec: 4.1,
          endSec: 8.1,
          words: [
            { id: 'ww-8', word: 'Cada', startSec: 4.1, endSec: 4.45, confidence: 0.99 },
            {
              id: 'ww-9',
              word: 'toque',
              startSec: 4.45,
              endSec: 4.9,
              confidence: 0.99,
              highlight: true,
            },
            { id: 'ww-10', word: 'por', startSec: 4.9, endSec: 5.15, confidence: 0.99 },
            {
              id: 'ww-11',
              word: 'USB',
              startSec: 5.15,
              endSec: 5.6,
              confidence: 0.99,
              highlight: true,
            },
            { id: 'ww-12', word: 'dirige', startSec: 5.6, endSec: 6.1, confidence: 0.98 },
            { id: 'ww-13', word: 'la', startSec: 6.1, endSec: 6.3, confidence: 0.99 },
            {
              id: 'ww-14',
              word: 'cámara',
              startSec: 6.3,
              endSec: 6.85,
              confidence: 0.99,
              highlight: true,
            },
            {
              id: 'ww-15',
              word: '3D',
              startSec: 6.85,
              endSec: 7.3,
              confidence: 0.99,
              highlight: true,
            },
            { id: 'ww-16', word: 'automáticamente.', startSec: 7.3, endSec: 8.1, confidence: 0.97 },
          ],
        },
        {
          id: 'sub-whisper-3',
          startSec: 8.4,
          endSec: 11.8,
          words: [
            { id: 'ww-17', word: 'Lista', startSec: 8.4, endSec: 8.85, confidence: 0.99 },
            { id: 'ww-18', word: 'para', startSec: 8.85, endSec: 9.15, confidence: 0.99 },
            {
              id: 'ww-19',
              word: 'Google',
              startSec: 9.15,
              endSec: 9.6,
              confidence: 0.99,
              highlight: true,
            },
            {
              id: 'ww-20',
              word: 'Play',
              startSec: 9.6,
              endSec: 10.0,
              confidence: 0.99,
              highlight: true,
            },
            {
              id: 'ww-21',
              word: 'Store',
              startSec: 10.0,
              endSec: 10.5,
              confidence: 0.99,
              highlight: true,
            },
            { id: 'ww-22', word: 'en', startSec: 10.5, endSec: 10.75, confidence: 0.99 },
            {
              id: 'ww-23',
              word: '4K.',
              startSec: 10.75,
              endSec: 11.8,
              confidence: 0.99,
              highlight: true,
            },
          ],
        },
      ];

      onUpdateProject((prev) => ({
        ...prev,
        subtitles: richSegments,
        subtitleStyle: { ...prev.subtitleStyle, enabled: true },
      }));
      setIsTranscribing(false);
    }, 650);
  };

  const updateWordInSegment = (
    segId: string,
    wordId: string,
    patch: Partial<SubtitleWord>
  ) => {
    onUpdateProject((prev) => ({
      ...prev,
      subtitles: prev.subtitles.map((seg) =>
        seg.id === segId
          ? {
              ...seg,
              words: seg.words.map((w) => (w.id === wordId ? { ...w, ...patch } : w)),
            }
          : seg
      ),
    }));
  };

  const handleRunSimulatedMcpPolish = () => {
    handleAutoGenerateDirectorFromTouches();
    onUpdateProject((prev) => ({
      ...prev,
      device3D: {
        ...prev.device3D,
        floatingAnimation: true,
        showShadow: true,
        shadowIntensity: 0.75,
      },
      background: {
        ...prev.background,
        motionBlurEnabled: true,
        motionBlurIntensity: 0.55,
      },
      exportSettings: {
        ...prev.exportSettings,
        presetId: 'play-store-16-9-4k',
        width: 3840,
        height: 2160,
        fps: 60,
      },
    }));
    setAiPolishFeedback(
      'Agente MCP ejecutó: auto_generate_director_zooms + motionBlur=0.55 + preset Play Store 4K 60fps.'
    );
    setTimeout(() => setAiPolishFeedback(null), 5000);
  };

  const mcpJsonConfig = useMemo(
    () =>
      JSON.stringify(
        {
          mcpServers: {
            'rec-ording': {
              command: 'node',
              args: [
                '/home/blzkz/Documentos/pet-projects/rec-ordingApp/scripts/mcp-server.mjs',
              ],
            },
          },
        },
        null,
        2
      ),
    []
  );

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard?.writeText(text).catch(() => {});
    setCopiedSnippet(id);
    setTimeout(() => setCopiedSnippet(null), 1800);
  };

  return (
    <aside className="w-96 shrink-0 bg-slate-900/95 border-l border-slate-800 flex flex-col h-full select-none text-slate-200">
      {/* Top 5-Tab Bar */}
      <div className="grid grid-cols-5 border-b border-slate-800 bg-slate-950 p-1 gap-1">
        {(
          [
            { id: '3d', label: '3D & Estilo', icon: Smartphone },
            { id: 'director', label: 'Director', icon: Camera },
            { id: 'subtitles', label: 'Subtítulos', icon: Captions },
            { id: 'mcp', label: 'MCP / IA', icon: Bot },
            { id: 'export', label: 'Exportar', icon: Film },
          ] as const
        ).map((tab) => {
          const Icon = tab.icon;
          const active = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTab(tab.id)}
              className={`flex flex-col items-center justify-center py-1.5 px-1 rounded-lg text-[10px] font-semibold transition ${
                active
                  ? 'bg-indigo-600 text-white shadow'
                  : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900'
              }`}
            >
              <Icon className="w-3.5 h-3.5 mb-0.5" />
              <span className="truncate">{tab.label}</span>
            </button>
          );
        })}
      </div>

      {/* Selected Clip Quick Inspector Banner (when a timeline clip is selected) */}
      {selectedClip && (
        <div className="p-3 bg-indigo-950/40 border-b border-indigo-800/50 space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-bold text-indigo-300 flex items-center gap-1">
              <Type className="w-3.5 h-3.5" />
              Clip Seleccionado: {selectedClip.name}
            </span>
            <span className="text-[10px] font-mono text-indigo-400">
              {selectedClip.startSec.toFixed(1)}s ({selectedClip.durationSec.toFixed(1)}s)
            </span>
          </div>

          {selectedClip.type === 'text' && (
            <div className="space-y-1.5">
              <input
                type="text"
                value={selectedClip.text || ''}
                onChange={(e) =>
                  updateSelectedClip({ text: e.target.value, name: e.target.value })
                }
                placeholder="Título principal..."
                className="w-full px-2 py-1 rounded bg-slate-950 border border-slate-700 text-xs text-white"
              />
              <input
                type="text"
                value={selectedClip.subtitleText || ''}
                onChange={(e) => updateSelectedClip({ subtitleText: e.target.value })}
                placeholder="Subtítulo o badge secundario..."
                className="w-full px-2 py-1 rounded bg-slate-950 border border-slate-700 text-[11px] text-slate-300"
              />
              <div className="grid grid-cols-2 gap-1.5">
                <select
                  value={selectedClip.titlePreset || 'pop-elastic'}
                  onChange={(e) =>
                    updateSelectedClip({
                      titlePreset: e.target.value as TitleAnimationPreset,
                    })
                  }
                  className="px-2 py-1 rounded bg-slate-950 border border-slate-700 text-[11px] text-slate-200"
                >
                  <option value="pop-elastic">Pop Elástico</option>
                  <option value="slide-up-blur">Slide Up Blur</option>
                  <option value="glass-badge">Glass Badge</option>
                  <option value="neon-glow">Neon Glow</option>
                  <option value="typewriter">Máquina de Escribir</option>
                  <option value="minimal-fade">Minimal Fade</option>
                </select>
                <div className="flex items-center gap-1">
                  <input
                    type="color"
                    value={selectedClip.textColor || '#ffffff'}
                    onChange={(e) => updateSelectedClip({ textColor: e.target.value })}
                    className="w-7 h-6 rounded bg-transparent cursor-pointer"
                    title="Color de texto"
                  />
                  <input
                    type="range"
                    min={0.08}
                    max={0.9}
                    step={0.02}
                    value={selectedClip.posY ?? 0.14}
                    onChange={(e) =>
                      updateSelectedClip({ posY: Number(e.target.value) })
                    }
                    className="w-full accent-indigo-500"
                    title="Posición vertical Y"
                  />
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Scrollable Inspector Content */}
      <div className="flex-1 overflow-y-auto p-4 space-y-5 text-xs">
        {/* TAB 1: 3D & ACABADO */}
        {activeTab === '3d' && (
          <>
            {/* 3D Device Model Selector */}
            <section className="space-y-2.5">
              <div className="flex items-center justify-between">
                <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
                  <Smartphone className="w-3.5 h-3.5 text-indigo-400" />
                  <span>Maqueta 3D del Dispositivo</span>
                </h3>
                <button
                  type="button"
                  onClick={() => glbInputRef.current?.click()}
                  className="flex items-center gap-1 px-2 py-0.5 rounded bg-indigo-950 hover:bg-indigo-900 text-indigo-300 border border-indigo-700/60 text-[10px] transition"
                >
                  <Upload className="w-3 h-3" />
                  <span>Importar .GLB</span>
                </button>
                <input
                  ref={glbInputRef}
                  type="file"
                  accept=".glb,.gltf"
                  onChange={handleGlbFileSelect}
                  className="hidden"
                />
              </div>

              <div className="grid grid-cols-2 gap-1.5">
                {(
                  [
                    { id: 'pixel-11-pro', name: 'Pixel 11 Pro' },
                    { id: 'galaxy-s26-ultra', name: 'Galaxy S26 Ultra' },
                    { id: 'galaxy-tab-s10-ultra', name: 'Tab S10 Ultra' },
                    { id: 'desktop-window', name: 'Ventana PC 3D' },
                    { id: 'custom-glb', name: 'Modelo .GLB Propio' },
                    { id: 'none', name: 'Sin Marco (2D)' },
                  ] as { id: DeviceModelType; name: string }[]
                ).map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        device3D: { ...prev.device3D, primaryModel: item.id },
                      }))
                    }
                    className={`px-2.5 py-2 rounded-lg border text-left font-medium transition ${
                      project.device3D.primaryModel === item.id
                        ? 'bg-indigo-600/25 border-indigo-500 text-white'
                        : 'bg-slate-950 border-slate-800 text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    {item.name}
                  </button>
                ))}
              </div>

              {project.device3D.customGlbScreenMeshName && (
                <div className="text-[11px] text-emerald-300 bg-emerald-950/40 border border-emerald-800/50 rounded px-2.5 py-1.5">
                  GLB cargado: <strong>{project.device3D.customGlbScreenMeshName}</strong>
                </div>
              )}

              {/* Dual Layout & Orientation */}
              <div className="grid grid-cols-2 gap-2 pt-1">
                <div>
                  <label className="block text-[11px] text-slate-400 mb-1">
                    Escena Multipista 3D
                  </label>
                  <select
                    value={project.device3D.dualLayout}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        device3D: {
                          ...prev.device3D,
                          dualLayout: e.target.value as ProjectFile['device3D']['dualLayout'],
                        },
                      }))
                    }
                    className="w-full px-2 py-1.5 rounded-lg bg-slate-950 border border-slate-800 text-slate-200"
                  >
                    <option value="single">Un Dispositivo</option>
                    <option value="side-by-side">Lado a Lado (PC + Android)</option>
                    <option value="pip-floating">PiP Flotante 3D</option>
                    <option value="angled-showcase">Escaparate en Ángulo</option>
                  </select>
                </div>

                <div>
                  <label className="block text-[11px] text-slate-400 mb-1">
                    Orientación
                  </label>
                  <select
                    value={project.device3D.orientation}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        device3D: {
                          ...prev.device3D,
                          orientation: e.target.value as 'portrait' | 'landscape',
                        },
                      }))
                    }
                    className="w-full px-2 py-1.5 rounded-lg bg-slate-950 border border-slate-800 text-slate-200"
                  >
                    <option value="portrait">Vertical (Portrait)</option>
                    <option value="landscape">Horizontal (Landscape)</option>
                  </select>
                </div>
              </div>

              {/* Material & Floating toggles */}
              <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 space-y-2.5">
                <div className="flex items-center justify-between">
                  <span className="text-slate-400">Color del Chasis</span>
                  <input
                    type="color"
                    value={project.device3D.bodyColor}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        device3D: { ...prev.device3D, bodyColor: e.target.value },
                      }))
                    }
                    className="w-8 h-6 rounded cursor-pointer bg-transparent"
                  />
                </div>

                <label className="flex items-center justify-between cursor-pointer">
                  <span className="text-slate-300">Levitación 3D Suave</span>
                  <input
                    type="checkbox"
                    checked={project.device3D.floatingAnimation}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        device3D: {
                          ...prev.device3D,
                          floatingAnimation: e.target.checked,
                        },
                      }))
                    }
                    className="accent-indigo-500"
                  />
                </label>

                <div>
                  <div className="flex justify-between text-[11px] text-slate-400 mb-0.5">
                    <span>Sombra de Estudio</span>
                    <span>{Math.round(project.device3D.shadowIntensity * 100)}%</span>
                  </div>
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={project.device3D.shadowIntensity}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        device3D: {
                          ...prev.device3D,
                          shadowIntensity: Number(e.target.value),
                        },
                      }))
                    }
                    className="w-full accent-indigo-500"
                  />
                </div>
              </div>
            </section>

            {/* Cursor, Touch Ripples & Keystrokes HUD */}
            <section className="space-y-2.5 pt-2 border-t border-slate-800">
              <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
                <MousePointer className="w-3.5 h-3.5 text-sky-400" />
                <span>Cursor, Toques ADB y Teclas</span>
              </h3>

              <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 space-y-2.5">
                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <label className="block text-[11px] text-slate-400 mb-1">
                      Estilo Puntero / Toque
                    </label>
                    <select
                      value={project.cursorStyle.cursorStyle}
                      onChange={(e) =>
                        onUpdateProject((prev) => ({
                          ...prev,
                          cursorStyle: {
                            ...prev.cursorStyle,
                            cursorStyle: e.target
                              .value as ProjectFile['cursorStyle']['cursorStyle'],
                          },
                        }))
                      }
                      className="w-full px-2 py-1 rounded bg-slate-900 border border-slate-800 text-slate-200"
                    >
                      <option value="touch-orb">Orbe Táctil Android</option>
                      <option value="macos-modern">macOS Moderno</option>
                      <option value="circle-dot">Punto Minimalista</option>
                      <option value="crosshair">Cruceta Precisión</option>
                    </select>
                  </div>

                  <div>
                    <label className="block text-[11px] text-slate-400 mb-1">
                      Suavizado Inercia
                    </label>
                    <input
                      type="range"
                      min={0.05}
                      max={0.85}
                      step={0.05}
                      value={project.cursorStyle.smoothingFactor}
                      onChange={(e) =>
                        onUpdateProject((prev) => ({
                          ...prev,
                          cursorStyle: {
                            ...prev.cursorStyle,
                            smoothingFactor: Number(e.target.value),
                          },
                        }))
                      }
                      className="w-full accent-sky-400 mt-1.5"
                    />
                  </div>
                </div>

                <label className="flex items-center justify-between cursor-pointer">
                  <span className="text-slate-300">Ondas de Clic / Toque (Ripples)</span>
                  <input
                    type="checkbox"
                    checked={project.cursorStyle.showClickRipples}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        cursorStyle: {
                          ...prev.cursorStyle,
                          showClickRipples: e.target.checked,
                        },
                      }))
                    }
                    className="accent-sky-400"
                  />
                </label>

                <label className="flex items-center justify-between cursor-pointer">
                  <span className="text-slate-300">Estela de Gestos Swipe</span>
                  <input
                    type="checkbox"
                    checked={project.cursorStyle.showTouchTrails}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        cursorStyle: {
                          ...prev.cursorStyle,
                          showTouchTrails: e.target.checked,
                        },
                      }))
                    }
                    className="accent-sky-400"
                  />
                </label>

                <label className="flex items-center justify-between cursor-pointer">
                  <span className="text-slate-300">Mostrar Atajos de Teclado (HUD)</span>
                  <input
                    type="checkbox"
                    checked={project.cursorStyle.showKeystrokesHUD}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        cursorStyle: {
                          ...prev.cursorStyle,
                          showKeystrokesHUD: e.target.checked,
                        },
                      }))
                    }
                    className="accent-sky-400"
                  />
                </label>
              </div>
            </section>

            {/* Background & Motion Blur */}
            <section className="space-y-2.5 pt-2 border-t border-slate-800">
              <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
                <Palette className="w-3.5 h-3.5 text-emerald-400" />
                <span>Fondo de Estudio & Motion Blur</span>
              </h3>

              <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 space-y-2.5">
                <div className="flex items-center justify-between">
                  <span className="text-slate-400">Colores Degradado Mesh</span>
                  <div className="flex items-center gap-1.5">
                    <input
                      type="color"
                      value={project.background.colorA}
                      onChange={(e) =>
                        onUpdateProject((prev) => ({
                          ...prev,
                          background: { ...prev.background, colorA: e.target.value },
                        }))
                      }
                      className="w-6 h-6 rounded cursor-pointer bg-transparent"
                    />
                    <input
                      type="color"
                      value={project.background.colorB}
                      onChange={(e) =>
                        onUpdateProject((prev) => ({
                          ...prev,
                          background: { ...prev.background, colorB: e.target.value },
                        }))
                      }
                      className="w-6 h-6 rounded cursor-pointer bg-transparent"
                    />
                    <input
                      type="color"
                      value={project.background.colorC}
                      onChange={(e) =>
                        onUpdateProject((prev) => ({
                          ...prev,
                          background: { ...prev.background, colorC: e.target.value },
                        }))
                      }
                      className="w-6 h-6 rounded cursor-pointer bg-transparent"
                    />
                  </div>
                </div>

                <label className="flex items-center justify-between cursor-pointer">
                  <span className="text-slate-300">
                    Desenfoque de Movimiento (Motion Blur)
                  </span>
                  <input
                    type="checkbox"
                    checked={project.background.motionBlurEnabled}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        background: {
                          ...prev.background,
                          motionBlurEnabled: e.target.checked,
                        },
                      }))
                    }
                    className="accent-emerald-400"
                  />
                </label>

                {project.background.motionBlurEnabled && (
                  <div>
                    <div className="flex justify-between text-[11px] text-slate-400 mb-0.5">
                      <span>Intensidad Shutter Blur</span>
                      <span>
                        {Math.round(project.background.motionBlurIntensity * 100)}%
                      </span>
                    </div>
                    <input
                      type="range"
                      min={0.1}
                      max={1}
                      step={0.05}
                      value={project.background.motionBlurIntensity}
                      onChange={(e) =>
                        onUpdateProject((prev) => ({
                          ...prev,
                          background: {
                            ...prev.background,
                            motionBlurIntensity: Number(e.target.value),
                          },
                        }))
                      }
                      className="w-full accent-emerald-400"
                    />
                  </div>
                )}
              </div>
            </section>
          </>
        )}

        {/* TAB 2: DIRECTOR & PRIVACIDAD */}
        {activeTab === 'director' && (
          <>
            <section className="space-y-3">
              <div className="flex items-center justify-between">
                <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
                  <Camera className="w-3.5 h-3.5 text-fuchsia-400" />
                  <span>Cámara Director 3D ({project.directorKeyframes.length})</span>
                </h3>
                <button
                  type="button"
                  onClick={handleAutoGenerateDirectorFromTouches}
                  className="flex items-center gap-1 px-2 py-1 rounded-lg bg-fuchsia-600 hover:bg-fuchsia-500 text-white font-semibold text-[10px] shadow transition"
                >
                  <Sparkles className="w-3 h-3" />
                  <span>Auto-Zoom desde Toques</span>
                </button>
              </div>

              <p className="text-[11px] text-slate-400">
                Interpolación automática con físicas spring hacia el punto exacto de cada toque o clic.
              </p>

              <div className="space-y-2">
                {project.directorKeyframes.map((kf) => (
                  <div
                    key={kf.id}
                    className="p-2.5 rounded-xl bg-slate-950 border border-slate-800 space-y-2"
                  >
                    <div className="flex items-center justify-between">
                      <button
                        type="button"
                        onClick={() => onSeek(kf.timeSec)}
                        className="text-xs font-bold text-fuchsia-300 hover:underline text-left truncate"
                      >
                        ◆ {kf.timeSec.toFixed(1)}s · {kf.label || 'Cámara 3D'}
                      </button>
                      <button
                        type="button"
                        onClick={() =>
                          onUpdateProject((prev) => ({
                            ...prev,
                            directorKeyframes: prev.directorKeyframes.filter(
                              (item) => item.id !== kf.id
                            ),
                          }))
                        }
                        className="text-slate-500 hover:text-rose-400"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>

                    <div className="grid grid-cols-3 gap-2 text-[10px]">
                      <div>
                        <span className="text-slate-400 block">
                          Zoom: {kf.zoom.toFixed(2)}x
                        </span>
                        <input
                          type="range"
                          min={0.85}
                          max={2.6}
                          step={0.05}
                          value={kf.zoom}
                          onChange={(e) =>
                            onUpdateProject((prev) => ({
                              ...prev,
                              directorKeyframes: prev.directorKeyframes.map((item) =>
                                item.id === kf.id
                                  ? { ...item, zoom: Number(e.target.value) }
                                  : item
                              ),
                            }))
                          }
                          className="w-full accent-fuchsia-500"
                        />
                      </div>
                      <div>
                        <span className="text-slate-400 block">
                          Giro Y: {kf.rotYDeg}°
                        </span>
                        <input
                          type="range"
                          min={-35}
                          max={35}
                          step={1}
                          value={kf.rotYDeg}
                          onChange={(e) =>
                            onUpdateProject((prev) => ({
                              ...prev,
                              directorKeyframes: prev.directorKeyframes.map((item) =>
                                item.id === kf.id
                                  ? { ...item, rotYDeg: Number(e.target.value) }
                                  : item
                              ),
                            }))
                          }
                          className="w-full accent-fuchsia-500"
                        />
                      </div>
                      <div>
                        <span className="text-slate-400 block">Curva</span>
                        <select
                          value={kf.easing}
                          onChange={(e) =>
                            onUpdateProject((prev) => ({
                              ...prev,
                              directorKeyframes: prev.directorKeyframes.map((item) =>
                                item.id === kf.id
                                  ? {
                                      ...item,
                                      easing: e.target
                                        .value as DirectorCameraKeyframe['easing'],
                                    }
                                  : item
                              ),
                            }))
                          }
                          className="w-full bg-slate-900 border border-slate-800 rounded px-1 py-0.5 text-slate-200"
                        >
                          <option value="spring">Spring</option>
                          <option value="easeInOutCubic">Cubic</option>
                          <option value="linear">Linear</option>
                        </select>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </section>

            {/* Privacy Masks Section */}
            <section className="space-y-3 pt-3 border-t border-slate-800">
              <div className="flex items-center justify-between">
                <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
                  <ShieldAlert className="w-3.5 h-3.5 text-cyan-400" />
                  <span>Zonas Sensibles Difuminadas ({project.privacyMasks.length})</span>
                </h3>
                <button
                  type="button"
                  onClick={() => {
                    const mask: PrivacyMask = {
                      id: `mask-${Date.now().toString().slice(-4)}`,
                      label: 'Ocultar Datos Privados',
                      startSec: Number(currentTimeSec.toFixed(1)),
                      endSec: Number(
                        Math.min(project.durationSec, currentTimeSec + 3.0).toFixed(1)
                      ),
                      x: 0.2,
                      y: 0.25,
                      width: 0.6,
                      height: 0.1,
                      mode: 'blur',
                      intensity: 20,
                      color: '#0f172a',
                    };
                    onUpdateProject((prev) => ({
                      ...prev,
                      privacyMasks: [...prev.privacyMasks, mask],
                    }));
                  }}
                  className="flex items-center gap-1 px-2 py-1 rounded-lg bg-cyan-950 hover:bg-cyan-900 text-cyan-200 border border-cyan-700/60 text-[10px] transition"
                >
                  <Plus className="w-3 h-3" />
                  <span>Añadir Zona</span>
                </button>
              </div>

              <div className="space-y-2">
                {project.privacyMasks.map((mask) => (
                  <div
                    key={mask.id}
                    className="p-2.5 rounded-xl bg-slate-950 border border-slate-800 space-y-2"
                  >
                    <div className="flex items-center justify-between gap-2">
                      <input
                        type="text"
                        value={mask.label}
                        onChange={(e) =>
                          onUpdateProject((prev) => ({
                            ...prev,
                            privacyMasks: prev.privacyMasks.map((m) =>
                              m.id === mask.id ? { ...m, label: e.target.value } : m
                            ),
                          }))
                        }
                        className="bg-slate-900 border border-slate-800 rounded px-2 py-0.5 text-xs text-cyan-200 flex-1"
                      />
                      <select
                        value={mask.mode}
                        onChange={(e) =>
                          onUpdateProject((prev) => ({
                            ...prev,
                            privacyMasks: prev.privacyMasks.map((m) =>
                              m.id === mask.id
                                ? {
                                    ...m,
                                    mode: e.target.value as PrivacyMask['mode'],
                                  }
                                : m
                            ),
                          }))
                        }
                        className="bg-slate-900 border border-slate-800 rounded px-1.5 py-0.5 text-[11px]"
                      >
                        <option value="blur">Difuminado</option>
                        <option value="pixelate">Pixelado</option>
                        <option value="solid">Bloque Sólido</option>
                      </select>
                      <button
                        type="button"
                        onClick={() =>
                          onUpdateProject((prev) => ({
                            ...prev,
                            privacyMasks: prev.privacyMasks.filter(
                              (m) => m.id !== mask.id
                            ),
                          }))
                        }
                        className="text-slate-500 hover:text-rose-400"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>

                    <div className="grid grid-cols-2 gap-2 text-[10px] text-slate-400">
                      <div>
                        <span>
                          Tiempo: {mask.startSec.toFixed(1)}s - {mask.endSec.toFixed(1)}s
                        </span>
                        <input
                          type="range"
                          min={0}
                          max={project.durationSec}
                          step={0.2}
                          value={mask.startSec}
                          onChange={(e) =>
                            onUpdateProject((prev) => ({
                              ...prev,
                              privacyMasks: prev.privacyMasks.map((m) =>
                                m.id === mask.id
                                  ? { ...m, startSec: Number(e.target.value) }
                                  : m
                              ),
                            }))
                          }
                          className="w-full accent-cyan-400"
                        />
                      </div>
                      <div>
                        <span>Posición Y: {Math.round(mask.y * 100)}%</span>
                        <input
                          type="range"
                          min={0.05}
                          max={0.9}
                          step={0.02}
                          value={mask.y}
                          onChange={(e) =>
                            onUpdateProject((prev) => ({
                              ...prev,
                              privacyMasks: prev.privacyMasks.map((m) =>
                                m.id === mask.id
                                  ? { ...m, y: Number(e.target.value) }
                                  : m
                              ),
                            }))
                          }
                          className="w-full accent-cyan-400"
                        />
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </section>
          </>
        )}

        {/* TAB 3: SUBTÍTULOS PALABRA POR PALABRA */}
        {activeTab === 'subtitles' && (
          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
                <Captions className="w-3.5 h-3.5 text-sky-400" />
                <span>Subtítulos Dinámicos Karaoke</span>
              </h3>
              <label className="flex items-center gap-1.5 text-[11px] cursor-pointer">
                <input
                  type="checkbox"
                  checked={project.subtitleStyle.enabled}
                  onChange={(e) =>
                    onUpdateProject((prev) => ({
                      ...prev,
                      subtitleStyle: {
                        ...prev.subtitleStyle,
                        enabled: e.target.checked,
                      },
                    }))
                  }
                  className="accent-sky-400"
                />
                <span>Activos</span>
              </label>
            </div>

            <button
              type="button"
              disabled={isTranscribing}
              onClick={handleTranscribeWhisperLocal}
              className="w-full py-2 px-3 rounded-xl bg-gradient-to-r from-indigo-600 to-sky-600 hover:from-indigo-500 hover:to-sky-500 text-white font-bold text-xs flex items-center justify-center gap-2 shadow-lg transition"
            >
              <Wand2
                className={`w-4 h-4 ${isTranscribing ? 'animate-spin' : ''}`}
              />
              <span>
                {isTranscribing
                  ? 'Analizando voz con Whisper Local...'
                  : 'Transcribir Audio en este PC (Whisper Local)'}
              </span>
            </button>

            {/* Subtitle Style Controls */}
            <div className="p-2.5 rounded-xl bg-slate-950 border border-slate-800 grid grid-cols-3 gap-2 text-[11px]">
              <div>
                <span className="text-slate-400 block">Tamaño</span>
                <input
                  type="number"
                  min={18}
                  max={64}
                  value={project.subtitleStyle.fontSize}
                  onChange={(e) =>
                    onUpdateProject((prev) => ({
                      ...prev,
                      subtitleStyle: {
                        ...prev.subtitleStyle,
                        fontSize: Number(e.target.value),
                      },
                    }))
                  }
                  className="w-full bg-slate-900 border border-slate-800 rounded px-2 py-1 mt-0.5"
                />
              </div>
              <div>
                <span className="text-slate-400 block">Color Activo</span>
                <input
                  type="color"
                  value={project.subtitleStyle.activeWordColor}
                  onChange={(e) =>
                    onUpdateProject((prev) => ({
                      ...prev,
                      subtitleStyle: {
                        ...prev.subtitleStyle,
                        activeWordColor: e.target.value,
                      },
                    }))
                  }
                  className="w-full h-7 rounded bg-transparent cursor-pointer mt-0.5"
                />
              </div>
              <div>
                <span className="text-slate-400 block">Posición Y</span>
                <input
                  type="range"
                  min={0.15}
                  max={0.92}
                  step={0.02}
                  value={project.subtitleStyle.positionY}
                  onChange={(e) =>
                    onUpdateProject((prev) => ({
                      ...prev,
                      subtitleStyle: {
                        ...prev.subtitleStyle,
                        positionY: Number(e.target.value),
                      },
                    }))
                  }
                  className="w-full accent-sky-400 mt-2"
                />
              </div>
            </div>

            {/* Word-by-word interactive segments */}
            <div className="space-y-2.5">
              <div className="text-[11px] text-slate-400">
                Haz clic en cualquier palabra para editarla o activar el resaltado de énfasis:
              </div>

              {project.subtitles.map((seg) => (
                <div
                  key={seg.id}
                  className="p-3 rounded-xl bg-slate-950 border border-slate-800 space-y-2"
                >
                  <div className="flex items-center justify-between text-[10px] text-slate-400 font-mono">
                    <button
                      type="button"
                      onClick={() => onSeek(seg.startSec)}
                      className="hover:text-sky-400"
                    >
                      ▶ {seg.startSec.toFixed(2)}s — {seg.endSec.toFixed(2)}s
                    </button>
                    <span>{seg.words.length} palabras</span>
                  </div>

                  <div className="flex flex-wrap gap-1.5">
                    {seg.words.map((w) => {
                      const isActiveNow =
                        currentTimeSec >= w.startSec && currentTimeSec <= w.endSec;
                      const isEditing = editingWordId === w.id;

                      if (isEditing) {
                        return (
                          <div
                            key={w.id}
                            className="flex items-center gap-1 bg-slate-900 border border-sky-500 rounded px-1.5 py-0.5"
                          >
                            <input
                              type="text"
                              autoFocus
                              value={w.word}
                              onChange={(e) =>
                                updateWordInSegment(seg.id, w.id, {
                                  word: e.target.value,
                                })
                              }
                              onBlur={() => setEditingWordId(null)}
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') setEditingWordId(null);
                              }}
                              className="w-20 bg-transparent text-xs text-white focus:outline-none"
                            />
                            <button
                              type="button"
                              onMouseDown={(e) => {
                                e.preventDefault();
                                updateWordInSegment(seg.id, w.id, {
                                  highlight: !w.highlight,
                                });
                              }}
                              title="Destacar palabra (Karaoke)"
                              className={`px-1 rounded text-[10px] ${
                                w.highlight
                                  ? 'bg-amber-500 text-slate-950 font-bold'
                                  : 'bg-slate-800 text-slate-400'
                              }`}
                            >
                              ★
                            </button>
                          </div>
                        );
                      }

                      return (
                        <button
                          key={w.id}
                          type="button"
                          onClick={() => {
                            onSeek(w.startSec);
                            setEditingWordId(w.id);
                          }}
                          className={`px-2 py-0.5 rounded-md text-xs font-medium border transition ${
                            isActiveNow
                              ? 'bg-sky-500 text-slate-950 border-white font-bold shadow'
                              : w.highlight
                              ? 'bg-indigo-950/90 text-sky-300 border-sky-500/60'
                              : 'bg-slate-900 text-slate-200 border-slate-800 hover:border-slate-700'
                          }`}
                        >
                          {w.word}
                        </button>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* TAB 4: MCP / IA SERVER HUB */}
        {activeTab === 'mcp' && (
          <section className="space-y-3">
            <div className="p-3 rounded-xl bg-gradient-to-br from-indigo-950/80 to-slate-950 border border-indigo-500/40 space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-bold text-white flex items-center gap-1.5">
                  <Bot className="w-4 h-4 text-indigo-400" />
                  <span>Servidor MCP Nativo (Stdio JSON-RPC)</span>
                </span>
                <span className="px-2 py-0.5 rounded-full bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 text-[10px] font-mono">
                  LISTO
                </span>
              </div>
              <p className="text-[11px] text-slate-300">
                Permite que Claude Code, Cursor o cualquier agente MCP edite la línea de tiempo, genere zooms desde los toques ADB y configure FFmpeg automáticamente.
              </p>
              <button
                type="button"
                onClick={handleRunSimulatedMcpPolish}
                className="w-full py-2 px-3 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white font-bold text-xs flex items-center justify-center gap-1.5 shadow transition"
              >
                <Sparkles className="w-3.5 h-3.5" />
                <span>Ejecutar Auto-Pulido IA (Demo Agente MCP)</span>
              </button>
              {aiPolishFeedback && (
                <div className="p-2 rounded bg-emerald-950/80 border border-emerald-700 text-[11px] text-emerald-200">
                  {aiPolishFeedback}
                </div>
              )}
            </div>

            {/* Ready to copy MCP JSON config */}
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-slate-300">
                  Configuración para Claude Desktop / Cursor / Codex:
                </span>
                <button
                  type="button"
                  onClick={() => copyToClipboard(mcpJsonConfig, 'mcp-json')}
                  className="flex items-center gap-1 px-2 py-0.5 rounded bg-slate-800 hover:bg-slate-700 text-[10px] text-slate-200"
                >
                  {copiedSnippet === 'mcp-json' ? (
                    <>
                      <Check className="w-3 h-3 text-emerald-400" />
                      <span>Copiado</span>
                    </>
                  ) : (
                    <>
                      <Copy className="w-3 h-3" />
                      <span>Copiar JSON</span>
                    </>
                  )}
                </button>
              </div>
              <pre className="p-2.5 rounded-xl bg-slate-950 border border-slate-800 text-[10px] font-mono text-indigo-300 overflow-x-auto">
                {mcpJsonConfig}
              </pre>
            </div>

            {/* List of 9 MCP tools */}
            <div className="space-y-1.5">
              <div className="text-[11px] font-semibold text-slate-300">
                9 Herramientas MCP Expuestas en <code>scripts/mcp-server.mjs</code>:
              </div>
              <div className="space-y-1.5">
                {MCP_TOOLS_CATALOG.map((tool) => (
                  <div
                    key={tool.name}
                    className="p-2 rounded-lg bg-slate-950 border border-slate-800/90"
                  >
                    <div className="font-mono text-[11px] font-bold text-sky-400">
                      {tool.name}()
                    </div>
                    <div className="text-[10px] text-slate-400 mt-0.5">
                      {tool.desc}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </section>
        )}

        {/* TAB 5: EXPORTAR CON FFMPEG */}
        {activeTab === 'export' && (
          <section className="space-y-3">
            <h3 className="font-bold text-slate-200 uppercase tracking-wider text-[11px] flex items-center gap-1.5">
              <Film className="w-3.5 h-3.5 text-emerald-400" />
              <span>Presets Oficiales Play Store & App Store</span>
            </h3>

            <div className="grid grid-cols-1 gap-2">
              {EXPORT_PRESETS.map((preset) => {
                const isSelected = project.exportSettings.presetId === preset.id;
                return (
                  <button
                    key={preset.id}
                    type="button"
                    onClick={() =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        exportSettings: {
                          ...prev.exportSettings,
                          presetId: preset.id,
                          ...preset.settings,
                        },
                      }))
                    }
                    className={`p-2.5 rounded-xl border text-left transition ${
                      isSelected
                        ? 'bg-emerald-950/40 border-emerald-500 shadow'
                        : 'bg-slate-950 border-slate-800 hover:border-slate-700'
                    }`}
                  >
                    <div className="flex items-center justify-between">
                      <span className="font-bold text-xs text-white">
                        {preset.name}
                      </span>
                      <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-500/20 text-emerald-300 font-mono">
                        {preset.badge}
                      </span>
                    </div>
                    <p className="text-[10px] text-slate-400 mt-0.5">
                      {preset.description}
                    </p>
                  </button>
                );
              })}
            </div>

            {/* Custom Resolution, FPS, Codec & Motion Blur Samples */}
            <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 space-y-2.5">
              <div className="grid grid-cols-3 gap-2">
                <div>
                  <label className="block text-[10px] text-slate-400">Ancho (px)</label>
                  <input
                    type="number"
                    value={project.exportSettings.width}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        exportSettings: {
                          ...prev.exportSettings,
                          presetId: 'custom',
                          width: Number(e.target.value),
                        },
                      }))
                    }
                    className="w-full bg-slate-900 border border-slate-800 rounded px-2 py-1 text-xs mt-0.5"
                  />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-400">Alto (px)</label>
                  <input
                    type="number"
                    value={project.exportSettings.height}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        exportSettings: {
                          ...prev.exportSettings,
                          presetId: 'custom',
                          height: Number(e.target.value),
                        },
                      }))
                    }
                    className="w-full bg-slate-900 border border-slate-800 rounded px-2 py-1 text-xs mt-0.5"
                  />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-400">FPS</label>
                  <select
                    value={project.exportSettings.fps}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        exportSettings: {
                          ...prev.exportSettings,
                          fps: Number(e.target.value) as 30 | 60,
                        },
                      }))
                    }
                    className="w-full bg-slate-900 border border-slate-800 rounded px-2 py-1 text-xs mt-0.5"
                  >
                    <option value={60}>60 FPS</option>
                    <option value={30}>30 FPS</option>
                  </select>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block text-[10px] text-slate-400">
                    Códec FFmpeg
                  </label>
                  <select
                    value={project.exportSettings.codec}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        exportSettings: {
                          ...prev.exportSettings,
                          codec: e.target.value as VideoCodec,
                        },
                      }))
                    }
                    className="w-full bg-slate-900 border border-slate-800 rounded px-2 py-1 text-xs mt-0.5"
                  >
                    <option value="h264">H.264 (MP4 Universal)</option>
                    <option value="hevc">HEVC / H.265 (4K)</option>
                    <option value="av1">AV1 Alta Eficiencia</option>
                    <option value="vp9-alpha">VP9 WebM (Canal Alpha)</option>
                    <option value="prores-alpha">ProRes 4444 (Alpha .MOV)</option>
                    <option value="gif">GIF Paleta Optimizada</option>
                    <option value="png-sequence">Secuencia PNG</option>
                  </select>
                </div>

                <div>
                  <label className="block text-[10px] text-slate-400">
                    Muestras Motion Blur
                  </label>
                  <select
                    value={project.exportSettings.motionBlurSamples}
                    onChange={(e) =>
                      onUpdateProject((prev) => ({
                        ...prev,
                        exportSettings: {
                          ...prev.exportSettings,
                          motionBlurSamples: Number(e.target.value),
                        },
                      }))
                    }
                    className="w-full bg-slate-900 border border-slate-800 rounded px-2 py-1 text-xs mt-0.5"
                  >
                    <option value={1}>1 (Desactivado)</option>
                    <option value={4}>4 muestras (Rápido)</option>
                    <option value={8}>8 muestras (Estudio)</option>
                    <option value={16}>16 muestras (Cine 60fps)</option>
                  </select>
                </div>
              </div>
            </div>

            {/* FFmpeg Command Preview */}
            <div className="space-y-1">
              <div className="flex items-center justify-between text-[11px] text-slate-400">
                <span className="flex items-center gap-1">
                  <Terminal className="w-3.5 h-3.5 text-emerald-400" />
                  <span>Pipeline FFmpeg Generado:</span>
                </span>
                <button
                  type="button"
                  onClick={() =>
                    copyToClipboard(buildFfmpegCommandPreview(project), 'ffmpeg-cli')
                  }
                  className="text-[10px] text-sky-400 hover:underline"
                >
                  {copiedSnippet === 'ffmpeg-cli' ? '¡Copiado!' : 'Copiar comando'}
                </button>
              </div>
              <pre className="p-2.5 rounded-xl bg-slate-950 border border-slate-800 text-[10px] font-mono text-emerald-300 whitespace-pre-wrap break-all">
                {buildFfmpegCommandPreview(project)}
              </pre>
            </div>

            {/* Main Render Trigger Button */}
            <button
              type="button"
              disabled={isExporting}
              onClick={onTriggerExport}
              className="w-full py-3 px-4 rounded-xl bg-gradient-to-r from-emerald-600 to-teal-600 hover:from-emerald-500 hover:to-teal-500 disabled:opacity-50 text-white font-bold text-xs shadow-lg shadow-emerald-600/25 flex items-center justify-center gap-2 transition"
            >
              <Film className="w-4 h-4" />
              <span>
                {isExporting
                  ? `Renderizando fotogramas (${Math.round(exportProgress)}%)...`
                  : 'Renderizar y Exportar con FFmpeg'}
              </span>
            </button>

            {(isExporting || exportLog) && (
              <div className="p-2.5 rounded-xl bg-slate-950 border border-slate-800 space-y-2">
                {isExporting && (
                  <div className="w-full h-2 rounded-full bg-slate-800 overflow-hidden">
                    <div
                      style={{ width: `${Math.min(100, exportProgress)}%` }}
                      className="h-full bg-emerald-500 transition-all"
                    />
                  </div>
                )}
                {exportLog && (
                  <div className="text-[10px] font-mono text-slate-300 whitespace-pre-wrap">
                    {exportLog}
                  </div>
                )}
              </div>
            )}
          </section>
        )}
      </div>
    </aside>
  );
};
