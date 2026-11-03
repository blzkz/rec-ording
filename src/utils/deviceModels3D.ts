import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { Device3DConfig, DeviceModelType } from '../types/project';

export interface DeviceRig3D {
  modelType: DeviceModelType;
  group: THREE.Group;
  screenMesh: THREE.Mesh | null;
  screenAspect: number; // width / height in portrait/default
  setScreenTexture: (texture: THREE.Texture) => void;
  updateMaterialConfig: (config: Device3DConfig) => void;
  dispose: () => void;
}

const gltfLoader = new GLTFLoader();

/**
 * Creates a 2D rounded rectangle THREE.Shape centered at (0, 0).
 */
function createRoundedRectShape(width: number, height: number, radius: number): THREE.Shape {
  const r = Math.min(radius, width * 0.45, height * 0.45);
  const x = -width / 2;
  const y = -height / 2;
  const shape = new THREE.Shape();

  shape.moveTo(x + r, y);
  shape.lineTo(x + width - r, y);
  shape.quadraticCurveTo(x + width, y, x + width, y + r);
  shape.lineTo(x + width, y + height - r);
  shape.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
  shape.lineTo(x + r, y + height);
  shape.quadraticCurveTo(x, y + height, x, y + height - r);
  shape.lineTo(x, y + r);
  shape.quadraticCurveTo(x, y, x + r, y);

  return shape;
}

/**
 * Normalizes planar UV coordinates of a ShapeGeometry so that textures map 0..1 across width/height.
 */
function normalizeShapeUVs(geometry: THREE.BufferGeometry, width: number, height: number): void {
  const pos = geometry.attributes.position;
  const uvs = new Float32Array(pos.count * 2);
  for (let i = 0; i < pos.count; i++) {
    const px = pos.getX(i);
    const py = pos.getY(i);
    uvs[i * 2] = (px + width / 2) / width;
    uvs[i * 2 + 1] = (py + height / 2) / height;
  }
  geometry.setAttribute('uv', new THREE.BufferAttribute(uvs, 2));
}

/**
 * Creates an extruded rounded chassis mesh centered in Z.
 */
