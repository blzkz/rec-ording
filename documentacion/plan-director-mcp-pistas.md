# Plan: Director, varias pistas y servidor MCP

Apuntado el 2026-11-04 e **implementado en la v0.7.0** (fases F1–F6; F7, música y títulos, queda pendiente). Decisiones tomadas: MCP por archivo (opción A, la app recarga los cambios en menos de un segundo), varios clips seguidos en una pista (sin vídeos simultáneos), Director 3D moderado con control de intensidad.

## Punto de partida (v0.6.4, rama `modelos-3d`)

- Un solo vídeo por proyecto, con recorte de inicio y fin.
- La cámara solo hace **zoom 2D** (nivel + punto de enfoque) en tramos ("zooms"), creados a partir de los toques (taps y swipes) o a mano.
- El ángulo 3D del dispositivo es **uno para todo el vídeo** (inclinación, giro y rotación), más tres efectos globales: enderezar al hacer zoom, flotar y animación de entrada. **No se pueden programar giros a mitad del vídeo.**
- Solo se guarda un proyecto (autoguardado en `~/.local/share/rec-ording/proyecto.json`).
- No hay deshacer ni servidor MCP (el de la v0.2.0 se retiró porque trabajaba sobre el modelo falso).

## Objetivo

1. **Movimientos de cámara y del dispositivo en cualquier momento**: fotogramas clave de zoom, encuadre y giro 3D, con curvas de suavizado.
2. **Director**, al estilo del de [Matte](https://matte.app/help/director/): marcas objetivos sobre el vídeo y se generan los planos de cámara solos.
3. **Varias pistas**: varios clips de vídeo en secuencia, y más adelante música y títulos.
4. **Servidor MCP**: todo lo anterior (y lo que ya existe) controlable por un agente externo (Claude, Cursor, Codex…).

## 1. Base: comandos, deshacer y proyectos con nombre

Antes de lo demás, porque la interfaz, el Director y el MCP deben hacer los cambios por el mismo camino.

- **Capa de comandos** (`src/commands.rs`): cada cambio del proyecto es un `Command` (añadir clip, mover fotograma clave, cambiar fondo…) que se aplica al `Project`. La interfaz y el MCP solo emiten comandos.
- **Deshacer / rehacer** (Ctrl+Z / Ctrl+Mayús+Z) guardando el estado anterior por comando (el proyecto es pequeño: basta con copias). Cada llamada MCP es un paso de deshacer, como en Matte.
- **Proyectos con nombre**: carpeta `~/.local/share/rec-ording/proyectos/<nombre>.recproj.json`, con Nuevo / Abrir / Duplicar en la interfaz. El autoguardado sigue funcionando sobre el proyecto abierto. Necesario para que un agente cree y abra proyectos.
- **Migración** del `proyecto.json` actual al nuevo formato (versión 4).

## 2. Pista de cámara con fotogramas clave

Sustituye a los "zooms" actuales sin perder lo que ya funciona.

- **Modelo**: `CameraClip { start, end, kind, keyframes }`, con `kind` = Zoom automático (de toques), Zoom manual, Director o Libre. Cada `Keyframe { time, zoom, focus_x, focus_y, tilt, turn, roll, easing }`.
- **Suavizado** por fotograma clave: muelle (el actual), suave (ease in/out), lineal y mantener. Más adelante, un editor de curvas.
- **Evaluación**: la cámara interpola entre fotogramas clave. El muelle actual queda como un tipo de suavizado más, para que los zooms automáticos se vean igual que hoy.
- **Los giros 3D pasan a ser animables**: la inclinación, el giro y la rotación del estilo se convierten en el valor por defecto, y los fotogramas clave los modifican en el tiempo. Así se puede, por ejemplo, empezar de frente, girar a los 5 s y volver de frente a los 8 s.
- **Interfaz**:
  - Fila "Cámara" en la línea de tiempo, con los clips y sus fotogramas clave en forma de rombo.
  - Botón "Añadir fotograma clave" en el cursor, que copia la cámara actual.
  - Al seleccionar un rombo, el inspector muestra sus valores (zoom, encuadre, ángulos, suavizado).
- **Migración**: cada zoom actual pasa a ser un clip de dos fotogramas clave.

## 3. Pista del dispositivo (movimiento del modelo)

- Fotogramas clave propios del dispositivo: posición (desplazamiento), escala y ángulo. Se suman a los de la cámara, igual que en Matte se añade una animación del dispositivo encima de la de cámara.
- Ajustes preestablecidos de un clic: "Entrada girando" (la animación de entrada actual), "Giro completo", "Asomarse desde abajo", "Salida".
- Las animaciones del propio modelo 3D (glTF) se pueden colocar como clip en esta pista para elegir cuándo empiezan, en vez de empezar siempre al principio.

## 4. Director

- **Modo Director** (botón en la barra de la línea de tiempo):
  - La vista previa se pone de frente y aparece una barra encima de la línea de tiempo.
  - Arrastrando sobre la vista previa se dibuja un **objetivo** en el momento del cursor: un rectángulo en coordenadas del vídeo.
  - Cada objetivo lleva un número y su marca en la regla, y solo puede haber uno por momento.
  - Los objetivos se pueden mover y redimensionar, y las marcas de la regla se arrastran para cambiar su momento.
- **Generar** ("Hecho") convierte cada objetivo en un plano (un clip de cámara de tipo Director):
  - **3D**: ángulo, órbita y zoom hacia el objetivo, con variedad entre planos alternando lados y alturas.
  - **2D**: zoom y desplazamiento planos.
  - El zoom sale del tamaño del rectángulo: pequeño → primer plano; casi toda la pantalla → solo un cambio de ángulo suave.
  - Las transiciones entre planos son suaves, con duración según el espacio disponible entre objetivos.
- **Rehacer** un plano (otra variación del movimiento), **rehacer todos**, **borrar** y **borrar todos**.
- Si los planos nuevos se solapan con clips de cámara existentes, se pregunta si reemplazarlos (y se puede deshacer).
- Si se cambia el formato (16:9, 9:16…), los planos se recomponen a partir de los objetivos.
- El resultado son fotogramas clave normales, editables a mano. Volver a generar los recalcula todos, así que los retoques a mano conviene hacerlos al final.
- **Objetivos sugeridos**: proponer objetivos a partir de los toques registrados, como punto de partida.

## 5. Varias pistas

### 5a. Varios clips de vídeo (prioritario)

- **Modelo**: `media: Vec<Media>` (grabaciones e importaciones) y una pista de vídeo con `Clip { media_id, start (en la línea de tiempo), source_in, source_out, speed }`. Los toques siguen ligados a cada grabación y se trasladan al tiempo de la línea.
- **Edición**: añadir otra grabación al final o en el cursor, cuchilla (dividir), recortar los bordes, reordenar arrastrando y borrar. Las transiciones entre clips son de corte o fundido.
- **Vista previa**: el decodificador cambia de archivo según el clip activo, con un proxy por grabación y miniaturas por clip.
- **Exportación**: decodificar tramo a tramo (`-ss`/`-t` por clip) y unir el audio con `concat`.
- **Velocidad por tramo** (acelerar partes lentas): más adelante.

### 5b. Audio y títulos (después)

- Pista de música con volumen, fundidos y bajada automática de la música mientras suena el móvil.
- Pista de títulos y textos animados (intro, llamada a la acción), con varios estilos.

## 6. Servidor MCP

- **Arranque**: `rec-ording --mcp`, JSON-RPC 2.0 por stdio con el protocolo MCP (`initialize`, `tools/list`, `tools/call`). Sin dependencias externas: el mismo binario.
- **Sincronización con la app abierta**. Opciones (a decidir):
  - **A (simple)**: el servidor edita el archivo del proyecto y la app abierta detecta el cambio y lo recarga. La app no puede editar a la vez: se usa un campo de versión para avisar de conflictos.
  - **B (en vivo)**: si la app está abierta, el servidor MCP le reenvía los comandos por un socket local (`$XDG_RUNTIME_DIR/rec-ording.sock`) y el agente ve los cambios al instante en la vista previa. Si no está abierta, trabaja sobre el archivo sin interfaz.
  - **Recomendación**: empezar por A y pasar a B cuando funcione la capa de comandos.
- **Herramientas** (todas pasan por la capa de comandos):
  - Proyecto: `list_projects`, `create_project`, `open_project`, `get_project` (resumen en JSON), `rename_project`, `undo`, `redo`.
  - Captura e importación: `list_devices`, `start_recording`, `stop_recording`, `import_video`.
  - Línea de tiempo: `add_clip`, `split_clip`, `trim_clip`, `move_clip`, `delete_clip`, `set_clip_speed`.
  - Cámara: `list_camera`, `add_zoom`, `add_camera_keyframe`, `update_camera_keyframe`, `delete_camera_keyframe`, `auto_zoom_from_touches`.
  - Director: `director_add_target`, `director_list_targets`, `director_generate` (3D/2D), `director_retake`, `director_clear`.
  - Dispositivo: `list_devices_models` (marcos y galería 3D), `set_device`, `import_model`, `set_model_screen`, `set_angle`, `add_device_keyframe`, `set_device_motion`.
  - Estilo: `set_aspect`, `set_background`, `set_layout` (margen, sombra), `set_touches`, `set_video_fit`.
  - Ver: `render_frame(time)`, que devuelve un PNG como contenido de imagen para que el agente vea el resultado, y `get_touches`.
  - Exportar: `export` (formato, resolución, fps, calidad) y `export_status`.
- **Seguridad**: solo stdio local; las rutas que pasa el agente se validan, y las exportaciones van a la carpeta configurada salvo que se indique otra.
- **Interfaz**: una sección "Asistentes (MCP)" en Ajustes con la configuración para copiar en Claude Desktop, Cursor o Codex. Reutilizar el texto de la v0.2.0 (`git show 1dabd7c:src/mcp.rs`) solo como referencia de formato.
- **Tests**: un test de integración que habla JSON-RPC con `rec-ording --mcp`, crea un proyecto, importa un vídeo, añade un plano y exporta.

## Orden propuesto

| Fase | Contenido | Por qué en este orden |
| --- | --- | --- |
| F1 | Comandos, deshacer y proyectos con nombre | Base de todo lo demás, sobre todo del MCP |
| F2 | Pista de cámara con fotogramas clave y giros 3D animables | Resuelve "no puedo poner giros entre medias" |
| F3 | Director | Usa la pista de cámara de F2 |
| F4 | Pista del dispositivo | Pequeña una vez hecha F2 |
| F5 | Varios clips de vídeo | Cambio grande del modelo y de la exportación |
| F6 | Servidor MCP | Expone F1–F5 de una vez y con la API ya estable |
| F7 | Música y títulos | Opcional |

Cada fase acaba con tests, paquete y commit en su rama.

## Decisiones pendientes para mañana

1. **MCP**: ¿empezamos con la opción A (archivo) o vamos directamente a B (en vivo con la app abierta)?
2. **Varias pistas**: ¿basta con varios clips seguidos en una pista de vídeo, o quieres también vídeos simultáneos (por ejemplo, dos móviles a la vez o el móvil con la pantalla del ordenador)?
3. **Director 3D**: ¿movimientos atrevidos (órbitas amplias) o discretos por defecto?
4. ¿Se fusionan antes las ramas `redesign-v0.3.0` y `modelos-3d` en `main` (ahora no hay nada subido ni PR)?
