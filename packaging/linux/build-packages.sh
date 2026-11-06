#!/usr/bin/env bash
# Builds the packages for other Linux distributions from an already compiled binary:
#   rec-ording_<versión>-1_amd64.deb      Debian, Ubuntu, Linux Mint, Pop!_OS…
#   rec-ording-<versión>-1.x86_64.rpm     Fedora, openSUSE…
#   rec-ording-<versión>-x86_64.AppImage  any other distribution
# Usage: packaging/linux/build-packages.sh [binary] [output folder]
# Needs dpkg-deb, rpmbuild, desktop-file-validate and appimagetool (APPIMAGETOOL=path, or it
# is downloaded).
# Build the binary on an old glibc (Ubuntu 22.04 in CI) so it runs on every distribution.
set -euo pipefail
cd "$(dirname "$0")/../.."

bin=${1:-target/release/rec-ording}
out=$(realpath -m "${2:-dist}")
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
summary="Graba tu móvil Android y conviértelo en un vídeo promocional"
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The installed files, as on Arch (see packaging/arch/PKGBUILD).
stage() {
  local root=$1
  install -Dm755 "$bin" "$root/usr/bin/rec-ording"
  install -Dm644 packaging/arch/rec-ording.desktop "$root/usr/share/applications/rec-ording.desktop"
  install -Dm644 assets/icons/rec-ording.svg "$root/usr/share/icons/hicolor/scalable/apps/rec-ording.svg"
  for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "assets/icons/rec-ording-$size.png" "$root/usr/share/icons/hicolor/${size}x${size}/apps/rec-ording.png"
  done
  install -Dm644 assets/icons/rec-ording-256.png "$root/usr/share/pixmaps/rec-ording.png"
  install -Dm644 assets/fonts/OFL.txt "$root/usr/share/doc/rec-ording/Inter-OFL.txt"
  install -dm755 "$root/usr/share/rec-ording/models"
  install -m644 assets/models/*.glb assets/models/models.json assets/models/CREDITS.md "$root/usr/share/rec-ording/models/"
}

# ---------- .deb ----------
deb="$work/deb"
stage "$deb"
mkdir -p "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<CONTROL
Package: rec-ording
Version: $version-1
Architecture: amd64
Maintainer: blzkz <sergiobl88@gmail.com>
Installed-Size: $(du -sk --exclude=DEBIAN "$deb" | cut -f1)
Depends: libc6 (>= 2.35), libgcc-s1, libvulkan1, ffmpeg, curl
Recommends: adb, scrcpy, zenity, unzip, pkexec | policykit-1
Section: video
Priority: optional
Homepage: https://github.com/blzkz/rec-ording-releases
Description: $summary
 Graba la pantalla del móvil por USB y la pone dentro de un marco o un modelo
 3D, sobre un fondo, con zooms suaves en cada toque, textos y movimientos de
 cámara. Exporta en MP4 o GIF.
CONTROL
dpkg-deb --root-owner-group -Zxz --build "$deb" "$out/rec-ording_${version}-1_amd64.deb"

# ---------- .rpm ----------
rpm="$work/rpm"
stage "$rpm/root"
mkdir -p "$rpm"/{BUILD,RPMS,SPECS,SOURCES,SRPMS}
cat > "$rpm/SPECS/rec-ording.spec" <<SPEC
Name:           rec-ording
Version:        $version
Release:        1
Summary:        $summary
License:        Proprietary
URL:            https://github.com/blzkz/rec-ording-releases
BuildArch:      x86_64
# Vulkan, ffmpeg and curl are opened or run at runtime, so they are named here; file and
# library names work on Fedora and openSUSE alike.
Requires:       libvulkan.so.1()(64bit)
Requires:       /usr/bin/ffmpeg
Requires:       /usr/bin/curl
Recommends:     android-tools
Recommends:     scrcpy
Recommends:     zenity
Recommends:     unzip
Recommends:     polkit
%global debug_package %{nil}
%global _build_id_links none
%global __strip /bin/true

%description
Graba la pantalla del móvil por USB y la pone dentro de un marco o un modelo 3D,
sobre un fondo, con zooms suaves en cada toque, textos y movimientos de cámara.

%install
cp -a $rpm/root/. %{buildroot}/

%files
/usr/bin/rec-ording
/usr/share/applications/rec-ording.desktop
/usr/share/icons/hicolor/*/apps/rec-ording.*
/usr/share/pixmaps/rec-ording.png
/usr/share/doc/rec-ording
/usr/share/rec-ording
SPEC
rpmbuild --quiet --define "_topdir $rpm" -bb "$rpm/SPECS/rec-ording.spec"
cp "$rpm"/RPMS/x86_64/rec-ording-"$version"-1.x86_64.rpm "$out/"

# ---------- AppImage ----------
app="$work/AppDir"
stage "$app"
cp packaging/arch/rec-ording.desktop "$app/rec-ording.desktop"
sed -i 's|^Exec=.*|Exec=rec-ording|' "$app/rec-ording.desktop"
cp assets/icons/rec-ording-256.png "$app/rec-ording.png"
cat > "$app/AppRun" <<'APPRUN'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
exec "$here/usr/bin/rec-ording" "$@"
APPRUN
chmod 755 "$app/AppRun"
tool=${APPIMAGETOOL:-}
if [ -z "$tool" ]; then
  tool="$work/appimagetool"
  curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-x86_64.AppImage
  chmod 755 "$tool"
fi
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$out/rec-ording-${version}-x86_64.AppImage"

ls -l "$out"
