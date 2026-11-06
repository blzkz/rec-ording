# rec-ording — Estado y siguientes pasos

## v0.3.0: rediseño

La versión 0.2.0 tenía mucha interfaz pero poco funcionamiento real:

- La vista previa y la exportación pintaban una pantalla de app inventada; el vídeo grabado nunca aparecía en el resultado.
- La "transcripción Whisper" devolvía siempre las mismas frases fijas.
- Sin móvil conectado, la app se inventaba dispositivos "simulados".
- Los toques se escalaban suponiendo una pantalla de 1080×2400 y la grabación se cortaba sin cerrar bien el MP4.
- La interfaz mezclaba 13 pistas, un servidor MCP, un autoactualizador y textos técnicos de cara al usuario.

La 0.3.0 se reconstruye alrededor de un flujo de tres pasos (grabar → ajustar → exportar):

- Grabación real con scrcpy (o `adb screenrecord`), con detección de la pantalla táctil, su rango de coordenadas y la orientación, y sincronización de los toques con el inicio del vídeo.
- Un único compositor para la vista previa y la exportación (lo que ves es lo que exportas), con marco de móvil, fondo degradado, sombra, indicadores de toque y filtrado de calidad al reducir la grabación.
- Zooms automáticos a partir de los toques, editables en la línea de tiempo y en la vista previa, con movimiento de cámara amortiguado.
- Interfaz en la línea de las apps de Apple: barra de herramientas mínima, inspector por secciones, línea de tiempo con miniaturas y hojas modales para grabar y exportar.

Se han retirado, de momento, el servidor MCP, el autoactualizador, los subtítulos falsos, las máscaras de privacidad, los títulos y el modo dual PC + móvil.

## v0.4.0: móvil en 3D

- Ángulo 3D con perspectiva real: inclinación, giro y rotación, con accesos rápidos (Frente, Izquierda, Derecha, Tumbado, Dinámico).
- El móvil tiene grosor: los cantos se ven e iluminan al girarlo, y la pantalla tiene un reflejo sutil.
- Se endereza solo al hacer zoom, para que la app se lea; opcionalmente flota o entra girando al empezar.
- Nuevos modelos: Curvo (pantalla con bordes curvados) y Tablet (marco ancho, cámara en el marco).

## v0.5.0: modelos 3D propios

- Importación de modelos `.glb` / `.gltf` (escena con jerarquía de nodos, materiales PBR básicos, texturas PNG/JPEG y transparencias).
- Detección de la pieza de pantalla por nombre (screen, display, pantalla…) o, si no hay pistas, como la superficie plana más adelantada; se puede cambiar a mano.
- El modelo se endereza y escala solo alrededor de su pantalla, y usa la misma cámara, ángulo 3D, zooms y toques que los marcos integrados.
- Render por software con perspectiva, iluminación de estudio y suavizado de bordes (2×2 al exportar y en pausa).

## v0.6.0: galería de modelos, encaje del vídeo y animaciones

- Galería con miniaturas renderizadas: cinco modelos gratuitos de Poly Pizza (CC0 y CC BY 3.0; desde v0.11 se sustituyen por el Galaxy S26 Ultra) y los que importe el usuario, que se copian a `~/.local/share/rec-ording/models` junto con las texturas y `.bin` que usen (vale tanto para `.glb` como para `.gltf`).
- Detección de pantalla más robusta: rectángulo mínimo (no le afecta la densidad de vértices de un agujero de cámara) y modelos low-poly pintados con una textura de paleta, que se separan en piezas por color.
- Ajuste del vídeo en la pantalla del modelo: Rellenar (con encuadre), Encajar (bandas negras) o Estirar.
- Animaciones glTF de nodos (traslación, rotación y escala; interpolación lineal, por pasos o cúbica), en bucle o una vez.
- Sigue sin soportarse: compresión Draco, esqueletos (skinning) y morph targets.

## v0.6.2: importación más robusta y gestión de la galería

- Se leen modelos que exigen extensiones como `KHR_texture_transform` (habitual en exportaciones de Blender), que antes se rechazaban; las extensiones desconocidas se toleran salvo las de geometría comprimida (Draco, meshopt, cuantización).
- Se implementa `KHR_texture_transform` (desplazamiento, giro y escala de texturas).
- La separación de piezas por color solo se aplica a modelos low-poly de paleta (uno o dos materiales), no a modelos detallados con decenas de materiales.
- Importar valida el modelo antes de copiarlo: un modelo que no carga no entra en la galería. Se puede importar el `.zip` de la descarga.
- Renombrar y eliminar modelos importados (clic derecho o, desde v0.10, el lápiz y la papelera de la sección), con confirmación; los que no cargan se marcan en la galería.

