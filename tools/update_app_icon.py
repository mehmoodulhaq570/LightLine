"""Build every Windows icon size from the supplied LightLine PNG."""

from io import BytesIO
from pathlib import Path
import struct

from PIL import Image, ImageFilter


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "LightLine-icon2.png"
OUTPUT = ROOT / "assets" / "lightline.ico"
SIZES = (16, 24, 32, 48, 64, 128, 256)
SHARPEN = {16: 120, 24: 100, 32: 80, 48: 40}


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
        artwork = crop_artwork(image.convert("RGBA"))
        frames = []
        for size in SIZES:
            frame = artwork.resize((size, size), Image.Resampling.LANCZOS)
            if size in SHARPEN:
                frame = frame.filter(
                    ImageFilter.UnsharpMask(
                        radius=0.6, percent=SHARPEN[size], threshold=0
                    )
                )
            frames.append(frame)

    # Preserve each resized frame. Pillow's ICO writer would scale one image
    # again, discarding the size-specific sharpening.
    directory = []
    data_blocks = []
    offset = 6 + 16 * len(frames)
    for size, frame in zip(SIZES, frames):
        stream = BytesIO()
        frame.save(stream, format="PNG")
        data = stream.getvalue()
        directory.append(
            struct.pack(
                "<BBBBHHII",
                0 if size == 256 else size,
                0 if size == 256 else size,
                0,
                0,
                1,
                32,
                len(data),
                offset,
            )
        )
        data_blocks.append(data)
        offset += len(data)
    OUTPUT.write_bytes(
        struct.pack("<HHH", 0, 1, len(frames))
        + b"".join(directory)
        + b"".join(data_blocks)
    )
    print(f"Wrote {OUTPUT} from {SOURCE.name}")


if __name__ == "__main__":
    main()
