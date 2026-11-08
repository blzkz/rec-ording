#!/usr/bin/env python3
"""Builds the download page (GitHub Pages of blzkz/rec-ording-releases) from the packages of a
version: index.html, licencias.html, latest.json (read by the app's updater) and the packages.

Usage: make-site.py <packages folder> <output folder> <version> <notes.md>
The notes are a Markdown list ("- " lines, **bold** and [links](url)).
"""

import hashlib
import html
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

BASE = "https://blzkz.github.io/rec-ording-releases"
ROOT = Path(__file__).resolve().parents[2]

# (end of the file name, system, who it is for, how to install it)
PLATFORMS = [
    ("-windows-x64.zip", "Windows", "Windows 10 y 11", "Descomprímelo y abre rec-ording.exe. Lee LEEME.txt para instalar ffmpeg, adb y scrcpy."),
    ("_amd64.deb", "Debian y Ubuntu", "Ubuntu 22.04+, Debian 12+, Linux Mint, Pop!_OS", "sudo apt install ./{file}"),
    (".x86_64.rpm", "Fedora y openSUSE", "Fedora 36+ y openSUSE Tumbleweed (en openSUSE: sudo zypper install <archivo>)", "sudo dnf install ./{file}"),
    ("-x86_64.pkg.tar.zst", "Arch Linux", "Arch, CachyOS, Manjaro", "sudo pacman -U {file}"),
    ("-x86_64.AppImage", "AppImage", "Cualquier otra distribución con glibc 2.35+", "chmod +x {file} && ./{file}"),
]


def inline(text: str) -> str:
    """Markdown bold and links to HTML (the rest escaped)."""
    out = html.escape(text)
    out = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", out)
    return re.sub(r"\[(.+?)\]\((https?://[^)\s]+)\)", r'<a href="\2">\1</a>', out)


def notes_html(md: str) -> str:
    items = [line[2:].strip() for line in md.splitlines() if line.startswith("- ")]
    if not items:
        return "".join(f"<p>{inline(p)}</p>" for p in md.split("\n\n") if p.strip())
    return "<ul>" + "".join(f"<li>{inline(i)}</li>" for i in items) + "</ul>"


def size_label(n: int) -> str:
    return f"{n / 1_000_000:.1f} MB".replace(".", ",")


def libraries() -> list[dict]:
    """The Rust libraries built into the app on Linux and Windows, from cargo."""
    found = {}
    for target in ("x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"):
        raw = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
        meta = json.loads(raw)
        used = {node["id"] for node in meta["resolve"]["nodes"]}
        for p in meta["packages"]:
            if p["id"] in used and p["name"] != "rec-ording":
                found[(p["name"], p["version"])] = p
    return [found[k] for k in sorted(found)]


def credits_page(version: str) -> str:
    rows = []
    for p in libraries():
        name = html.escape(p["name"])
        link = p.get("repository") or f"https://crates.io/crates/{p['name']}"
        if link.startswith(("https://", "http://")):
            name = f'<a href="{html.escape(link)}">{name}</a>'
        rows.append(f"<tr><td>{name}</td><td>{html.escape(p['version'])}</td><td>{html.escape(p.get('license') or 'Sin indicar')}</td></tr>")
    ofl = html.escape((ROOT / "assets/fonts/OFL.txt").read_text(encoding="utf-8"))
    style = re.search(r"<style>.*?</style>", TEMPLATE, re.S).group(0)
    return (
        CREDITS.replace("{{STYLE}}", style)
        .replace("{{VERSION}}", html.escape(version))
        .replace("{{LIBRARIES}}", "\n".join(rows))
        .replace("{{COUNT}}", str(len(rows)))
        .replace("{{OFL}}", ofl)
    )


