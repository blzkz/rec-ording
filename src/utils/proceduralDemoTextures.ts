import * as THREE from 'three';
import { TimelineClip } from '../types/project';

export interface ManagedSourceTexture {
  texture: THREE.Texture;
  update: (timelineTimeSec: number, clip: TimelineClip | null, isPlaying: boolean) => void;
  dispose: () => void;
  getCanvas: () => HTMLCanvasElement | HTMLVideoElement | null;
}

function roundRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number
) {
  const radius = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}

export function computeClipSourceTime(
  clip: TimelineClip,
  timelineTimeSec: number
): { sourceTimeSec: number; opacity: number } {
  const localElapsed = Math.max(0, timelineTimeSec - clip.startSec);

  // Check if inside a freeze frame segment
  if (clip.freezeFrames && clip.freezeFrames.length > 0) {
    for (const ff of clip.freezeFrames) {
      if (
        timelineTimeSec >= ff.timelineStartSec &&
        timelineTimeSec <= ff.timelineStartSec + ff.durationSec
      ) {
        return {
          sourceTimeSec: ff.sourceTimeSec,
          opacity: computeClipOpacity(clip, timelineTimeSec),
        };
      }
    }
  }

  const speed = clip.speed || 1.0;
  const rawSourceTime = clip.trimInSec + localElapsed * speed;
  const maxOut = clip.trimOutSec > clip.trimInSec ? clip.trimOutSec : clip.trimInSec + clip.durationSec;
  const sourceTimeSec = Math.min(maxOut, Math.max(clip.trimInSec, rawSourceTime));

  return {
    sourceTimeSec,
    opacity: computeClipOpacity(clip, timelineTimeSec),
  };
}

function computeClipOpacity(clip: TimelineClip, timelineTimeSec: number): number {
  const rel = timelineTimeSec - clip.startSec;
  if (rel < 0 || rel > clip.durationSec) return 0;

  let alpha = 1.0;
  if (clip.fadeInSec > 0 && rel < clip.fadeInSec) {
    alpha *= Math.max(0, Math.min(1, rel / clip.fadeInSec));
  }
  const timeFromEnd = clip.durationSec - rel;
  if (clip.fadeOutSec > 0 && timeFromEnd < clip.fadeOutSec) {
    alpha *= Math.max(0, Math.min(1, timeFromEnd / clip.fadeOutSec));
  }
  return alpha;
}

export class DemoTextureGenerator {
  private androidCanvas: HTMLCanvasElement;
  private androidCtx: CanvasRenderingContext2D;
  private androidTexture: THREE.CanvasTexture;

  private pcCanvas: HTMLCanvasElement;
  private pcCtx: CanvasRenderingContext2D;
  private pcTexture: THREE.CanvasTexture;

  private videoElements: Map<string, { video: HTMLVideoElement; texture: THREE.VideoTexture }> =
    new Map();

  constructor() {
    // High-DPI crisp Android 9:19.5 canvas (720 x 1560)
    this.androidCanvas = document.createElement('canvas');
    this.androidCanvas.width = 720;
    this.androidCanvas.height = 1560;
    const aCtx = this.androidCanvas.getContext('2d');
    if (!aCtx) throw new Error('Could not initialize Android 2D context');
    this.androidCtx = aCtx;

    this.androidTexture = new THREE.CanvasTexture(this.androidCanvas);
    this.androidTexture.colorSpace = THREE.SRGBColorSpace;
    this.androidTexture.minFilter = THREE.LinearFilter;
    this.androidTexture.magFilter = THREE.LinearFilter;
    this.androidTexture.anisotropy = 8;

    // Crisp Desktop / Android Studio IDE 16:10 canvas (1280 x 800)
    this.pcCanvas = document.createElement('canvas');
    this.pcCanvas.width = 1280;
    this.pcCanvas.height = 800;
    const pCtx = this.pcCanvas.getContext('2d');
    if (!pCtx) throw new Error('Could not initialize PC 2D context');
    this.pcCtx = pCtx;

    this.pcTexture = new THREE.CanvasTexture(this.pcCanvas);
    this.pcTexture.colorSpace = THREE.SRGBColorSpace;
    this.pcTexture.minFilter = THREE.LinearFilter;
    this.pcTexture.magFilter = THREE.LinearFilter;
    this.pcTexture.anisotropy = 8;

    this.renderAndroidDemo(0, 1);
    this.renderPcDemo(0, 1);
  }

