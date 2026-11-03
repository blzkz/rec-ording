# rec-ording v0.2.0 — Estudio 3D 100% Rust Nativo (egui + Vulkan `wgpu` + Rayon Multihilo + FFmpeg + ADB + MCP)

Aplicación nativa de escritorio para Linux (**CachyOS / Arch Linux**) escrita **100 % en Rust puro** — **cero Node.js, cero JavaScript y cero WebView**.

---

## Arquitectura 100% Rust + Vulkan + Multihilo Real

- **Interfaz y Texturas GPU (`egui` + `eframe` con backend `wgpu` Vulkan)**:
  - Renderizado inmediato sin recolector de basura ni procesos de navegador en segundo plano.
- **Motor 3D y Composición Paralela en todos los núcleos (`rayon` + `gltf`)** ([`src/renderer3d.rs`](./src/renderer3d.rs)):
  - Trazado de rayos 3D y sombreado PBR paralelizado por líneas de escaneo (`par_chunks_mut`) en todos los hilos lógicos de la CPU.
  - Maquetas 3D de **Google Pixel 11 Pro**, **Samsung Galaxy S26 Ultra**, **Galaxy Tab S10 Ultra**, **Ventana 3D de PC** e importador nativo de archivos **`.GLB` / `.GLTF`** mediante el crate `gltf`.
  - Disposición dual en 3D (PC + Android simultáneamente), **Modo Director** con físicas *spring* y **Auto-Zoom** desde toques, *motion blur* por velocidad de cámara, máscaras de privacidad (*blur*, pixelado, sólido), ondas táctiles, estelas de gestos swipe, títulos animados, HUD de teclas y subtítulos palabra por palabra.
- **Hilos de Trabajo Dedicados (`std::thread` + `crossbeam-channel`)** ([`src/workers.rs`](./src/workers.rs)):
  - **Captura Multihilo**: hilos independientes para `ffmpeg -f x11grab` (pantalla/ventana PC), `adb shell screenrecord` (vídeo USB Android) y `adb shell getevent -lt` (telemetría táctil en tiempo real).
  - **Tubería Directa a FFmpeg (`pipe:0`)**: renderiza lotes de fotogramas en paralelo con `rayon` (`into_par_iter`) e inyecta los buffers RGBA crudos directamente en el `stdin` de `/usr/bin/ffmpeg` hasta en **4K (3840×2160) a 60 fps** en **H.264**, **HEVC**, **AV1**, **VP9/ProRes con transparencia alpha**, **GIF** y **PNG**.
- **Servidor MCP Compilado en el Propio Binario Rust (`rec-ording --mcp`)** ([`src/mcp.rs`](./src/mcp.rs)):
  - Ejecuta `rec-ording --mcp` (o `/usr/bin/rec-ording-mcp`) para conectar Claude Desktop, Cursor o Codex por `stdio` JSON-RPC 2.0 sin instalar Node.js.

---

## Instalación en CachyOS / Arch Linux

```bash
./install-cachyos.sh
# o directamente:
sudo pacman -U rec-ording-0.2.0-1-x86_64.pkg.tar.zst
```

## Compilación manual desde código fuente

```bash
cargo build --release
./target/release/rec-ording
```
