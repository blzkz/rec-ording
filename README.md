# rec-ording — Grabación y Edición 3D para Demos de Apps Android y PC

Estudio todo-en-uno para desarrolladores de Android y escritorio: captura pantalla de PC y dispositivos Android por USB (`adb`), edita en una línea de tiempo multipista (4 vídeo, 4 audio, 5 texto), aplica maquetas 3D interactivas (**Pixel 11 Pro**, **Galaxy S26 Ultra**, **Galaxy Tab S10 Ultra** o modelos `.GLB`), planifica movimientos de cámara con **Modo Director / Auto-Zoom**, transcribe subtítulos localmente palabra por palabra, expone un **Servidor MCP** para agentes de IA y exporta hasta en **4K a 60 fps** con **FFmpeg**.

---

## Instalación nativa en Arch Linux / CachyOS (`.pkg.tar.zst`)

El proyecto incluye empaquetado nativo para `pacman` en [`packaging/arch/PKGBUILD`](./packaging/arch/PKGBUILD):

```bash
# Instalar directamente el paquete compilado en CachyOS / Arch Linux:
./install-cachyos.sh
# o manualmente con pacman:
sudo pacman -U rec-ording-0.1.0-1-x86_64.pkg.tar.zst
```

Una vez instalado tendrás disponibles:
- **`rec-ording`**: Aplicación de escritorio nativa (y acceso directo en el lanzador de aplicaciones de KDE/GNOME/Hyprland).
- **`rec-ording-mcp`**: Comando global para conectar el servidor MCP directamente en Claude Desktop, Cursor o Codex.
- **Auto-actualización integrada**: Desde el botón **`v0.1.0 / Actualizar`** de la barra superior puedes comprobar e instalar nuevas versiones publicadas en el repositorio privado [`github.com/blzkz/rec-ording`](https://github.com/blzkz/rec-ording).

---

## Cómo iniciar en modo desarrollo

### 1. Modo Desarrollo Web Rápido (Vite + React + Three.js)
```bash
npm run dev
```
Abre `http://localhost:1420` en tu navegador.

### 2. Modo Escritorio Nativo (Tauri v2 + Rust + FFmpeg + ADB)
```bash
npm run tauri dev
```

### 3. Servidor MCP para Agentes de IA (Claude Desktop, Cursor, Codex)
```bash
npm run mcp-server
```
Configuración para añadir a tu cliente MCP (`claude_desktop_config.json` o `.cursor/mcp.json`):
```json
{
  "mcpServers": {
    "rec-ording": {
      "command": "node",
      "args": ["/home/blzkz/Documentos/pet-projects/rec-ordingApp/scripts/mcp-server.mjs"]
    }
  }
}
```

---

## Arquitectura del Proyecto

- **Frontend 3D & Editor (`src/`)**:
  - [`src/types/project.ts`](./src/types/project.ts): Esquema completo serializable del proyecto (`.recproj.json`).
  - [`src/utils/deviceModels3D.ts`](./src/utils/deviceModels3D.ts): Generador 3D en Three.js para **Pixel 11 Pro**, **Galaxy S26 Ultra**, **Galaxy Tab S10 Ultra**, ventana de escritorio y cargador de archivos `.GLB`/`.GLTF`.
  - [`src/utils/proceduralDemoTextures.ts`](./src/utils/proceduralDemoTextures.ts): Sintetizador de texturas interactivas de alta resolución y soporte para texturas de vídeo reales.
  - [`src/components/Viewport3DCanvas.tsx`](./src/components/Viewport3DCanvas.tsx): Escenario 3D con interpolación de fotogramas clave de cámara, modo dual (PC + Android), desenfoque de movimiento (*motion blur*), cursor suavizado, ondas táctiles, máscaras de privacidad, títulos animados, HUD de teclas y subtítulos karaoke.
  - [`src/components/MultiTrackTimeline.tsx`](./src/components/MultiTrackTimeline.tsx): Línea de tiempo con 4 pistas de vídeo (`V1..V4`), 4 de audio (`A1..A4`), 5 de texto (`T1..T5`), carril de Cámara Director y carril de Zonas Sensibles.
  - [`src/components/RecordingModal.tsx`](./src/components/RecordingModal.tsx): Estudio de captura para pantalla de PC, ventana, simulador o teléfono/tablet Android por USB (`adb`) con registro de toques/gestos y auto-zoom.
  - [`src/components/RightInspectorPanel.tsx`](./src/components/RightInspectorPanel.tsx): Inspector de 5 pestañas (`3D & Estilo`, `Director`, `Subtítulos`, `MCP / IA`, `Exportar`).
- **Backend Nativo (`src-tauri/`)**:
  - [`src-tauri/src/lib.rs`](./src-tauri/src/lib.rs): Integración nativa con `ffmpeg`, `adb devices`, `adb shell screenrecord`, `adb shell getevent -lt` y Whisper local.
- **Servidor MCP (`scripts/mcp-server.mjs`)**:
  - [`scripts/mcp-server.mjs`](./scripts/mcp-server.mjs): Servidor JSON-RPC 2.0 sobre `stdio` con 9 herramientas para importar grabaciones, editar pistas, cambiar maquetas 3D, aplicar auto-zoom y renderizar con FFmpeg manteniendo el archivo `.recproj.json` editable en la app.
