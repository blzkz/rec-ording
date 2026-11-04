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

- Galería con miniaturas renderizadas: cinco modelos gratuitos de Poly Pizza (CC0 y CC BY 3.0, créditos en `assets/models/CREDITS.md`) y los que importe el usuario, que se copian a `~/.local/share/rec-ording/models` junto con las texturas y `.bin` que usen (vale tanto para `.glb` como para `.gltf`).
- Detección de pantalla más robusta: rectángulo mínimo (no le afecta la densidad de vértices de un agujero de cámara) y modelos low-poly pintados con una textura de paleta, que se separan en piezas por color.
- Ajuste del vídeo en la pantalla del modelo: Rellenar (con encuadre), Encajar (bandas negras) o Estirar.
- Animaciones glTF de nodos (traslación, rotación y escala; interpolación lineal, por pasos o cúbica), en bucle o una vez.
- Sigue sin soportarse: compresión Draco, esqueletos (skinning) y morph targets.

## v0.6.2: importación más robusta y gestión de la galería

- Se leen modelos que exigen extensiones como `KHR_texture_transform` (habitual en exportaciones de Blender), que antes se rechazaban; las extensiones desconocidas se toleran salvo las de geometría comprimida (Draco, meshopt, cuantización).
- Se implementa `KHR_texture_transform` (desplazamiento, giro y escala de texturas).
- La separación de piezas por color solo se aplica a modelos low-poly de paleta (uno o dos materiales), no a modelos detallados con decenas de materiales.
- Importar valida el modelo antes de copiarlo: un modelo que no carga no entra en la galería. Se puede importar el `.zip` de la descarga.
- Renombrar y eliminar modelos importados (clic derecho o desde «Ajustes del modelo»), con confirmación; los que no cargan se marcan en la galería.

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

## Ideas para siguientes versiones

- Desenfocar zonas sensibles (correos, tokens).
- Fondos con imagen propia.
- Recortar y acelerar tramos intermedios (no solo el inicio y el final).
- Música de fondo (lo que queda de la fase F7 del plan).
- Tarjeta final con el icono de la app y la insignia de Google Play.
- Exportar aún más rápido: solapar el trabajo de la tarjeta de un fotograma con la preparación del siguiente.
- Vista previa dibujada en la tarjeta gráfica.
- Control en vivo por MCP (socket local) en lugar de por archivo.
- Grabar el escritorio además del móvil.