def main() -> None:
    packages, out, version, notes_file = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3].lstrip("vV"), Path(sys.argv[4])
    notes = notes_file.read_text(encoding="utf-8").strip()
    out.mkdir(parents=True, exist_ok=True)

    assets, cards = [], []
    files = sorted(p for p in packages.iterdir() if p.is_file())
    for suffix, system, who, how in PLATFORMS:
        f = next((p for p in files if p.name.endswith(suffix)), None)
        if f is None:
            continue
        shutil.copy2(f, out / f.name)
        sha = hashlib.sha256(f.read_bytes()).hexdigest()
        url = f"{BASE}/{f.name}"
        assets.append({"name": f.name, "url": url, "sha256": sha, "size": f.stat().st_size})
        install = how.format(file=f.name)
        is_command = install.startswith(("sudo ", "chmod "))
        install_html = (
            f'<div class="cmd"><code>{html.escape(install)}</code><button type="button" data-copy="{html.escape(install)}">Copiar</button></div>'
            if is_command
            else f'<p class="how">{html.escape(install)}</p>'
        )
        cards.append(
            f"""<article class="card" data-os="{'windows' if 'windows' in suffix else 'linux'}">
  <h3>{html.escape(system)}</h3>
  <p class="who">{html.escape(who)}</p>
  <a class="download" href="{html.escape(url)}" download>Descargar <span>{html.escape(f.name)} · {size_label(f.stat().st_size)}</span></a>
  {install_html}
  <p class="sha">SHA-256 {sha}</p>
</article>"""
        )
    if not assets:
        sys.exit("No hay paquetes en " + str(packages))

    latest = {"version": version, "notes": notes, "page": f"{BASE}/", "assets": assets}
    (out / "latest.json").write_text(json.dumps(latest, ensure_ascii=False, indent=2), encoding="utf-8")
    shutil.copy2(ROOT / "assets/icons/rec-ording-256.png", out / "icon.png")
    (out / ".nojekyll").write_text("")

    page = TEMPLATE.replace("{{VERSION}}", html.escape(version)).replace("{{CARDS}}", "\n".join(cards)).replace("{{NOTES}}", notes_html(notes))
    (out / "index.html").write_text(page, encoding="utf-8")
    (out / "licencias.html").write_text(credits_page(version), encoding="utf-8")
    print(f"Página de descargas de la {version}: {len(assets)} paquetes en {out}")


TEMPLATE = """<!doctype html>
<html lang="es">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>rec·ording · Descargas</title>
<meta name="description" content="Graba tu móvil Android y conviértelo en un vídeo promocional con marco, fondo y zooms suaves.">
<link rel="icon" href="icon.png">
<style>
:root { --bg: #f5f5f7; --card: #ffffff; --text: #1d1d1f; --text2: #6e6e73; --line: #d2d2d7; --accent: #0a84ff; --red: #ff453a; --code: #f0f0f3; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #141416; --card: #1f1f22; --text: #f5f5f7; --text2: #a1a1a6; --line: #333338; --code: #2a2a2e; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 16px/1.5 -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", system-ui, sans-serif; }
main { max-width: 1040px; margin: 0 auto; padding: 56px 20px 72px; }
header { text-align: center; margin-bottom: 44px; }
header img { width: 88px; height: 88px; border-radius: 20px; }
h1 { font-size: 44px; font-weight: 600; letter-spacing: -0.02em; margin: 14px 0 4px; }
h1 .dot { color: var(--red); }
.tagline { color: var(--text2); font-size: 19px; margin: 0; }
.version { display: inline-block; margin-top: 14px; padding: 3px 12px; border-radius: 999px; background: var(--card); border: 1px solid var(--line); color: var(--text2); font-size: 14px; }
h2 { font-size: 22px; font-weight: 600; margin: 48px 0 16px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 16px; }
.card { background: var(--card); border: 1px solid var(--line); border-radius: 16px; padding: 20px; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
.card.mine { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
.card h3 { margin: 0; font-size: 18px; font-weight: 600; }
.who, .how { margin: 0; color: var(--text2); font-size: 14px; }
.download { display: block; background: var(--accent); color: #fff; text-decoration: none; border-radius: 10px; padding: 10px 14px; font-weight: 600; }
.download span { display: block; font-weight: 400; font-size: 12.5px; opacity: .85; overflow-wrap: anywhere; }
.cmd { display: flex; gap: 8px; align-items: center; background: var(--code); border-radius: 8px; padding: 6px 6px 6px 10px; }
.cmd code { flex: 1; font: 12.5px ui-monospace, "SF Mono", Menlo, monospace; overflow-x: auto; white-space: nowrap; }
.cmd button { border: 0; border-radius: 6px; padding: 4px 10px; background: var(--card); color: var(--text); font: inherit; font-size: 13px; cursor: pointer; }
.sha { margin: 0; color: var(--text2); font: 11px ui-monospace, Menlo, monospace; overflow-wrap: anywhere; opacity: .8; }
.notes, .info { background: var(--card); border: 1px solid var(--line); border-radius: 16px; padding: 8px 24px; }
.notes li { margin: 10px 0; }
a { color: var(--accent); }
footer { margin-top: 48px; text-align: center; color: var(--text2); font-size: 13px; }
</style>
</head>
<body>
<main>
<header>
  <img src="icon.png" alt="" width="88" height="88">
  <h1>rec<span class="dot">·</span>ording</h1>
  <p class="tagline">Graba tu móvil Android y conviértelo en un vídeo promocional.</p>
  <span class="version">Versión {{VERSION}}</span>
</header>

<section>
  <h2>Descargar</h2>
  <div class="grid">
{{CARDS}}
  </div>
</section>

<section>
  <h2>Novedades</h2>
  <div class="notes">{{NOTES}}</div>
</section>

<section>
  <h2>Antes de empezar</h2>
  <div class="info">
    <p>Para importar y exportar vídeos hace falta <strong>ffmpeg</strong>; para grabar el móvil o un emulador, <strong>adb</strong> y <strong>scrcpy</strong>. Los paquetes .deb, .rpm y de Arch los instalan o recomiendan; con el AppImage y en Windows se instalan aparte.</p>
    <p>Una vez instalada, la app avisa cuando hay una versión nueva y la instala sola.</p>
    <p>El modelo 3D incluido, «Samsung S26 Ultra», es de <a href="https://sketchfab.com/modis1761">Sagar Modi</a> (<a href="https://sketchfab.com/3d-models/samsung-s26-ultra-7ff228963ea446f39f46a5541f71e352">Sketchfab</a>), con licencia <a href="https://creativecommons.org/licenses/by/4.0/">CC BY 4.0</a>.</p>
  </div>
</section>

<footer>rec·ording {{VERSION}} · <a href="licencias.html">Licencias y créditos</a></footer>
</main>
<script>
// Highlight the downloads for this computer's system.
const os = /Windows/i.test(navigator.userAgent) ? "windows" : /Linux|X11/i.test(navigator.userAgent) ? "linux" : "";
document.querySelectorAll(".card").forEach((c) => { if (c.dataset.os === os) c.classList.add("mine"); });
document.querySelectorAll("button[data-copy]").forEach((b) => b.addEventListener("click", async () => {
  try { await navigator.clipboard.writeText(b.dataset.copy); b.textContent = "Copiado"; setTimeout(() => (b.textContent = "Copiar"), 1500); } catch (e) {}
}));
</script>
</body>
</html>
"""

