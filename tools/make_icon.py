"""Genera res/esme.ico (16-256 px) a partir de assets/Esme.jpg.

Herramienta de desarrollo, se corre a mano solo si cambia el ícono:
    python tools/make_icon.py
(Se hace aparte para no necesitar un build.rs.)
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "Esme.jpg"
DEST = ROOT / "res" / "esme.ico"
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def main():
    img = Image.open(SRC).convert("RGB")
    w, h = img.size
    # El jpg trae el ícono (cuadrado redondeado claro) sobre un fondo oscuro:
    # recortamos el cuadrado y hacemos transparentes las esquinas.
    m = round(w * 0.125)
    tile = img.crop((m, m, w - m, h - m)).convert("RGBA")
    side = tile.size[0]
    scale = 4
    mask = Image.new("L", (side * scale, side * scale), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, side * scale - 1, side * scale - 1), radius=round(side * scale * 0.225), fill=255
    )
    tile.putalpha(mask.resize((side, side), Image.LANCZOS))
    base = tile.resize((256, 256), Image.LANCZOS)
    base.save(DEST, format="ICO", sizes=[(s, s) for s in SIZES])
    print(f"{DEST} ({DEST.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