## v0.6.3: pantalla automática

- La pieza detectada como pantalla es la opción por defecto: aparece la primera del desplegable con la etiqueta «RECOMENDADA» y es la que se usa al elegir un modelo. Elegir otra a mano queda indicado, con un enlace para volver a la recomendada. Las elecciones guardadas por versiones anteriores se descartan (numeraban las piezas de otra forma).

## v0.7.0: Director, pistas y asistente (MCP)

- Proyectos con nombre (`~/.local/share/rec-ording/proyectos/`) y migración automática del proyecto de versiones anteriores (se conserva una copia `proyecto.v3.bak.json`).
- Deshacer y rehacer de todo, también de los cambios hechos por el asistente.
- Pista de vídeo con varios clips seguidos: recortar, dividir, reordenar y eliminar; exportación con el audio de todos los clips unido.
- Pista de cámara con fotogramas clave: zoom, encuadre e inclinación, giro y rotación animables en cualquier momento, con llegada en muelle, suave, lineal o corte. Los zooms anteriores pasan a ser clips de esta pista.
- Pista del móvil: movimientos del dispositivo (entrada, giro completo, asomarse, saltito, salida) editables por fotogramas clave. Los marcos integrados tienen ahora parte trasera (con módulo de cámaras) para los giros completos.
- Director: objetivos dibujados sobre la vista previa en cada momento, planos 3D o 2D con intensidad, otra toma por plano o de todos; avisa si sustituye movimientos que se solapan.
- Servidor MCP (`rec-ording --mcp`) con más de 40 herramientas, incluida `render_frame`; la app muestra al momento los cambios del asistente. Hoja «Asistente» con la configuración para copiar.

## v0.8.0: textos, giros 360° y gráfica

- Pista de texto: títulos al estilo Apple junto al móvil, arriba, abajo o en el centro; tamaño grande para cifras; entrada al aparecer, palabra a palabra o línea a línea; el móvil se aparta mientras se ven. Tipografía Inter Display incluida, la misma en la vista previa, la exportación y Windows. Un halo suave del tono contrario los mantiene legibles sobre la grabación.
- Giros 360° sobre el eje vertical y volteretas sobre el horizontal, con vueltas, sentido y duración; junto a un cambio de clip se centran en él para ocultar el corte. Nueva entrada «Caída desde arriba». Desenfoque de movimiento en los movimientos rápidos (exportación y vista previa en pausa).
- Codificación con la tarjeta gráfica (NVENC, AMF, Quick Sync, VAAPI), activada por defecto si hay una que funcione, con vuelta al procesador si falla. Ahorra poco tiempo: lo que más cuesta es dibujar los fotogramas, que sigue haciendo el procesador.
- MCP: `add_text`, `update_text`, `delete_text`; `add_device_motion` acepta `degrees` y `duration`; `export` acepta `hardware`.

## v0.8.1: exportar con la tarjeta gráfica

- Los fotogramas de la exportación se dibujan en la tarjeta gráfica (wgpu: Vulkan, DirectX 12 o Metal) con shaders que portan el compositor paso a paso: fondo, sombras, marco integrado con cantos y trasera, modelo 3D con su iluminación, pantalla, toques y desenfoque de movimiento. Un test compara cada caso con el procesador (diferencia media inferior a 0,01 niveles).
- Un fotograma 1080p pasa de 8,8 ms a 0,8 ms con el marco integrado y de 28,5 ms a 2,1 ms con un modelo 3D. Una exportación de 16 s a 1080p60 con el S26 pasa de 41 s a unos 6 s (con el marco integrado, de 16 s a 6 s).
- Con la tarjeta, los títulos se ponen encima en la propia tarjeta (se preparan en otro hilo) y el fotograma sale ya convertido a YUV 4:2:0: se leen 3 MB por fotograma en lugar de 8 y ffmpeg no tiene que convertir. Descodificar, preparar las mips, dibujar y codificar van en hilos distintos, en paralelo. La exportación de 16 s con el S26 queda en unos 6 s.
- Colores: todas las rutas exportan en BT.709, rango TV, con las etiquetas correspondientes (antes x264 convertía con BT.601 sin etiquetar).
- Si no hay tarjeta (o es un adaptador por software) o falla algo, se exporta con el procesador.
- Arreglado: con el modelo de canto, algunos píxeles se quedaban sin modelo.

- Los workflows de GitHub Actions solo se lanzan al subir una etiqueta `v*` o a mano, para no gastar minutos del repositorio privado.

## v0.8.2: nombre, icono y márgenes

