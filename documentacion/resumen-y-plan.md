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

## Ideas para siguientes versiones

- Títulos y textos animados sobre el vídeo (intro, llamada a la acción).
- Desenfocar zonas sensibles (correos, tokens).
- Fondos con imagen propia.
- Recortar y acelerar tramos intermedios (no solo el inicio y el final).
- Servidor MCP sobre el nuevo modelo de proyecto, para que un asistente pueda montar el vídeo.
- Grabar el escritorio además del móvil.
