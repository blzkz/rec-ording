import React, { useEffect, useRef, useState, useCallback } from 'react';
import * as THREE from 'three';
import {
  Camera,
  RotateCcw,
  Grid,
  Sparkles,
  Smartphone,
  Layers,
  Eye,
  Maximize2,
} from 'lucide-react';
import {
  DirectorCameraKeyframe,
  ProjectFile,
  TimelineClip,
} from '../types/project';
import { DemoTextureGenerator } from '../utils/proceduralDemoTextures';
import { buildDeviceRig3D, DeviceRig3D } from '../utils/deviceModels3D';

export interface Viewport3DCanvasProps {
  project: ProjectFile;
  currentTimeSec: number;
  isPlaying: boolean;
  onTimeUpdate?: (timeSec: number) => void;
  onUpdateProject?: (updater: (prev: ProjectFile) => ProjectFile) => void;
  canvasRefOut?: React.MutableRefObject<HTMLCanvasElement | null>;
}

interface InterpolatedCameraPose {
  focusX: number;
  focusY: number;
  zoom: number;
  rotXDeg: number;
  rotYDeg: number;
  rotZDeg: number;
  activeLabel?: string;
}

function applyEasing(
  t: number,
  mode: 'spring' | 'easeInOutCubic' | 'linear' | 'easeOutExpo'
): number {
  const clamped = Math.max(0, Math.min(1, t));
  if (mode === 'linear') return clamped;
  if (mode === 'easeOutExpo') {
    return clamped === 1 ? 1 : 1 - Math.pow(2, -10 * clamped);
  }
  if (mode === 'spring') {
    // Critically damped / slight overshoot cinematic spring approximation
    const c4 = (2 * Math.PI) / 3.4;
    return clamped === 0
      ? 0
      : clamped === 1
      ? 1
      : 1 - Math.pow(2, -8.5 * clamped) * Math.cos(clamped * c4);
  }
  // easeInOutCubic
  return clamped < 0.5
    ? 4 * clamped * clamped * clamped
    : 1 - Math.pow(-2 * clamped + 2, 3) / 2;
}

function interpolateDirectorCamera(
  keyframes: DirectorCameraKeyframe[],
  timeSec: number
): InterpolatedCameraPose {
  const defaultPose: InterpolatedCameraPose = {
    focusX: 0.5,
    focusY: 0.5,
    zoom: 1.0,
    rotXDeg: 0,
    rotYDeg: 0,
    rotZDeg: 0,
  };

  if (!keyframes || keyframes.length === 0) return defaultPose;

  const sorted = [...keyframes].sort((a, b) => a.timeSec - b.timeSec);
  if (timeSec <= sorted[0].timeSec) {
    const first = sorted[0];
    return {
      focusX: first.focusX,
      focusY: first.focusY,
      zoom: first.zoom,
      rotXDeg: first.rotXDeg,
      rotYDeg: first.rotYDeg,
      rotZDeg: first.rotZDeg,
      activeLabel: first.label,
    };
  }

  for (let i = 0; i < sorted.length; i++) {
    const curr = sorted[i];
    const next = sorted[i + 1];
    const holdEnd = curr.timeSec + Math.max(0.1, curr.durationSec);

    if (!next) {
      return {
        focusX: curr.focusX,
        focusY: curr.focusY,
        zoom: curr.zoom,
        rotXDeg: curr.rotXDeg,
        rotYDeg: curr.rotYDeg,
        rotZDeg: curr.rotZDeg,
        activeLabel: curr.label,
      };
    }

    if (timeSec >= curr.timeSec && timeSec < next.timeSec) {
      // Transition smoothly between curr and next across the span
      const span = Math.max(0.05, next.timeSec - curr.timeSec);
      const rawT = (timeSec - curr.timeSec) / span;
      const eased = applyEasing(rawT, next.easing || curr.easing || 'spring');
      return {
        focusX: curr.focusX + (next.focusX - curr.focusX) * eased,
        focusY: curr.focusY + (next.focusY - curr.focusY) * eased,
        zoom: curr.zoom + (next.zoom - curr.zoom) * eased,
        rotXDeg: curr.rotXDeg + (next.rotXDeg - curr.rotXDeg) * eased,
        rotYDeg: curr.rotYDeg + (next.rotYDeg - curr.rotYDeg) * eased,
        rotZDeg: curr.rotZDeg + (next.rotZDeg - curr.rotZDeg) * eased,
        activeLabel: timeSec <= holdEnd ? curr.label : next.label,
      };
    }
  }

  const last = sorted[sorted.length - 1];
  return {
    focusX: last.focusX,
    focusY: last.focusY,
    zoom: last.zoom,
    rotXDeg: last.rotXDeg,
    rotYDeg: last.rotYDeg,
    rotZDeg: last.rotZDeg,
    activeLabel: last.label,
  };
}

function roundRect2D(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number
) {
  const radius = Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2);
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}

