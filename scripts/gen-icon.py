#!/usr/bin/env python3
"""Generate a 1024x1024 source app icon (PNG) using only the stdlib.

Produces a dark rounded-square tile with a cyan->blue gradient and a white
">_" developer glyph. Run `cargo tauri icon icons/source.png` afterwards to
emit every size/format Tauri needs (PNG sizes, .icns, .ico).
"""

import math
import struct
import sys
import zlib

SIZE = 1024


def lerp(a, b, t):
    return a + (b - a) * t


def dist_point_to_segment(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    if dx == 0 and dy == 0:
        return math.hypot(px - ax, py - ay)
    t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    cx, cy = ax + t * dx, ay + t * dy
    return math.hypot(px - cx, py - cy)


def build_pixels():
    # Background: slate-950.
    bg = (2, 6, 23)
    # Tile gradient endpoints: cyan-400 -> blue-600.
    g_top = (34, 211, 238)
    g_bot = (37, 99, 235)
    glyph = (255, 255, 255)

    margin = 96
    radius = 180  # corner radius of the tile

    inner_lo = margin
    inner_hi = SIZE - margin

    # ">" chevron strokes + underscore, centred.
    cx, cy = SIZE / 2, SIZE / 2
    thickness = 46
    # Chevron points (a ">" shape).
    chev = [
        ((cx - 150, cy - 170), (cx + 70, cy)),
        ((cx + 70, cy), (cx - 150, cy + 170)),
    ]
    # Underscore bar.
    bar = ((cx - 150, cy + 240), (cx + 190, cy + 240))

    rows = []
    for y in range(SIZE):
        row = bytearray()
        for x in range(SIZE):
            r, g, b = bg

            inside_x = inner_lo <= x <= inner_hi
            inside_y = inner_lo <= y <= inner_hi
            in_tile = inside_x and inside_y
            if in_tile:
                # Rounded corners.
                near_corner = False
                for corx in (inner_lo + radius, inner_hi - radius):
                    for cory in (inner_lo + radius, inner_hi - radius):
                        if (x < inner_lo + radius or x > inner_hi - radius) and (
                            y < inner_lo + radius or y > inner_hi - radius
                        ):
                            if math.hypot(x - corx, y - cory) > radius:
                                near_corner = True
                if not near_corner:
                    t = (y - inner_lo) / (inner_hi - inner_lo)
                    r = int(lerp(g_top[0], g_bot[0], t))
                    g = int(lerp(g_top[1], g_bot[1], t))
                    b = int(lerp(g_top[2], g_bot[2], t))

                    # Glyph overlay.
                    d = min(
                        dist_point_to_segment(x, y, *chev[0][0], *chev[0][1]),
                        dist_point_to_segment(x, y, *chev[1][0], *chev[1][1]),
                        dist_point_to_segment(x, y, *bar[0], *bar[1]),
                    )
                    if d <= thickness / 2:
                        r, g, b = glyph

            row += bytes((r, g, b))
        rows.append(bytes(row))
    return rows


def write_png(path, rows):
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    raw = bytearray()
    for row in rows:
        raw += b"\x00" + row  # filter type 0
    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", ihdr)
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "icons/source.png"
    write_png(out, build_pixels())
    print(f"wrote {out} ({SIZE}x{SIZE})")
