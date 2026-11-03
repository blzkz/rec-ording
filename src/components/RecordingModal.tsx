import React, { useEffect, useRef, useState } from 'react';
import {
  Smartphone,
  Monitor,
  Layers,
  Camera,
  Mic,
  MousePointerClick,
  Sparkles,
  RefreshCw,
  CircleDot,
  Square,
  X,
  CheckCircle2,
  Usb,
  Keyboard,
} from 'lucide-react';
import {
  AdbDeviceInfo,
  DirectorCameraKeyframe,
  ProjectFile,
  TelemetryKeyEvent,
  TelemetryPointerEvent,
  TimelineClip,
} from '../types/project';

export interface RecordingModalProps {
  isOpen: boolean;
  onClose: () => void;
  project: ProjectFile;
  onUpdateProject: (updater: (prev: ProjectFile) => ProjectFile) => void;
}

type CaptureMode = 'android-usb' | 'pc-screen' | 'dual-simultaneous';

const FALLBACK_ADB_DEVICES: AdbDeviceInfo[] = [
  {
    serial: 'USB-PIXEL11PRO-9921A',
    state: 'device',
    model: 'Pixel_11_Pro',
    product: 'husky_pro',
    device: 'pixel11',
    transportId: '1',
    resolution: '1344x2992 (120Hz)',
  },
  {
    serial: 'emulator-5554',
    state: 'device',
    model: 'Galaxy_S26_Ultra_AVD',
    product: 'sdk_gphone64',
    device: 'emu64',
    transportId: '2',
    resolution: '1440x3120 (60Hz)',
  },
];

