"""Generate an original synthetic BMP for manual desktop testing (stdlib only)."""
from pathlib import Path
import math
import struct

w, h = 640, 400
stride = (w * 3 + 3) // 4 * 4
pixels = bytearray(stride * h)
for y in range(h):
    for x in range(w):
        mountain = 220 + 55 * math.sin(x / 94) + 32 * math.sin(x / 37)
        color = (77 + y // 5, 99 + y // 4, 112 + y // 4)
        if y > mountain:
            color = (63, 83 + y // 9, 62)
        if (x - 470) ** 2 + (y - 100) ** 2 < 34 ** 2:
            color = (125, 209, 235)
        if 45 < x < 230 and 60 < y < 145:
            color = (34 + x // 9, 48 + y // 7, 45)
        i = (h - 1 - y) * stride + x * 3
        pixels[i:i + 3] = bytes(color)
header = struct.pack('<2sIHHI', b'BM', 54 + len(pixels), 0, 0, 54)
header += struct.pack('<IiiHHIIiiII', 40, w, h, 1, 24, 0, len(pixels), 2835, 2835, 0, 0)
path = Path(__file__).resolve().parents[1] / 'artifacts' / 'sample.bmp'
path.parent.mkdir(exist_ok=True)
path.write_bytes(header + pixels)
print(path)
