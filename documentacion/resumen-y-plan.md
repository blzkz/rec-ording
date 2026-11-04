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

## Ideas para siguientes versiones

- **Plan detallado para la siguiente sesión**: Director, pista de cámara con giros animables, varias pistas y servidor MCP en [`plan-director-mcp-pistas.md`](plan-director-mcp-pistas.md).

- Títulos y textos animados sobre el vídeo (intro, llamada a la acción).
- Desenfocar zonas sensibles (correos, tokens).
- Fondos con imagen propia.
- Recortar y acelerar tramos intermedios (no solo el inicio y el final).
- Servidor MCP sobre el nuevo modelo de proyecto, para que un asistente pueda montar el vídeo.
- Grabar el escritorio además del móvil.