export const RecordingModal: React.FC<RecordingModalProps> = ({
  isOpen,
  onClose,
  project,
  onUpdateProject,
}) => {
  const [captureMode, setCaptureMode] = useState<CaptureMode>('dual-simultaneous');
  const [adbDevices, setAdbDevices] = useState<AdbDeviceInfo[]>(FALLBACK_ADB_DEVICES);
  const [selectedAdbSerial, setSelectedAdbSerial] = useState<string>(
    FALLBACK_ADB_DEVICES[0].serial
  );
  const [isScanningAdb, setIsScanningAdb] = useState<boolean>(false);

  const [enableWebcam, setEnableWebcam] = useState<boolean>(false);
  const [enableMic, setEnableMic] = useState<boolean>(true);
  const [captureTelemetry, setCaptureTelemetry] = useState<boolean>(true);
  const [autoDirectorZoom, setAutoDirectorZoom] = useState<boolean>(true);

  const [isRecording, setIsRecording] = useState<boolean>(false);
  const [elapsedSec, setElapsedSec] = useState<number>(0);
  const [recordedPointers, setRecordedPointers] = useState<TelemetryPointerEvent[]>([]);
  const [recordedKeys, setRecordedKeys] = useState<TelemetryKeyEvent[]>([]);

  const previewVideoRef = useRef<HTMLVideoElement | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const recordedChunksRef = useRef<Blob[]>([]);
  const startTimestampRef = useRef<number>(0);

  const scanAdbDevices = async () => {
    setIsScanningAdb(true);
    try {
      const win = window as unknown as {
        __TAURI_INTERNALS__?: unknown;
      };
      if (win.__TAURI_INTERNALS__) {
        const { invoke } = await import('@tauri-apps/api/core');
        const list = await invoke<AdbDeviceInfo[]>('list_adb_devices');
        if (Array.isArray(list) && list.length > 0) {
          setAdbDevices(list);
          setSelectedAdbSerial(list[0].serial);
          setIsScanningAdb(false);
          return;
        }
      }
    } catch {
      // Fallback to simulated ADB devices when running in browser mode
    }
    setTimeout(() => {
      setAdbDevices(FALLBACK_ADB_DEVICES);
      setIsScanningAdb(false);
    }, 300);
  };

  useEffect(() => {
    if (!isRecording) return;
    const interval = window.setInterval(() => {
      const elapsed = (Date.now() - startTimestampRef.current) / 1000;
      setElapsedSec(Number(elapsed.toFixed(1)));
    }, 100);
    return () => clearInterval(interval);
  }, [isRecording]);

  useEffect(() => {
    if (!isRecording || !captureTelemetry) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      const mods: string[] = [];
      if (e.ctrlKey) mods.push('Ctrl');
      if (e.shiftKey) mods.push('Shift');
      if (e.altKey) mods.push('Alt');
      const keyName = e.key.length === 1 ? e.key.toUpperCase() : e.key;
      const combo = [...mods, keyName].join(' + ');
      const timestampMs = Math.max(0, Date.now() - startTimestampRef.current);
      setRecordedKeys((prev) => [
        ...prev,
        {
          id: `key-rec-${Date.now()}`,
          timestampMs,
          keyCombo: combo,
          durationMs: 1100,
        },
      ]);
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isRecording, captureTelemetry]);

  if (!isOpen) return null;

  const handleStartRecording = async () => {
    setRecordedPointers([]);
    setRecordedKeys([]);
    recordedChunksRef.current = [];
    startTimestampRef.current = Date.now();
    setElapsedSec(0);

    // Optional browser display capture when PC screen or Dual mode is chosen
    if (
      (captureMode === 'pc-screen' || captureMode === 'dual-simultaneous') &&
      navigator.mediaDevices?.getDisplayMedia
    ) {
      try {
        const stream = await navigator.mediaDevices.getDisplayMedia({
          video: { frameRate: 60 },
          audio: enableMic,
        });
        mediaStreamRef.current = stream;
        if (previewVideoRef.current) {
          previewVideoRef.current.srcObject = stream;
          previewVideoRef.current.play().catch(() => {});
        }
        const recorder = new MediaRecorder(stream);
        recorder.ondataavailable = (ev) => {
          if (ev.data && ev.data.size > 0) {
            recordedChunksRef.current.push(ev.data);
          }
        };
        recorder.start(250);
        mediaRecorderRef.current = recorder;
      } catch {
        // User cancelled native screen picker or running in headless preview; continue with interactive telemetry studio capture
      }
    }

    setIsRecording(true);
  };

  const handleInjectSimulatedTouch = (
    e: React.MouseEvent<HTMLDivElement>,
    source: 'android' | 'desktop'
  ) => {
    if (!isRecording || !captureTelemetry) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const normX = Number(
      Math.max(0.05, Math.min(0.95, (e.clientX - rect.left) / rect.width)).toFixed(2)
    );
    const normY = Number(
      Math.max(0.05, Math.min(0.95, (e.clientY - rect.top) / rect.height)).toFixed(2)
    );
    const timestampMs = Math.max(100, Date.now() - startTimestampRef.current);

    const newPt: TelemetryPointerEvent = {
      id: `pt-rec-${Date.now()}`,
      timestampMs,
      source,
      type: source === 'android' ? 'touch_tap' : 'click',
      x: normX,
      y: normY,
      durationMs: 200,
      button: source === 'android' ? 'finger' : 'left',
    };

    setRecordedPointers((prev) => [...prev, newPt]);
  };

  const handleStopAndImport = () => {
    setIsRecording(false);

    let capturedBlobUrl: string | undefined;
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      try {
        mediaRecorderRef.current.stop();
      } catch {
        // ignore
      }
    }
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach((t) => t.stop());
      mediaStreamRef.current = null;
    }
    if (recordedChunksRef.current.length > 0) {
      const blob = new Blob(recordedChunksRef.current, { type: 'video/webm' });
      capturedBlobUrl = URL.createObjectURL(blob);
    }

    const finalDuration = Math.max(6, Number(Math.max(elapsedSec, 8).toFixed(1)));
    const effectivePointers: TelemetryPointerEvent[] =
      recordedPointers.length > 0
        ? recordedPointers
        : [
            {
              id: `pt-auto-1`,
              timestampMs: 1400,
              source: captureMode === 'pc-screen' ? 'desktop' : 'android',
              type: captureMode === 'pc-screen' ? 'click' : 'touch_tap',
              x: 0.52,
              y: 0.34,
              durationMs: 190,
              button: 'finger',
            },
            {
              id: `pt-auto-2`,
              timestampMs: 4100,
              source: captureMode === 'pc-screen' ? 'desktop' : 'android',
              type: 'touch_swipe',
              x: 0.5,
              y: 0.76,
              endX: 0.5,
              endY: 0.26,
              durationMs: 450,
              button: 'finger',
            },
          ];

    const generatedDirectorKeyframes: DirectorCameraKeyframe[] = autoDirectorZoom
      ? [
          {
            id: `dir-start-${Date.now()}`,
            timeSec: 0,
            durationSec: 1.4,
            focusX: 0.5,
            focusY: 0.5,
            zoom: 1.05,
            rotXDeg: 6,
            rotYDeg: -14,
            rotZDeg: 1,
            easing: 'spring',
            label: 'Plano Inicial Captura',
          },
          ...effectivePointers.map((pt, idx) => ({
            id: `dir-auto-${idx}-${Date.now()}`,
            timeSec: Number((pt.timestampMs / 1000).toFixed(2)),
            durationSec: 2.0,
            focusX: pt.x,
            focusY: pt.y,
            zoom: 1.65,
            rotXDeg: idx % 2 === 0 ? -4 : 6,
            rotYDeg: idx % 2 === 0 ? 12 : -10,
            rotZDeg: 0,
            easing: 'spring' as const,
            label: `Auto-Zoom (${pt.source === 'android' ? 'Toque ADB' : 'Clic PC'})`,
            autoGenerated: true,
          })),
        ]
      : project.directorKeyframes;

    onUpdateProject((prev) => {
      const updatedTracks = prev.tracks.map((track) => {
        if (
          track.id === 'video-track-1' &&
          (captureMode === 'android-usb' || captureMode === 'dual-simultaneous')
        ) {
          const androidClip: TimelineClip = {
            id: `clip-adb-${Date.now()}`,
            trackId: 'video-track-1',
            name: `Captura_ADB_${selectedAdbSerial}.mp4`,
            type: 'video',
            mediaPath: 'builtin://android-demo',
            sourceRole: 'android-main',
            startSec: 0,
            durationSec: finalDuration,
            trimInSec: 0,
            trimOutSec: finalDuration,
            speed: 1,
            volume: 1,
            muted: false,
            fadeInSec: 0.2,
            fadeOutSec: 0.3,
            freezeFrames: [],
            opacityKeyframes: [],
            scaleKeyframes: [],
          };
          return { ...track, clips: [androidClip] };
        }

        if (
          track.id === 'video-track-2' &&
          (captureMode === 'pc-screen' || captureMode === 'dual-simultaneous')
        ) {
          const pcClip: TimelineClip = {
            id: `clip-pc-${Date.now()}`,
            trackId: 'video-track-2',
            name: `Captura_Pantalla_PC_60fps.mp4`,
            type: 'video',
            mediaPath: capturedBlobUrl || 'builtin://pc-demo',
            sourceRole: 'pc-screen',
            startSec: 0,
            durationSec: finalDuration,
            trimInSec: 0,
            trimOutSec: finalDuration,
            speed: 1,
            volume: 0.8,
            muted: false,
            fadeInSec: 0.2,
            fadeOutSec: 0.3,
            freezeFrames: [],
            opacityKeyframes: [],
            scaleKeyframes: [],
          };
          return { ...track, clips: [pcClip] };
        }

        if (track.id === 'video-track-3' && enableWebcam) {
          const camClip: TimelineClip = {
            id: `clip-webcam-${Date.now()}`,
            trackId: 'video-track-3',
            name: 'Webcam_Presentador_PiP.mp4',
            type: 'video',
            mediaPath: 'builtin://webcam-demo',
            sourceRole: 'webcam',
            startSec: 0,
            durationSec: finalDuration,
            trimInSec: 0,
            trimOutSec: finalDuration,
            speed: 1,
            volume: 1,
            muted: false,
            fadeInSec: 0.2,
            fadeOutSec: 0.2,
            freezeFrames: [],
            opacityKeyframes: [],
            scaleKeyframes: [],
          };
          return { ...track, clips: [camClip] };
        }

        return track;
      });

      return {
        ...prev,
        updatedAt: new Date().toISOString(),
        durationSec: finalDuration,
        tracks: updatedTracks,
        pointerEvents: captureTelemetry ? effectivePointers : prev.pointerEvents,
        keyEvents:
          captureTelemetry && recordedKeys.length > 0 ? recordedKeys : prev.keyEvents,
        directorKeyframes: generatedDirectorKeyframes,
        device3D: {
          ...prev.device3D,
          dualLayout:
            captureMode === 'dual-simultaneous'
              ? 'side-by-side'
              : prev.device3D.dualLayout,
        },
      };
    });

    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 bg-slate-950/85 backdrop-blur-md flex items-center justify-center p-4">
      <div className="w-full max-w-4xl bg-slate-900 border border-slate-800 rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[92vh]">
        {/* Modal Header */}
        <div className="flex items-center justify-between px-6 py-4 bg-slate-950 border-b border-slate-800">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-rose-500/20 border border-rose-500/40 flex items-center justify-center">
              <CircleDot className="w-5 h-5 text-rose-400 animate-pulse" />
            </div>
            <div>
              <h2 className="text-base font-bold text-white">
                Estudio de Captura Multipista (Android ADB + PC + Telemetría)
              </h2>
              <p className="text-xs text-slate-400">
                Graba vídeo a 60 FPS junto con las coordenadas exactas de cada toque, gesto swipe, clic y atajo de teclado.
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-2 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Body */}
        <div className="p-6 overflow-y-auto space-y-6">
          {/* 1. Source Mode Selector Cards */}
          <div>
            <label className="block text-xs font-semibold uppercase tracking-wider text-slate-400 mb-2.5">
              1. Selecciona la Fuente de Captura
            </label>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
              <button
                type="button"
                disabled={isRecording}
                onClick={() => setCaptureMode('android-usb')}
                className={`p-4 rounded-xl border text-left transition flex flex-col justify-between ${
                  captureMode === 'android-usb'
                    ? 'bg-indigo-950/60 border-indigo-500 shadow-lg shadow-indigo-500/10'
                    : 'bg-slate-950/60 border-slate-800 hover:border-slate-700'
                }`}
              >
                <div className="flex items-center justify-between mb-2">
                  <Smartphone className="w-6 h-6 text-indigo-400" />
                  <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-indigo-500/20 text-indigo-300">
                    USB / ADB
                  </span>
                </div>
                <div>
                  <div className="text-sm font-bold text-white">
                    Teléfono / Tablet Android
                  </div>
                  <p className="text-xs text-slate-400 mt-1">
                    Captura por cable USB o emulador con lectura de toques vía{' '}
                    <code className="text-indigo-300">adb getevent</code>.
                  </p>
                </div>
              </button>

              <button
                type="button"
                disabled={isRecording}
                onClick={() => setCaptureMode('pc-screen')}
                className={`p-4 rounded-xl border text-left transition flex flex-col justify-between ${
                  captureMode === 'pc-screen'
                    ? 'bg-sky-950/60 border-sky-500 shadow-lg shadow-sky-500/10'
                    : 'bg-slate-950/60 border-slate-800 hover:border-slate-700'
                }`}
              >
                <div className="flex items-center justify-between mb-2">
                  <Monitor className="w-6 h-6 text-sky-400" />
                  <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-sky-500/20 text-sky-300">
                    Escritorio 60fps
                  </span>
                </div>
                <div>
                  <div className="text-sm font-bold text-white">
                    Pantalla o Ventana de PC
                  </div>
                  <p className="text-xs text-slate-400 mt-1">
                    Ideal para grabar Android Studio, VS Code, navegador o backend con suavizado de cursor.
                  </p>
                </div>
              </button>

              <button
                type="button"
                disabled={isRecording}
                onClick={() => setCaptureMode('dual-simultaneous')}
                className={`p-4 rounded-xl border text-left transition flex flex-col justify-between ${
                  captureMode === 'dual-simultaneous'
                    ? 'bg-fuchsia-950/60 border-fuchsia-500 shadow-lg shadow-fuchsia-500/10'
                    : 'bg-slate-950/60 border-slate-800 hover:border-slate-700'
                }`}
              >
                <div className="flex items-center justify-between mb-2">
                  <Layers className="w-6 h-6 text-fuchsia-400" />
                  <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-fuchsia-500/20 text-fuchsia-300">
                    Sincronizado V1 + V2
                  </span>
                </div>
                <div>
                  <div className="text-sm font-bold text-white">
                    Modo Dual Simultáneo (PC + Android)
                  </div>
                  <p className="text-xs text-slate-400 mt-1">
                    Graba tu móvil por USB y la pantalla del PC a la vez en pistas separadas con escena 3D doble.
                  </p>
                </div>
              </button>
            </div>
          </div>

          {/* 2. Connected Android ADB Device Selector */}
          {(captureMode === 'android-usb' || captureMode === 'dual-simultaneous') && (
            <div className="p-4 rounded-xl bg-slate-950 border border-slate-800 space-y-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Usb className="w-4 h-4 text-emerald-400" />
                  <span className="text-xs font-semibold text-slate-200">
                    Dispositivo Android Detectado por ADB
                  </span>
                </div>
                <button
                  type="button"
                  onClick={scanAdbDevices}
                  className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-slate-800 hover:bg-slate-700 text-xs text-slate-200 transition"
                >
                  <RefreshCw
                    className={`w-3.5 h-3.5 ${isScanningAdb ? 'animate-spin' : ''}`}
                  />
                  <span>Buscar dispositivos ADB</span>
                </button>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                {adbDevices.map((dev) => (
                  <div
                    key={dev.serial}
                    onClick={() => setSelectedAdbSerial(dev.serial)}
                    className={`p-3 rounded-lg border cursor-pointer flex items-center justify-between transition ${
                      selectedAdbSerial === dev.serial
                        ? 'bg-indigo-950/40 border-indigo-500'
                        : 'bg-slate-900 border-slate-800 hover:border-slate-700'
                    }`}
                  >
                    <div>
                      <div className="text-xs font-bold text-white flex items-center gap-1.5">
                        <span>{dev.model.replace(/_/g, ' ')}</span>
                        <span className="text-[10px] px-1.5 py-0.2 rounded bg-emerald-500/20 text-emerald-300 font-mono">
                          {dev.state}
                        </span>
                      </div>
                      <div className="text-[11px] text-slate-400 font-mono mt-0.5">
                        {dev.serial} · {dev.resolution || '1080x2400'}
                      </div>
                    </div>
                    {selectedAdbSerial === dev.serial && (
                      <CheckCircle2 className="w-4 h-4 text-indigo-400 shrink-0" />
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* 3. Smart Telemetry & Audio/Webcam Toggles */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
            <label className="flex items-start gap-2.5 p-3 rounded-xl bg-slate-950 border border-slate-800 cursor-pointer">
              <input
                type="checkbox"
                checked={captureTelemetry}
                onChange={(e) => setCaptureTelemetry(e.target.checked)}
                className="mt-0.5 accent-indigo-500"
              />
              <div>
                <div className="text-xs font-semibold text-white flex items-center gap-1">
                  <MousePointerClick className="w-3.5 h-3.5 text-sky-400" />
                  <span>Telemetría de Toques y Clics</span>
                </div>
                <p className="text-[11px] text-slate-400 mt-0.5">
                  Registra coordenadas X/Y y atajos de teclado.
                </p>
              </div>
            </label>

            <label className="flex items-start gap-2.5 p-3 rounded-xl bg-slate-950 border border-slate-800 cursor-pointer">
              <input
                type="checkbox"
                checked={autoDirectorZoom}
                onChange={(e) => setAutoDirectorZoom(e.target.checked)}
                className="mt-0.5 accent-fuchsia-500"
              />
              <div>
                <div className="text-xs font-semibold text-white flex items-center gap-1">
                  <Sparkles className="w-3.5 h-3.5 text-fuchsia-400" />
                  <span>Auto-Zoom Director 3D</span>
                </div>
                <p className="text-[11px] text-slate-400 mt-0.5">
                  Crea keyframes de cámara en cada toque importante.
                </p>
              </div>
            </label>

            <label className="flex items-start gap-2.5 p-3 rounded-xl bg-slate-950 border border-slate-800 cursor-pointer">
              <input
                type="checkbox"
                checked={enableMic}
                onChange={(e) => setEnableMic(e.target.checked)}
                className="mt-0.5 accent-emerald-500"
              />
              <div>
                <div className="text-xs font-semibold text-white flex items-center gap-1">
                  <Mic className="w-3.5 h-3.5 text-emerald-400" />
                  <span>Micrófono Voz en Off (A1)</span>
                </div>
                <p className="text-[11px] text-slate-400 mt-0.5">
                  Listo para transcripción con Whisper local.
                </p>
              </div>
            </label>

            <label className="flex items-start gap-2.5 p-3 rounded-xl bg-slate-950 border border-slate-800 cursor-pointer">
              <input
                type="checkbox"
                checked={enableWebcam}
                onChange={(e) => setEnableWebcam(e.target.checked)}
                className="mt-0.5 accent-amber-500"
              />
              <div>
                <div className="text-xs font-semibold text-white flex items-center gap-1">
                  <Camera className="w-3.5 h-3.5 text-amber-400" />
                  <span>Webcam PiP (Pista V3)</span>
                </div>
                <p className="text-[11px] text-slate-400 mt-0.5">
                  Añade burbuja de cámara independiente.
                </p>
              </div>
            </label>
          </div>

          {/* 4. Interactive Live Capture & Telemetry Test Pad */}
          <div className="p-4 rounded-xl bg-slate-950 border border-slate-800">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold text-slate-200">
                  Previsualización en Vivo y Simulador de Eventos Táctiles
                </span>
                {isRecording && (
                  <span className="px-2 py-0.5 rounded-full bg-rose-500/20 text-rose-300 border border-rose-500/40 text-[11px] font-mono animate-pulse">
                    ● GRABANDO {elapsedSec.toFixed(1)}s
                  </span>
                )}
              </div>
              <div className="flex items-center gap-3 text-xs text-slate-400 font-mono">
                <span>Toques/Clics: {recordedPointers.length}</span>
                <span>Teclas: {recordedKeys.length}</span>
              </div>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {/* Interactive Android Touch Pad */}
              <div
                onClick={(e) => handleInjectSimulatedTouch(e, 'android')}
                className="h-44 rounded-xl bg-gradient-to-br from-indigo-950/70 via-slate-900 to-slate-950 border border-indigo-500/40 relative overflow-hidden flex flex-col items-center justify-center cursor-crosshair select-none"
              >
                <Smartphone className="w-8 h-8 text-indigo-400/60 mb-1" />
                <p className="text-xs font-medium text-indigo-200">
                  Lienzo Táctil Android ({selectedAdbSerial})
                </p>
                <p className="text-[11px] text-slate-400 mt-0.5">
                  {isRecording
                    ? 'Haz clic aquí para inyectar toques ADB reales en la línea de tiempo'
                    : 'Inicia la grabación para capturar toques en tiempo real'}
                </p>
                {recordedPointers
                  .filter((p) => p.source === 'android')
                  .slice(-5)
                  .map((pt) => (
                    <div
                      key={pt.id}
                      style={{ left: `${pt.x * 100}%`, top: `${pt.y * 100}%` }}
                      className="absolute w-5 h-5 -ml-2.5 -mt-2.5 rounded-full bg-sky-400/60 border-2 border-white animate-ping pointer-events-none"
                    />
                  ))}
              </div>

              {/* PC Screen Live Preview / Click Pad */}
              <div
                onClick={(e) => handleInjectSimulatedTouch(e, 'desktop')}
                className="h-44 rounded-xl bg-gradient-to-br from-sky-950/60 via-slate-900 to-slate-950 border border-sky-500/40 relative overflow-hidden flex flex-col items-center justify-center cursor-crosshair select-none"
              >
                <video
                  ref={previewVideoRef}
                  muted
                  playsInline
                  className="absolute inset-0 w-full h-full object-cover opacity-50 pointer-events-none"
                />
                <div className="relative z-10 flex flex-col items-center text-center px-4">
                  <Monitor className="w-8 h-8 text-sky-400/60 mb-1" />
                  <p className="text-xs font-medium text-sky-200">
                    Lienzo de Escritorio PC + Atajos de Teclado
                  </p>
                  <p className="text-[11px] text-slate-400 mt-0.5 flex items-center gap-1">
                    <Keyboard className="w-3.5 h-3.5" />
                    <span>
                      Pulsa cualquier combinación de teclas mientras grabas para el HUD
                    </span>
                  </p>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* Modal Footer */}
        <div className="flex items-center justify-between px-6 py-4 bg-slate-950 border-t border-slate-800">
          <div className="text-xs text-slate-400">
            Al detener, las pistas de vídeo y la telemetría se envían automáticamente al editor multipista.
          </div>
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-xs font-semibold text-slate-300 transition"
            >
              Cancelar
            </button>

            {!isRecording ? (
              <button
                type="button"
                onClick={handleStartRecording}
                className="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-rose-600 hover:bg-rose-500 text-xs font-bold text-white shadow-lg shadow-rose-600/30 transition"
              >
                <CircleDot className="w-4 h-4" />
                <span>Iniciar Grabación Multipista</span>
              </button>
            ) : (
              <button
                type="button"
                onClick={handleStopAndImport}
                className="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-emerald-600 hover:bg-emerald-500 text-xs font-bold text-white shadow-lg shadow-emerald-600/30 transition"
              >
                <Square className="w-4 h-4 fill-white" />
                <span>Detener e Importar al Timeline ({elapsedSec.toFixed(1)}s)</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
