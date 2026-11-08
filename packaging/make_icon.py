"""Draws the Oxidraft icon and writes assets/icon.png and assets/icon.ico.

Standard library only: python3 packaging/make_icon.py (also writes
assets/icon.icns, for the macOS app).
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


def render(size, samples=4, inset=0.0):
    """RGBA rows, antialiased by supersampling; the icon `inset` (a fraction
    of the size) in from each side."""

    def at(x, y):
        return color_at((x - inset) / (1 - 2 * inset), (y - inset) / (1 - 2 * inset))

    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(samples):
                for sx in range(samples):
                    col = at((px + (sx + 0.5) / samples) / size, (py + (sy + 0.5) / samples) / size)
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


def png(size, samples=4, inset=0.0):
    rows = render(size, samples, inset)
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


def icns():
    """The macOS icon: the design on Apple's grid (824 of 1024 across, a
    margin about it, as the Dock's other icons have), at each size macOS
    looks for."""
    inset = 100 / 1024
    images = {s: png(s, 4 if s <= 256 else 2, inset) for s in (16, 32, 64, 128, 256, 512, 1024)}
    kinds = [
        (b"icp4", 16), (b"ic11", 32), (b"icp5", 32), (b"ic12", 64), (b"ic07", 128),
        (b"ic13", 256), (b"ic08", 256), (b"ic14", 512), (b"ic09", 512), (b"ic10", 1024),
    ]
    body = b"".join(k + struct.pack(">I", 8 + len(images[s])) + images[s] for k, s in kinds)
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


if __name__ == "__main__":
    assets = Path(__file__).resolve().parent.parent / "assets"
    assets.mkdir(exist_ok=True)
    (assets / "icon.png").write_bytes(png(256))
    (assets / "icon.ico").write_bytes(ico([16, 24, 32, 48, 64, 256]))
    (assets / "icon.icns").write_bytes(icns())
    print("wrote", assets / "icon.png", assets / "icon.ico", "and", assets / "icon.icns")
