"""Build every Windows icon size from the supplied LightLine PNG."""

from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "LightLine-icon2.png"
OUTPUT = ROOT / "assets" / "lightline.ico"
SIZES = [(size, size) for size in (16, 24, 32, 48, 64, 128, 256)]


def crop_artwork(source: Image.Image) -> Image.Image:
    # A low alpha threshold excludes the PNG's almost invisible outer glow.
    # Cropping only transparent margins makes the original image fill small icons.
    visible = source.getchannel("A").point(lambda alpha: 255 if alpha >= 8 else 0)
    bounds = visible.getbbox()
    if bounds is None:
        raise ValueError(f"No visible artwork in {SOURCE}")
    left, top, right, bottom = bounds
    side = max(right - left, bottom - top)
    side += round(side * 0.05)
    x = max(0, min(source.width - side, round((left + right - side) / 2)))
    y = max(0, min(source.height - side, round((top + bottom - side) / 2)))
    return source.crop((x, y, x + side, y + side))


def main() -> None:
    with Image.open(SOURCE) as image:
        crop_artwork(image.convert("RGBA")).save(OUTPUT, format="ICO", sizes=SIZES)
    print(f"Wrote {OUTPUT} from {SOURCE.name}")


if __name__ == "__main__":
    main()
