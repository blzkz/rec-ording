# Plan: iluminación y postprocesado

Apuntado el 2026-11-05 para la siguiente sesión. Partimos de la v0.8.5.

## Punto de partida

**Iluminación.** Hay una sola luz fija para todo:

- una luz principal desde arriba a la izquierda (`light = (-0.45, -0.65, 0.6)`), con brillo especular;
- un «cielo» suave en los modelos 3D (`shade` en `model3d.rs` y `model.wgsl`);
- un reflejo diagonal (*sheen*) sobre la pantalla;
- en el marco integrado: cara frontal, cantos y trasera iluminados con la misma luz.

La sombra es 2D, siempre debajo del móvil (una sombra difusa y otra de contacto), y no depende de la luz. No se puede elegir nada: todos los vídeos tienen el mismo aspecto de estudio.

**Antialiasing.**

- El marco integrado, la pantalla, los toques y los títulos ya tienen bordes suaves, porque se calcula la cobertura de cada píxel.
- Los modelos 3D se dibujan con 2×2 muestras por píxel al exportar, y con 1 al reproducir la vista previa. Con modelos detallados o pequeños se notan dientes de sierra.
- Las texturas de los modelos no tienen mipmaps y parpadean al alejarse.
- La grabación usa mips de nivel fijo, sin interpolar entre niveles, así que puede «saltar» de nitidez durante un zoom.

**Desenfoque de movimiento.**

- Ya existe para giros y desplazamientos rápidos del móvil y de la cámara: obturador de medio fotograma y hasta 12 subfotogramas (`timeline::shutter`).
- Solo se aplica al exportar y en la vista previa en pausa.
- No cubre los zooms ni las animaciones de los modelos, y no se puede ajustar ni desactivar.

**Restricción.** Todo lo nuevo tiene que estar igual en el compositor del procesador (`compositor.rs`, que dibuja la vista previa) y en el de la tarjeta (`gpu/*.wgsl`, que dibuja la exportación). Los tests `gpu::tests` comparan los dos, y cada efecto nuevo necesita su caso.

## 1. Iluminación con estilos

Una sección **«Iluminación»** en el inspector, con estilos para elegir con un clic y un par de ajustes finos.

| Estilo | Idea |
| --- | --- |
| **Estudio** | El actual (por defecto, para que los proyectos no cambien). |
| **Suave** | Luz grande y difusa, sombras claras, casi sin brillos: producto limpio. |
| **Dramática** | Luz principal lateral y fuerte, relleno casi nulo y luz de contorno que recorta los cantos sobre fondos oscuros, como en los anuncios de Apple. |
| **Contraluz** | Luz principal detrás: el móvil oscuro con un borde brillante. Va bien en entradas y giros. |
| **Cálida / Fría** | Luces con color (atardecer, neón) que tiñen cantos y reflejos. |

Ajustes finos:

- dirección de la luz (un control circular o un deslizador de ángulo);
- intensidad;
- «Brillo de pantalla»: cuánto se ve el reflejo sobre el cristal.

Técnicamente:

- Pasar de una luz a un **conjunto de luces** (principal, relleno y contorno, con dirección, color e intensidad, más un ambiente de color superior e inferior) en `Scene`. Lo usarían `shade`, los cantos y la trasera del marco integrado, y las dos versiones (CPU y WGSL).
- **Reflejos del entorno**: el metal y el cristal reflejan un degradado hecho con los colores del fondo. Así el móvil «pertenece» a la escena.
- **Sombra que sigue a la luz**: su posición, alargamiento y suavidad dependen de la dirección de la luz principal. Con contraluz, la sombra va hacia delante.
- Guardado en `Style` (`lighting`, `light_angle`, `light_intensity`), con valores por defecto iguales a hoy. MCP: nuevos campos en `set_style` y el resumen en `get_project`.
- Opcional: **animar la luz** con fotogramas clave (un barrido de luz durante un giro queda muy bien). Sería una propiedad más de la pista de cámara o del móvil.

