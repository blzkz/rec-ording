import React, { useRef, useState } from 'react';
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Scissors,
  Gauge,
  Snowflake,
  Diamond,
  Trash2,
  Plus,
  Video,
  Volume2,
  Type,
  Camera,
  ShieldAlert,
  Lock,
  Unlock,
  ZoomIn,
  ZoomOut,
} from 'lucide-react';
import {
  DirectorCameraKeyframe,
  PrivacyMask,
  ProjectFile,
  TimelineClip,
  TimelineTrack,
} from '../types/project';

export interface MultiTrackTimelineProps {
  project: ProjectFile;
  currentTimeSec: number;
  isPlaying: boolean;
  selectedClipId: string | null;
  onSelectClip: (clipId: string | null) => void;
  onSeek: (timeSec: number) => void;
  onTogglePlay: () => void;
  onUpdateProject: (updater: (prev: ProjectFile) => ProjectFile) => void;
}

type TrackFilter = 'all' | 'video' | 'audio' | 'text';

function formatTimecode(sec: number): string {
  const safe = Math.max(0, sec);
  const mins = Math.floor(safe / 60);
  const secs = Math.floor(safe % 60);
  const hundredths = Math.floor((safe % 1) * 100);
  return `${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}.${String(
    hundredths
  ).padStart(2, '0')}`;
}