- La app se llama rec·ording, con el punto en rojo como la luz de una cámara grabando (el paquete y el ejecutable siguen siendo `rec-ording`).
- Icono propio (SVG y PNG instalados en el tema hicolor, y dentro del .exe en Windows): ya aparece en la barra de tareas y el lanzador.
- El panel derecho ya no se sale por la derecha: el desplegable de la pantalla del modelo recorta los nombres largos en lugar de ensanchar todo el panel.

## v0.8.3: icono en todos los tamaños y barra de desplazamiento

- El icono se instala en todos los tamaños habituales (16 a 512 px) y en `/usr/share/pixmaps`. Si KDE no lo muestra tras instalar por primera vez, hay que reiniciar Plasma o volver a iniciar sesión: Plasma guarda en memoria que el icono no existía.
- La barra de desplazamiento del panel derecho tiene su propio carril y ya no tapa los controles.

## v0.8.4: más MCP

- `update_device_motion`: cambia un movimiento del móvil ya añadido (inicio, duración y, en los giros, grados y sentido).
- `get_hardware`: qué tarjeta gráfica dibuja y qué codificador por hardware hay.
- `get_project` incluye el ajuste `hardware` de la exportación.

## v0.9.0: luz, acabado, montaje y vista previa en la tarjeta

Todo el plan de `plan-iluminacion-postproceso.md`, más las mejoras pedidas tras montar el vídeo de Form PDF Reader:

- **Iluminación** con seis estilos (Estudio, Suave, Dramática, Contraluz, Cálida y Fría), dirección, intensidad y reflejo. Un conjunto de luces (principal, relleno, contorno y ambiente) en el marco integrado y en los modelos 3D, y la sombra sigue a la luz.
- **Acabado**: desenfoque de movimiento ajustable (también en zooms, encuadres y animaciones de modelos), bordes Normal, Alta o Máxima (supersampling y mips trilineales), viñeta, grano, fundidos de entrada y salida (imagen y sonido) y fundido entre clips.
- **Vista previa en la tarjeta gráfica**: el mismo compositor que la exportación, en el dispositivo de la ventana y sin leer la imagen de vuelta. Con desenfoque y bordes completos también al reproducir. Si la tarjeta falla, vuelve sola al procesador (`REC_CPU_PREVIEW=1` lo fuerza).
- **Tiempos reales de la grabación**: con la pantalla quieta no hay fotogramas; ahora se mantiene el último en vez de saltar al siguiente, en la exportación, `render_frame` y la vista previa.
- **Montaje**: velocidad por clip, congelar fotograma, cambiar el vídeo de un clip conservando todo, y que recortar, mover o borrar clips mantenga cada zoom, texto y movimiento en su momento de la grabación (borrar un clip dice qué se pierde).
- **Marcas** de cambios de pantalla en la pista de vídeo; textos, zooms y movimientos se ajustan a ellas.
- **Textos** que el móvil mira al apartarse (`face_text`) y duración del movimiento; curva elegible en los movimientos del móvil.
- **Grabación**: barra de estado limpia (modo demo de Android) y `play_gestures` para tocar y escribir en el móvil desde el agente, registrando cada gesto.
- **MCP**: parámetro `project` y nombre del proyecto en cada respuesta, `batch`, `export_texts`/`import_texts`, `set_clip_speed`, `freeze_frame`, `set_freeze_duration`, `replace_clip_video`, `detect_marks` y `mark`, `render_strip`, `render_preview`, `play_gestures`, iluminación y acabado en `set_style`, y reinicio automático con `notifications/tools/list_changed` al actualizar.

## v0.10.2: inicio, panel e iconos

- La app abre en una pantalla de inicio: proyecto nuevo, abrir un proyecto (de la lista o un archivo) y los cinco últimos con una imagen de su vídeo, dibujada como la exportación y guardada en caché hasta que el proyecto cambia. **Proyectos** guarda el proyecto abierto y vuelve al inicio. Si el asistente abre o crea un proyecto mientras se ve el inicio, la app lo muestra.
- Panel de ajustes más ancho (360 px), que se puede ensanchar arrastrando su borde y ocultar (botón arriba a la derecha o Ctrl+4). Los controles segmentados reparten el ancho según sus etiquetas y, si no caben junto al título, pasan a la línea de abajo: ya no se montan.
- Modelos 3D: lápiz y papelera junto al título (activos con un modelo importado seleccionado); los ajustes del modelo pasan a **Ajustes avanzados**, plegados dentro de la sección.
- Barra de la línea de tiempo con iconos agrupados y su nombre y atajo al pasar el ratón; en los inspectores, la papelera va junto al título y las acciones del clip (dividir, congelar, cambiar vídeo) son iconos. La velocidad del clip es un control segmentado.
- El renderizador por software de Windows (WARP, «Microsoft Basic Render Driver») ya no se toma por una tarjeta gráfica: sin drivers se usa el procesador, que es más rápido y fiable.
- Publicación: un solo workflow (`release.yml`) prueba y compila Arch y Windows y, con una etiqueta `v*`, publica los dos paquetes en una sola release del repositorio público `blzkz/rec-ording-releases`.