## 2. Postprocesado

Una sección **«Acabado»** (o dentro de «Exportar», con vista previa) con:

### Desenfoque de movimiento

- Selector Desactivado / Suave / Cinematográfico, que corresponde a obturadores de 90°, 180° y 270°. Hoy está fijo en 180° y aplicado de forma automática.
- Extenderlo a **zooms y desplazamientos de cámara** (cambios de escala y de encuadre) y a las **animaciones de los modelos** (las poses entre subfotogramas).
- Que también se vea **mientras se reproduce** la vista previa, lo que pide el punto 3.

### Antialiasing

- Calidad de exportación: **Normal** (lo de hoy), **Alta** (4×4 muestras en el modelo y supersampling 2× de todo el fotograma en la tarjeta) y **Máxima**.
- **Mipmaps** con interpolación entre niveles (trilineal) para las texturas de los modelos y para la grabación. Así se acaban el parpadeo y los saltos de nitidez en los zooms.
- En la vista previa, un filtro barato (FXAA) cuando se reproduce, y la calidad completa en pausa.
- Antes de nada, medir **dónde están los dientes de sierra** (bordes del modelo, texturas, cantos del marco en giros, título sobre la grabación) con capturas ampliadas, para atacar los que se ven.

### Ideas para decidir

- Resplandor (*bloom*) de la pantalla en escenas oscuras.
- Viñeta.
- Grano.
- Profundidad de campo: el fondo desenfocado cuando la cámara se acerca.
- Corrección de color (cálido, frío, contraste).

Cada uno es poco código en la tarjeta. La pregunta es cuáles aportan sin recargar la interfaz.

## 3. Vista previa en la tarjeta gráfica (base de lo anterior)

Con más luces, supersampling y desenfoque, la vista previa en el procesador irá lenta.

- Dibujarla con `GpuCompositor` usando el dispositivo wgpu que ya tiene la ventana (`frame.wgpu_render_state()`), directamente en una textura de egui, sin leerla de vuelta.
- La vista previa y la exportación quedarían idénticas y el compositor del procesador pasaría a ser solo el plan B (sin tarjeta) y la referencia de los tests.
- Hay que resolver cómo compartir el `Device` y la cola con eframe y cómo subir la grabación (ya decodificada) cada fotograma.

## Fases propuestas

| Fase | Contenido | Comprobación |
| --- | --- | --- |
| P1 | Vista previa en la tarjeta (punto 3) | Igual que la exportación; reproducción fluida |
| P2 | Conjunto de luces y estilos Estudio, Suave, Dramática y Contraluz, sombra que sigue a la luz | Tests CPU = GPU por estilo; capturas |
| P3 | Reflejos del entorno, luces con color, dirección e intensidad; MCP | Capturas, `render_frame` |
| P4 | Antialiasing: mipmaps trilineales, calidad Alta y Máxima, FXAA en reproducción | Capturas ampliadas antes y después; tiempos de exportación |
| P5 | Desenfoque de movimiento ajustable, también en zooms, animaciones y reproducción | Vídeo de prueba con giros y zooms |
| P6 | Extras elegidos (viñeta, grano, *bloom*…) | Capturas |

## Decisiones para mañana

1. ¿Qué estilos de luz queremos (la tabla de arriba es una propuesta)? ¿Animar la luz con fotogramas clave ahora o más adelante?
2. ¿Iluminación y acabado son globales al proyecto o se pueden cambiar a lo largo del vídeo?
3. Calidad de antialiasing por defecto: más calidad hace la exportación más lenta. ¿«Alta» por defecto con la tarjeta y «Normal» sin ella?
4. ¿Qué extras de postprocesado entran (si entra alguno)?
5. ¿Empezamos por la vista previa en la tarjeta (P1)? Es lo que permite ver todo esto en tiempo real.
