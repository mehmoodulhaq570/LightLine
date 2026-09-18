"""Build the Windows application icon from the LightLine PNG."""

from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "LightLine-icon.png"
OUTPUT = ROOT / "assets" / "lightline.ico"
SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def main() -> None:
    with Image.open(SOURCE) as image:
        icon = image.convert("RGBA")
        side = min(icon.size)
        left = (icon.width - side) // 2
        top = (icon.height - side) // 2
        icon = icon.crop((left, top, left + side, top + side))
        # The source has a broad transparent glow. A small crop keeps the bolt
        # readable at Windows' 16 px title-bar size.
        margin = round(side * 0.06)
        icon = icon.crop((margin, margin, side - margin, side - margin))
        icon.save(OUTPUT, format="ICO", sizes=SIZES)
    print(f"Wrote {OUTPUT}")


if __name__ == "__main__":
    main()