  public getAndroidTexture(
    timelineTimeSec: number,
    clip: TimelineClip | null,
    isPlaying: boolean
  ): THREE.Texture {
    if (clip && clip.mediaPath && !clip.mediaPath.startsWith('builtin://')) {
      return this.getOrLoadVideoTexture(clip.mediaPath, timelineTimeSec, clip, isPlaying);
    }
    const { sourceTimeSec, opacity } = clip
      ? computeClipSourceTime(clip, timelineTimeSec)
      : { sourceTimeSec: timelineTimeSec, opacity: 1 };
    this.renderAndroidDemo(sourceTimeSec, opacity);
    this.androidTexture.needsUpdate = true;
    return this.androidTexture;
  }

  public getPcTexture(
    timelineTimeSec: number,
    clip: TimelineClip | null,
    isPlaying: boolean
  ): THREE.Texture {
    if (clip && clip.mediaPath && !clip.mediaPath.startsWith('builtin://')) {
      return this.getOrLoadVideoTexture(clip.mediaPath, timelineTimeSec, clip, isPlaying);
    }
    const { sourceTimeSec, opacity } = clip
      ? computeClipSourceTime(clip, timelineTimeSec)
      : { sourceTimeSec: timelineTimeSec, opacity: 1 };
    this.renderPcDemo(sourceTimeSec, opacity);
    this.pcTexture.needsUpdate = true;
    return this.pcTexture;
  }

  private getOrLoadVideoTexture(
    mediaPath: string,
    timelineTimeSec: number,
    clip: TimelineClip,
    isPlaying: boolean
  ): THREE.Texture {
    let entry = this.videoElements.get(mediaPath);
    if (!entry) {
      const video = document.createElement('video');
      video.src = mediaPath;
      video.crossOrigin = 'anonymous';
      video.muted = true;
      video.playsInline = true;
      video.preload = 'auto';
      video.load();

      const texture = new THREE.VideoTexture(video);
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.minFilter = THREE.LinearFilter;
      texture.magFilter = THREE.LinearFilter;

      entry = { video, texture };
      this.videoElements.set(mediaPath, entry);
    }

    const { sourceTimeSec } = computeClipSourceTime(clip, timelineTimeSec);
    const { video, texture } = entry;

    if (video.readyState >= 1) {
      const drift = Math.abs(video.currentTime - sourceTimeSec);
      if (!isPlaying || drift > 0.25) {
        try {
          video.currentTime = sourceTimeSec;
        } catch {
          // Ignore seek errors on unbuffered streams
        }
      }
      if (isPlaying && video.paused) {
        video.play().catch(() => {});
      } else if (!isPlaying && !video.paused) {
        video.pause();
      }
    }

    texture.needsUpdate = true;
    return texture;
  }

  /**
   * Renders a rich Material 3 Android 16 flagship interface synced to `timeSec`.
   * Includes a clearly marked sensitive API Key / Token card at normalized y=0.22
   * so the user can see the Privacy Blur Mask working live!
   */
  public renderAndroidDemo(timeSec: number, clipOpacity: number = 1): void {
    const ctx = this.androidCtx;
    const w = this.androidCanvas.width;
    const h = this.androidCanvas.height;

    ctx.save();
    ctx.clearRect(0, 0, w, h);

    // Base Material You dark surface
    const bgGrad = ctx.createLinearGradient(0, 0, 0, h);
    bgGrad.addColorStop(0, '#090d16');
    bgGrad.addColorStop(0.45, '#0f172a');
    bgGrad.addColorStop(1, '#090d16');
    ctx.fillStyle = bgGrad;
    ctx.fillRect(0, 0, w, h);

    // Ambient top glow
    const glow = ctx.createRadialGradient(w * 0.5, 160, 20, w * 0.5, 160, 480);
    glow.addColorStop(0, 'rgba(99, 102, 241, 0.26)');
    glow.addColorStop(0.6, 'rgba(56, 189, 248, 0.08)');
    glow.addColorStop(1, 'rgba(15, 23, 42, 0)');
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, w, 650);

