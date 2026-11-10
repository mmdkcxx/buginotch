"""Checks the screenshots scripts/ui-check.sh took and writes a report and a contact sheet.

Automatic checks (they point at pages to look at, they do not replace looking):
  * smooth glass: down a column inside the left edge of the flat-backdrop shots, the brightness
    must change gradually. A jump means a band or a seam in the glass (for example a footer-like
    strip near the bottom).
Colours (white text on light glass and so on) are checked by unit tests in
crates/isle-logic/src/glass.rs; look at the contact sheet for the rest.
Standard library only (PNG is decoded by hand).
"""
import html
import os
import struct
import sys
import zlib

PAGES = ["Musik", "Timer", "To-do", "Clipboard", "Shelf", "Air minum", "Widget", "Notifikasi", "Digioh", "Claude", "Pembaruan"]
LOOKS = ["solid", "dark", "light"]
W, H = 800, 270
# Shape of the open notch inside a shot (540x184 DIP at 100%), centred at x=400.
LEFT, RIGHT, BOTTOM = 130, 670, 184


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    pos, idat, w, h, ctype = 8, b"", 0, 0, 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and interlace == 0 and ctype in (2, 6), "unsupported PNG"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    bpp = 4 if ctype == 6 else 3
    raw = zlib.decompress(idat)
    stride = w * bpp
    rows, prev = [], bytearray(stride)
    for y in range(h):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        rows.append(line)
        prev = line
    return w, h, bpp, rows


def lum(row, x, bpp):
    i = x * bpp
    return 0.2126 * row[i + 2] + 0.7152 * row[i + 1] + 0.0722 * row[i]  # BGR order from GDI+


def smooth_glass(rows, bpp):
    """Biggest brightness jump between neighbouring rows down a column near the left edge."""
    x = LEFT + 4
    worst, at = 0.0, 0
    for y in range(10, BOTTOM - 28):
        d = abs(lum(rows[y + 1], x, bpp) - lum(rows[y], x, bpp))
        if d > worst:
            worst, at = d, y
    return worst, at


def main(out):
    shots = os.path.join(out, "shots")
    findings, cells = [], {}
    for look in LOOKS:
        for i, page in enumerate(PAGES):
            for backdrop in ("flat", "stripes"):
                name = f"{look}_{backdrop}_{i:02d}.png"
                cells[(look, backdrop, i)] = name
            path = os.path.join(shots, f"{look}_flat_{i:02d}.png")
            if not os.path.exists(path):
                findings.append((look, page, "no screenshot"))
                continue
            w, h, bpp, rows = read_png(path)
            if look != "solid":
                step, y = smooth_glass(rows, bpp)
                if step > 4.5:
                    findings.append((look, page, f"glass is not smooth: brightness jumps {step:.0f} at row {y}"))
    lines = ["# UI check", ""]
    if findings:
        lines.append(f"{len(findings)} thing(s) to look at:")
        lines += [f"- [{look}] {page}: {msg}" for look, page, msg in findings]
    else:
        lines.append("Nothing flagged. Still look at the contact sheet.")
    open(os.path.join(out, "report.md"), "w", encoding="utf-8").write("\n".join(lines) + "\n")

    flagged = {(look, page) for look, page, _ in findings}
    parts = ["<!doctype html><meta charset=utf-8><title>UI check</title>",
             "<style>body{font:14px system-ui;margin:20px;background:#eef0f4;color:#14161a}h2{margin:26px 0 8px}"
             ".row{display:flex;gap:10px;flex-wrap:wrap}figure{margin:0}figcaption{font-size:12px;margin-bottom:3px}"
             "img{width:400px;height:auto;border-radius:6px;display:block}.bad figcaption{color:#c2261b;font-weight:600}</style>",
             "<h1>UI check</h1>", "<pre>" + html.escape("\n".join(lines)) + "</pre>"]
    for i, page in enumerate(PAGES):
        parts.append(f"<h2>{i + 1}. {html.escape(page)}</h2><div class=row>")
        for look in LOOKS:
            for backdrop in ("flat", "stripes"):
                cls = " class=bad" if (look, page) in flagged and backdrop == "flat" else ""
                parts.append(f"<figure{cls}><figcaption>{look} / {backdrop}</figcaption><img src='shots/{cells[(look, backdrop, i)]}'></figure>")
        parts.append("</div>")
    open(os.path.join(out, "index.html"), "w", encoding="utf-8").write("\n".join(parts))
    print("\n".join(lines))
    print(f"\nContact sheet: {os.path.join(out, 'index.html')}")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "target/ui-check"))
