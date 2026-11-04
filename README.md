# rec-ording

Graba tu móvil Android por USB y conviértelo en un vídeo promocional listo para publicar: tu app dentro de un marco de móvil, sobre un fondo cuidado, con zooms suaves que siguen cada toque.

Pensado para la ficha de Google Play, YouTube, Reels/Shorts o redes, sin pasar por un editor de vídeo.

## Cómo se usa

1. **Graba.** Conecta el móvil por USB (con la depuración USB activada), pulsa **Grabar** y usa tu app con normalidad. rec-ording registra dónde tocas.
   ¿Ya tienes una grabación? Pulsa **Importar** o arrastra el vídeo a la ventana.
2. **Ajusta.** Elige formato (16:9, 9:16, 1:1, 4:5), modelo (redondeado, recto, curvo, tablet o sin marco) y color del dispositivo, fondo, margen y sombra.
   En **Modelos 3D** tienes una galería de móviles en 3D (cinco incluidos, gratuitos) y puedes **importar** los tuyos en `.glb`, `.gltf` o directamente el `.zip` de la descarga (las texturas y `.bin` sueltos se copian a tu galería con el modelo; si el modelo no se puede usar, no se añade). Con clic derecho sobre un modelo importado puedes renombrarlo o eliminarlo: rec-ording busca la pantalla, endereza el modelo y proyecta tu grabación en ella. En «Ajustes del modelo» eliges la pieza que hace de pantalla, cómo encaja el vídeo (**Rellenar** recortando, **Encajar** con bandas negras o **Estirar**) y, si el modelo trae animaciones, cuál reproducir y si se repite. Los modelos con licencia CC BY piden mencionar a su autor al publicar el vídeo (ver `assets/models/CREDITS.md`).
   En **Ángulo 3D** puedes girar e inclinar el móvil (con cantos y reflejos reales), hacer que se enderece al hacer zoom, que flote suavemente o que entre girando al empezar.
   Los zooms se crean solos a partir de los toques; puedes moverlos, alargarlos, cambiar su nivel o elegir dónde enfocan haciendo clic en la vista previa.
3. **Monta.** La línea de tiempo tiene cuatro pistas:
   - **Vídeo**: varias grabaciones seguidas. Arrastra los bordes de un clip para recortarlo, el clip para cambiarlo de orden, y **Dividir** (Ctrl+B) para cortarlo en el cursor. **Añadir vídeo** pone otro al final.
   - **Cámara**: zooms, **movimientos** con fotogramas clave (zoom, encuadre, inclinación, giro y rotación, con llegada en muelle, suave, lineal o corte) y planos del **Director**. Así puedes girar el móvil a mitad del vídeo.
   - **Móvil**: movimientos del dispositivo: **giro 360°** sobre el eje vertical, **voltereta 360°** sobre el horizontal, entrada girando, caída desde arriba, asomarse desde abajo, saltito y salida. En los giros eliges vueltas, sentido y duración; si los pones junto a un cambio de clip, se centran en él y el corte ocurre mientras se ve la parte de atrás del móvil. Los movimientos rápidos llevan desenfoque de movimiento.
   - **Texto**: títulos al estilo de los anuncios de Apple, junto al móvil (que se aparta para dejarles sitio), arriba, abajo o en el centro. Un titular (una línea por salto de línea) y, si quieres, una línea más pequeña debajo; tamaño grande para cifras («100 / ciudades reales.»). Entran apareciendo, palabra a palabra o línea a línea (las anteriores se atenúan). **+ Texto**, la tecla T o doble clic en la pista.
   El **Director** funciona como en Matte: mueve el cursor a cada momento importante, arrastra sobre la vista previa lo que debe verse y pulsa **Generar planos**. Elige movimiento 3D (ángulos y órbitas) o 2D (zoom plano), su intensidad, y pide **otra toma** de un plano si no te convence.
   Todo se puede deshacer (Ctrl+Z) y rehacer (Ctrl+Mayús+Z). Con **Proyectos** creas y cambias de proyecto.
4. **Exporta** en MP4 (H.264 o HEVC) o GIF, a 720p, 1080p o 4K, a 30 o 60 fps, con el sonido del móvil. Si tienes una tarjeta gráfica compatible (NVIDIA con NVENC, AMD o Intel con VAAPI en Linux, AMF o Quick Sync en Windows), el vídeo se codifica con ella; si falla, se vuelve a hacer con el procesador. Los fotogramas se siguen dibujando con el procesador.

### Atajos

| Tecla | Acción |
| --- | --- |
| Espacio | Reproducir / pausar |
| Z | Añadir un zoom en el cursor |
| T | Añadir un texto en el cursor |
| Ctrl+B | Dividir el clip en el cursor |
| Ctrl+Z / Ctrl+Mayús+Z | Deshacer / rehacer |
| Supr / Retroceso | Eliminar lo seleccionado (clip, movimiento, texto, fotograma clave u objetivo) |
| Esc | Quitar la selección (ver el resultado) o salir del Director |
| ← / → | Fotograma anterior / siguiente (con Mayús: 1 segundo) |
| Inicio / Fin | Ir al principio / al final |

## Requisitos

```bash
sudo pacman -S ffmpeg android-tools scrcpy zenity
```

- **ffmpeg**: leer y exportar vídeo (imprescindible).
- **scrcpy** (recomendado): graba con sonido y sin límite de duración. Sin scrcpy se usa `adb screenrecord` (máximo 3 minutos, sin sonido).
- **android-tools** (`adb`): detectar el móvil y registrar los toques.
- **zenity** o **kdialog**: diálogos para elegir archivos (también puedes arrastrar vídeos a la ventana).