function createExtrudedChassis(
  width: number,
  height: number,
  depth: number,
  cornerRadius: number,
  bevelThickness: number,
  material: THREE.Material
): THREE.Mesh {
  const shape = createRoundedRectShape(width, height, cornerRadius);
  const extrudeSettings: THREE.ExtrudeGeometryOptions = {
    depth: depth,
    bevelEnabled: bevelThickness > 0,
    bevelSegments: 5,
    steps: 1,
    bevelSize: bevelThickness,
    bevelThickness: bevelThickness,
    curveSegments: 24,
  };
  const geom = new THREE.ExtrudeGeometry(shape, extrudeSettings);
  geom.translate(0, 0, -depth / 2);
  geom.computeVertexNormals();
  const mesh = new THREE.Mesh(geom, material);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

/**
 * Builds a procedural or GLB-based 3D device mockup rig.
 */
export function buildDeviceRig3D(
  modelType: DeviceModelType,
  config: Device3DConfig,
  initialTexture?: THREE.Texture
): DeviceRig3D {
  const group = new THREE.Group();
  group.name = `device-rig-${modelType}`;

  const bodyMat = new THREE.MeshStandardMaterial({
    color: new THREE.Color(config.bodyColor || '#1e293b'),
    metalness: config.frameMetalness ?? 0.85,
    roughness: config.frameRoughness ?? 0.22,
  });

  const darkAccentMat = new THREE.MeshStandardMaterial({
    color: new THREE.Color('#090d16'),
    metalness: 0.4,
    roughness: 0.15,
  });

  const chromeMat = new THREE.MeshStandardMaterial({
    color: new THREE.Color('#cbd5e1'),
    metalness: 0.95,
    roughness: 0.12,
  });

  const lensGlassMat = new THREE.MeshPhysicalMaterial({
    color: new THREE.Color('#020617'),
    metalness: 0.1,
    roughness: 0.05,
    reflectivity: 1.0,
    clearcoat: 1.0,
  });

  const screenMat = new THREE.MeshBasicMaterial({
    color: 0xffffff,
    map: initialTexture || null,
    toneMapped: false,
  });

  let screenMesh: THREE.Mesh | null = null;
  let screenAspect = 9 / 19.5;

  if (modelType === 'pixel-11-pro') {
    // Google Pixel 11 Pro dimensions in scene units
    const w = 2.32;
    const h = 4.88;
    const depth = 0.22;
    const radius = Math.max(0.18, Math.min(0.45, config.screenCornerRadius * 3.8));
    const bezel = Math.max(0.03, Math.min(0.16, config.bezelWidth * 1.8));
    screenAspect = w / h;

    // Metallic rounded chassis
    const chassis = createExtrudedChassis(w, h, depth, radius, 0.035, bodyMat);
    group.add(chassis);

    // Front dark bezel inset
    const bezelShape = createRoundedRectShape(w - 0.03, h - 0.03, radius * 0.94);
    const bezelGeom = new THREE.ShapeGeometry(bezelShape, 24);
    const bezelMesh = new THREE.Mesh(bezelGeom, darkAccentMat);
    bezelMesh.position.z = depth / 2 + 0.036;
    group.add(bezelMesh);

    // Front active OLED display plane
    const sw = w - bezel * 2;
    const sh = h - bezel * 2;
    const screenShape = createRoundedRectShape(sw, sh, Math.max(0.08, radius - bezel));
    const screenGeom = new THREE.ShapeGeometry(screenShape, 24);
    normalizeShapeUVs(screenGeom, sw, sh);
    screenMesh = new THREE.Mesh(screenGeom, screenMat);
    screenMesh.name = 'screen_display';
    screenMesh.position.z = depth / 2 + 0.039;
    group.add(screenMesh);

    // Front punch-hole selfie camera + speaker slit
    const punchGeom = new THREE.CircleGeometry(0.052, 24);
    const punchMesh = new THREE.Mesh(punchGeom, lensGlassMat);
    punchMesh.position.set(0, h / 2 - bezel - 0.14, depth / 2 + 0.042);
    group.add(punchMesh);

    // Iconic Rear Horizontal Pill Camera Bar (Pixel 11 Pro visor)
    const visorW = w - 0.28;
    const visorH = 0.72;
    const visorMesh = createExtrudedChassis(visorW, visorH, 0.12, 0.34, 0.02, chromeMat);
    visorMesh.position.set(0, h / 2 - 0.88, -depth / 2 - 0.05);
    group.add(visorMesh);

    // Rear glass pill inside visor
    const pillGlassShape = createRoundedRectShape(visorW - 0.14, visorH - 0.14, 0.28);
    const pillGlassGeom = new THREE.ShapeGeometry(pillGlassShape, 24);
    const pillGlassMesh = new THREE.Mesh(pillGlassGeom, lensGlassMat);
    pillGlassMesh.rotation.y = Math.PI;
    pillGlassMesh.position.set(0, h / 2 - 0.88, -depth / 2 - 0.132);
    group.add(pillGlassMesh);

    // 3 Rear camera lenses inside the visor + flash
    [-0.52, -0.05, 0.42].forEach((lx) => {
      const ringGeom = new THREE.CylinderGeometry(0.15, 0.15, 0.02, 24);
      ringGeom.rotateX(Math.PI / 2);
      const ringMesh = new THREE.Mesh(ringGeom, bodyMat);
      ringMesh.position.set(lx, h / 2 - 0.88, -depth / 2 - 0.14);
      group.add(ringMesh);
    });

    // Side Power & Volume buttons on right edge
    const pwrBtn = createExtrudedChassis(0.06, 0.38, 0.08, 0.02, 0.01, chromeMat);
    pwrBtn.position.set(w / 2 + 0.035, 0.85, 0);
    group.add(pwrBtn);

    const volBtn = createExtrudedChassis(0.06, 0.76, 0.08, 0.02, 0.01, chromeMat);
    volBtn.position.set(w / 2 + 0.035, 0.12, 0);
    group.add(volBtn);
  } else if (modelType === 'galaxy-s26-ultra') {
    // Samsung Galaxy S26 Ultra: sharp corners, flat titanium rails, 4 floating camera rings on back
    const w = 2.42;
    const h = 5.02;
    const depth = 0.21;
    const radius = 0.11; // Characteristic Ultra sharp titanium corners
    const bezel = Math.max(0.025, Math.min(0.12, config.bezelWidth * 1.4));
    screenAspect = w / h;

    const chassis = createExtrudedChassis(w, h, depth, radius, 0.025, bodyMat);
    group.add(chassis);

    const bezelShape = createRoundedRectShape(w - 0.025, h - 0.025, radius * 0.9);
    const bezelGeom = new THREE.ShapeGeometry(bezelShape, 20);
    const bezelMesh = new THREE.Mesh(bezelGeom, darkAccentMat);
    bezelMesh.position.z = depth / 2 + 0.026;
    group.add(bezelMesh);

    const sw = w - bezel * 2;
    const sh = h - bezel * 2;
    const screenShape = createRoundedRectShape(sw, sh, Math.max(0.04, radius - bezel * 0.8));
    const screenGeom = new THREE.ShapeGeometry(screenShape, 20);
    normalizeShapeUVs(screenGeom, sw, sh);
    screenMesh = new THREE.Mesh(screenGeom, screenMat);
    screenMesh.name = 'screen_display';
    screenMesh.position.z = depth / 2 + 0.029;
    group.add(screenMesh);

    // Tiny Infinity-O punch-hole camera
    const punchGeom = new THREE.CircleGeometry(0.045, 24);
    const punchMesh = new THREE.Mesh(punchGeom, lensGlassMat);
    punchMesh.position.set(0, h / 2 - bezel - 0.12, depth / 2 + 0.032);
    group.add(punchMesh);

    // 4 Individual Raised Titanium Camera Rings on back + Laser AF sensor
    const mainRings = [
      { x: 0.62, y: h / 2 - 0.52, r: 0.21 },
      { x: 0.62, y: h / 2 - 1.04, r: 0.21 },
      { x: 0.62, y: h / 2 - 1.56, r: 0.21 },
      { x: 0.14, y: h / 2 - 1.32, r: 0.15 },
      { x: 0.14, y: h / 2 - 0.68, r: 0.13 },
    ];
    mainRings.forEach((c) => {
      const ringGeom = new THREE.CylinderGeometry(c.r, c.r, 0.09, 28);
      ringGeom.rotateX(Math.PI / 2);
      const ringMesh = new THREE.Mesh(ringGeom, chromeMat);
      ringMesh.position.set(c.x, c.y, -depth / 2 - 0.04);
      group.add(ringMesh);

      const glassGeom = new THREE.CylinderGeometry(c.r * 0.82, c.r * 0.82, 0.095, 28);
      glassGeom.rotateX(Math.PI / 2);
      const glassMesh = new THREE.Mesh(glassGeom, lensGlassMat);
      glassMesh.position.set(c.x, c.y, -depth / 2 - 0.042);
      group.add(glassMesh);
    });

    // S-Pen slot accent at bottom left
    const spenSlot = createExtrudedChassis(0.32, 0.04, 0.09, 0.015, 0.005, chromeMat);
    spenSlot.position.set(-w / 2 + 0.42, -h / 2 - 0.015, 0);
    group.add(spenSlot);
  } else if (modelType === 'galaxy-tab-s10-ultra') {
    // Flagship 16:10 Android Tablet
    const w = 6.2;
    const h = 3.88;
    const depth = 0.14;
    const radius = 0.24;
    const bezel = 0.14;
    screenAspect = w / h;

    const chassis = createExtrudedChassis(w, h, depth, radius, 0.02, bodyMat);
    group.add(chassis);

    const bezelShape = createRoundedRectShape(w - 0.03, h - 0.03, radius * 0.92);
    const bezelGeom = new THREE.ShapeGeometry(bezelShape, 24);
    const bezelMesh = new THREE.Mesh(bezelGeom, darkAccentMat);
    bezelMesh.position.z = depth / 2 + 0.021;
    group.add(bezelMesh);

    const sw = w - bezel * 2;
    const sh = h - bezel * 2;
    const screenShape = createRoundedRectShape(sw, sh, Math.max(0.08, radius - bezel * 0.7));
    const screenGeom = new THREE.ShapeGeometry(screenShape, 24);
    normalizeShapeUVs(screenGeom, sw, sh);
    screenMesh = new THREE.Mesh(screenGeom, screenMat);
    screenMesh.name = 'screen_display';
    screenMesh.position.z = depth / 2 + 0.025;
    group.add(screenMesh);

    // Subtle top camera notch + dual selfie lenses
    const notchShape = createRoundedRectShape(0.68, 0.12, 0.05);
    const notchGeom = new THREE.ShapeGeometry(notchShape, 16);
    const notchMesh = new THREE.Mesh(notchGeom, darkAccentMat);
    notchMesh.position.set(0, h / 2 - bezel - 0.03, depth / 2 + 0.028);
    group.add(notchMesh);

    // Magnetic S-Pen charging strip on back
    const stripShape = createRoundedRectShape(2.4, 0.18, 0.08);
    const stripGeom = new THREE.ShapeGeometry(stripShape, 16);
    const stripMesh = new THREE.Mesh(stripGeom, darkAccentMat);
    stripMesh.rotation.y = Math.PI;
    stripMesh.position.set(-1.1, h / 2 - 0.45, -depth / 2 - 0.022);
    group.add(stripMesh);
  } else if (modelType === 'desktop-window') {
    // Sleek floating 3D Desktop Window Frame with glassmorphic titlebar and macOS/Linux dots
    const w = 6.4;
    const h = 4.15;
    const headerH = 0.28;
    const depth = 0.11;
    const radius = 0.18;
    const border = 0.06;
    screenAspect = w / (h - headerH);

    const chassis = createExtrudedChassis(w, h, depth, radius, 0.018, bodyMat);
    group.add(chassis);

    // Window header bar accent
    const headerShape = createRoundedRectShape(w - border * 2, headerH, 0.08);
    const headerGeom = new THREE.ShapeGeometry(headerShape, 16);
    const headerMat = new THREE.MeshStandardMaterial({
      color: new THREE.Color('#0f172a'),
      metalness: 0.4,
      roughness: 0.3,
    });
    const headerMesh = new THREE.Mesh(headerGeom, headerMat);
    headerMesh.position.set(0, h / 2 - border - headerH / 2, depth / 2 + 0.02);
    group.add(headerMesh);

    // 3 Window Traffic Light Dots
    const dotColors = ['#ef4444', '#f59e0b', '#10b981'];
    dotColors.forEach((hex, idx) => {
      const dotGeom = new THREE.CircleGeometry(0.055, 20);
      const dotMat = new THREE.MeshBasicMaterial({ color: hex });
      const dotMesh = new THREE.Mesh(dotGeom, dotMat);
      dotMesh.position.set(
        -w / 2 + border + 0.22 + idx * 0.17,
        h / 2 - border - headerH / 2,
        depth / 2 + 0.024
      );
      group.add(dotMesh);
    });

    // Screen content plane below header
    const sw = w - border * 2;
    const sh = h - headerH - border * 2.5;
    const screenShape = createRoundedRectShape(sw, sh, 0.1);
    const screenGeom = new THREE.ShapeGeometry(screenShape, 20);
    normalizeShapeUVs(screenGeom, sw, sh);
    screenMesh = new THREE.Mesh(screenGeom, screenMat);
    screenMesh.name = 'screen_display';
    screenMesh.position.set(0, -headerH / 2, depth / 2 + 0.022);
    group.add(screenMesh);
  } else if (modelType === 'custom-glb') {
    // Placeholder while loading or if no GLB path provided yet
    const w = 2.4;
    const h = 4.9;
    screenAspect = w / h;
    const placeholderChassis = createExtrudedChassis(w, h, 0.2, 0.28, 0.03, bodyMat);
    group.add(placeholderChassis);

    const sw = w - 0.14;
    const sh = h - 0.14;
    const screenShape = createRoundedRectShape(sw, sh, 0.22);
    const screenGeom = new THREE.ShapeGeometry(screenShape, 24);
    normalizeShapeUVs(screenGeom, sw, sh);
    screenMesh = new THREE.Mesh(screenGeom, screenMat);
    screenMesh.name = 'screen_display';
    screenMesh.position.z = 0.135;
    group.add(screenMesh);

    if (config.customGlbPath) {
      gltfLoader.load(
        config.customGlbPath,
        (gltf) => {
          // Clear procedural placeholder and normalize loaded GLB
          while (group.children.length > 0) {
            group.remove(group.children[0]);
          }
          const root = gltf.scene;
          const box = new THREE.Box3().setFromObject(root);
          const size = new THREE.Vector3();
          const center = new THREE.Vector3();
          box.getSize(size);
          box.getCenter(center);

          const maxDim = Math.max(size.x, size.y, size.z, 0.001);
          const scale = 4.9 / maxDim;
          root.scale.setScalar(scale);
          root.position.sub(center.multiplyScalar(scale));

          const targetHint = (config.customGlbScreenMeshName || '').toLowerCase().trim();
          let matchedMesh: THREE.Mesh | null = null;
          let fallbackMesh: THREE.Mesh | null = null;
          let largestArea = 0;

          root.traverse((child) => {
            if ((child as THREE.Mesh).isMesh) {
              const m = child as THREE.Mesh;
              m.castShadow = true;
              m.receiveShadow = true;
              const lowerName = (m.name || '').toLowerCase();
              if (
                (targetHint && lowerName.includes(targetHint)) ||
                lowerName.includes('screen') ||
                lowerName.includes('display') ||
                lowerName.includes('monitor') ||
                lowerName.includes('pantalla')
              ) {
                matchedMesh = m;
              }
              m.geometry.computeBoundingBox();
              if (m.geometry.boundingBox) {
                const s = new THREE.Vector3();
                m.geometry.boundingBox.getSize(s);
                const area = s.x * s.y;
                if (area > largestArea) {
                  largestArea = area;
                  fallbackMesh = m;
                }
              }
            }
          });

          const chosen = matchedMesh || fallbackMesh;
          if (chosen) {
            (chosen as THREE.Mesh).material = screenMat;
            screenMesh = chosen as THREE.Mesh;
          }
          group.add(root);
        },
        undefined,
        () => {
          // Keep procedural fallback if GLB fails to load
        }
      );
    }
  } else {
    // 'none': frameless floating rounded video card with subtle depth shadow
    const w = 2.35;
    const h = 4.85;
    screenAspect = w / h;
    const screenShape = createRoundedRectShape(w, h, 0.24);
    const screenGeom = new THREE.ShapeGeometry(screenShape, 24);
    normalizeShapeUVs(screenGeom, w, h);
    screenMesh = new THREE.Mesh(screenGeom, screenMat);
    screenMesh.name = 'screen_display';
    group.add(screenMesh);
  }

  // Apply portrait vs landscape orientation for phone/none models
  if (
    config.orientation === 'landscape' &&
    (modelType === 'pixel-11-pro' || modelType === 'galaxy-s26-ultra' || modelType === 'none')
  ) {
    group.rotation.z = -Math.PI / 2;
  }

  return {
    modelType,
    group,
    screenMesh,
    screenAspect,
    setScreenTexture: (texture: THREE.Texture) => {
      if (screenMat.map !== texture) {
        screenMat.map = texture;
        screenMat.needsUpdate = true;
      }
    },
    updateMaterialConfig: (nextConfig: Device3DConfig) => {
      bodyMat.color.set(nextConfig.bodyColor || '#1e293b');
      bodyMat.metalness = nextConfig.frameMetalness ?? 0.85;
      bodyMat.roughness = nextConfig.frameRoughness ?? 0.22;
      bodyMat.needsUpdate = true;
    },
    dispose: () => {
      group.traverse((obj) => {
        if ((obj as THREE.Mesh).isMesh) {
          const m = obj as THREE.Mesh;
          m.geometry?.dispose();
        }
      });
      bodyMat.dispose();
      darkAccentMat.dispose();
      chromeMat.dispose();
      lensGlassMat.dispose();
      screenMat.dispose();
    },
  };
}