## v0.11.0: Galaxy S26 Ultra, aviso de actualización y paquetes para más distribuciones

- La galería incluida solo trae el **Galaxy S26 Ultra** (de Sagar Modi en Sketchfab, CC BY 4.0, con su crédito en `assets/models/CREDITS.md` y en el panel). Los cinco modelos de Poly Pizza ya no se instalan.
- Al encontrar una versión nueva, la app abre una ventana que la anuncia y ofrece actualizar (en cuanto no hay otra ventana abierta ni una grabación o exportación en marcha).
- Paquetes `.deb` (Debian, Ubuntu, Mint…), `.rpm` (Fedora, openSUSE) y AppImage, además del de Arch y el .zip de Windows. Se compilan en Ubuntu 22.04 (glibc 2.35) con `packaging/linux/build-packages.sh`. El actualizador reconoce cómo se instaló la app (pacman, dpkg, rpm o AppImage) y usa el paquete que le corresponde.
- Grabar un **emulador** de Android Studio: aparece como «Emulador · <nombre del AVD>» y se escuchan todas sus pantallas táctiles virtuales (tiene once y los clics de la ventana pueden llegar a cualquiera). Su codificador no repite el último fotograma con la pantalla quieta, así que al terminar se alarga el vídeo hasta lo que duró la grabación. Probado con un AVD de Android 16 (API 36).
- Con AppImage, la configuración MCP que muestra el Asistente apunta al archivo `.AppImage` y no a su montaje temporal.

## v0.12.0: descargas sin releases de GitHub

- Las versiones ya no se publican como releases de GitHub, que siempre muestran unos archivos «Source code». El workflow genera una página de descargas (`packaging/site/make-site.py`) con los paquetes de cada sistema, sus sumas SHA-256, las novedades y los créditos, y la publica en GitHub Pages del repositorio público: https://blzkz.github.io/rec-ording-releases/
- El actualizador lee el `latest.json` de esa página. Las notas de la versión son el mensaje de la etiqueta anotada y la app las muestra como texto, sin las marcas de Markdown.
- Las copias 0.10.2 y 0.11.0 buscan en las releases de GitHub y no ven esta versión: se instala a mano una vez y desde ahí se actualiza sola.

## v0.13.0: imágenes y pistas de sonido

- Pista de **Imágenes**: PNG, WebP o JPEG con transparencia sobre el vídeo (bajo los títulos), con posición, tamaño, opacidad y fundidos; se colocan arrastrándolas en la vista previa. Se dibujan en la misma capa que los títulos, así que el procesador y la tarjeta gráfica dan el mismo resultado.
- Pistas de **Efectos**, **Voz en off** y **Música**. Cada una admite varios sonidos solapados (carriles), con recorte por los bordes, volumen, fundidos y bucle, y se pueden pasar de una pista a otra arrastrándolos. La música baja sola mientras habla la voz (sidechaincompress) y hay un volumen por fuente (grabación, efectos, voz, música).
- Un solo grafo de ffmpeg (`audio::graph`) mezcla todo para la exportación y para la vista previa, que ahora suena (cpal sobre ALSA/PipeWire en Linux, WASAPI en Windows). La mezcla de la vista previa se recalcula al soltar el ratón tras un cambio.
- Al recortar o mover clips de vídeo, imágenes y sonidos siguen a su momento de la grabación, como los textos.
- MCP: `add_image`, `update_image`, `delete_image`, `add_sound`, `update_sound`, `delete_sound` y `set_mix`; `add_image` y `add_sound` aceptan `mark`.

## Ideas para siguientes versiones

- Luz animada con fotogramas clave (un barrido de luz durante un giro).
- Resplandor de la pantalla, profundidad de campo y corrección de color.
- Pellizcar en `play_gestures` (adb solo inyecta un dedo; haría falta una pantalla táctil virtual).

- Desenfocar zonas sensibles (correos, tokens).
- Fondos con imagen propia.
- Música de fondo (lo que queda de la fase F7 del plan).
- Tarjeta final con el icono de la app y la insignia de Google Play.
- Exportar aún más rápido: solapar el trabajo de la tarjeta de un fotograma con la preparación del siguiente.
- Control en vivo por MCP (socket local) en lugar de por archivo.
- Grabar el escritorio además del móvil.