    // Compute smooth vertical scroll triggered around t=4.2s swipe gesture
    const scrollProgress =
      timeSec < 4.2
        ? 0
        : timeSec < 4.75
        ? Math.sin(((timeSec - 4.2) / 0.55) * (Math.PI / 2))
        : timeSec < 8.0
        ? 1
        : timeSec < 8.6
        ? 1 - ((timeSec - 8.0) / 0.6) * 0.5
        : 0.5;
    const scrollY = scrollProgress * 220;

    // Horizontal tab switch around t=8.2s swipe
    const tabSwitchProgress =
      timeSec < 8.2
        ? 0
        : timeSec < 8.65
        ? Math.sin(((timeSec - 8.2) / 0.45) * (Math.PI / 2))
        : 1;

    // --- SCROLLABLE BODY CONTENT ---
    ctx.save();
    ctx.translate(0, -scrollY);

    // 1. Top App Bar Header (y = 95..185)
    ctx.fillStyle = '#f8fafc';
    ctx.font = '700 32px Inter, system-ui, sans-serif';
    ctx.fillText('NovaPulse AI', 44, 145);

    // Live 120Hz pill badge
    roundRectPath(ctx, w - 210, 112, 166, 46, 23);
    ctx.fillStyle = 'rgba(16, 185, 129, 0.16)';
    ctx.fill();
    ctx.strokeStyle = 'rgba(52, 211, 153, 0.45)';
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.beginPath();
    ctx.arc(w - 182, 135, 7, 0, Math.PI * 2);
    ctx.fillStyle = '#34d399';
    ctx.fill();

    ctx.fillStyle = '#6ee7b7';
    ctx.font = '600 20px Inter, system-ui, sans-serif';
    ctx.fillText('120Hz USB', w - 164, 142);

    // 2. Filter Chips row (y = 200..260)
    const chips = ['Resumen Pro', 'Telemetría 3D', 'Nodos Cloud', 'Exportar'];
    let chipX = 44;
    chips.forEach((label, idx) => {
      const isSelected = (idx === 0 && tabSwitchProgress < 0.5) || (idx === 1 && tabSwitchProgress >= 0.5);
      const cw = label.length * 14 + 44;
      roundRectPath(ctx, chipX, 204, cw, 54, 27);
      ctx.fillStyle = isSelected ? '#4f46e5' : 'rgba(30, 41, 59, 0.85)';
      ctx.fill();
      ctx.strokeStyle = isSelected ? '#818cf8' : 'rgba(148, 163, 184, 0.2)';
      ctx.lineWidth = 2;
      ctx.stroke();

      ctx.fillStyle = isSelected ? '#ffffff' : '#94a3b8';
      ctx.font = '600 21px Inter, system-ui, sans-serif';
      ctx.fillText(label, chipX + 22, 238);
      chipX += cw + 16;
    });

    // 3. SENSITIVE CREDENTIALS / TOKEN BANNER at y = 0.22 * 1560 = 343px!
    // (Matches defaultProject privacyMask: x=0.18..0.82, y=0.22..0.29 => y=343..452)
    const credY = 335;
    roundRectPath(ctx, 44, credY, w - 88, 108, 24);
    ctx.fillStyle = 'rgba(30, 41, 59, 0.92)';
    ctx.fill();
    ctx.strokeStyle = 'rgba(245, 158, 11, 0.45)';
    ctx.lineWidth = 2;
    ctx.stroke();

    // Lock / Key badge
    roundRectPath(ctx, 66, credY + 22, 64, 64, 16);
    ctx.fillStyle = 'rgba(245, 158, 11, 0.18)';
    ctx.fill();
    ctx.fillStyle = '#fbbf24';
    ctx.font = '700 24px monospace';
    ctx.fillText('KEY', 76, credY + 62);

