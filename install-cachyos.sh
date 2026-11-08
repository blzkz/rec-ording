#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

PKG_FILE=$(ls -1 rec-ording-0.14.0-*.pkg.tar.zst 2>/dev/null | head -n 1 || true)

if [[ -z "${PKG_FILE}" ]]; then
  echo "==> Compilando rec-ording v0.14.0..."
  cargo build --release
  (cd packaging/arch && makepkg -f)
  cp packaging/arch/rec-ording-*.pkg.tar.zst .
  PKG_FILE=$(ls -1 rec-ording-0.14.0-*.pkg.tar.zst | head -n 1)
fi

echo "==> Instalando ${PKG_FILE} con pacman..."
sudo pacman -U "${PKG_FILE}"
echo "==> ¡Listo! Puedes abrir 'rec-ording' desde tu lanzador o ejecutando: rec-ording"
