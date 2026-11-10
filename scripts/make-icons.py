"""Draws the Buginotch icon (the pixel bug under a notch) and writes the .ico files.

  crates/isle-win/assets/buginotch.ico       white rounded tile (exe, shortcut, installer)
  crates/isle-win/assets/buginotch-tray.ico  no tile, so it sits on any taskbar colour

Sizes 16 and 24 are always the tile-less pixel grid: 8x8 cells, so every cell is a whole number
of pixels. Standard library only.   usage: python3 scripts/make-icons.py
"""
import os
import struct
import sys
import zlib

ORANGE = (217, 119, 87)
INK = (20, 22, 26)
WHITE = (255, 255, 255)
# The bug as 6x6 cells: 1 = body, 0 = empty, 2 = eye hole (white, cut out of the body).
BUG = [
    "100001",
    "010010",
    "012210",
    "111111",
    "101101",
    "010010",
]
BUG = [
    "..XX..".replace("X", "1"),
]
# Cells (col, row) of the original 64-unit drawing, 6 units per cell, starting at x=14, y=20.
BODY = {(0, 0), (5, 0), (1, 1), (4, 1), (1, 2), (2, 2), (3, 2), (4, 2),
        (0, 3), (1, 3), (2, 3), (3, 3), (4, 3), (5, 3),
        (0, 4), (2, 4), (3, 4), (5, 4), (1, 5), (4, 5)}
# In the 64-unit drawing the body is: rows y=20,26 sparse; y=32 wide (20..44); y=38 wide (14..50);
# y=44 legs + belly; y=50 feet. Eyes are two 4x6 holes in the y=32 row.
RECTS = [  # (x, y, w, h) in 64-unit space
    (14, 20, 6, 6), (44, 20, 6, 6),
    (20, 26, 6, 6), (38, 26, 6, 6),
    (20, 32, 24, 6),
    (14, 38, 36, 6),
    (14, 44, 6, 6), (26, 44, 12, 6), (44, 44, 6, 6),
    (20, 50, 6, 6), (38, 50, 6, 6),
]
EYES = [(26, 32, 4, 6), (34, 32, 4, 6)]


def in_rect(x, y, r):
    return r[0] <= x < r[0] + r[2] and r[1] <= y < r[1] + r[3]


def in_notch(x, y, top, bottom, left, right, rad):
    """Rectangle with the two bottom corners rounded."""
    if not (left <= x < right and top <= y < bottom):
        return False
    for cx in (left + rad, right - rad):
        cy = bottom - rad
        if y > cy and ((x < left + rad and cx == left + rad) or (x >= right - rad and cx == right - rad)):
            if (x - cx) ** 2 + (y - cy) ** 2 > rad ** 2:
                return False
    return True


def in_tile(x, y, rad=14.0):
    if not (0 <= x < 64 and 0 <= y < 64):
        return False
    cx = min(max(x, rad), 64 - rad)
    cy = min(max(y, rad), 64 - rad)
    return (x - cx) ** 2 + (y - cy) ** 2 <= rad ** 2


def tile_color(x, y):
    """Colour at a point of the tile icon (64-unit space), or None for transparent."""
    if not in_tile(x, y):
        return None
    if in_notch(x, y, -1, 14.5, 19, 45, 6.5):
        return INK
    if any(in_rect(x, y, e) for e in EYES):
        return WHITE
    if any(in_rect(x, y, r) for r in RECTS):
        return ORANGE
    # A hairline border so the white tile shows on a white background.
    if not in_tile(x, y, 14.0) or not (in_tile(x - 0.9 * (1 if x < 32 else -1), y - 0.9 * (1 if y < 32 else -1))):
        return (225, 228, 234)
    return WHITE