    ctx.fillStyle = '#94a3b8';
    ctx.font = '600 18px Inter, system-ui, sans-serif';
    ctx.fillText('CREDENCIAL PRIVADA (ZONA DEMO DE DIFUMINADO)', 148, credY + 42);

    ctx.fillStyle = '#fde68a';
    ctx.font = '700 23px JetBrains Mono, monospace';
    ctx.fillText('sk_live_98a7f4c2 · alex.dev@rec-ording.io', 148, credY + 78);

    // 4. HERO METRIC CARD (y = 470..780) - tapped at t=1.2s and t=2.6s
    const heroTapBoost =
      (timeSec > 1.15 && timeSec < 1.65) || (timeSec > 2.55 && timeSec < 3.1) ? 1 : 0;
    const heroY = 472;
    const heroGrad = ctx.createLinearGradient(44, heroY, w - 44, heroY + 300);
    heroGrad.addColorStop(0, heroTapBoost ? '#4338ca' : '#312e81');
    heroGrad.addColorStop(0.55, '#1e1b4b');
    heroGrad.addColorStop(1, '#0f172a');

    roundRectPath(ctx, 44, heroY, w - 88, 305, 34);
    ctx.fillStyle = heroGrad;
    ctx.fill();
    ctx.strokeStyle = heroTapBoost ? '#38bdf8' : 'rgba(129, 140, 248, 0.4)';
    ctx.lineWidth = heroTapBoost ? 4 : 2;
    ctx.stroke();

    ctx.fillStyle = '#a5b4fc';
    ctx.font = '600 21px Inter, system-ui, sans-serif';
    ctx.fillText('RENDIMIENTO DE RENDERIZADO NPU', 78, heroY + 52);

    const dynamicScore = (98.4 + Math.sin(timeSec * 2.2) * 1.2).toFixed(1);
    ctx.fillStyle = '#ffffff';
    ctx.font = '800 68px Inter, system-ui, sans-serif';
    ctx.fillText(`${dynamicScore}%`, 78, heroY + 132);

    roundRectPath(ctx, 335, heroY + 84, 154, 42, 21);
    ctx.fillStyle = 'rgba(52, 211, 153, 0.2)';
    ctx.fill();
    ctx.fillStyle = '#34d399';
    ctx.font = '700 21px Inter, system-ui, sans-serif';
    ctx.fillText('+24.8% FPS', 354, heroY + 112);