CREDITS = """<!doctype html>
<html lang="es">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>rec·ording · Licencias</title>
<link rel="icon" href="icon.png">
{{STYLE}}
<style>
.info h3 { margin: 16px 0 2px; font-size: 16px; font-weight: 600; }
.info p { margin: 4px 0 14px; color: var(--text2); }
.scroll { overflow-x: auto; }
table { width: 100%; border-collapse: collapse; font-size: 14px; }
th, td { text-align: left; padding: 6px 12px 6px 0; border-bottom: 1px solid var(--line); }
th { font-weight: 600; }
details { margin: 8px 0 14px; }
pre { white-space: pre-wrap; font: 12px/1.5 ui-monospace, Menlo, monospace; color: var(--text2); }
</style>
</head>
<body>
<main>
<header>
  <h1>Licencias y créditos</h1>
  <p class="tagline">rec<span style="color: var(--red)">·</span>ording {{VERSION}}</p>
</header>

<section>
  <h2>Modelo 3D</h2>
  <div class="info">
    <h3>Samsung S26 Ultra</h3>
    <p>De <a href="https://sketchfab.com/modis1761">Sagar Modi</a>, en <a href="https://sketchfab.com/3d-models/samsung-s26-ultra-7ff228963ea446f39f46a5541f71e352">Sketchfab</a>. Licencia <a href="https://creativecommons.org/licenses/by/4.0/">CC BY 4.0</a>. Si publicas un vídeo hecho con este modelo, menciona a su autor.</p>
  </div>
</section>

<section>
  <h2>Tipografía</h2>
  <div class="info">
    <h3>Inter</h3>
    <p>De <a href="https://github.com/rsms/inter">The Inter Project Authors</a>, con licencia SIL Open Font License 1.1.</p>
    <details><summary>Texto de la licencia</summary><pre>{{OFL}}</pre></details>
  </div>
</section>

<section>
  <h2>Programas que usa</h2>
  <div class="info">
    <p>No vienen incluidos: la app usa los que tengas instalados, cada uno con su propia licencia.</p>
    <p><a href="https://ffmpeg.org/">FFmpeg</a> · <a href="https://github.com/Genymobile/scrcpy">scrcpy</a> · <a href="https://developer.android.com/tools/adb">Android Debug Bridge</a></p>
  </div>
</section>

<section>
  <h2>Bibliotecas ({{COUNT}})</h2>
  <div class="info scroll">
    <table>
      <thead><tr><th>Nombre</th><th>Versión</th><th>Licencia</th></tr></thead>
      <tbody>
{{LIBRARIES}}
      </tbody>
    </table>
  </div>
</section>

<footer><a href="index.html">Volver a las descargas</a></footer>
</main>
</body>
</html>
"""

if __name__ == "__main__":
    main()
