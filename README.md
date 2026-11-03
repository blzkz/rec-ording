# rec-ording

Graba tu móvil Android por USB y conviértelo en un vídeo promocional listo para publicar: tu app dentro de un marco de móvil, sobre un fondo cuidado, con zooms suaves que siguen cada toque.

Pensado para la ficha de Google Play, YouTube, Reels/Shorts o redes, sin pasar por un editor de vídeo.

## Cómo se usa

1. **Graba.** Conecta el móvil por USB (con la depuración USB activada), pulsa **Grabar** y usa tu app con normalidad. rec-ording registra dónde tocas.
   ¿Ya tienes una grabación? Pulsa **Importar** o arrastra el vídeo a la ventana.
2. **Ajusta.** Elige formato (16:9, 9:16, 1:1, 4:5), marco y color del dispositivo, fondo, margen y sombra.
   Los zooms se crean solos a partir de los toques; puedes moverlos, alargarlos, cambiar su nivel o elegir dónde enfocan haciendo clic en la vista previa.
3. **Exporta** en MP4 (H.264 o HEVC) o GIF, a 720p, 1080p o 4K, a 30 o 60 fps, con el sonido del móvil.

### Atajos

| Tecla | Acción |
| --- | --- |
| Espacio | Reproducir / pausar |
| Z | Añadir un zoom en el cursor |
| Supr / Retroceso | Eliminar el zoom seleccionado |
| Esc | Salir de la edición del zoom (ver el resultado) |
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

Las grabaciones y exportaciones se guardan en `~/Vídeos/rec-ording`. El proyecto se guarda solo en `~/.local/share/rec-ording/proyecto.json`.

## Instalación

```bash
./install-cachyos.sh
```

o compilando a mano:

```bash
cargo build --release
./target/release/rec-ording
```

## Cómo está hecho

Aplicación nativa en Rust con [egui](https://github.com/emilk/egui).

| Módulo | Qué hace |
| --- | --- |
| `capture.rs` | Graba con scrcpy (o `adb screenrecord`) y lee los toques de la pantalla táctil con `getevent`, sincronizados con el vídeo. |
| `camera.rs` | Genera los zooms automáticos a partir de los toques y anima la cámara con un muelle amortiguado. |
| `compositor.rs` | Dibuja cada fotograma: fondo, sombra, marco del móvil, la grabación y los toques. La vista previa y la exportación usan el mismo código. |
| `media.rs` | Lee vídeos con ffprobe/ffmpeg, crea una copia ligera para que la vista previa sea fluida y genera las miniaturas. |
| `export.rs` | Decodifica la grabación, compone cada fotograma y codifica el resultado con ffmpeg. |
| `app.rs`, `theme.rs` | Interfaz. |

Tests: `cargo test`. Con un vídeo o un móvil reales:

```bash
REC_TEST_VIDEO=/ruta/video.mp4 cargo test exports_real_video -- --ignored
REC_TEST_SERIAL=<serial de adb> cargo test records_from_device -- --ignored
```
