"""Gera o ícone conver.ico (quadrado roxo com setas de conversão)."""
from PIL import Image, ImageDraw

S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle((40, 40, S - 40, S - 40), radius=220, fill=(0xF1, 0x30, 0x30))

w = 70
c = S // 2
r = 300
white = (0xE8, 0xEB, 0xE4)
# dois arcos formando um ciclo
d.arc((c - r, c - r, c + r, c + r), start=200, end=340, fill=white, width=w)
d.arc((c - r, c - r, c + r, c + r), start=20, end=160, fill=white, width=w)


def arrow(cx, cy, direction):
    s = 95
    if direction == "right":
        pts = [(cx - s, cy - s), (cx + s, cy), (cx - s, cy + s)]
    else:
        pts = [(cx + s, cy - s), (cx - s, cy), (cx + s, cy + s)]
    d.polygon(pts, fill=white)


import math

a = math.radians(340)
arrow(c + r * math.cos(a) + 10, c + r * math.sin(a) + 40, "right")
a = math.radians(160)
arrow(c + r * math.cos(a) - 10, c + r * math.sin(a) - 40, "left")

import os
os.makedirs("assets", exist_ok=True)
img.save("assets/conver.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
img.resize((256, 256), Image.LANCZOS).save("assets/logo.png")
