#!/usr/bin/env python3
"""Build the karez K mark as SVG, and optionally PNG and ICO, from one geometry definition.

The shapes were measured from the first Gemini master (docs/brand/reference/karez-k-solid-v1.webp)
and rebuilt as exact vector geometry, so the colour, white, dark, amber, tile and avatar
variants all come from one source and keep a true transparent background.

    python scripts/brand/build_icons.py          # writes assets/svg/*.svg
    python scripts/brand/build_icons.py --png    # also writes assets/png/*.png and assets/favicon.ico
                                                 # (renders with Microsoft Edge in headless mode, resizes with Pillow)

The geometry is in the master's pixel coordinates. Two presets share it:
  full   the look of the master, for 128 px and above
  small  a thicker stream, thicker gaps and a bolder three-line fan, for favicons and avatars
"""
import argparse
import math
import subprocess
import sys
import tempfile
from pathlib import Path

AMBER, CYAN, NAVY, WHITE = "#F59E0B", "#22D3EE", "#0A1020", "#FFFFFF"

# Silhouette of the K: stem, upper arm, lower arm, with the notch under the join.
K_PATH = ("M631,542 H825 V915 L1187,542 H1442 L1032,968 L1363,1340 H1115 "
          "L915,1118 L825,1221 V1495 H631 Z")
CAP = (686, 510, 84, 60, 26)                      # x, y, w, h, rx: the round cap on the shaft
# Centre line of the channel: up the stem, over an arch, then down the lower arm.
CHANNEL = "M728,1495 V1230 C728,1110 790,1023 905,1023 C965,1023 993,1065 1033,1110 L1242,1340"
CHANNEL_CUT = "M728,1540 V1230 C728,1110 790,1023 905,1023 C965,1023 993,1065 1033,1110 L1296,1399"
FAN_ORIGIN = (1242, 1340)
BASE_ANGLE = math.degrees(math.atan2(0.741, 0.672))   # direction of the lower arm, below horizontal

PRESETS = {
    #         stream, gap, fan line, dot radius, spread, lengths
    "full":  dict(cw=30, gap=23, fw=26, dr=22, spread=24, lens=(160, 170, 160)),
    "small": dict(cw=54, gap=22, fw=36, dr=30, spread=30, lens=(150, 175, 150)),
}


def fan_ends(p):
    ox, oy = FAN_ORIGIN
    ends = []
    for ang, ln in zip((BASE_ANGLE - p["spread"], BASE_ANGLE, BASE_ANGLE + p["spread"]), p["lens"]):
        a = math.radians(ang)
        ends.append((ox + ln * math.cos(a), oy + ln * math.sin(a)))
    return ends