def glyph_cells(n):
    """The tile-less icon as n x n pixels, n a multiple of 8: returns rows of (r,g,b,a)."""
    cell = n // 8
    grid = [[None] * 8 for _ in range(8)]
    # Notch: 2 cells tall, 4 wide, bottom corners cut. Bug starts one cell lower, 6x6.
    for c in range(1, 5):
        grid[0][c + 1 - 1 + 1 - 1 + 1] = None
    notch = {(0, 2), (0, 3), (0, 4), (0, 5), (1, 3), (1, 4)}
    # centred: bug columns 1..6, notch columns 2..5
    for (r, c) in notch:
        grid[r][c] = (59, 65, 76, 255) if n <= 24 else (*INK, 255)
    bug = set()
    for (x, y, w, h) in RECTS:
        for cx in range(int(x - 14) // 6, int(x - 14 + w) // 6):
            for cy in range(int(y - 20) // 6, int(y - 20 + h) // 6):
                bug.add((cx, cy))
    eyes = set()
    for (x, y, w, h) in EYES:
        eyes.add((int(x - 14) // 6, int(y - 20) // 6))
    for (cx, cy) in bug:
        grid[cy + 2][cx + 1] = (*ORANGE, 255)
    out = []
    for py in range(n):
        row = []
        for px in range(n):
            row.append(grid[py // cell][px // cell] or (0, 0, 0, 0))
        out.append(row)
    # eyes: 4 units wide of a 6 unit cell; at small sizes make them a half-cell notch in the cell
    for (cx, cy) in {(2, 3), (3, 3)}:
        x0 = (cx + 1) * cell
        y0 = (cy + 2) * cell
        for py in range(y0, y0 + cell):
            for px in range(x0 + cell // 4, x0 + cell - cell // 4):
                out[py][px] = (255, 255, 255, 255)
    return out


def render_tile(n, tile):
    """Supersampled vector render of the tile icon (tile=True) or the glyph on transparent."""
    ss = 8 if n <= 64 else 4
    rows = []
    for py in range(n):
        row = []
        for px in range(n):
            r = g = b = a = 0
            for sy in range(ss):
                for sx in range(ss):
                    x = (px + (sx + .5) / ss) * 64 / n
                    y = (py + (sy + .5) / ss) * 64 / n
                    col = tile_color(x, y)
                    if col is not None:
                        r += col[0]; g += col[1]; b += col[2]; a += 1
            if a:
                row.append((r // a, g // a, b // a, round(255 * a / (ss * ss))))
            else:
                row.append((0, 0, 0, 0))
        rows.append(row)
    return rows


def png(rows):
    n = len(rows)
    raw = b"".join(b"\x00" + bytes(v for p in row for v in p) for row in rows)
    def chunk(kind, body):
        c = struct.pack(">I", len(body)) + kind + body
        return c + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(images):
    head = struct.pack("<HHH", 0, 1, len(images))
    entries, blobs, offset = b"", b"", 6 + 16 * len(images)
    for n, data in images:
        entries += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    return head + entries + blobs


def main():
    out = os.path.join(os.path.dirname(__file__), "..", "crates", "isle-win", "assets")
    main_sizes, tray_sizes = [16, 24, 32, 48, 64, 128, 256], [16, 20, 24, 32]
    imgs = []
    for n in main_sizes:
        rows = glyph_cells(n) if n < 32 else render_tile(n, True)
        imgs.append((n, png(rows)))
    open(os.path.join(out, "buginotch.ico"), "wb").write(ico(imgs))
    timgs = []
    for n in tray_sizes:
        m = n if n % 8 == 0 else 8 * ((n + 7) // 8)
        rows = glyph_cells(m)
        if m != n:  # 20: sample the 24 grid down by picking nearest pixels
            rows = [[rows[y * m // n][x * m // n] for x in range(n)] for y in range(n)]
        timgs.append((n, png(rows)))
    open(os.path.join(out, "buginotch-tray.ico"), "wb").write(ico(timgs))
    open(os.path.join(out, "buginotch-256.png"), "wb").write(imgs[-1][1])
    open(os.path.join(out, "buginotch-tray-32.png"), "wb").write(timgs[-1][1])
    print("wrote", out)


if __name__ == "__main__":
    sys.exit(main())