export const Viewport3DCanvas: React.FC<Viewport3DCanvasProps> = ({
  project,
  currentTimeSec,
  isPlaying,
  onUpdateProject,
  canvasRefOut,
}) => {
  const masterCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const [showSafeZones, setShowSafeZones] = useState<boolean>(false);
  const [showPrivacyOutlines, setShowPrivacyOutlines] = useState<boolean>(true);
  const [orbitOffset, setOrbitOffset] = useState<{ rotX: number; rotY: number; zoomMul: number }>({
    rotX: 0,
    rotY: 0,
    zoomMul: 1,
  });

  // Interactive drag state for orbiting or moving a privacy mask
  const dragStateRef = useRef<{
    mode: 'orbit' | 'mask-move' | null;
    startX: number;
    startY: number;
    initialRotX: number;
    initialRotY: number;
    maskId?: string;
    initialMaskX?: number;
    initialMaskY?: number;
  }>({
    mode: null,
    startX: 0,
    startY: 0,
    initialRotX: 0,
    initialRotY: 0,
  });

  // Offscreen Three.js engine refs
  const threeEngineRef = useRef<{
    renderer: THREE.WebGLRenderer;
    scene: THREE.Scene;
    camera: THREE.PerspectiveCamera;
    ambientLight: THREE.AmbientLight;
    dirLight: THREE.DirectionalLight;
    rimLight: THREE.PointLight;
    stageGroup: THREE.Group;
    primaryRig: DeviceRig3D | null;
    secondaryRig: DeviceRig3D | null;
    shadowPlane: THREE.Mesh;
    demoTextures: DemoTextureGenerator;
    prevPose: InterpolatedCameraPose | null;
    motionAccumCanvas: HTMLCanvasElement;
    blurTempCanvas: HTMLCanvasElement;
  } | null>(null);

  // Smoothed cursor state
  const smoothedCursorRef = useRef<{ x: number; y: number; initialized: boolean }>({
    x: 0.5,
    y: 0.5,
    initialized: false,
  });

  // Expose master canvas ref to parent for export/capture
  const setCanvasRef = useCallback(
    (el: HTMLCanvasElement | null) => {
      masterCanvasRef.current = el;
      if (canvasRefOut) {
        canvasRefOut.current = el;
      }
    },
    [canvasRefOut]
  );

  // Initialize Three.js offscreen renderer once
  useEffect(() => {
    const renderer = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      preserveDrawingBuffer: true,
      powerPreference: 'high-performance',
    });
    renderer.setPixelRatio(1);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.1;

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(34, 16 / 9, 0.1, 100);
    camera.position.set(0, 0, 10.2);

    const ambientLight = new THREE.AmbientLight(0xffffff, 1.15);
    scene.add(ambientLight);

    const dirLight = new THREE.DirectionalLight(0xffffff, 1.85);
    dirLight.position.set(5.5, 8.5, 7.5);
    scene.add(dirLight);

    const rimLight = new THREE.PointLight(0x38bdf8, 1.4, 25);
    rimLight.position.set(-6, 3.5, -4);
    scene.add(rimLight);

    const stageGroup = new THREE.Group();
    scene.add(stageGroup);

    // Procedural radial soft shadow plane beneath devices
    const shadowCanvas = document.createElement('canvas');
    shadowCanvas.width = 256;
    shadowCanvas.height = 256;
    const sCtx = shadowCanvas.getContext('2d');
    if (sCtx) {
      const grad = sCtx.createRadialGradient(128, 128, 12, 128, 128, 120);
      grad.addColorStop(0, 'rgba(0, 0, 0, 0.72)');
      grad.addColorStop(0.55, 'rgba(0, 0, 0, 0.25)');
      grad.addColorStop(1, 'rgba(0, 0, 0, 0.0)');
      sCtx.fillStyle = grad;
      sCtx.fillRect(0, 0, 256, 256);
    }
    const shadowTex = new THREE.CanvasTexture(shadowCanvas);
    const shadowGeom = new THREE.PlaneGeometry(5.8, 5.8);
    const shadowMat = new THREE.MeshBasicMaterial({
      map: shadowTex,
      transparent: true,
      depthWrite: false,
      opacity: 0.65,
    });
    const shadowPlane = new THREE.Mesh(shadowGeom, shadowMat);
    shadowPlane.position.set(0, -2.95, -0.8);
    shadowPlane.rotation.x = -Math.PI * 0.36;
    scene.add(shadowPlane);

    const demoTextures = new DemoTextureGenerator();
    const motionAccumCanvas = document.createElement('canvas');
    const blurTempCanvas = document.createElement('canvas');

    threeEngineRef.current = {
      renderer,
      scene,
      camera,
      ambientLight,
      dirLight,
      rimLight,
      stageGroup,
      primaryRig: null,
      secondaryRig: null,
      shadowPlane,
      demoTextures,
      prevPose: null,
      motionAccumCanvas,
      blurTempCanvas,
    };

    return () => {
      demoTextures.dispose();
      renderer.dispose();
      shadowTex.dispose();
      shadowGeom.dispose();
      shadowMat.dispose();
      threeEngineRef.current = null;
    };
  }, []);

  // Rebuild primary/secondary 3D device rigs when model type, orientation, or geometry config changes
  useEffect(() => {
    const eng = threeEngineRef.current;
    if (!eng) return;

    if (eng.primaryRig) {
      eng.stageGroup.remove(eng.primaryRig.group);
      eng.primaryRig.dispose();
      eng.primaryRig = null;
    }
    if (eng.secondaryRig) {
      eng.stageGroup.remove(eng.secondaryRig.group);
      eng.secondaryRig.dispose();
      eng.secondaryRig = null;
    }

    const primaryRig = buildDeviceRig3D(project.device3D.primaryModel, project.device3D);
    eng.stageGroup.add(primaryRig.group);
    eng.primaryRig = primaryRig;

    if (project.device3D.dualLayout !== 'single') {
      const secondaryRig = buildDeviceRig3D(project.device3D.secondaryModel, project.device3D);
      eng.stageGroup.add(secondaryRig.group);
      eng.secondaryRig = secondaryRig;
    }
  }, [
    project.device3D.primaryModel,
    project.device3D.secondaryModel,
    project.device3D.dualLayout,
    project.device3D.orientation,
    project.device3D.screenCornerRadius,
    project.device3D.bezelWidth,
    project.device3D.customGlbPath,
    project.device3D.customGlbScreenMeshName,
  ]);

  // Update material properties (color, metalness, roughness) dynamically without rebuilding geometry
  useEffect(() => {
    const eng = threeEngineRef.current;
    if (!eng) return;
    eng.primaryRig?.updateMaterialConfig(project.device3D);
    eng.secondaryRig?.updateMaterialConfig(project.device3D);
  }, [
    project.device3D.bodyColor,
    project.device3D.frameMetalness,
    project.device3D.frameRoughness,
  ]);

  // Compute target output aspect ratio and resolution (capped for smooth real-time preview while preserving exact aspect)
  const exportW = project.exportSettings.width || 1920;
  const exportH = project.exportSettings.height || 1080;
  const aspect = exportW / exportH;
  const internalHeight = Math.min(1080, Math.max(720, exportH));
  const internalWidth = Math.round(internalHeight * aspect);

  // Helper to project normalized (u, v) on primary device screen into 2D master canvas pixel coordinates
  const projectScreenUVToCanvas = useCallback(
    (u: number, v: number, width: number, height: number): { x: number; y: number } => {
      const eng = threeEngineRef.current;
      if (!eng || !eng.primaryRig || !eng.primaryRig.screenMesh) {
        return { x: u * width, y: v * height };
      }

      const mesh = eng.primaryRig.screenMesh;
      mesh.geometry.computeBoundingBox();
      const bbox = mesh.geometry.boundingBox;
      if (!bbox) {
        return { x: u * width, y: v * height };
      }

      const localX = bbox.min.x + u * (bbox.max.x - bbox.min.x);
      const localY = bbox.max.y - v * (bbox.max.y - bbox.min.y);
      const vec = new THREE.Vector3(localX, localY, 0.01);
      mesh.updateWorldMatrix(true, false);
      vec.applyMatrix4(mesh.matrixWorld);
      vec.project(eng.camera);

      return {
        x: (vec.x * 0.5 + 0.5) * width,
        y: (-vec.y * 0.5 + 0.5) * height,
      };
    },
    []
  );

  // Master composite render pass executed whenever project state, time, or orbit changes
  useEffect(() => {
    const canvas = masterCanvasRef.current;
    const eng = threeEngineRef.current;
    if (!canvas || !eng) return;

    if (canvas.width !== internalWidth || canvas.height !== internalHeight) {
      canvas.width = internalWidth;
      canvas.height = internalHeight;
    }

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const w = canvas.width;
    const h = canvas.height;

    // 1. CLEAR & RENDER BACKGROUND LAYER
    ctx.clearRect(0, 0, w, h);

    const bg = project.background;
    const isTransparent =
      bg.type === 'transparent' || project.exportSettings.transparentBackground;

    if (!isTransparent) {
      if (bg.type === 'solid') {
        ctx.fillStyle = bg.colorA || '#0f172a';
        ctx.fillRect(0, 0, w, h);
      } else if (bg.type === 'mesh-gradient' || bg.type === 'wallpaper-preset') {
        const angleRad = ((bg.gradientAngle || 135) * Math.PI) / 180;
        const cx = w / 2;
        const cy = h / 2;
        const rx = Math.cos(angleRad) * (w * 0.65);
        const ry = Math.sin(angleRad) * (h * 0.65);

        const linGrad = ctx.createLinearGradient(cx - rx, cy - ry, cx + rx, cy + ry);
        linGrad.addColorStop(0, bg.colorA || '#0f172a');
        linGrad.addColorStop(0.52, bg.colorB || '#1e1b4b');
        linGrad.addColorStop(1, bg.colorC || '#042f2e');
        ctx.fillStyle = linGrad;
        ctx.fillRect(0, 0, w, h);

        // Add subtle organic mesh glow orbs
        const orb1 = ctx.createRadialGradient(
          w * 0.22,
          h * 0.25,
          10,
          w * 0.22,
          h * 0.25,
          w * 0.48
        );
        orb1.addColorStop(0, 'rgba(99, 102, 241, 0.22)');
        orb1.addColorStop(1, 'rgba(99, 102, 241, 0)');
        ctx.fillStyle = orb1;
        ctx.fillRect(0, 0, w, h);

        const orb2 = ctx.createRadialGradient(
          w * 0.78,
          h * 0.75,
          10,
          w * 0.78,
          h * 0.75,
          w * 0.45
        );
        orb2.addColorStop(0, 'rgba(20, 184, 166, 0.18)');
        orb2.addColorStop(1, 'rgba(20, 184, 166, 0)');
        ctx.fillStyle = orb2;
        ctx.fillRect(0, 0, w, h);
      }
    }

    // 2. RESOLVE ACTIVE VIDEO CLIPS & UPDATE 3D SCREEN TEXTURES
    const anySolo = project.tracks.some((t) => t.kind === 'video' && t.solo);
    const activeVideoClips: TimelineClip[] = [];
    for (const track of project.tracks) {
      if (track.kind !== 'video') continue;
      if (track.muted) continue;
      if (anySolo && !track.solo) continue;
      for (const clip of track.clips) {
        if (
          currentTimeSec >= clip.startSec &&
          currentTimeSec <= clip.startSec + clip.durationSec
        ) {
          activeVideoClips.push(clip);
        }
      }
    }

    const androidClip =
      activeVideoClips.find((c) => c.sourceRole === 'android-main') ||
      activeVideoClips[0] ||
      null;
    const pcClip =
      activeVideoClips.find((c) => c.sourceRole === 'pc-screen') ||
      activeVideoClips[1] ||
      null;

    const androidTex = eng.demoTextures.getAndroidTexture(
      currentTimeSec,
      androidClip,
      isPlaying
    );
    eng.primaryRig?.setScreenTexture(androidTex);

    if (eng.secondaryRig) {
      const pcTex = eng.demoTextures.getPcTexture(currentTimeSec, pcClip, isPlaying);
      eng.secondaryRig.setScreenTexture(pcTex);
    }

    // 3. POSITION DUAL-DEVICE LAYOUTS & APPLY DIRECTOR CAMERA + FLOATING ANIMATION
    const layout = project.device3D.dualLayout;
    if (eng.primaryRig && eng.secondaryRig) {
      if (layout === 'side-by-side') {
        eng.primaryRig.group.position.set(2.4, 0, 0.35);
        eng.primaryRig.group.rotation.set(0, -0.18, 0);
        eng.primaryRig.group.scale.setScalar(0.92);

        eng.secondaryRig.group.position.set(-1.65, 0.1, -0.45);
        eng.secondaryRig.group.rotation.set(0, 0.14, 0);
        eng.secondaryRig.group.scale.setScalar(0.78);
      } else if (layout === 'pip-floating') {
        eng.primaryRig.group.position.set(2.35, -0.45, 0.95);
        eng.primaryRig.group.rotation.set(0, -0.22, 0.03);
        eng.primaryRig.group.scale.setScalar(0.75);

        eng.secondaryRig.group.position.set(-0.55, 0.15, -0.5);
        eng.secondaryRig.group.rotation.set(0, 0.05, 0);
        eng.secondaryRig.group.scale.setScalar(0.95);
      } else if (layout === 'angled-showcase') {
        eng.primaryRig.group.position.set(1.55, -0.1, 0.8);
        eng.primaryRig.group.rotation.set(-0.08, -0.32, 0.05);
        eng.primaryRig.group.scale.setScalar(0.92);

        eng.secondaryRig.group.position.set(-1.35, 0.25, -0.7);
        eng.secondaryRig.group.rotation.set(0.05, 0.26, -0.02);
        eng.secondaryRig.group.scale.setScalar(0.84);
      }
    } else if (eng.primaryRig) {
      eng.primaryRig.group.position.set(0, 0, 0);
      if (
        project.device3D.orientation === 'landscape' &&
        (project.device3D.primaryModel === 'pixel-11-pro' ||
          project.device3D.primaryModel === 'galaxy-s26-ultra' ||
          project.device3D.primaryModel === 'none')
      ) {
        eng.primaryRig.group.rotation.set(0, 0, -Math.PI / 2);
      } else {
        eng.primaryRig.group.rotation.set(0, 0, 0);
      }
      eng.primaryRig.group.scale.setScalar(1.0);
    }

    // Apply Director Camera Keyframe interpolation
    const pose = interpolateDirectorCamera(project.directorKeyframes, currentTimeSec);

    // Idle floating animation
    const floatAmp = project.device3D.floatingAnimation
      ? project.device3D.floatingAmplitude
      : 0;
    const floatY = Math.sin(currentTimeSec * 1.9) * 0.18 * floatAmp;
    const floatRotZ = Math.cos(currentTimeSec * 1.35) * 1.5 * floatAmp;

    const totalRotX = THREE.MathUtils.degToRad(pose.rotXDeg + orbitOffset.rotX);
    const totalRotY = THREE.MathUtils.degToRad(pose.rotYDeg + orbitOffset.rotY);
    const totalRotZ = THREE.MathUtils.degToRad(pose.rotZDeg + floatRotZ);

    const paddingScale = Math.max(0.55, 1 - (bg.paddingRatio || 0.08) * 1.35);
    const effectiveZoom = Math.max(0.4, Math.min(3.2, pose.zoom * orbitOffset.zoomMul * paddingScale));

    // Focus offset shifts the stage so the camera centers on (focusX, focusY)
    const panX = (0.5 - pose.focusX) * 2.4 * (pose.zoom - 0.85);
    const panY = (pose.focusY - 0.5) * 3.8 * (pose.zoom - 0.85);

    eng.stageGroup.position.set(panX, panY + floatY, 0);
    eng.stageGroup.rotation.set(totalRotX, totalRotY, totalRotZ);
    eng.stageGroup.scale.setScalar(effectiveZoom);

    // Update lights & shadow
    eng.ambientLight.intensity = project.device3D.ambientLightIntensity;
    eng.dirLight.intensity = project.device3D.directionalLightIntensity;
    eng.shadowPlane.visible = project.device3D.showShadow;
    (eng.shadowPlane.material as THREE.MeshBasicMaterial).opacity =
      project.device3D.shadowIntensity * 0.75;
    eng.shadowPlane.scale.setScalar(effectiveZoom);

    // Render Three.js scene
    if (
      eng.renderer.domElement.width !== w ||
      eng.renderer.domElement.height !== h
    ) {
      eng.renderer.setSize(w, h, false);
      eng.camera.aspect = w / h;
      eng.camera.updateProjectionMatrix();
    }
    eng.renderer.render(eng.scene, eng.camera);

    // Velocity-based Motion Blur when camera moves rapidly
    if (bg.motionBlurEnabled && bg.motionBlurIntensity > 0.05 && eng.prevPose) {
      const dZoom = Math.abs(pose.zoom - eng.prevPose.zoom);
      const dRot =
        Math.abs(pose.rotXDeg - eng.prevPose.rotXDeg) +
        Math.abs(pose.rotYDeg - eng.prevPose.rotYDeg);
      const velocity = Math.min(1, (dZoom * 8 + dRot * 0.12) * bg.motionBlurIntensity);

      if (velocity > 0.06 && eng.motionAccumCanvas.width === w) {
        ctx.save();
        ctx.globalAlpha = Math.min(0.42, velocity * 0.45);
        const shiftX = (pose.rotYDeg - eng.prevPose.rotYDeg) * 1.4;
        const shiftY = (pose.focusY - eng.prevPose.focusY) * 18;
        ctx.drawImage(eng.motionAccumCanvas, shiftX, shiftY, w, h);
        ctx.restore();
      }
    }

    // Draw crisp current 3D WebGL frame onto master canvas
    ctx.drawImage(eng.renderer.domElement, 0, 0, w, h);

    // Store frame in motion accumulation buffer for next frame's velocity trail
    if (bg.motionBlurEnabled) {
      if (
        eng.motionAccumCanvas.width !== w ||
        eng.motionAccumCanvas.height !== h
      ) {
        eng.motionAccumCanvas.width = w;
        eng.motionAccumCanvas.height = h;
      }
      const mCtx = eng.motionAccumCanvas.getContext('2d');
      if (mCtx) {
        mCtx.clearRect(0, 0, w, h);
        mCtx.drawImage(eng.renderer.domElement, 0, 0, w, h);
      }
    }
    eng.prevPose = pose;

    // 4. COMPOSITE PRIVACY MASKS (Projected onto the 3D Device Screen!)
    const activeMasks = project.privacyMasks.filter(
      (m) => currentTimeSec >= m.startSec && currentTimeSec <= m.endSec
    );

    for (const mask of activeMasks) {
      const pTopLeft = projectScreenUVToCanvas(mask.x, mask.y, w, h);
      const pTopRight = projectScreenUVToCanvas(mask.x + mask.width, mask.y, w, h);
      const pBottomLeft = projectScreenUVToCanvas(mask.x, mask.y + mask.height, w, h);
      const pBottomRight = projectScreenUVToCanvas(
        mask.x + mask.width,
        mask.y + mask.height,
        w,
        h
      );

      const minX = Math.max(
        0,
        Math.min(pTopLeft.x, pTopRight.x, pBottomLeft.x, pBottomRight.x)
      );
      const maxX = Math.min(
        w,
        Math.max(pTopLeft.x, pTopRight.x, pBottomLeft.x, pBottomRight.x)
      );
      const minY = Math.max(
        0,
        Math.min(pTopLeft.y, pTopRight.y, pBottomLeft.y, pBottomRight.y)
      );
      const maxY = Math.min(
        h,
        Math.max(pTopLeft.y, pTopRight.y, pBottomLeft.y, pBottomRight.y)
      );
      const boxW = Math.max(8, maxX - minX);
      const boxH = Math.max(8, maxY - minY);

      ctx.save();
      // Clip to exact 4-corner perspective polygon on the 3D screen
      ctx.beginPath();
      ctx.moveTo(pTopLeft.x, pTopLeft.y);
      ctx.lineTo(pTopRight.x, pTopRight.y);
      ctx.lineTo(pBottomRight.x, pBottomRight.y);
      ctx.lineTo(pBottomLeft.x, pBottomLeft.y);
      ctx.closePath();
      ctx.clip();

      if (mask.mode === 'solid') {
        ctx.fillStyle = mask.color || '#0f172a';
        ctx.fillRect(minX, minY, boxW, boxH);
      } else if (mask.mode === 'pixelate') {
        const downscale = Math.max(4, Math.round(mask.intensity || 16));
        const sw = Math.max(2, Math.floor(boxW / downscale));
        const sh = Math.max(2, Math.floor(boxH / downscale));
        eng.blurTempCanvas.width = sw;
        eng.blurTempCanvas.height = sh;
        const bCtx = eng.blurTempCanvas.getContext('2d');
        if (bCtx) {
          bCtx.imageSmoothingEnabled = false;
          bCtx.drawImage(canvas, minX, minY, boxW, boxH, 0, 0, sw, sh);
          ctx.imageSmoothingEnabled = false;
          ctx.drawImage(eng.blurTempCanvas, 0, 0, sw, sh, minX, minY, boxW, boxH);
          ctx.imageSmoothingEnabled = true;
        }
      } else {
        // Frosted Gaussian Blur mask
        eng.blurTempCanvas.width = Math.ceil(boxW);
        eng.blurTempCanvas.height = Math.ceil(boxH);
        const bCtx = eng.blurTempCanvas.getContext('2d');
        if (bCtx) {
          bCtx.filter = `blur(${Math.max(4, mask.intensity || 16)}px)`;
          bCtx.drawImage(canvas, minX, minY, boxW, boxH, 0, 0, boxW, boxH);
          ctx.drawImage(eng.blurTempCanvas, minX, minY, boxW, boxH);
        }
        ctx.fillStyle = 'rgba(15, 23, 42, 0.32)';
        ctx.fillRect(minX, minY, boxW, boxH);
      }
      ctx.restore();

      // Draw interactive editor outline when paused and outlines enabled
      if (!isPlaying && showPrivacyOutlines) {
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(pTopLeft.x, pTopLeft.y);
        ctx.lineTo(pTopRight.x, pTopRight.y);
        ctx.lineTo(pBottomRight.x, pBottomRight.y);
        ctx.lineTo(pBottomLeft.x, pBottomLeft.y);
        ctx.closePath();
        ctx.strokeStyle = '#fbbf24';
        ctx.lineWidth = 2;
        ctx.setLineDash([6, 4]);
        ctx.stroke();

        // Label pill
        ctx.setLineDash([]);
        roundRect2D(ctx, pTopLeft.x, pTopLeft.y - 24, 175, 20, 6);
        ctx.fillStyle = 'rgba(245, 158, 11, 0.92)';
        ctx.fill();
        ctx.fillStyle = '#090d16';
        ctx.font = '700 11px Inter, system-ui, sans-serif';
        ctx.fillText(`🔒 ${mask.label.slice(0, 22)}`, pTopLeft.x + 6, pTopLeft.y - 10);
        ctx.restore();
      }
    }

    // 5. COMPOSITE SMOOTHED CURSOR, CLICK RIPPLES & TOUCH SWIPE TRAILS
    const cs = project.cursorStyle;
    const nowMs = currentTimeSec * 1000;

    for (const ev of project.pointerEvents) {
      const elapsedMs = nowMs - ev.timestampMs;
      const durMs = ev.durationMs || 250;

      // Active Touch Tap / Click Ripple
      if (
        cs.showClickRipples &&
        (ev.type === 'touch_tap' || ev.type === 'click') &&
        elapsedMs >= -30 &&
        elapsedMs <= 650
      ) {
        const progress = Math.max(0, Math.min(1, (elapsedMs + 30) / 680));
        const pos = projectScreenUVToCanvas(ev.x, ev.y, w, h);

        ctx.save();
        const maxRadius = 46 * cs.cursorSize * effectiveZoom;
        const radius = 10 + maxRadius * applyEasing(progress, 'easeOutExpo');
        const alpha = (1 - progress) * 0.85;

        ctx.beginPath();
        ctx.arc(pos.x, pos.y, radius, 0, Math.PI * 2);
        ctx.strokeStyle = cs.clickRippleColor || '#818cf8';
        ctx.globalAlpha = alpha;
        ctx.lineWidth = 3.5;
        ctx.stroke();

        // Inner touch orb
        ctx.beginPath();
        ctx.arc(pos.x, pos.y, 14 * cs.cursorSize * (1 - progress * 0.4), 0, Math.PI * 2);
        ctx.fillStyle = cs.cursorColor || '#38bdf8';
        ctx.globalAlpha = alpha * 0.65;
        ctx.fill();
        ctx.restore();
      }

      // Active Touch Swipe Trail
      if (
        cs.showTouchTrails &&
        ev.type === 'touch_swipe' &&
        ev.endX !== undefined &&
        ev.endY !== undefined &&
        elapsedMs >= 0 &&
        elapsedMs <= durMs + 320
      ) {
        const moveP = Math.max(0, Math.min(1, elapsedMs / durMs));
        const fadeP =
          elapsedMs > durMs ? Math.max(0, 1 - (elapsedMs - durMs) / 320) : 1;

        const startPt = projectScreenUVToCanvas(ev.x, ev.y, w, h);
        const curU = ev.x + (ev.endX - ev.x) * applyEasing(moveP, 'easeInOutCubic');
        const curV = ev.y + (ev.endY - ev.y) * applyEasing(moveP, 'easeInOutCubic');
        const headPt = projectScreenUVToCanvas(curU, curV, w, h);

        ctx.save();
        const trailGrad = ctx.createLinearGradient(
          startPt.x,
          startPt.y,
          headPt.x,
          headPt.y
        );
        trailGrad.addColorStop(0, 'rgba(56, 189, 248, 0.0)');
        trailGrad.addColorStop(1, cs.cursorColor || '#38bdf8');

        ctx.beginPath();
        ctx.moveTo(startPt.x, startPt.y);
        ctx.lineTo(headPt.x, headPt.y);
        ctx.strokeStyle = trailGrad;
        ctx.lineWidth = 10 * cs.cursorSize;
        ctx.lineCap = 'round';
        ctx.globalAlpha = fadeP * 0.85;
        ctx.stroke();

        // Glowing finger head orb
        ctx.beginPath();
        ctx.arc(headPt.x, headPt.y, 14 * cs.cursorSize, 0, Math.PI * 2);
        ctx.fillStyle = '#ffffff';
        ctx.globalAlpha = fadeP;
        ctx.fill();
        ctx.lineWidth = 4;
        ctx.strokeStyle = cs.cursorColor || '#38bdf8';
        ctx.stroke();
        ctx.restore();
      }
    }

    // Draw smoothed pointer cursor if recent pointer event is within hideIdleCursorAfterMs
    const recentEvents = project.pointerEvents.filter(
      (e) => nowMs >= e.timestampMs - 250 && nowMs - e.timestampMs <= cs.hideIdleCursorAfterMs
    );
    if (recentEvents.length > 0) {
      const latest = recentEvents[recentEvents.length - 1];
      let targetU = latest.x;
      let targetV = latest.y;
      if (
        latest.type === 'touch_swipe' &&
        latest.endX !== undefined &&
        latest.endY !== undefined
      ) {
        const p = Math.max(
          0,
          Math.min(1, (nowMs - latest.timestampMs) / (latest.durationMs || 400))
        );
        targetU = latest.x + (latest.endX - latest.x) * p;
        targetV = latest.y + (latest.endY - latest.y) * p;
      }

      if (!smoothedCursorRef.current.initialized || !cs.smoothCursor) {
        smoothedCursorRef.current = { x: targetU, y: targetV, initialized: true };
      } else {
        const alpha = Math.max(0.08, 1 - cs.smoothingFactor * 0.85);
        smoothedCursorRef.current.x += (targetU - smoothedCursorRef.current.x) * alpha;
        smoothedCursorRef.current.y += (targetV - smoothedCursorRef.current.y) * alpha;
      }

      const cPos = projectScreenUVToCanvas(
        smoothedCursorRef.current.x,
        smoothedCursorRef.current.y,
        w,
        h
      );
      const idleAge = Math.max(0, nowMs - latest.timestampMs);
      const idleFade =
        idleAge > cs.hideIdleCursorAfterMs - 400
          ? Math.max(0, (cs.hideIdleCursorAfterMs - idleAge) / 400)
          : 1;

      ctx.save();
      ctx.globalAlpha = idleFade;
      if (cs.cursorStyle === 'touch-orb' || cs.cursorStyle === 'circle-dot') {
        ctx.beginPath();
        ctx.arc(cPos.x, cPos.y, 13 * cs.cursorSize, 0, Math.PI * 2);
        ctx.fillStyle = cs.cursorColor || '#38bdf8';
        ctx.globalAlpha = idleFade * 0.45;
        ctx.fill();

        ctx.beginPath();
        ctx.arc(cPos.x, cPos.y, 6 * cs.cursorSize, 0, Math.PI * 2);
        ctx.fillStyle = '#ffffff';
        ctx.globalAlpha = idleFade * 0.95;
        ctx.fill();
      } else if (cs.cursorStyle === 'crosshair') {
        const r = 14 * cs.cursorSize;
        ctx.strokeStyle = cs.cursorColor || '#38bdf8';
        ctx.lineWidth = 2.5;
        ctx.beginPath();
        ctx.moveTo(cPos.x - r, cPos.y);
        ctx.lineTo(cPos.x + r, cPos.y);
        ctx.moveTo(cPos.x, cPos.y - r);
        ctx.lineTo(cPos.x, cPos.y + r);
        ctx.stroke();
      } else {
        // macos-modern pointer arrow
        ctx.translate(cPos.x, cPos.y);
        ctx.scale(cs.cursorSize, cs.cursorSize);
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.lineTo(0, 22);
        ctx.lineTo(6, 17);
        ctx.lineTo(11, 26);
        ctx.lineTo(15, 24);
        ctx.lineTo(10, 15);
        ctx.lineTo(17, 15);
        ctx.closePath();
        ctx.fillStyle = '#0f172a';
        ctx.fill();
        ctx.strokeStyle = '#ffffff';
        ctx.lineWidth = 2;
        ctx.stroke();
      }
      ctx.restore();
    }

    // 6. COMPOSITE ANIMATED TEXT / TITLE TRACKS (T1..T5)
    const anyTextSolo = project.tracks.some((t) => t.kind === 'text' && t.solo);
    for (const track of project.tracks) {
      if (track.kind !== 'text' || track.muted) continue;
      if (anyTextSolo && !track.solo) continue;

      for (const clip of track.clips) {
        if (
          currentTimeSec < clip.startSec ||
          currentTimeSec > clip.startSec + clip.durationSec ||
          !clip.text
        ) {
          continue;
        }

        const rel = currentTimeSec - clip.startSec;
        const enterP = Math.min(1, rel / Math.max(0.2, clip.fadeInSec || 0.35));
        const exitP = Math.min(
          1,
          (clip.durationSec - rel) / Math.max(0.2, clip.fadeOutSec || 0.35)
        );
        const alpha = Math.max(0, Math.min(enterP, exitP));

        const preset = clip.titlePreset || 'pop-elastic';
        const scaleFactor =
          preset === 'pop-elastic'
            ? 0.75 + 0.25 * applyEasing(enterP, 'spring')
            : 1.0;
        const slideY =
          preset === 'slide-up-blur' ? (1 - applyEasing(enterP, 'easeOutExpo')) * 34 : 0;

        const tx = (clip.posX ?? 0.5) * w;
        const ty = (clip.posY ?? 0.14) * h + slideY;
        const fontSize = Math.round(((clip.fontSize || 42) * h) / 1080);

        ctx.save();
        ctx.translate(tx, ty);
        ctx.scale(scaleFactor, scaleFactor);
        ctx.globalAlpha = alpha;

        const displayMainText =
          preset === 'typewriter'
            ? clip.text.slice(
                0,
                Math.max(1, Math.floor(clip.text.length * Math.min(1, rel / 0.9)))
              )
            : clip.text;

        ctx.font = `800 ${fontSize}px Inter, system-ui, sans-serif`;
        const mainWidth = ctx.measureText(displayMainText).width;
        let subWidth = 0;
        if (clip.subtitleText) {
          ctx.font = `600 ${Math.round(fontSize * 0.48)}px Inter, system-ui, sans-serif`;
          subWidth = ctx.measureText(clip.subtitleText).width;
        }

        const boxW = Math.max(mainWidth, subWidth) + 56;
        const boxH = clip.subtitleText ? fontSize * 2.15 : fontSize * 1.55;

        roundRect2D(ctx, -boxW / 2, -boxH / 2, boxW, boxH, 20);
        ctx.fillStyle = clip.bgColor || 'rgba(15, 23, 42, 0.82)';
        ctx.fill();

        if (preset === 'neon-glow') {
          ctx.shadowColor = clip.textColor || '#34d399';
          ctx.shadowBlur = 18;
        }
        ctx.strokeStyle =
          preset === 'neon-glow'
            ? clip.textColor || '#34d399'
            : 'rgba(148, 163, 184, 0.28)';
        ctx.lineWidth = preset === 'neon-glow' ? 2.5 : 1.5;
        ctx.stroke();
        ctx.shadowBlur = 0;

        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillStyle = clip.textColor || '#ffffff';
        ctx.font = `800 ${fontSize}px Inter, system-ui, sans-serif`;
        ctx.fillText(
          displayMainText,
          0,
          clip.subtitleText ? -fontSize * 0.26 : 0
        );

        if (clip.subtitleText) {
          ctx.fillStyle = '#cbd5e1';
          ctx.font = `600 ${Math.round(fontSize * 0.46)}px Inter, system-ui, sans-serif`;
          ctx.fillText(clip.subtitleText, 0, fontSize * 0.48);
        }

        ctx.restore();
      }
    }

    // 7. COMPOSITE ON-SCREEN KEYSTROKES / ADB GESTURE HUD
    if (cs.showKeystrokesHUD) {
      const activeKey = project.keyEvents.find(
        (k) => nowMs >= k.timestampMs && nowMs <= k.timestampMs + k.durationMs
      );
      if (activeKey) {
        const kElapsed = nowMs - activeKey.timestampMs;
        const kAlpha = Math.min(
          1,
          kElapsed / 140,
          (activeKey.durationMs - kElapsed) / 220
        );

        ctx.save();
        ctx.globalAlpha = Math.max(0, kAlpha);
        ctx.font = `700 ${Math.round((20 * h) / 1080)}px JetBrains Mono, monospace`;
        const textW = ctx.measureText(activeKey.keyCombo).width;
        const padX = 22;
        const badgeW = textW + padX * 2 + 24;
        const badgeH = Math.round((46 * h) / 1080);

        let bx = w - badgeW - 36;
        let by = h - badgeH - 36;
        if (cs.keystrokePosition === 'bottom-left') {
          bx = 36;
        } else if (cs.keystrokePosition === 'bottom-center') {
          bx = (w - badgeW) / 2;
        } else if (cs.keystrokePosition === 'top-right') {
          by = 36;
        }

        roundRect2D(ctx, bx, by, badgeW, badgeH, 12);
        ctx.fillStyle = 'rgba(15, 23, 42, 0.9)';
        ctx.fill();
        ctx.strokeStyle = 'rgba(56, 189, 248, 0.5)';
        ctx.lineWidth = 2;
        ctx.stroke();

        ctx.fillStyle = '#38bdf8';
        ctx.beginPath();
        ctx.arc(bx + 18, by + badgeH / 2, 5, 0, Math.PI * 2);
        ctx.fill();

        ctx.fillStyle = '#f8fafc';
        ctx.textBaseline = 'middle';
        ctx.fillText(activeKey.keyCombo, bx + 32, by + badgeH / 2 + 1);
        ctx.restore();
      }
    }

    // 8. COMPOSITE DYNAMIC KARAOKE SUBTITLES (Word-by-Word Highlight)
    const subStyle = project.subtitleStyle;
    if (subStyle.enabled) {
      const activeSeg = project.subtitles.find(
        (s) => currentTimeSec >= s.startSec && currentTimeSec <= s.endSec
      );
      if (activeSeg && activeSeg.words.length > 0) {
        ctx.save();
        const fontSize = Math.round(((subStyle.fontSize || 32) * h) / 1080);
        ctx.font = `800 ${fontSize}px ${subStyle.fontFamily || 'Inter, system-ui, sans-serif'}`;
        ctx.textBaseline = 'middle';

        const spaceW = ctx.measureText(' ').width * 1.15;
        const wordWidths = activeSeg.words.map((wObj) => ctx.measureText(wObj.word).width);
        const totalTextW =
          wordWidths.reduce((acc, v) => acc + v, 0) +
          spaceW * Math.max(0, activeSeg.words.length - 1);

        const pillW = totalTextW + 48;
        const pillH = fontSize * 1.75;
        const pillX = (w - pillW) / 2;
        const pillY = (subStyle.positionY || 0.87) * h - pillH / 2;

        roundRect2D(ctx, pillX, pillY, pillW, pillH, 16);
        ctx.fillStyle = subStyle.backgroundColor || 'rgba(15, 23, 42, 0.84)';
        ctx.fill();
        ctx.strokeStyle = 'rgba(148, 163, 184, 0.25)';
        ctx.lineWidth = 1.5;
        ctx.stroke();

        let cursorX = pillX + 24;
        const centerY = pillY + pillH / 2;

        activeSeg.words.forEach((wObj, idx) => {
          const isCurrentWord =
            subStyle.wordByWordHighlight &&
            currentTimeSec >= wObj.startSec &&
            currentTimeSec <= wObj.endSec;
          const isPastWord = currentTimeSec > wObj.endSec;
          const wWidth = wordWidths[idx];

          if (isCurrentWord) {
            roundRect2D(
              ctx,
              cursorX - 6,
              centerY - fontSize * 0.62,
              wWidth + 12,
              fontSize * 1.24,
              8
            );
            ctx.fillStyle = 'rgba(56, 189, 248, 0.22)';
            ctx.fill();
          }

          ctx.fillStyle = isCurrentWord
            ? subStyle.activeWordColor || '#38bdf8'
            : wObj.highlight && isPastWord
            ? '#a5b4fc'
            : subStyle.textColor || '#f8fafc';
          ctx.fillText(wObj.word, cursorX, centerY);
          cursorX += wWidth + spaceW;
        });

        ctx.restore();
      }
    }

    // 9. OPTIONAL SAFE-ZONE GUIDES OVERLAY
    if (showSafeZones) {
      ctx.save();
      ctx.strokeStyle = 'rgba(56, 189, 248, 0.45)';
      ctx.lineWidth = 1.5;
      ctx.setLineDash([8, 6]);
      // 90% Title Safe Margin
      ctx.strokeRect(w * 0.05, h * 0.05, w * 0.9, h * 0.9);
      // Center Crosshair
      ctx.beginPath();
      ctx.moveTo(w / 2 - 20, h / 2);
      ctx.lineTo(w / 2 + 20, h / 2);
      ctx.moveTo(w / 2, h / 2 - 20);
      ctx.lineTo(w / 2, h / 2 + 20);
      ctx.stroke();
      ctx.restore();
    }
  }, [
    project,
    currentTimeSec,
    isPlaying,
    internalWidth,
    internalHeight,
    orbitOffset,
    showSafeZones,
    showPrivacyOutlines,
    projectScreenUVToCanvas,
  ]);

  // Interactive Mouse Drag on Viewport (Orbit 3D Device or Drag Privacy Mask)
  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = masterCanvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const clickX = ((e.clientX - rect.left) / rect.width) * canvas.width;
    const clickY = ((e.clientY - rect.top) / rect.height) * canvas.height;

    // Check if user clicked inside an active privacy mask when paused
    if (!isPlaying && onUpdateProject) {
      const activeMasks = project.privacyMasks.filter(
        (m) => currentTimeSec >= m.startSec && currentTimeSec <= m.endSec
      );
      for (const mask of activeMasks) {
        const tl = projectScreenUVToCanvas(mask.x, mask.y, canvas.width, canvas.height);
        const br = projectScreenUVToCanvas(
          mask.x + mask.width,
          mask.y + mask.height,
          canvas.width,
          canvas.height
        );
        const minX = Math.min(tl.x, br.x);
        const maxX = Math.max(tl.x, br.x);
        const minY = Math.min(tl.y, br.y);
        const maxY = Math.max(tl.y, br.y);
        if (clickX >= minX && clickX <= maxX && clickY >= minY && clickY <= maxY) {
          dragStateRef.current = {
            mode: 'mask-move',
            startX: e.clientX,
            startY: e.clientY,
            initialRotX: orbitOffset.rotX,
            initialRotY: orbitOffset.rotY,
            maskId: mask.id,
            initialMaskX: mask.x,
            initialMaskY: mask.y,
          };
          return;
        }
      }
    }

    dragStateRef.current = {
      mode: 'orbit',
      startX: e.clientX,
      startY: e.clientY,
      initialRotX: orbitOffset.rotX,
      initialRotY: orbitOffset.rotY,
    };
  };

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const st = dragStateRef.current;
    if (!st.mode) return;

    const dx = e.clientX - st.startX;
    const dy = e.clientY - st.startY;

    if (st.mode === 'orbit') {
      setOrbitOffset((prev) => ({
        ...prev,
        rotY: Math.max(-55, Math.min(55, st.initialRotY + dx * 0.25)),
        rotX: Math.max(-45, Math.min(45, st.initialRotX + dy * 0.22)),
      }));
    } else if (
      st.mode === 'mask-move' &&
      st.maskId &&
      onUpdateProject &&
      st.initialMaskX !== undefined &&
      st.initialMaskY !== undefined
    ) {
      const newX = Math.max(0, Math.min(0.85, st.initialMaskX + dx * 0.002));
      const newY = Math.max(0, Math.min(0.9, st.initialMaskY + dy * 0.0018));
      onUpdateProject((prev) => ({
        ...prev,
        privacyMasks: prev.privacyMasks.map((m) =>
          m.id === st.maskId ? { ...m, x: Number(newX.toFixed(3)), y: Number(newY.toFixed(3)) } : m
        ),
      }));
    }
  };

  const handleMouseUp = () => {
    dragStateRef.current.mode = null;
  };

  const handleWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 0.06 : -0.06;
    setOrbitOffset((prev) => ({
      ...prev,
      zoomMul: Math.max(0.55, Math.min(2.2, Number((prev.zoomMul + delta).toFixed(2)))),
    }));
  };

  const handleDownloadSnapshot = () => {
    const canvas = masterCanvasRef.current;
    if (!canvas) return;
    const dataUrl = canvas.toDataURL('image/png');
    const a = document.createElement('a');
    a.href = dataUrl;
    a.download = `${project.name.replace(/\s+/g, '_')}_${currentTimeSec.toFixed(1)}s.png`;
    a.click();
  };

  const activePose = interpolateDirectorCamera(project.directorKeyframes, currentTimeSec);

  return (
    <div className="viewport-3d-wrapper">
      {/* Top Floating Viewport HUD Bar */}
      <div className="viewport-hud-bar">
        <div className="viewport-hud-group">
          <span className="viewport-badge-pill">
            <Smartphone size={13} />
            {project.device3D.primaryModel.toUpperCase()}
          </span>
          <span className="viewport-badge-pill subtle">
            <Maximize2 size={12} />
            {exportW}×{exportH} · {project.exportSettings.fps} FPS
          </span>
          {activePose.activeLabel && (
            <span className="viewport-badge-pill camera-active">
              <Sparkles size={12} />
              {activePose.activeLabel}
            </span>
          )}
        </div>

        <div className="viewport-hud-group">
          <button
            type="button"
            className={`viewport-hud-btn ${
              project.device3D.dualLayout !== 'single' ? 'active' : ''
            }`}
            title="Alternar diseño 3D (Móvil Solo / Móvil + PC Lado a Lado / PiP)"
            onClick={() => {
              if (!onUpdateProject) return;
              const order: Array<ProjectFile['device3D']['dualLayout']> = [
                'single',
                'side-by-side',
                'pip-floating',
                'angled-showcase',
              ];
              const nextIdx =
                (order.indexOf(project.device3D.dualLayout) + 1) % order.length;
              onUpdateProject((prev) => ({
                ...prev,
                device3D: { ...prev.device3D, dualLayout: order[nextIdx] },
              }));
            }}
          >
            <Layers size={14} />
            <span>
              {project.device3D.dualLayout === 'single'
                ? '1 Dispositivo'
                : project.device3D.dualLayout === 'side-by-side'
                ? 'Dual: Lado a Lado'
                : project.device3D.dualLayout === 'pip-floating'
                ? 'Dual: PiP 3D'
                : 'Dual: Showcase'}
            </span>
          </button>

          <button
            type="button"
            className={`viewport-hud-btn ${showPrivacyOutlines ? 'active' : ''}`}
            title="Mostrar/ocultar bordes de zonas de privacidad"
            onClick={() => setShowPrivacyOutlines((v) => !v)}
          >
            <Eye size={14} />
          </button>

          <button
            type="button"
            className={`viewport-hud-btn ${showSafeZones ? 'active' : ''}`}
            title="Guías de margen seguro Play Store"
            onClick={() => setShowSafeZones((v) => !v)}
          >
            <Grid size={14} />
          </button>

          {(orbitOffset.rotX !== 0 ||
            orbitOffset.rotY !== 0 ||
            orbitOffset.zoomMul !== 1) && (
            <button
              type="button"
              className="viewport-hud-btn warning"
              title="Restablecer órbita manual 3D"
              onClick={() => setOrbitOffset({ rotX: 0, rotY: 0, zoomMul: 1 })}
            >
              <RotateCcw size={13} />
              <span>Reset Órbita</span>
            </button>
          )}

          <button
            type="button"
            className="viewport-hud-btn primary"
            title="Capturar fotograma actual en PNG de alta resolución"
            onClick={handleDownloadSnapshot}
          >
            <Camera size={14} />
            <span>Capturar PNG</span>
          </button>
        </div>
      </div>

      {/* Master Compositor Stage */}
      <div
        className={`viewport-stage-frame ${
          project.background.type === 'transparent' ||
          project.exportSettings.transparentBackground
            ? 'checkerboard-alpha'
            : ''
        }`}
      >
        <canvas
          ref={setCanvasRef}
          className="viewport-master-canvas"
          style={{
            aspectRatio: `${exportW} / ${exportH}`,
          }}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
          onWheel={handleWheel}
        />
      </div>

      {/* Bottom Interactive Hint Footer */}
      <div className="viewport-footer-hint">
        <span>
          Arrastra en el lienzo para rotar el teléfono 3D · Rueda del ratón para zoom · En pausa,
          arrastra las máscaras 🔒 para moverlas
        </span>
        <span>
          Zoom: {(activePose.zoom * orbitOffset.zoomMul).toFixed(2)}x · Rotación: (
          {Math.round(activePose.rotXDeg + orbitOffset.rotX)}°,{' '}
          {Math.round(activePose.rotYDeg + orbitOffset.rotY)}°)
        </span>
      </div>
    </div>
  );
};
