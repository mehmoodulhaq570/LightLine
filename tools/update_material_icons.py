"""Refresh LightLine's bundled Material Icon Theme snapshot.

Usage: python tools/update_material_icons.py --source PATH_TO_THEME
The source can be a cloned upstream repository or an installed VS Code extension.
Requires PyQt5 and Pillow for SVG rasterization. Neither is needed at runtime.
"""

import argparse
import json
import shutil
from pathlib import Path

from PIL import Image
from PyQt5.QtCore import Qt
from PyQt5.QtGui import QImage, QPainter
from PyQt5.QtSvg import QSvgRenderer


ICONS = (
    "file",
    "folder",
    "folder-open",
    "folder-src",
    "folder-src-open",
    "folder-docs",
    "folder-docs-open",
    "rust",
    "toml",
    "markdown",
    "json",
    "python",
    "c",
    "cpp",
    "html",
    "css",
    "javascript",
    "typescript",
    "git",
    "lock",
    "yaml",
    "readme",
)


def render_icon(svg_path: Path, ico_path: Path) -> None:
    renderer = QSvgRenderer(str(svg_path))
    if not renderer.isValid():
        raise ValueError(f"Invalid SVG: {svg_path}")
    canvas = QImage(64, 64, QImage.Format_ARGB32)
    canvas.fill(Qt.transparent)
    painter = QPainter(canvas)
    painter.setRenderHint(QPainter.Antialiasing)
    renderer.render(painter)
    painter.end()
    rgba = canvas.convertToFormat(QImage.Format_RGBA8888)
    pixels = rgba.bits().asstring(rgba.byteCount())
    image = Image.frombytes("RGBA", (64, 64), pixels)
    image.save(ico_path, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48)])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    license_file = next((source / name for name in ("LICENSE", "LICENSE.txt") if (source / name).is_file()), None)
    if license_file is None:
        raise FileNotFoundError("Material Icon Theme license was not found")
    version = json.loads((source / "package.json").read_text(encoding="utf-8"))["version"]
    destination = Path(__file__).resolve().parent.parent / "assets" / "material-icon-theme"
    destination.mkdir(parents=True, exist_ok=True)
    for name in ICONS:
        svg = source / "icons" / f"{name}.svg"
        if not svg.is_file():
            raise FileNotFoundError(svg)
        shutil.copyfile(svg, destination / svg.name)
        render_icon(svg, destination / f"{name}.ico")
    shutil.copyfile(license_file, destination / "LICENSE.txt")
    (destination / "VERSION.json").write_text(
        json.dumps(
            {
                "name": "Material Icon Theme",
                "version": version,
                "source": "https://github.com/material-extensions/vscode-material-icon-theme",
                "icons": list(ICONS),
            },
            indent=2,
        ) + "\n",
        encoding="utf-8",
    )
    print(f"Bundled {len(ICONS)} Material Icon Theme icons from version {version}")


if __name__ == "__main__":
    main()
