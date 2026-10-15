"""Draws the Clipvault icon (a clipboard with a keyhole) into crates/app/assets/.

Run: python scripts/make_icon.py
Outputs icon.ico (16..256 px, exe resource), icon.png (256 px) and icon64.rgba (raw window icon).
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "crates" / "app" / "assets"
S = 1024  # drawn large, downscaled with antialiasing


def draw() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((32, 32, S - 32, S - 32), radius=200, fill=(30, 34, 46, 255))
    # board
    d.rounded_rectangle((210, 200, S - 210, S - 150), radius=90, fill=(124, 108, 240, 255))
    d.rounded_rectangle((250, 250, S - 250, S - 190), radius=60, fill=(46, 50, 72, 255))
    # clip
    d.rounded_rectangle((372, 140, S - 372, 300), radius=60, fill=(216, 220, 235, 255))
    d.rounded_rectangle((430, 180, S - 430, 262), radius=30, fill=(30, 34, 46, 255))
    # keyhole
    cx, cy = S // 2, 560
    d.ellipse((cx - 96, cy - 130, cx + 96, cy + 62), fill=(124, 108, 240, 255))
    d.polygon([(cx - 50, cy + 20), (cx + 50, cy + 20), (cx + 84, cy + 230), (cx - 84, cy + 230)], fill=(124, 108, 240, 255))
    d.ellipse((cx - 46, cy - 84, cx + 46, cy + 8), fill=(46, 50, 72, 255))
    return img


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    big = draw()
    sizes = [16, 24, 32, 48, 64, 128, 256]
    big.resize((256, 256), Image.LANCZOS).save(OUT / "icon.png")
    big.save(OUT / "icon.ico", sizes=[(s, s) for s in sizes])
    (OUT / "icon64.rgba").write_bytes(big.resize((64, 64), Image.LANCZOS).tobytes())


if __name__ == "__main__":
    main()