def mark(preset, k_color, s_color, uid):
    """Return (defs, body, bbox) for one mark."""
    p = PRESETS[preset]
    cut_w = p["cw"] + 2 * p["gap"]
    mid = f"cut-{uid}"
    defs = (f'<mask id="{mid}" maskUnits="userSpaceOnUse" x="400" y="400" width="1400" height="1400">'
            f'<rect x="400" y="400" width="1400" height="1400" fill="#fff"/>'
            f'<path d="{CHANNEL_CUT}" fill="none" stroke="#000" stroke-width="{cut_w}"/></mask>')
    cx, cy, cw_, ch, crx = CAP
    ends = fan_ends(p)
    fan = "".join(
        f'<line x1="{FAN_ORIGIN[0]}" y1="{FAN_ORIGIN[1]}" x2="{x:.1f}" y2="{y:.1f}" stroke="{s_color}" '
        f'stroke-width="{p["fw"]}" stroke-linecap="round"/>'
        f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{p["dr"]}" fill="{s_color}"/>' for x, y in ends)
    body = (f'<path d="{K_PATH}" fill="{k_color}" mask="url(#{mid})"/>'
            f'<rect x="{cx}" y="{cy}" width="{cw_}" height="{ch}" rx="{crx}" fill="{k_color}"/>'
            f'<path d="{CHANNEL}" fill="none" stroke="{s_color}" stroke-width="{p["cw"]}"/>'
            f'{fan}')
    x1 = max(1442, max(x for x, _ in ends) + p["dr"])
    y1 = max(1495, max(y for _, y in ends) + p["dr"])
    return defs, body, (631, 510, x1, y1)


def doc(viewbox, defs, body, width=512):
    vx, vy, vw, vh = viewbox
    return ('<svg xmlns="http://www.w3.org/2000/svg" '
            f'viewBox="{vx:.1f} {vy:.1f} {vw:.1f} {vh:.1f}" width="{width}" height="{round(width * vh / vw)}" '
            'shape-rendering="geometricPrecision">'
            f'<defs>{defs}</defs>{body}</svg>\n')


def plain(preset, k, s, uid, margin=0.07):
    defs, body, (x0, y0, x1, y1) = mark(preset, k, s, uid)
    side = max(x1 - x0, y1 - y0) * (1 + 2 * margin)
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    return doc((cx - side / 2, cy - side / 2, side, side), defs, body)


def placed(preset, k, s, uid, size, height_frac, cx_t=None, cy_t=None):
    """The mark scaled to height_frac of a size x size canvas, centred. Returns (defs, group)."""
    defs, body, (x0, y0, x1, y1) = mark(preset, k, s, uid)
    sc = size * height_frac / (y1 - y0)
    cx_t = size / 2 if cx_t is None else cx_t
    cy_t = size / 2 if cy_t is None else cy_t
    tx = cx_t - sc * (x0 + x1) / 2
    ty = cy_t - sc * (y0 + y1) / 2
    return defs, f'<g transform="translate({tx:.2f} {ty:.2f}) scale({sc:.5f})">{body}</g>'


def tile():
    d, g = placed("full", AMBER, CYAN, "tile", 1024, 0.62)
    body = (f'<rect width="1024" height="1024" rx="224" fill="{NAVY}"/>'
            f'<rect x="56" y="56" width="912" height="912" rx="176" fill="none" stroke="{AMBER}" stroke-width="8"/>{g}')
    return doc((0, 0, 1024, 1024), d, body, 1024)


def avatar(circle):
    d, g = placed("small", AMBER, CYAN, "av", 1024, 0.58)
    if circle:
        body = (f'<circle cx="512" cy="512" r="506" fill="{NAVY}"/>'
                f'<circle cx="512" cy="512" r="482" fill="none" stroke="{AMBER}" stroke-width="20"/>{g}')
    else:
        body = f'<rect width="1024" height="1024" fill="{NAVY}"/>{g}'
    return doc((0, 0, 1024, 1024), d, body, 1024)


def build_svgs(out):
    out.mkdir(parents=True, exist_ok=True)
    files = {
        "karez-mark.svg": plain("full", AMBER, CYAN, "m"),
        "karez-mark-small.svg": plain("small", AMBER, CYAN, "s", margin=0.05),
        "karez-mark-white.svg": plain("full", WHITE, WHITE, "w"),
        "karez-mark-dark.svg": plain("full", NAVY, NAVY, "d"),
        "karez-mark-amber.svg": plain("full", AMBER, AMBER, "a"),
        "karez-tile.svg": tile(),
        "karez-avatar.svg": avatar(False),
        "karez-avatar-circle.svg": avatar(True),
    }
    for name, text in files.items():
        (out / name).write_text(text, encoding="utf-8", newline="\n")
        print("wrote", out / name)
    return files


EDGE = Path(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")


def render(svg, png, px):
    """Render an SVG to a transparent PNG at px x px with headless Edge, via a temporary page."""
    from PIL import Image
    with tempfile.TemporaryDirectory() as tmp:
        page = Path(tmp) / "page.html"
        page.write_text(
            f'<!doctype html><html><body style="margin:0;background:transparent">'
            f'<img src="{svg.resolve().as_uri()}" style="display:block;width:{px}px;height:{px}px"></body></html>',
            encoding="utf-8")
        raw = Path(tmp) / "shot.png"
        subprocess.run([str(EDGE), "--headless=new", "--disable-gpu", "--hide-scrollbars",
                        "--allow-file-access-from-files", f"--user-data-dir={tmp}/profile",
                        "--default-background-color=00000000", f"--window-size={px},{px}",
                        f"--screenshot={raw}", page.as_uri()],
                       check=True, timeout=90, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        im = Image.open(raw).convert("RGBA")
        if im.size != (px, px):
            im = im.crop((0, 0, px, px))
        png.parent.mkdir(parents=True, exist_ok=True)
        im.save(png)
        print("wrote", png, im.size, "corner alpha", im.getpixel((0, 0))[3])


def build_pngs(svg_dir, png_dir, root):
    from PIL import Image
    jobs = [("karez-mark.svg", "karez-mark-512.png", 512),
            ("karez-mark-white.svg", "karez-mark-white-512.png", 512),
            ("karez-mark-dark.svg", "karez-mark-dark-512.png", 512),
            ("karez-mark-amber.svg", "karez-mark-amber-512.png", 512),
            ("karez-tile.svg", "karez-tile-1024.png", 1024),
            ("karez-avatar.svg", "karez-avatar-500.png", 500),
            ("karez-avatar-circle.svg", "karez-avatar-circle-500.png", 500),
            ("karez-avatar.svg", "apple-touch-icon-180.png", 180),
            ("karez-mark-small.svg", "favicon-512-source.png", 512)]
    for svg, png, px in jobs:
        render(svg_dir / svg, png_dir / png, px)
    # Favicon sizes: downscale the 512 px render with Lanczos.
    src = Image.open(png_dir / "favicon-512-source.png").convert("RGBA")
    sizes = [16, 32, 48, 64, 128, 192]
    for s in sizes:
        src.resize((s, s), Image.LANCZOS).save(png_dir / f"favicon-{s}.png")
    (png_dir / "favicon-512-source.png").replace(png_dir / "favicon-512.png")
    src.save(root / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
    print("wrote", root / "favicon.ico")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--png", action="store_true", help="also render PNG and ICO files (needs Edge and Pillow)")
    ap.add_argument("--out", default=str(Path(__file__).resolve().parents[2] / "assets"))
    a = ap.parse_args()
    root = Path(a.out)
    build_svgs(root / "svg")
    if a.png:
        build_pngs(root / "svg", root / "png", root)


if __name__ == "__main__":
    sys.exit(main())