export const MultiTrackTimeline: React.FC<MultiTrackTimelineProps> = ({
  project,
  currentTimeSec,
  isPlaying,
  selectedClipId,
  onSelectClip,
  onSeek,
  onTogglePlay,
  onUpdateProject,
}) => {
  const [pixelsPerSec, setPixelsPerSec] = useState<number>(72);
  const [trackFilter, setTrackFilter] = useState<TrackFilter>('all');
  const rulerRef = useRef<HTMLDivElement | null>(null);
  const [draggingInfo, setDraggingInfo] = useState<{
    kind: 'playhead' | 'clip-move' | 'clip-trim-end' | 'director-kf';
    targetId?: string;
    startClientX: number;
    initialStartSec: number;
    initialDurationSec: number;
  } | null>(null);

  const totalDuration = Math.max(project.durationSec, 6);
  const timelineWidthPx = Math.max(850, totalDuration * pixelsPerSec + 140);

  const selectedClip: TimelineClip | null = React.useMemo(() => {
    if (!selectedClipId) return null;
    for (const track of project.tracks) {
      const found = track.clips.find((c) => c.id === selectedClipId);
      if (found) return found;
    }
    return null;
  }, [project.tracks, selectedClipId]);

  const filteredTracks = React.useMemo(() => {
    if (trackFilter === 'all') return project.tracks;
    return project.tracks.filter((t) => t.kind === trackFilter);
  }, [project.tracks, trackFilter]);

  const clientXToTime = (clientX: number): number => {
    if (!rulerRef.current) return currentTimeSec;
    const rect = rulerRef.current.getBoundingClientRect();
    const offsetX = clientX - rect.left;
    const sec = offsetX / pixelsPerSec;
    return Math.max(0, Math.min(totalDuration, Number(sec.toFixed(2))));
  };

  const handleRulerMouseDown = (e: React.MouseEvent) => {
    const newTime = clientXToTime(e.clientX);
    onSeek(newTime);
    setDraggingInfo({
      kind: 'playhead',
      startClientX: e.clientX,
      initialStartSec: newTime,
      initialDurationSec: 0,
    });
  };

  React.useEffect(() => {
    if (!draggingInfo) return;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaX = e.clientX - draggingInfo.startClientX;
      const deltaSec = deltaX / pixelsPerSec;

      if (draggingInfo.kind === 'playhead') {
        onSeek(clientXToTime(e.clientX));
      } else if (draggingInfo.kind === 'clip-move' && draggingInfo.targetId) {
        const nextStart = Math.max(
          0,
          Number((draggingInfo.initialStartSec + deltaSec).toFixed(2))
        );
        onUpdateProject((prev) => ({
          ...prev,
          tracks: prev.tracks.map((track) =>
            track.locked
              ? track
              : {
                  ...track,
                  clips: track.clips.map((c) =>
                    c.id === draggingInfo.targetId ? { ...c, startSec: nextStart } : c
                  ),
                }
          ),
        }));
      } else if (draggingInfo.kind === 'clip-trim-end' && draggingInfo.targetId) {
        const nextDuration = Math.max(
          0.4,
          Number((draggingInfo.initialDurationSec + deltaSec).toFixed(2))
        );
        onUpdateProject((prev) => ({
          ...prev,
          tracks: prev.tracks.map((track) =>
            track.locked
              ? track
              : {
                  ...track,
                  clips: track.clips.map((c) =>
                    c.id === draggingInfo.targetId
                      ? {
                          ...c,
                          durationSec: nextDuration,
                          trimOutSec: c.trimInSec + nextDuration * c.speed,
                        }
                      : c
                  ),
                }
          ),
        }));
      } else if (draggingInfo.kind === 'director-kf' && draggingInfo.targetId) {
        const nextTime = Math.max(
          0,
          Math.min(
            totalDuration,
            Number((draggingInfo.initialStartSec + deltaSec).toFixed(2))
          )
        );
        onUpdateProject((prev) => ({
          ...prev,
          directorKeyframes: prev.directorKeyframes
            .map((kf) =>
              kf.id === draggingInfo.targetId ? { ...kf, timeSec: nextTime } : kf
            )
            .sort((a, b) => a.timeSec - b.timeSec),
        }));
      }
    };

    const handleMouseUp = () => {
      setDraggingInfo(null);
    };

    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, [draggingInfo, pixelsPerSec, totalDuration, onSeek, onUpdateProject]);

  // Split selected clip at playhead
  const handleSplitSelectedClip = () => {
    if (!selectedClip) return;
    const splitPoint = currentTimeSec;
    if (
      splitPoint <= selectedClip.startSec + 0.15 ||
      splitPoint >= selectedClip.startSec + selectedClip.durationSec - 0.15
    ) {
      return;
    }

    const firstDuration = Number((splitPoint - selectedClip.startSec).toFixed(2));
    const secondDuration = Number(
      (selectedClip.durationSec - firstDuration).toFixed(2)
    );

    const secondClip: TimelineClip = {
      ...selectedClip,
      id: `${selectedClip.id}-part-${Date.now().toString().slice(-4)}`,
      name: `${selectedClip.name} (Parte 2)`,
      startSec: splitPoint,
      durationSec: secondDuration,
      trimInSec: Number(
        (selectedClip.trimInSec + firstDuration * selectedClip.speed).toFixed(2)
      ),
    };

    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((track) => {
        if (track.id !== selectedClip.trackId || track.locked) return track;
        return {
          ...track,
          clips: track.clips.flatMap((c) =>
            c.id === selectedClip.id
              ? [
                  {
                    ...c,
                    durationSec: firstDuration,
                    trimOutSec: Number(
                      (c.trimInSec + firstDuration * c.speed).toFixed(2)
                    ),
                  },
                  secondClip,
                ]
              : [c]
          ),
        };
      }),
    }));
    onSelectClip(secondClip.id);
  };

  // Change speed of selected clip
  const handleChangeClipSpeed = (newSpeed: number) => {
    if (!selectedClip) return;
    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((track) => {
        if (track.id !== selectedClip.trackId || track.locked) return track;
        return {
          ...track,
          clips: track.clips.map((c) => {
            if (c.id !== selectedClip.id) return c;
            const sourceSpan = c.durationSec * c.speed;
            const nextDuration = Math.max(
              0.4,
              Number((sourceSpan / newSpeed).toFixed(2))
            );
            return {
              ...c,
              speed: newSpeed,
              durationSec: nextDuration,
            };
          }),
        };
      }),
    }));
  };

  // Add Freeze Frame at playhead
  const handleAddFreezeFrame = () => {
    if (!selectedClip) return;
    const freezeDuration = 1.5;
    const relTime = Math.max(0, currentTimeSec - selectedClip.startSec);
    onUpdateProject((prev) => ({
      ...prev,
      durationSec: Math.max(
        prev.durationSec,
        Number((selectedClip.startSec + selectedClip.durationSec + freezeDuration).toFixed(2))
      ),
      tracks: prev.tracks.map((track) => {
        if (track.id !== selectedClip.trackId || track.locked) return track;
        return {
          ...track,
          clips: track.clips.map((c) =>
            c.id === selectedClip.id
              ? {
                  ...c,
                  durationSec: Number((c.durationSec + freezeDuration).toFixed(2)),
                  freezeFrames: [
                    ...c.freezeFrames,
                    {
                      id: `ff-${Date.now()}`,
                      sourceTimeSec: Number((c.trimInSec + relTime).toFixed(2)),
                      timelineStartSec: Number(currentTimeSec.toFixed(2)),
                      durationSec: freezeDuration,
                    },
                  ],
                }
              : c
          ),
        };
      }),
    }));
  };

  // Add Visual Keyframe on selected clip at playhead
  const handleAddClipKeyframe = () => {
    if (!selectedClip) return;
    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((track) => {
        if (track.id !== selectedClip.trackId || track.locked) return track;
        return {
          ...track,
          clips: track.clips.map((c) =>
            c.id === selectedClip.id
              ? {
                  ...c,
                  scaleKeyframes: [
                    ...c.scaleKeyframes,
                    {
                      id: `kf-sc-${Date.now()}`,
                      timeSec: Number(currentTimeSec.toFixed(2)),
                      value: 1.15,
                      easing: 'spring',
                    },
                  ],
                }
              : c
          ),
        };
      }),
    }));
  };

  // Delete selected clip
  const handleDeleteSelectedClip = () => {
    if (!selectedClip) return;
    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((track) =>
        track.locked
          ? track
          : {
              ...track,
              clips: track.clips.filter((c) => c.id !== selectedClip.id),
            }
      ),
    }));
    onSelectClip(null);
  };

  // Quick Add Clips / Keyframes / Masks
  const handleQuickAddClip = (kind: 'video' | 'audio' | 'text') => {
    const targetTrack =
      project.tracks.find((t) => t.kind === kind && !t.locked && t.clips.length === 0) ||
      project.tracks.find((t) => t.kind === kind && !t.locked);
    if (!targetTrack) return;

    const newClipId = `clip-${kind}-${Date.now().toString().slice(-4)}`;
    const newClip: TimelineClip =
      kind === 'text'
        ? {
            id: newClipId,
            trackId: targetTrack.id,
            name: 'Título Destacado Play Store',
            type: 'text',
            startSec: Number(currentTimeSec.toFixed(2)),
            durationSec: 3.0,
            trimInSec: 0,
            trimOutSec: 3.0,
            speed: 1,
            volume: 1,
            muted: false,
            fadeInSec: 0.25,
            fadeOutSec: 0.25,
            freezeFrames: [],
            opacityKeyframes: [],
            scaleKeyframes: [],
            text: 'Nueva Característica 3D',
            subtitleText: 'Animación elástica a 60 FPS',
            titlePreset: 'pop-elastic',
            fontSize: 40,
            textColor: '#ffffff',
            bgColor: 'rgba(79, 70, 229, 0.82)',
            posX: 0.5,
            posY: 0.14,
          }
        : kind === 'audio'
        ? {
            id: newClipId,
            trackId: targetTrack.id,
            name: 'Efecto_Whoosh_Transicion.wav',
            type: 'audio',
            mediaPath: 'builtin://sfx-whoosh',
            startSec: Number(currentTimeSec.toFixed(2)),
            durationSec: 2.4,
            trimInSec: 0,
            trimOutSec: 2.4,
            speed: 1,
            volume: 1.0,
            muted: false,
            fadeInSec: 0.1,
            fadeOutSec: 0.2,
            freezeFrames: [],
            opacityKeyframes: [],
            scaleKeyframes: [],
          }
        : {
            id: newClipId,
            trackId: targetTrack.id,
            name: 'Captura_Overlay_Extra.mp4',
            type: 'video',
            mediaPath: 'builtin://android-demo',
            sourceRole: targetTrack.index === 2 ? 'pc-screen' : 'overlay',
            startSec: Number(currentTimeSec.toFixed(2)),
            durationSec: 4.0,
            trimInSec: 0,
            trimOutSec: 4.0,
            speed: 1,
            volume: 1,
            muted: false,
            fadeInSec: 0.2,
            fadeOutSec: 0.2,
            freezeFrames: [],
            opacityKeyframes: [],
            scaleKeyframes: [],
          };

    onUpdateProject((prev) => ({
      ...prev,
      durationSec: Math.max(
        prev.durationSec,
        Number((newClip.startSec + newClip.durationSec).toFixed(2))
      ),
      tracks: prev.tracks.map((t) =>
        t.id === targetTrack.id ? { ...t, clips: [...t.clips, newClip] } : t
      ),
    }));
    onSelectClip(newClipId);
  };

  const handleQuickAddDirectorKeyframe = () => {
    const newKf: DirectorCameraKeyframe = {
      id: `dir-kf-${Date.now().toString().slice(-4)}`,
      timeSec: Number(currentTimeSec.toFixed(2)),
      durationSec: 2.0,
      focusX: 0.5,
      focusY: 0.45,
      zoom: 1.55,
      rotXDeg: 6,
      rotYDeg: -12,
      rotZDeg: 1,
      easing: 'spring',
      label: `Zoom Cámara @ ${currentTimeSec.toFixed(1)}s`,
    };
    onUpdateProject((prev) => ({
      ...prev,
      directorKeyframes: [...prev.directorKeyframes, newKf].sort(
        (a, b) => a.timeSec - b.timeSec
      ),
    }));
  };

  const handleQuickAddPrivacyMask = () => {
    const newMask: PrivacyMask = {
      id: `mask-${Date.now().toString().slice(-4)}`,
      label: 'Zona Confidencial Difuminada',
      startSec: Number(currentTimeSec.toFixed(2)),
      endSec: Number(Math.min(totalDuration, currentTimeSec + 3.0).toFixed(2)),
      x: 0.25,
      y: 0.35,
      width: 0.5,
      height: 0.1,
      mode: 'blur',
      intensity: 20,
      color: '#0f172a',
    };
    onUpdateProject((prev) => ({
      ...prev,
      privacyMasks: [...prev.privacyMasks, newMask],
    }));
  };

  const toggleTrackProp = (
    trackId: string,
    prop: 'muted' | 'solo' | 'locked'
  ) => {
    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((t) =>
        t.id === trackId ? { ...t, [prop]: !t[prop] } : t
      ),
    }));
  };

  const updateTrackVolume = (trackId: string, volume: number) => {
    onUpdateProject((prev) => ({
      ...prev,
      tracks: prev.tracks.map((t) =>
        t.id === trackId ? { ...t, volume } : t
      ),
    }));
  };

  // Generate second ticks for the ruler
  const ticks = React.useMemo(() => {
    const list: number[] = [];
    for (let s = 0; s <= Math.ceil(totalDuration); s += 1) {
      list.push(s);
    }
    return list;
  }, [totalDuration]);

  return (
    <div className="flex flex-col h-full bg-slate-950 border-t border-slate-800/90 select-none text-slate-200">
      {/* Top Timeline Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-2 px-3 py-1.5 bg-slate-900/95 border-b border-slate-800 text-xs">
        {/* Left: Playback & Timecode & Track Filter */}
        <div className="flex items-center gap-2">
          <div className="flex items-center bg-slate-950 border border-slate-800 rounded-lg p-0.5">
            <button
              type="button"
              onClick={() => onSeek(0)}
              title="Ir al inicio"
              className="p-1.5 hover:bg-slate-800 rounded text-slate-300 hover:text-white transition"
            >
              <SkipBack className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              onClick={onTogglePlay}
              title={isPlaying ? 'Pausar (Espacio)' : 'Reproducir (Espacio)'}
              className="p-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded mx-0.5 transition shadow"
            >
              {isPlaying ? (
                <Pause className="w-3.5 h-3.5" />
              ) : (
                <Play className="w-3.5 h-3.5" />
              )}
            </button>
            <button
              type="button"
              onClick={() => onSeek(project.durationSec)}
              title="Ir al final"
              className="p-1.5 hover:bg-slate-800 rounded text-slate-300 hover:text-white transition"
            >
              <SkipForward className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="font-mono text-[11px] bg-slate-950 border border-slate-800 px-2.5 py-1 rounded-md text-sky-400">
            {formatTimecode(currentTimeSec)}{' '}
            <span className="text-slate-500">/</span>{' '}
            <span className="text-slate-400">{formatTimecode(project.durationSec)}</span>
          </div>

          {/* Track filter pills */}
          <div className="flex items-center bg-slate-950 border border-slate-800 rounded-lg p-0.5 ml-1">
            {(
              [
                { id: 'all', label: 'Todas (13)' },
                { id: 'video', label: 'Vídeo (4)' },
                { id: 'audio', label: 'Audio (4)' },
                { id: 'text', label: 'Texto (5)' },
              ] as { id: TrackFilter; label: string }[]
            ).map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setTrackFilter(tab.id)}
                className={`px-2 py-0.5 rounded text-[11px] font-medium transition ${
                  trackFilter === tab.id
                    ? 'bg-indigo-600/90 text-white'
                    : 'text-slate-400 hover:text-slate-200'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>
        </div>

        {/* Center: Clip Editing Tools (Split, Speed, Freeze Frame, Keyframe, Delete) */}
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            disabled={!selectedClip}
            onClick={handleSplitSelectedClip}
            title="Dividir clip seleccionado en el cabezal"
            className="flex items-center gap-1 px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 disabled:opacity-40 disabled:pointer-events-none text-slate-200 border border-slate-700/80 transition"
          >
            <Scissors className="w-3.5 h-3.5 text-amber-400" />
            <span>Dividir</span>
          </button>

          <div className="flex items-center gap-1 bg-slate-800 border border-slate-700/80 rounded px-2 py-0.5">
            <Gauge className="w-3.5 h-3.5 text-sky-400" />
            <span className="text-[11px] text-slate-400">Vel:</span>
            <select
              disabled={!selectedClip}
              value={selectedClip?.speed ?? 1}
              onChange={(e) => handleChangeClipSpeed(Number(e.target.value))}
              className="bg-transparent text-slate-100 text-[11px] font-medium focus:outline-none disabled:opacity-40"
            >
              {[0.25, 0.5, 1, 1.5, 2, 4].map((spd) => (
                <option key={spd} value={spd} className="bg-slate-900 text-slate-100">
                  {spd}x
                </option>
              ))}
            </select>
          </div>

          <button
            type="button"
            disabled={!selectedClip}
            onClick={handleAddFreezeFrame}
            title="Congelar fotograma (+1.5s) en el cabezal"
            className="flex items-center gap-1 px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 disabled:opacity-40 disabled:pointer-events-none text-slate-200 border border-slate-700/80 transition"
          >
            <Snowflake className="w-3.5 h-3.5 text-cyan-400" />
            <span>Congelar +1.5s</span>
          </button>

          <button
            type="button"
            disabled={!selectedClip}
            onClick={handleAddClipKeyframe}
            title="Añadir fotograma clave de escala/opacidad al clip"
            className="flex items-center gap-1 px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 disabled:opacity-40 disabled:pointer-events-none text-slate-200 border border-slate-700/80 transition"
          >
            <Diamond className="w-3.5 h-3.5 text-fuchsia-400" />
            <span>+ Keyframe</span>
          </button>

          <button
            type="button"
            disabled={!selectedClip}
            onClick={handleDeleteSelectedClip}
            title="Eliminar clip seleccionado"
            className="flex items-center gap-1 px-2 py-1 rounded bg-rose-950/70 hover:bg-rose-900/80 disabled:opacity-40 disabled:pointer-events-none text-rose-200 border border-rose-800/60 transition"
          >
            <Trash2 className="w-3.5 h-3.5 text-rose-400" />
          </button>
        </div>

        {/* Right: Quick Add Buttons & Timeline Zoom */}
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => handleQuickAddClip('video')}
            className="flex items-center gap-1 px-2 py-1 rounded bg-indigo-950/80 hover:bg-indigo-900 text-indigo-200 border border-indigo-800/60 transition"
          >
            <Plus className="w-3 h-3" />
            <Video className="w-3 h-3 text-indigo-400" />
            <span>Clip Vídeo</span>
          </button>
          <button
            type="button"
            onClick={() => handleQuickAddClip('audio')}
            className="flex items-center gap-1 px-2 py-1 rounded bg-emerald-950/80 hover:bg-emerald-900 text-emerald-200 border border-emerald-800/60 transition"
          >
            <Plus className="w-3 h-3" />
            <Volume2 className="w-3 h-3 text-emerald-400" />
            <span>Audio</span>
          </button>
          <button
            type="button"
            onClick={() => handleQuickAddClip('text')}
            className="flex items-center gap-1 px-2 py-1 rounded bg-amber-950/80 hover:bg-amber-900 text-amber-200 border border-amber-800/60 transition"
          >
            <Plus className="w-3 h-3" />
            <Type className="w-3 h-3 text-amber-400" />
            <span>Título</span>
          </button>
          <button
            type="button"
            onClick={handleQuickAddDirectorKeyframe}
            className="flex items-center gap-1 px-2 py-1 rounded bg-fuchsia-950/80 hover:bg-fuchsia-900 text-fuchsia-200 border border-fuchsia-800/60 transition"
          >
            <Camera className="w-3 h-3 text-fuchsia-400" />
            <span>+ Cámara 3D</span>
          </button>
          <button
            type="button"
            onClick={handleQuickAddPrivacyMask}
            className="flex items-center gap-1 px-2 py-1 rounded bg-cyan-950/80 hover:bg-cyan-900 text-cyan-200 border border-cyan-800/60 transition"
          >
            <ShieldAlert className="w-3 h-3 text-cyan-400" />
            <span>+ Blur</span>
          </button>

          {/* Zoom slider */}
          <div className="flex items-center gap-1 pl-2 border-l border-slate-800">
            <ZoomOut className="w-3.5 h-3.5 text-slate-400" />
            <input
              type="range"
              min={36}
              max={150}
              value={pixelsPerSec}
              onChange={(e) => setPixelsPerSec(Number(e.target.value))}
              className="w-16 accent-indigo-500 cursor-pointer"
              title="Zoom de la línea de tiempo"
            />
            <ZoomIn className="w-3.5 h-3.5 text-slate-400" />
          </div>
        </div>
      </div>

      {/* Scrollable Multi-Track Body */}
      <div className="flex-1 overflow-auto relative flex">
        {/* Left Sticky Track Headers Column */}
        <div className="w-60 shrink-0 sticky left-0 z-20 bg-slate-900/95 border-r border-slate-800 flex flex-col">
          {/* Ruler Header Corner */}
          <div className="h-7 px-3 flex items-center justify-between border-b border-slate-800 bg-slate-950/90 text-[11px] font-semibold text-slate-400">
            <span>PISTAS ({filteredTracks.length} + Director)</span>
            <span className="text-[10px] text-slate-500">M · S · L · Vol</span>
          </div>

          {/* Director Camera Lane Header */}
          <div className="h-8 px-3 flex items-center justify-between border-b border-slate-800/80 bg-fuchsia-950/20">
            <div className="flex items-center gap-1.5 text-xs font-medium text-fuchsia-300 truncate">
              <Camera className="w-3.5 h-3.5 text-fuchsia-400 shrink-0" />
              <span className="truncate">Cámara Director 3D</span>
            </div>
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-fuchsia-950 text-fuchsia-300 border border-fuchsia-800/50">
              {project.directorKeyframes.length} KF
            </span>
          </div>

          {/* Privacy Masks Lane Header */}
          <div className="h-7 px-3 flex items-center justify-between border-b border-slate-800/80 bg-cyan-950/15">
            <div className="flex items-center gap-1.5 text-xs font-medium text-cyan-300 truncate">
              <ShieldAlert className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
              <span className="truncate">Privacidad / Blur</span>
            </div>
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-cyan-950 text-cyan-300 border border-cyan-800/50">
              {project.privacyMasks.length} zonas
            </span>
          </div>

          {/* 13 Standard Tracks Headers */}
          {filteredTracks.map((track) => (
            <div
              key={track.id}
              className="h-11 px-2.5 flex items-center justify-between border-b border-slate-800/80 bg-slate-900/80 hover:bg-slate-900 transition"
            >
              <div className="flex items-center gap-1.5 min-w-0 pr-1">
                {track.kind === 'video' && (
                  <Video className="w-3.5 h-3.5 text-indigo-400 shrink-0" />
                )}
                {track.kind === 'audio' && (
                  <Volume2 className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                )}
                {track.kind === 'text' && (
                  <Type className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                )}
                <span
                  className="text-xs font-medium text-slate-200 truncate"
                  title={track.name}
                >
                  {track.name}
                </span>
              </div>

              <div className="flex items-center gap-1 shrink-0">
                <button
                  type="button"
                  onClick={() => toggleTrackProp(track.id, 'muted')}
                  title="Silenciar pista (Mute)"
                  className={`w-5 h-5 rounded text-[10px] font-bold transition ${
                    track.muted
                      ? 'bg-rose-600 text-white'
                      : 'bg-slate-800 text-slate-400 hover:text-white'
                  }`}
                >
                  M
                </button>
                <button
                  type="button"
                  onClick={() => toggleTrackProp(track.id, 'solo')}
                  title="Solo pista"
                  className={`w-5 h-5 rounded text-[10px] font-bold transition ${
                    track.solo
                      ? 'bg-amber-500 text-slate-950'
                      : 'bg-slate-800 text-slate-400 hover:text-white'
                  }`}
                >
                  S
                </button>
                <button
                  type="button"
                  onClick={() => toggleTrackProp(track.id, 'locked')}
                  title="Bloquear pista"
                  className={`w-5 h-5 rounded flex items-center justify-center transition ${
                    track.locked
                      ? 'bg-indigo-600 text-white'
                      : 'bg-slate-800 text-slate-400 hover:text-white'
                  }`}
                >
                  {track.locked ? (
                    <Lock className="w-3 h-3" />
                  ) : (
                    <Unlock className="w-3 h-3" />
                  )}
                </button>
                {track.kind !== 'text' && (
                  <input
                    type="range"
                    min={0}
                    max={2}
                    step={0.05}
                    value={track.volume}
                    onChange={(e) =>
                      updateTrackVolume(track.id, Number(e.target.value))
                    }
                    title={`Volumen de pista: ${Math.round(track.volume * 100)}%`}
                    className="w-11 accent-emerald-400 cursor-pointer"
                  />
                )}
              </div>
            </div>
          ))}
        </div>

        {/* Right Timeline Lanes Canvas */}
        <div
          ref={rulerRef}
          style={{ width: `${timelineWidthPx}px` }}
          className="relative flex flex-col bg-slate-950"
        >
          {/* Time Ruler */}
          <div
            onMouseDown={handleRulerMouseDown}
            className="h-7 border-b border-slate-800 bg-slate-900/90 relative cursor-ew-resize sticky top-0 z-10"
          >
            {ticks.map((sec) => (
              <div
                key={sec}
                style={{ left: `${sec * pixelsPerSec}px` }}
                className="absolute top-0 bottom-0 flex flex-col justify-between pointer-events-none"
              >
                <span className="text-[10px] font-mono text-slate-400 pl-1 pt-0.5">
                  {sec}s
                </span>
                <div className="h-2 w-px bg-slate-600" />
              </div>
            ))}
            {/* Telemetry Touch / Click markers on the ruler */}
            {project.pointerEvents.map((pt) => (
              <div
                key={pt.id}
                style={{ left: `${(pt.timestampMs / 1000) * pixelsPerSec}px` }}
                title={`Evento ${pt.type} (${pt.source}) @ ${(
                  pt.timestampMs / 1000
                ).toFixed(2)}s`}
                className="absolute bottom-0.5 w-1.5 h-1.5 -ml-0.5 rounded-full bg-sky-400/90 pointer-events-none"
              />
            ))}
          </div>

          {/* Director Camera Keyframes Lane */}
          <div className="h-8 border-b border-slate-800/70 bg-fuchsia-950/10 relative">
            {project.directorKeyframes.map((kf) => {
              const leftPx = kf.timeSec * pixelsPerSec;
              const widthPx = Math.max(24, kf.durationSec * pixelsPerSec);
              return (
                <div
                  key={kf.id}
                  style={{ left: `${leftPx}px`, width: `${widthPx}px` }}
                  onMouseDown={(e) => {
                    e.stopPropagation();
                    onSeek(kf.timeSec);
                    setDraggingInfo({
                      kind: 'director-kf',
                      targetId: kf.id,
                      startClientX: e.clientX,
                      initialStartSec: kf.timeSec,
                      initialDurationSec: kf.durationSec,
                    });
                  }}
                  title={`${kf.label || 'Cámara 3D'} · Zoom ${kf.zoom.toFixed(
                    2
                  )}x (${kf.easing})`}
                  className="absolute top-1 bottom-1 rounded-md bg-fuchsia-600/35 border border-fuchsia-400/80 hover:bg-fuchsia-600/50 cursor-grab active:cursor-grabbing flex items-center gap-1 px-1.5 overflow-hidden"
                >
                  <Diamond className="w-3 h-3 text-fuchsia-300 shrink-0 fill-fuchsia-400" />
                  <span className="text-[10px] font-medium text-fuchsia-100 truncate">
                    {kf.zoom.toFixed(1)}x · {kf.label || 'Cámara'}
                  </span>
                </div>
              );
            })}
          </div>

          {/* Privacy Masks Lane */}
          <div className="h-7 border-b border-slate-800/70 bg-cyan-950/10 relative">
            {project.privacyMasks.map((mask) => {
              const leftPx = mask.startSec * pixelsPerSec;
              const widthPx = Math.max(
                20,
                (mask.endSec - mask.startSec) * pixelsPerSec
              );
              return (
                <div
                  key={mask.id}
                  style={{ left: `${leftPx}px`, width: `${widthPx}px` }}
                  onClick={() => onSeek(mask.startSec)}
                  title={`${mask.label} (${mask.mode})`}
                  className="absolute top-1 bottom-1 rounded bg-cyan-600/35 border border-cyan-400/70 hover:bg-cyan-600/50 cursor-pointer flex items-center px-1.5 overflow-hidden"
                >
                  <span className="text-[10px] font-medium text-cyan-100 truncate">
                    {mask.label} ({mask.mode})
                  </span>
                </div>
              );
            })}
          </div>

          {/* Standard Tracks Lanes */}
          {filteredTracks.map((track: TimelineTrack) => (
            <div
              key={track.id}
              onClick={() => onSelectClip(null)}
              className={`h-11 border-b border-slate-800/70 relative ${
                track.locked ? 'bg-slate-900/40 opacity-75' : 'bg-slate-950/60'
              }`}
            >
              {track.clips.map((clip) => {
                const leftPx = clip.startSec * pixelsPerSec;
                const widthPx = Math.max(24, clip.durationSec * pixelsPerSec);
                const isSelected = clip.id === selectedClipId;

                const colorClasses =
                  clip.type === 'video'
                    ? 'bg-gradient-to-r from-indigo-600/80 to-blue-600/75 border-indigo-400'
                    : clip.type === 'audio'
                    ? 'bg-gradient-to-r from-emerald-600/80 to-teal-600/75 border-emerald-400'
                    : 'bg-gradient-to-r from-amber-600/80 to-orange-600/75 border-amber-400';

                return (
                  <div
                    key={clip.id}
                    style={{ left: `${leftPx}px`, width: `${widthPx}px` }}
                    onMouseDown={(e) => {
                      e.stopPropagation();
                      onSelectClip(clip.id);
                      if (track.locked) return;
                      setDraggingInfo({
                        kind: 'clip-move',
                        targetId: clip.id,
                        startClientX: e.clientX,
                        initialStartSec: clip.startSec,
                        initialDurationSec: clip.durationSec,
                      });
                    }}
                    className={`absolute top-1 bottom-1 rounded-md border px-2 flex flex-col justify-center cursor-grab active:cursor-grabbing overflow-hidden transition-shadow ${colorClasses} ${
                      isSelected
                        ? 'ring-2 ring-white shadow-lg shadow-indigo-500/25 z-10'
                        : 'opacity-90 hover:opacity-100'
                    }`}
                  >
                    <div className="flex items-center justify-between gap-1 text-[11px] font-semibold text-white truncate">
                      <span className="truncate">
                        {clip.type === 'text' && clip.text ? clip.text : clip.name}
                      </span>
                      {clip.speed !== 1 && (
                        <span className="px-1 py-0.2 bg-black/40 rounded text-[9px] font-mono text-sky-300 shrink-0">
                          {clip.speed}x
                        </span>
                      )}
                    </div>

                    {/* Freeze Frames & Keyframe badges inside clip */}
                    <div className="flex items-center gap-1 text-[9px] text-white/80 truncate">
                      <span>{clip.durationSec.toFixed(1)}s</span>
                      {clip.freezeFrames.length > 0 && (
                        <span className="px-1 rounded bg-cyan-500/40 text-cyan-100">
                          ❄ {clip.freezeFrames.length}
                        </span>
                      )}
                      {clip.scaleKeyframes.length > 0 && (
                        <span className="px-1 rounded bg-fuchsia-500/40 text-fuchsia-100">
                          ◆ {clip.scaleKeyframes.length} KF
                        </span>
                      )}
                    </div>

                    {/* Right Trim Handle */}
                    {!track.locked && (
                      <div
                        onMouseDown={(e) => {
                          e.stopPropagation();
                          onSelectClip(clip.id);
                          setDraggingInfo({
                            kind: 'clip-trim-end',
                            targetId: clip.id,
                            startClientX: e.clientX,
                            initialStartSec: clip.startSec,
                            initialDurationSec: clip.durationSec,
                          });
                        }}
                        title="Arrastrar para recortar duración"
                        className="absolute right-0 top-0 bottom-0 w-2 bg-white/30 hover:bg-white/80 cursor-ew-resize"
                      />
                    )}
                  </div>
                );
              })}
            </div>
          ))}

          {/* Playhead Vertical Line */}
          <div
            style={{ left: `${currentTimeSec * pixelsPerSec}px` }}
            className="absolute top-0 bottom-0 w-0.5 bg-rose-500 pointer-events-none z-30"
          >
            <div className="w-3 h-3 -ml-[5px] bg-rose-500 rotate-45 rounded-xs shadow" />
          </div>
        </div>
      </div>
    </div>
  );
};