    // Animated Spline Chart inside Hero Card
    ctx.beginPath();
    const chartLeft = 78;
    const chartRight = w - 78;
    const chartBaseY = heroY + 252;
    const pointsCount = 28;
    for (let i = 0; i <= pointsCount; i++) {
      const tNorm = i / pointsCount;
      const px = chartLeft + (chartRight - chartLeft) * tNorm;
      const wave =
        Math.sin(tNorm * 6.5 + timeSec * 3.2) * 28 +
        Math.cos(tNorm * 11 - timeSec * 2.0) * 14 +
        tabSwitchProgress * 18 * Math.sin(tNorm * 8);
      const py = chartBaseY - 45 - wave;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.strokeStyle = '#38bdf8';
    ctx.lineWidth = 5;
    ctx.stroke();

    // Area fill under spline
    ctx.lineTo(chartRight, chartBaseY + 20);
    ctx.lineTo(chartLeft, chartBaseY + 20);
    ctx.closePath();
    const areaGrad = ctx.createLinearGradient(0, heroY + 150, 0, chartBaseY + 20);
    areaGrad.addColorStop(0, 'rgba(56, 189, 248, 0.38)');
    areaGrad.addColorStop(1, 'rgba(56, 189, 248, 0.0)');
    ctx.fillStyle = areaGrad;
    ctx.fill();

    // 5. TWO BENTO GRID CARDS (y = 805..1055)
    const bentoY = 806;
    const cardW = (w - 88 - 24) / 2;

    // Left Bento
    roundRectPath(ctx, 44, bentoY, cardW, 230, 28);
    ctx.fillStyle = 'rgba(30, 41, 59, 0.85)';
    ctx.fill();
    ctx.strokeStyle = 'rgba(148, 163, 184, 0.18)';
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.fillStyle = '#38bdf8';
    ctx.font = '700 22px Inter, system-ui, sans-serif';
    ctx.fillText('Latencia ADB', 72, bentoY + 52);
    ctx.fillStyle = '#f8fafc';
    ctx.font = '800 48px Inter, system-ui, sans-serif';
    ctx.fillText('1.8 ms', 72, bentoY + 122);
    ctx.fillStyle = '#94a3b8';
    ctx.font = '500 20px Inter, system-ui, sans-serif';
    ctx.fillText('Zero-Lag USB 3.2', 72, bentoY + 168);

    // Right Bento
    const rightX = 44 + cardW + 24;
    roundRectPath(ctx, rightX, bentoY, cardW, 230, 28);
    ctx.fillStyle = 'rgba(30, 41, 59, 0.85)';
    ctx.fill();
    ctx.strokeStyle = 'rgba(148, 163, 184, 0.18)';
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.fillStyle = '#c084fc';
    ctx.font = '700 22px Inter, system-ui, sans-serif';
    ctx.fillText('Exportación 4K', rightX + 28, bentoY + 52);
    ctx.fillStyle = '#f8fafc';
    ctx.font = '800 48px Inter, system-ui, sans-serif';
    ctx.fillText('60 FPS', rightX + 28, bentoY + 122);
    ctx.fillStyle = '#94a3b8';
    ctx.font = '500 20px Inter, system-ui, sans-serif';
    ctx.fillText('FFmpeg H.264 / VP9', rightX + 28, bentoY + 168);

    // 6. INTERACTIVE ACTION LIST (y = 1065..1580) - tapped at t=6.1s
    const listItems = [
      {
        title: 'Auto-Zoom Cinemático Inteligente',
        sub: 'Detecta toques en Android y genera curvas Spring',
        badge: 'ACTIVO',
        color: '#34d399',
      },
      {
        title: 'Subtítulos Dinámicos Karaoke',
        sub: 'Sincronización palabra por palabra con resaltado',
        badge: 'IA SYNC',
        color: '#38bdf8',
      },
      {
        title: 'Máscaras de Privacidad Automáticas',
        sub: 'Difumina tokens, emails y contraseñas en vivo',
        badge: 'PROTEGIDO',
        color: '#fbbf24',
      },
    ];

    listItems.forEach((item, idx) => {
      const itemY = 1068 + idx * 144;
      const isHighlighted = idx === 0 && timeSec > 5.8 && timeSec < 6.8;
      roundRectPath(ctx, 44, itemY, w - 88, 124, 26);
      ctx.fillStyle = isHighlighted ? 'rgba(56, 189, 248, 0.18)' : 'rgba(15, 23, 42, 0.9)';
      ctx.fill();
      ctx.strokeStyle = isHighlighted ? '#38bdf8' : 'rgba(148, 163, 184, 0.16)';
      ctx.lineWidth = isHighlighted ? 3 : 2;
      ctx.stroke();

      ctx.fillStyle = '#f8fafc';
      ctx.font = '700 24px Inter, system-ui, sans-serif';
      ctx.fillText(item.title, 76, itemY + 52);

      ctx.fillStyle = '#94a3b8';
      ctx.font = '500 19px Inter, system-ui, sans-serif';
      ctx.fillText(item.sub, 76, itemY + 88);

      roundRectPath(ctx, w - 205, itemY + 38, 134, 44, 22);
      ctx.fillStyle = 'rgba(15, 23, 42, 0.85)';
      ctx.fill();
      ctx.strokeStyle = item.color;
      ctx.lineWidth = 2;
      ctx.stroke();

      ctx.fillStyle = item.color;
      ctx.font = '700 17px Inter, system-ui, sans-serif';
      ctx.fillText(item.badge, w - 182, itemY + 66);
    });

    ctx.restore(); // End scrollable region

    // --- FIXED ANDROID STATUS BAR (Top y=0..82) ---
    const topBarGrad = ctx.createLinearGradient(0, 0, 0, 90);
    topBarGrad.addColorStop(0, 'rgba(9, 13, 22, 0.95)');
    topBarGrad.addColorStop(1, 'rgba(9, 13, 22, 0.0)');
    ctx.fillStyle = topBarGrad;
    ctx.fillRect(0, 0, w, 90);

    ctx.fillStyle = '#f8fafc';
    ctx.font = '700 24px Inter, system-ui, sans-serif';
    ctx.fillText('12:45', 52, 54);

    // 5G + Wi-Fi + Battery icons on right
    ctx.font = '700 20px Inter, system-ui, sans-serif';
    ctx.fillText('5G', w - 168, 53);

    // Battery body
    roundRectPath(ctx, w - 116, 33, 56, 24, 6);
    ctx.strokeStyle = '#f8fafc';
    ctx.lineWidth = 2.5;
    ctx.stroke();
    roundRectPath(ctx, w - 112, 37, 44, 16, 4);
    ctx.fillStyle = '#34d399';
    ctx.fill();

    // --- FLOATING ACTION BUTTON (FAB) ---
    roundRectPath(ctx, w - 156, h - 250, 108, 108, 32);
    ctx.fillStyle = '#4f46e5';
    ctx.fill();
    ctx.strokeStyle = '#a5b4fc';
    ctx.lineWidth = 3;
    ctx.stroke();
    ctx.fillStyle = '#ffffff';
    ctx.font = '700 48px Inter, system-ui, sans-serif';
    ctx.fillText('+', w - 117, h - 180);

    // --- FIXED BOTTOM NAVIGATION BAR + GESTURE PILL ---
    roundRectPath(ctx, 32, h - 122, w - 64, 92, 46);
    ctx.fillStyle = 'rgba(15, 23, 42, 0.94)';
    ctx.fill();
    ctx.strokeStyle = 'rgba(148, 163, 184, 0.22)';
    ctx.lineWidth = 2;
    ctx.stroke();

    const navLabels = ['Inicio', 'Capturas', '3D Studio', 'Perfil'];
    navLabels.forEach((nav, idx) => {
      const nx = 96 + idx * 165;
      const active = idx === 2;
      if (active) {
        roundRectPath(ctx, nx - 48, h - 106, 136, 60, 30);
        ctx.fillStyle = 'rgba(79, 70, 229, 0.32)';
        ctx.fill();
      }
      ctx.fillStyle = active ? '#818cf8' : '#94a3b8';
      ctx.font = active ? '700 20px Inter, system-ui, sans-serif' : '500 20px Inter, system-ui, sans-serif';
      ctx.fillText(nav, nx - 18, h - 68);
    });

    // Android gesture navigation bar pill
    roundRectPath(ctx, w / 2 - 90, h - 16, 180, 8, 4);
    ctx.fillStyle = 'rgba(248, 250, 252, 0.65)';
    ctx.fill();

    // Apply clip fade opacity if < 1
    if (clipOpacity < 1) {
      ctx.fillStyle = `rgba(0, 0, 0, ${1 - clipOpacity})`;
      ctx.fillRect(0, 0, w, h);
    }

    ctx.restore();
  }

