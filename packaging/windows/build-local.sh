#!/usr/bin/env bash
# Cross-compiles rec-ording for Windows from Linux and leaves the .zip in the repository root.
# Needs: rustup target add x86_64-pc-windows-gnu, and mingw-w64-gcc (sudo pacman -S mingw-w64-gcc).
set -euo pipefail
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="rec-ording-$version-windows-x64"
cargo build --release --target x86_64-pc-windows-gnu

dist=$(mktemp -d)
trap 'rm -rf "$dist"' EXIT
mkdir "$dist/$name"
cp target/x86_64-pc-windows-gnu/release/rec-ording.exe "$dist/$name/"
cp -r assets/models "$dist/$name/models"
cp packaging/windows/LEEME.txt "$dist/$name/"
cp assets/fonts/OFL.txt "$dist/$name/Inter-OFL.txt"
rm -f "$name.zip"
(cd "$dist" && bsdtar -a -cf "$OLDPWD/$name.zip" "$name")
echo "Listo: $name.zip"
