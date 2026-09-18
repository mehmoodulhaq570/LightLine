"""Build a multi-size Windows icon from LightLine's supplied artwork.

Small icons use a simplified version of the same lightning mark. Resizing the
source PNG directly makes its glow and narrow edges muddy at taskbar sizes.
"""

from io import BytesIO
from pathlib import Path
import struct

from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "LightLine-icon2.png"
OUTPUT = ROOT / "assets" / "lightline.ico"
SIZES = (16, 24, 32, 48, 64, 128, 256)
SMALL_MAX = 64
SUPERSAMPLE = 8


def blend(first: tuple[int, int, int], second: tuple[int, int, int], amount: float):
    return tuple(round(a * (1 - amount) + b * amount) for a, b in zip(first, second))


def gradient(size: int, first: tuple[int, int, int], second: tuple[int, int, int]):
    image = Image.new("RGBA", (size, size))
    pixels = image.load()
    for y in range(size):
        for x in range(size):
            color = blend(first, second, (x + y * 0.18) / (size * 1.18))
            pixels[x, y] = (*color, 255)
    return image


def small_icon(size: int):
    canvas = size * SUPERSAMPLE
    unit = canvas / 64
    result = Image.new("RGBA", (canvas, canvas))

    # Keep the colored edge crisp and the center dark enough for a white bolt.
    outer = Image.new("L", (canvas, canvas))
    ImageDraw.Draw(outer).rounded_rectangle(
        (1.5 * unit, 1.5 * unit, 62.5 * unit, 62.5 * unit),
        radius=12 * unit,
        fill=255,
    )
    result.paste(gradient(canvas, (229, 42, 240), (0, 190, 248)), (0, 0), outer)

    inner = Image.new("L", (canvas, canvas))
    ImageDraw.Draw(inner).rounded_rectangle(
        (4.5 * unit, 4.5 * unit, 59.5 * unit, 59.5 * unit),
        radius=9.5 * unit,
        fill=255,
    )
    result.paste(gradient(canvas, (76, 21, 153), (8, 48, 130)), (0, 0), inner)

    # The simple silhouette keeps its sharp corners after downsampling.
    bolt = Image.new("L", (canvas, canvas))
    points = [(49, 7), (25, 25), (14, 38), (30, 36), (19, 57), (51, 29), (34, 29)]
    ImageDraw.Draw(bolt).polygon([(x * unit, y * unit) for x, y in points], fill=255)
    result.paste(gradient(canvas, (255, 247, 255), (137, 249, 255)), (0, 0), bolt)
    return result.resize((size, size), Image.Resampling.LANCZOS)


def large_icon(source: Image.Image, size: int):
    side = min(source.size)
    left = (source.width - side) // 2
    top = (source.height - side) // 2
    square = source.crop((left, top, left + side, top + side))
    margin = round(side * 0.06)
    square = square.crop((margin, margin, side - margin, side - margin))
    return square.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    with Image.open(SOURCE) as image:
        source = image.convert("RGBA")
        frames = [
            small_icon(size) if size <= SMALL_MAX else large_icon(source, size)
            for size in SIZES
        ]

    # An ICO can contain PNG-compressed images at distinct sizes. Writing the
    # directory directly preserves our custom small frames instead of letting
    # Pillow rescale a single source image for every frame.
    entries = []
    images = []
    offset = 6 + 16 * len(frames)
    for size, frame in zip(SIZES, frames):
        stream = BytesIO()
        frame.save(stream, format="PNG")
        data = stream.getvalue()
        entries.append(
            struct.pack(
                "<BBBBHHII", 0 if size == 256 else size, 0 if size == 256 else size,
                0, 0, 1, 32, len(data), offset,
            )
        )
        images.append(data)
        offset += len(data)

    OUTPUT.write_bytes(struct.pack("<HHH", 0, 1, len(frames)) + b"".join(entries + images))
    print(f"Wrote {OUTPUT} ({', '.join(map(str, SIZES))} px)")


if __name__ == "__main__":
    main()
