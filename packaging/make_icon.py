"""Draws the Oxidraft icon and writes assets/icon.png and assets/icon.ico.

Standard library only: python3 packaging/make_icon.py
The design: a CAD crosshair with a pickbox on a dark rounded square, in the
app's rust orange.
"""
import struct
import zlib
from pathlib import Path

BG = (38, 45, 56)
EDGE = (64, 74, 90)
ORANGE = (214, 98, 38)
WHITE = (240, 240, 240)


def color_at(x, y):
    """Color of the point (x, y) in a 0..1 square, or None outside the icon."""
    r = 0.19  # corner radius
    cx, cy = min(max(x, r), 1 - r), min(max(y, r), 1 - r)
    d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
    if d > r:
        return None
    if d > r - 0.025 and (x < r or x > 1 - r or y < r or y > 1 - r) or min(x, y, 1 - x, 1 - y) < 0.025:
        return EDGE
    c, half, line = 0.5, 0.13, 0.045  # center, pickbox half-size, stroke width
    in_box = abs(x - c) <= half and abs(y - c) <= half
    on_box = in_box and (abs(x - c) >= half - line or abs(y - c) >= half - line)
    if on_box:
        return WHITE
    arm = 0.33
    if not in_box and (
        (abs(y - c) <= line / 2 and abs(x - c) <= arm) or (abs(x - c) <= line / 2 and abs(y - c) <= arm)
    ):
        return ORANGE
    return BG


def render(size, samples=4):
    """RGBA rows, antialiased by supersampling."""
    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(samples):
                for sx in range(samples):
                    col = color_at((px + (sx + 0.5) / samples) / size, (py + (sy + 0.5) / samples) / size)
                    if col:
                        acc[0] += col[0]
                        acc[1] += col[1]
                        acc[2] += col[2]
                        acc[3] += 255
            n = samples * samples
            alpha = acc[3] / 255
            row += bytes([int(acc[i] / alpha) if alpha else 0 for i in range(3)] + [acc[3] // n])
        rows.append(bytes(row))
    return rows


def png(size):
    rows = render(size)
    raw = b"".join(b"\0" + r for r in rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(sizes):
    images = [png(s) for s in sizes]
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for s, data in zip(sizes, images):
        out += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b"".join(images)


if __name__ == "__main__":
    assets = Path(__file__).resolve().parent.parent / "assets"
    assets.mkdir(exist_ok=True)
    (assets / "icon.png").write_bytes(png(256))
    (assets / "icon.ico").write_bytes(ico([16, 24, 32, 48, 64, 256]))
    print("wrote", assets / "icon.png", "and", assets / "icon.ico")