  /**
   * Renders a dark-themed Android Studio / ADB Logcat & Profiler desktop window
   */
  public renderPcDemo(timeSec: number, clipOpacity: number = 1): void {
    const ctx = this.pcCtx;
    const w = this.pcCanvas.width;
    const h = this.pcCanvas.height;

    ctx.save();
    ctx.clearRect(0, 0, w, h);

    // IDE Dark Background
    ctx.fillStyle = '#0b0f19';
    ctx.fillRect(0, 0, w, h);

    // Left Sidebar Activity + Project Tree
    ctx.fillStyle = '#111827';
    ctx.fillRect(0, 0, 270, h);
    ctx.strokeStyle = '#1e293b';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(270, 0);
    ctx.lineTo(270, h);
    ctx.stroke();

    ctx.fillStyle = '#94a3b8';
    ctx.font = '700 16px Inter, system-ui, sans-serif';
    ctx.fillText('EXPLORADOR ANDROID', 24, 42);

    const files = [
      '▼ app/src/main/kotlin',
      '  ▸ ui/theme/Material3.kt',
      '  ▸ camera/DirectorRig3D.kt',
      '  ▸ adb/UsbFrameStream.kt',
      '  ▸ ffmpeg/AlphaExporter.kt',
      '▼ res/values',
      '  ▸ strings.xml',
      '  ▸ shaders_pbr.glsl',
    ];
    files.forEach((f, idx) => {
      const y = 85 + idx * 36;
      if (idx === 2) {
        roundRectPath(ctx, 12, y - 22, 246, 30, 8);
        ctx.fillStyle = 'rgba(56, 189, 248, 0.16)';
        ctx.fill();
      }
      ctx.fillStyle = idx === 2 ? '#38bdf8' : '#cbd5e1';
      ctx.font = '500 15px JetBrains Mono, monospace';
      ctx.fillText(f, 22, y);
    });

    // Main Code Editor Area (x = 270..w, y = 0..500)
    const codeLines = [
      '@Composable',
      'fun PlayStoreShowcaseScreen(telemetry: AdbTelemetryStream) {',
      '    val frameState by telemetry.collectAsStateWithLifecycle()',
      '    val cameraSpring = animateFloatAsState(',
      '        targetValue = frameState.focusZoom,',
      '        animationSpec = spring(dampingRatio = 0.78f, stiffness = 380f)',
      '    )',
      '    Surface(modifier = Modifier.fillMaxSize(), tonalElevation = 6.dp) {',
      '        NovaPulseDashboard(fps = frameState.currentFps, zoom = cameraSpring.value)',
      '    }',
      '}',
    ];

    codeLines.forEach((line, idx) => {
      const y = 64 + idx * 36;
      ctx.fillStyle = '#475569';
      ctx.font = '500 16px JetBrains Mono, monospace';
      ctx.fillText(String(idx + 1).padStart(2, '0'), 296, y);

      ctx.fillStyle = line.includes('@Composable')
        ? '#fbbf24'
        : line.includes('fun ') || line.includes('val ')
        ? '#38bdf8'
        : '#e2e8f0';
      ctx.fillText(line, 344, y);
    });

    // Bottom ADB Logcat + Frame Timing Profiler (y = 500..800)
    ctx.fillStyle = '#0f172a';
    ctx.fillRect(270, 500, w - 270, h - 500);
    ctx.strokeStyle = '#1e293b';
    ctx.beginPath();
    ctx.moveTo(270, 500);
    ctx.lineTo(w, 500);
    ctx.stroke();

    ctx.fillStyle = '#34d399';
    ctx.font = '700 16px JetBrains Mono, monospace';
    ctx.fillText('ADB LOGCAT · PIXEL 11 PRO (USB-C 120Hz STREAM ACTIVE)', 296, 536);

    const logOffset = Math.floor(timeSec * 4);
    for (let i = 0; i < 6; i++) {
      const seq = logOffset + i;
      const ly = 576 + i * 32;
      ctx.fillStyle = i === 5 ? '#38bdf8' : '#94a3b8';
      ctx.font = '500 15px JetBrains Mono, monospace';
      ctx.fillText(
        `[12:45:${String(seq % 60).padStart(2, '0')}.${String((seq * 183) % 999).padStart(
          3,
          '0'
        )}] I/PlayStudio3D: Frame #${seq * 12 + 400} synced · GPU 4.2ms · Audio PCM Locked`,
        296,
        ly
      );
    }

    if (clipOpacity < 1) {
      ctx.fillStyle = `rgba(0, 0, 0, ${1 - clipOpacity})`;
      ctx.fillRect(0, 0, w, h);
    }

    ctx.restore();
  }

  public dispose(): void {
    this.androidTexture.dispose();
    this.pcTexture.dispose();
    this.videoElements.forEach(({ video, texture }) => {
      video.pause();
      video.src = '';
      texture.dispose();
    });
    this.videoElements.clear();
  }
}
