import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  Camera,
  CircleDot,
  Cpu,
  Download,
  Film,
  FolderOpen,
  Layers,
  MonitorSmartphone,
  Play,
  Pause,
  RefreshCw,
  RotateCcw,
  Save,
  ShieldCheck,
  Smartphone,
  Sparkles,
  Tablet,
  Wand2,
  X,
} from 'lucide-react';
import {
  DeviceModelType,
  DirectorCameraKeyframe,
  ProjectFile,
} from './types/project';
import { createDefaultProject } from './utils/defaultProject';
import { Viewport3DCanvas } from './components/Viewport3DCanvas';
import { MultiTrackTimeline } from './components/MultiTrackTimeline';
import { RightInspectorPanel } from './components/RightInspectorPanel';
import { RecordingModal } from './components/RecordingModal';
import {
  AppUpdateInfo,
  checkAppUpdates,
  exportWithFfmpeg,
  installAppUpdate,
  loadProjectFromDisk,
  saveProjectToDisk,
} from './services/backendBridge';

export const App: React.FC = () => {
  const [project, setProject] = useState<ProjectFile>(() => createDefaultProject());
  const [currentTimeSec, setCurrentTimeSec] = useState<number>(1.8);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [selectedClipId, setSelectedClipId] = useState<string | null>('clip-demo-android');
  const [isRecordingModalOpen, setIsRecordingModalOpen] = useState<boolean>(false);
  const [isExporting, setIsExporting] = useState<boolean>(false);
  const [exportProgress, setExportProgress] = useState<number>(0);
  const [exportLog, setExportLog] = useState<string>('');
  const [statusToast, setStatusToast] = useState<string | null>(null);
  const [isUpdateModalOpen, setIsUpdateModalOpen] = useState<boolean>(false);
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [isCheckingUpdate, setIsCheckingUpdate] = useState<boolean>(false);
  const [isInstallingUpdate, setIsInstallingUpdate] = useState<boolean>(false);
  const [updateInstallLog, setUpdateInstallLog] = useState<string>('');
  const [autoUpdateOnStart, setAutoUpdateOnStart] = useState<boolean>(() => {
    try {
      return localStorage.getItem('rec_ording_auto_update') !== 'false';
    } catch {
      return true;
    }
  });

  const canvasRefOut = useRef<HTMLCanvasElement | null>(null);
  const rafRef = useRef<number | null>(null);
  const lastFrameTimeRef = useRef<number>(performance.now());

  const showToast = useCallback((msg: string) => {
    setStatusToast(msg);
    setTimeout(() => {
      setStatusToast((prev) => (prev === msg ? null : prev));
    }, 3800);
  }, []);

  const handleUpdateProject = useCallback(
    (updater: (prev: ProjectFile) => ProjectFile) => {
      setProject((prev) => {
        const next = updater(prev);
        return {
          ...next,
          updatedAt: new Date().toISOString(),
        };
      });
    },
    []
  );

  // Playback loop
  useEffect(() => {
    if (!isPlaying) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      return;
    }

    lastFrameTimeRef.current = performance.now();

    const tick = (now: number) => {
      const dt = (now - lastFrameTimeRef.current) / 1000;
      lastFrameTimeRef.current = now;

      setCurrentTimeSec((prev) => {
        const next = prev + dt;
        if (next >= project.durationSec) {
          return 0;
        }
        return next;
      });

      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, [isPlaying, project.durationSec]);

  // Initial load from disk if active-project.recproj.json exists
  useEffect(() => {
    loadProjectFromDisk()
      .then((loaded) => {
        if (loaded && loaded.id) {
          setProject(loaded);
          showToast('Proyecto sincronizado con el servidor MCP local');
        } else {
          saveProjectToDisk(project).catch(() => {});
        }
      })
      .catch(() => {});

    // Check updates on startup if autoUpdateOnStart is enabled
    if (autoUpdateOnStart) {
      checkAppUpdates()
        .then((info) => {
          setUpdateInfo(info);
          if (info.updateAvailable) {
            showToast(`Nueva versión v${info.latestVersion} disponible en GitHub (${info.repo})`);
          }
        })
        .catch(() => {});
    }
  }, []);

  const handleCheckUpdatesNow = async () => {
    setIsCheckingUpdate(true);
    try {
      const info = await checkAppUpdates();
      setUpdateInfo(info);
      if (info.updateAvailable) {
        showToast(`Actualización v${info.latestVersion} disponible`);
      } else {
        showToast(`rec-ording v${info.currentVersion} está actualizado`);
      }
    } finally {
      setIsCheckingUpdate(false);
    }
  };

  const handleInstallUpdateNow = async () => {
    setIsInstallingUpdate(true);
    setUpdateInstallLog('Descargando paquete .pkg.tar.zst desde el repositorio privado blzkz/rec-ording...');
    try {
      const res = await installAppUpdate();
      setUpdateInstallLog(res.log);
      if (res.success) {
        showToast('Actualización instalada correctamente en CachyOS / Arch Linux');
      } else {
        showToast('Paquete descargado — revisa el registro de actualización');
      }
    } finally {
      setIsInstallingUpdate(false);
    }
  };

  const handleToggleAutoUpdate = (checked: boolean) => {
    setAutoUpdateOnStart(checked);
    try {
      localStorage.setItem('rec_ording_auto_update', String(checked));
    } catch {
      // Ignore
    }
  };

  // Keyboard shortcuts (Space = Play/Pause)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (e.code === 'Space') {
        e.preventDefault();
        setIsPlaying((p) => !p);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const handleQuickModelSelect = (model: DeviceModelType) => {
    handleUpdateProject((prev) => ({
      ...prev,
      device3D: {
        ...prev.device3D,
        primaryModel: model,
        orientation: model === 'galaxy-tab-s10-ultra' ? 'landscape' : prev.device3D.orientation,
      },
    }));
    showToast(`Maqueta 3D cambiada a ${model}`);
  };

  const handleAutoDirectorFromTouches = () => {
    handleUpdateProject((prev) => {
      const taps = prev.pointerEvents.filter(
        (ev) => ev.type === 'touch_tap' || ev.type === 'click' || ev.type === 'touch_swipe'
      );
      if (taps.length === 0) return prev;

      const generated: DirectorCameraKeyframe[] = [
        {
          id: `dir-auto-intro-${Date.now()}`,
          timeSec: 0,
          durationSec: 1.5,
          focusX: 0.5,
          focusY: 0.5,
          zoom: 1.02,
          rotXDeg: 6,
          rotYDeg: -14,
          rotZDeg: 1,
          easing: 'spring',
          label: 'Intro 3D',
          autoGenerated: true,
        },
        ...taps.map((tap, idx) => ({
          id: `dir-auto-${idx}-${Date.now()}`,
          timeSec: Math.max(0.2, tap.timestampMs / 1000 - 0.25),
          durationSec: 1.6,
          focusX: tap.x,
          focusY: tap.y,
          zoom: tap.type === 'touch_swipe' ? 1.35 : 1.68,
          rotXDeg: (0.5 - tap.y) * 16,
          rotYDeg: (tap.x - 0.5) * 24,
          rotZDeg: idx % 2 === 0 ? 1.5 : -1.5,
          easing: 'spring' as const,
          label: `Auto-Zoom #${idx + 1} (${tap.type === 'touch_swipe' ? 'Gesto' : 'Toque'})`,
          autoGenerated: true,
        })),
        {
          id: `dir-auto-outro-${Date.now()}`,
          timeSec: Math.max(prev.durationSec - 2.0, 1.0),
          durationSec: 2.0,
          focusX: 0.5,
          focusY: 0.5,
          zoom: 1.0,
          rotXDeg: 3,
          rotYDeg: 0,
          rotZDeg: 0,
          easing: 'spring',
          label: 'Cierre Completo',
          autoGenerated: true,
        },
      ];

      return {
        ...prev,
        directorKeyframes: generated,
      };
    });
    showToast('Modo Director: Fotogramas clave de cámara y zoom generados desde toques');
  };

  const handleSaveProjectSync = async () => {
    try {
      const savedPath = await saveProjectToDisk(project);
      showToast(`Proyecto guardado en ${savedPath} (listo para MCP)`);
    } catch (err) {
      showToast('Proyecto guardado en almacenamiento local');
    }
  };

  const handleReloadFromMcp = async () => {
    const loaded = await loadProjectFromDisk();
    if (loaded) {
      setProject(loaded);
      showToast('Proyecto recargado con los cambios del Agente MCP');
    } else {
      showToast('No se encontraron cambios externos en disco');
    }
  };

  const handleTriggerExport = async () => {
    if (isExporting) return;
    setIsExporting(true);
    setExportProgress(10);
    setExportLog('Preparando motor de composición 3D y pipeline FFmpeg...');

    try {
      const frames: string[] = [];
      if (canvasRefOut.current) {
        try {
          frames.push(canvasRefOut.current.toDataURL('image/png'));
        } catch {
          // Ignore canvas taint if any
        }
      }

      setExportProgress(45);
      setExportLog(
        `Ejecutando FFmpeg (${project.exportSettings.width}×${project.exportSettings.height} @ ${project.exportSettings.fps}fps · ${project.exportSettings.codec.toUpperCase()})...`
      );

      const result = await exportWithFfmpeg(project, frames);
      setExportProgress(100);
      setExportLog(`${result.log}\n\nComando FFmpeg:\n${result.commandUsed}`);
      showToast(`Exportación completada: ${result.outputPath}`);
    } catch (err: any) {
      setExportLog(`Error durante la exportación: ${err?.message || String(err)}`);
      showToast('Error al exportar con FFmpeg');
    } finally {
      setTimeout(() => {
        setIsExporting(false);
      }, 600);
    }
  };

  return (
    <div className="flex flex-col h-screen w-screen bg-slate-950 text-slate-100 overflow-hidden select-none">
      {/* Top Studio Header */}
      <header className="h-13 shrink-0 bg-slate-900/95 border-b border-slate-800 px-4 flex items-center justify-between gap-3 z-20">
        {/* Left: Brand & Project Title */}
        <div className="flex items-center gap-3 min-w-0">
          <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-indigo-500 via-purple-500 to-emerald-400 flex items-center justify-center shadow-md shadow-indigo-500/20">
            <Film className="w-4 h-4 text-white" />
          </div>
          <div className="flex flex-col">
            <div className="flex items-center gap-2">
              <span className="font-bold text-sm tracking-tight bg-gradient-to-r from-white via-indigo-200 to-emerald-300 bg-clip-text text-transparent">
                rec-ording
              </span>
              <span className="text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded bg-indigo-500/20 text-indigo-300 border border-indigo-500/30">
                FFmpeg + ADB + MCP
              </span>
            </div>
            <input
              type="text"
              value={project.name}
              onChange={(e) =>
                handleUpdateProject((prev) => ({ ...prev, name: e.target.value }))
              }
              className="text-xs text-slate-400 hover:text-slate-200 focus:text-white bg-transparent border-none outline-none w-56 truncate"
              title="Nombre del proyecto (.recproj.json)"
            />
          </div>
        </div>

        {/* Center: Quick 3D Device & Capture Bar */}
        <div className="flex items-center gap-1.5 bg-slate-950/90 border border-slate-800 rounded-xl p-1">
          <button
            onClick={() => setIsRecordingModalOpen(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-rose-600 hover:bg-rose-500 text-white font-semibold text-xs shadow-sm shadow-rose-600/30 transition cursor-pointer"
          >
            <CircleDot className="w-3.5 h-3.5 animate-pulse" />
            <span>Grabar USB / PC</span>
          </button>

          <div className="h-4 w-px bg-slate-800 mx-1" />

          <button
            onClick={() => handleQuickModelSelect('pixel-11-pro')}
            className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer ${
              project.device3D.primaryModel === 'pixel-11-pro'
                ? 'bg-indigo-600 text-white'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/70'
            }`}
          >
            <Smartphone className="w-3.5 h-3.5" />
            <span>Pixel 11 Pro</span>
          </button>

          <button
            onClick={() => handleQuickModelSelect('galaxy-s26-ultra')}
            className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer ${
              project.device3D.primaryModel === 'galaxy-s26-ultra'
                ? 'bg-indigo-600 text-white'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/70'
            }`}
          >
            <Smartphone className="w-3.5 h-3.5" />
            <span>Galaxy S26 Ultra</span>
          </button>

          <button
            onClick={() => handleQuickModelSelect('galaxy-tab-s10-ultra')}
            className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer ${
              project.device3D.primaryModel === 'galaxy-tab-s10-ultra'
                ? 'bg-indigo-600 text-white'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/70'
            }`}
          >
            <Tablet className="w-3.5 h-3.5" />
            <span>Galaxy Tab Ultra</span>
          </button>

          <button
            onClick={() =>
              handleUpdateProject((prev) => ({
                ...prev,
                device3D: {
                  ...prev.device3D,
                  dualLayout:
                    prev.device3D.dualLayout === 'single' ? 'side-by-side' : 'single',
                },
              }))
            }
            className={`flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer ${
              project.device3D.dualLayout !== 'single'
                ? 'bg-emerald-600 text-white'
                : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/70'
            }`}
            title="Mostrar PC y Android a la vez en el escenario 3D"
          >
            <MonitorSmartphone className="w-3.5 h-3.5" />
            <span>Dual PC + Android</span>
          </button>

          <div className="h-4 w-px bg-slate-800 mx-1" />

          <button
            onClick={handleAutoDirectorFromTouches}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-amber-500/15 hover:bg-amber-500/25 text-amber-300 border border-amber-500/30 text-xs font-medium transition cursor-pointer"
            title="Planificar movimientos de cámara 3D y zoom automático a partir de los toques registrados"
          >
            <Wand2 className="w-3.5 h-3.5" />
            <span>Director Auto-Zoom</span>
          </button>
        </div>

        {/* Right: Auto-Update, MCP Sync, Save & Export */}
        <div className="flex items-center gap-2">
          <button
            onClick={() => {
              setIsUpdateModalOpen(true);
              if (!updateInfo) handleCheckUpdatesNow();
            }}
            className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer border ${
              updateInfo?.updateAvailable
                ? 'bg-amber-500/20 border-amber-500/50 text-amber-200 hover:bg-amber-500/30'
                : 'bg-slate-800/90 border-slate-700/80 hover:bg-slate-700 text-slate-200'
            }`}
            title="Auto-actualización desde repositorio privado GitHub (blzkz/rec-ording)"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isCheckingUpdate ? 'animate-spin text-indigo-400' : updateInfo?.updateAvailable ? 'text-amber-400' : 'text-emerald-400'}`} />
            <span>{updateInfo?.updateAvailable ? `Actualizar v${updateInfo.latestVersion}` : `v${updateInfo?.currentVersion || '0.1.0'}`}</span>
          </button>

          <button
            onClick={handleReloadFromMcp}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-medium transition cursor-pointer"
            title="Recargar cambios realizados por agentes de IA (Claude, Cursor, Codex) vía servidor MCP"
          >
            <Cpu className="w-3.5 h-3.5 text-cyan-400" />
            <span>Sync MCP</span>
          </button>

          <button
            onClick={handleSaveProjectSync}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-medium transition cursor-pointer"
            title="Guardar proyecto editable (.recproj.json)"
          >
            <Save className="w-3.5 h-3.5 text-indigo-400" />
            <span>Guardar</span>
          </button>

          <button
            onClick={handleTriggerExport}
            disabled={isExporting}
            className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg bg-gradient-to-r from-indigo-600 to-emerald-600 hover:from-indigo-500 hover:to-emerald-500 disabled:opacity-50 text-white font-semibold text-xs shadow-md shadow-indigo-600/20 transition cursor-pointer"
          >
            <Download className="w-3.5 h-3.5" />
            <span>
              {isExporting
                ? `Exportando (${exportProgress}%)`
                : `Exportar ${project.exportSettings.width === 3840 ? '4K' : 'HD'} · ${
                    project.exportSettings.fps
                  }fps`}
            </span>
          </button>
        </div>
      </header>

      {/* Toast Notification */}
      {statusToast && (
        <div className="fixed top-15 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-full bg-indigo-950/95 border border-indigo-500/40 text-indigo-100 text-xs font-medium shadow-xl flex items-center gap-2 animate-fade-in">
          <Sparkles className="w-3.5 h-3.5 text-indigo-400" />
          <span>{statusToast}</span>
        </div>
      )}

      {/* Main Workspace: 3D Stage + Right Inspector */}
      <div className="flex-1 flex min-h-0 overflow-hidden">
        {/* Center 3D & Compositor Stage */}
        <div className="flex-1 flex flex-col min-w-0 bg-slate-950 relative">
          <Viewport3DCanvas
            project={project}
            currentTimeSec={currentTimeSec}
            isPlaying={isPlaying}
            onTimeUpdate={setCurrentTimeSec}
            onUpdateProject={handleUpdateProject}
            canvasRefOut={canvasRefOut}
          />
        </div>

        {/* Right Inspector Panel (3D, Director, Subtitles, MCP, Export) */}
        <div className="w-96 shrink-0 border-l border-slate-800 bg-slate-900/95 flex flex-col min-h-0">
          <RightInspectorPanel
            project={project}
            currentTimeSec={currentTimeSec}
            selectedClipId={selectedClipId}
            onSeek={(t) => {
              setIsPlaying(false);
              setCurrentTimeSec(t);
            }}
            onUpdateProject={handleUpdateProject}
            onTriggerExport={handleTriggerExport}
            isExporting={isExporting}
            exportProgress={exportProgress}
            exportLog={exportLog}
          />
        </div>
      </div>

      {/* Bottom Multi-Track Timeline (4 Video, 4 Audio, 5 Text + Director + Masks) */}
      <div className="h-72 shrink-0 border-t border-slate-800 bg-slate-900 flex flex-col">
        <MultiTrackTimeline
          project={project}
          currentTimeSec={currentTimeSec}
          isPlaying={isPlaying}
          selectedClipId={selectedClipId}
          onSelectClip={setSelectedClipId}
          onSeek={(t) => setCurrentTimeSec(t)}
          onTogglePlay={() => setIsPlaying((p) => !p)}
          onUpdateProject={handleUpdateProject}
        />
      </div>

      {/* Recording Studio Modal (Desktop + ADB USB + Dual Mode + Telemetry) */}
      <RecordingModal
        isOpen={isRecordingModalOpen}
        onClose={() => setIsRecordingModalOpen(false)}
        project={project}
        onUpdateProject={handleUpdateProject}
      />

      {/* Auto-Update Modal (GitHub Private Repo blzkz/rec-ording · CachyOS / Arch Linux) */}
      {isUpdateModalOpen && (
        <div className="fixed inset-0 z-50 bg-slate-950/80 backdrop-blur-sm flex items-center justify-center p-4">
          <div className="w-full max-w-lg rounded-2xl bg-slate-900 border border-slate-800 shadow-2xl overflow-hidden">
            <div className="px-5 py-4 border-b border-slate-800 flex items-center justify-between">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-lg bg-emerald-500/15 border border-emerald-500/30 flex items-center justify-center">
                  <ShieldCheck className="w-4 h-4 text-emerald-400" />
                </div>
                <div>
                  <h3 className="text-sm font-bold text-white">
                    Auto-Actualización · CachyOS / Arch Linux
                  </h3>
                  <p className="text-[11px] text-slate-400">
                    Repositorio privado: <code className="text-indigo-300">github.com/blzkz/rec-ording</code>
                  </p>
                </div>
              </div>
              <button
                onClick={() => setIsUpdateModalOpen(false)}
                className="p-1.5 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800 transition cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-5 space-y-4">
              <div className="grid grid-cols-2 gap-3">
                <div className="p-3 rounded-xl bg-slate-950 border border-slate-800">
                  <div className="text-[11px] text-slate-400">Versión instalada</div>
                  <div className="text-base font-bold text-white mt-0.5">
                    v{updateInfo?.currentVersion || '0.1.0'}
                  </div>
                  <div className="text-[10px] text-emerald-400 mt-1">
                    Paquete nativo x86_64 (.pkg.tar.zst)
                  </div>
                </div>
                <div className="p-3 rounded-xl bg-slate-950 border border-slate-800">
                  <div className="text-[11px] text-slate-400">Última release en GitHub</div>
                  <div className="text-base font-bold text-indigo-300 mt-0.5">
                    v{updateInfo?.latestVersion || '0.1.0'}
                  </div>
                  <div className="text-[10px] text-slate-400 mt-1 truncate">
                    {updateInfo?.assetName || 'rec-ording-0.1.0-1-x86_64.pkg.tar.zst'}
                  </div>
                </div>
              </div>

              <label className="flex items-center justify-between p-3 rounded-xl bg-slate-950/70 border border-slate-800 cursor-pointer">
                <div>
                  <div className="text-xs font-semibold text-slate-200">
                    Buscar actualizaciones automáticamente al iniciar
                  </div>
                  <div className="text-[11px] text-slate-400">
                    Consulta nuevas releases en tu repositorio privado de GitHub vía <code className="text-indigo-300">gh release</code>
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={autoUpdateOnStart}
                  onChange={(e) => handleToggleAutoUpdate(e.target.checked)}
                  className="w-4 h-4 accent-indigo-500 rounded cursor-pointer"
                />
              </label>

              {updateInfo && (
                <div className="p-3 rounded-xl bg-slate-950 border border-slate-800 space-y-1">
                  <div className="text-xs font-semibold text-slate-200">
                    {updateInfo.releaseTitle}
                  </div>
                  <p className="text-xs text-slate-400 whitespace-pre-wrap leading-relaxed">
                    {updateInfo.releaseNotes}
                  </p>
                </div>
              )}

              {updateInstallLog && (
                <pre className="p-3 rounded-xl bg-slate-950 border border-slate-800 text-[11px] text-emerald-300 font-mono whitespace-pre-wrap max-h-36 overflow-y-auto">
                  {updateInstallLog}
                </pre>
              )}

              <div className="flex items-center justify-between gap-2 pt-1">
                <button
                  onClick={handleCheckUpdatesNow}
                  disabled={isCheckingUpdate}
                  className="flex items-center gap-1.5 px-3.5 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 disabled:opacity-50 text-slate-200 text-xs font-medium transition cursor-pointer"
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${isCheckingUpdate ? 'animate-spin' : ''}`} />
                  <span>{isCheckingUpdate ? 'Comprobando...' : 'Comprobar ahora'}</span>
                </button>

                <button
                  onClick={handleInstallUpdateNow}
                  disabled={isInstallingUpdate}
                  className="flex items-center gap-1.5 px-4 py-2 rounded-xl bg-gradient-to-r from-indigo-600 to-emerald-600 hover:from-indigo-500 hover:to-emerald-500 disabled:opacity-50 text-white text-xs font-semibold shadow-md transition cursor-pointer"
                >
                  <Download className="w-3.5 h-3.5" />
                  <span>
                    {isInstallingUpdate
                      ? 'Descargando e instalando...'
                      : 'Descargar e Instalar (.pkg.tar.zst)'}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default App;