Las grabaciones y exportaciones se guardan en `~/Vídeos/rec-ording`. Los proyectos se guardan solos en `~/.local/share/rec-ording/proyectos/`.

## Asistente (MCP)

Un agente (Claude, Cursor, Codex…) puede montar el vídeo por ti con `rec-ording --mcp`, un servidor MCP por stdio que edita el proyecto abierto en la app (la app muestra sus cambios al momento y se pueden deshacer). El botón **Asistente** de la app muestra la configuración lista para copiar. Por ejemplo, en Claude Code:

```bash
claude mcp add rec-ording -- /usr/bin/rec-ording --mcp
```

Herramientas: proyectos (`get_project`, `create_project`, `open_project`, `undo`…), grabar e importar, editar clips, zooms y fotogramas clave de cámara, Director, dispositivo y modelos 3D, estilo, `render_frame` (devuelve una imagen del resultado) y `export`.

## Instalación

```bash
./install-cachyos.sh
```

o compilando a mano:

```bash
cargo build --release
./target/release/rec-ording
```

### Actualizaciones

Al abrirse, la app mira si hay una versión nueva en las releases de [blzkz/rec-ording-releases](https://github.com/blzkz/rec-ording-releases), un repositorio público solo para los paquetes (el del código es privado). Si la hay, aparece el botón **Actualización** en la barra:

- en Linux instala el paquete con `pkexec pacman -U` (pide tu contraseña);
- en Windows reemplaza los archivos de la carpeta de rec-ording;
- en una copia compilada a mano, abre la página de descarga.

Después basta con reiniciar. Las descargas se comprueban con la suma SHA-256 que publica GitHub.

Para publicar una versión, sube una etiqueta `vX.Y.Z` (con la versión de `Cargo.toml` y del `PKGBUILD`). Los workflows publican el paquete de Arch y el .zip de Windows en el repositorio de releases, usando como notas el mensaje del commit. Necesitan el secret `RELEASES_TOKEN`: un token con permiso *Contents: read and write* sobre `blzkz/rec-ording-releases`.

### Windows

Descarga `rec-ording-<versión>-windows-x64.zip`, descomprímelo y abre `rec-ording.exe`. Lo genera el workflow **Windows** de GitHub Actions en cada cambio de `main`, y en cada etiqueta `v*` lo añade a la release. Instala las herramientas con:

```powershell
winget install Gyan.FFmpeg
winget install Google.PlatformTools
winget install Genymobile.scrcpy
```

En Windows las grabaciones se guardan en `.mkv` (scrcpy no se puede detener limpiamente desde otro proceso, y un MKV cortado sigue funcionando). Los proyectos están en `%APPDATA%\rec-ording`.

Para compilarlo desde Linux (necesita `rustup target add x86_64-pc-windows-gnu` y `mingw-w64-gcc`):

```bash
./packaging/windows/build-local.sh
```

## Cómo está hecho

Aplicación nativa en Rust con [egui](https://github.com/emilk/egui).

| Módulo | Qué hace |
| --- | --- |
| `capture.rs` | Graba con scrcpy (o `adb screenrecord`) y lee los toques de la pantalla táctil con `getevent`, sincronizados con el vídeo. |
| `timeline.rs` | Línea de tiempo: clips, cámara y movimientos del móvil por fotogramas clave, zoom automático, Director y todas las operaciones de edición (las usan la app y el MCP). |
| `mcp.rs` | Servidor MCP (`rec-ording --mcp`). |
| `compositor.rs` | Dibuja cada fotograma: fondo, sombra, el móvil en 3D (perspectiva, grosor e iluminación), la grabación y los toques. La vista previa y la exportación usan el mismo código. |
| `media.rs` | Lee vídeos con ffprobe/ffmpeg, crea una copia ligera para que la vista previa sea fluida y genera las miniaturas. |
| `model3d.rs` | Carga modelos glTF/GLB (con animaciones de nodos), encuentra y endereza la pantalla y los rasteriza con perspectiva, texturas e iluminación. |
| `library.rs` | Galería de modelos: los incluidos en `assets/models` y los importados, con miniaturas renderizadas. |
| `export.rs` | Decodifica la grabación, compone cada fotograma y codifica el resultado con ffmpeg. |
| `app/`, `theme.rs` | Interfaz: visor, línea de tiempo, inspector y hojas. |
| `text.rs` | Títulos: maquetación con Inter Display (incluida en `assets/fonts`, licencia OFL), animaciones de entrada y salida, y el sitio que el móvil les deja. |
| `hwenc.rs` | Codificación con la tarjeta gráfica (NVENC, AMF, Quick Sync, VAAPI): prueba qué funciona en el equipo. |
| `update.rs` | Busca versiones nuevas en las releases y las instala (pacman en Linux, reemplazando la carpeta en Windows). |
| `platform.rs` | Lo que cambia entre Linux y Windows: carpetas, diálogos de archivos, abrir archivos y lanzar herramientas sin ventana de consola. |

Tests: `cargo test` (incluye una prueba que monta y exporta un vídeo a través del servidor MCP). Con un vídeo o un móvil reales:

```bash
REC_TEST_VIDEO=/ruta/video.mp4 cargo test exports_real_video -- --ignored
REC_TEST_SERIAL=<serial de adb> cargo test records_from_device -- --ignored
REC_TEST_VIDEO=/ruta/video.mp4 REC_TEST_MODEL=/ruta/movil.glb cargo test exports_real_video -- --ignored
```
