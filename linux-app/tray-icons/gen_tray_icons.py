#!/usr/bin/env python3
"""
OctoPad tray icon generator (3 states, Nextcloud-style).

The whole pad body is colored according to status so the difference is clearly
visible at small tray sizes (16-22px), with white buttons on top:

- "connected"       : GREEN pad  -> octopad-tray
- "checking"        : YELLOW pad -> octopad-tray-checking
- "off" (disconnected): RED pad  -> octopad-tray-off

Geometry (viewBox 512):
- Pad silhouette: x=88,y=120,w=336,h=272,r=48
- LED strip: x=132,y=150,w=248,h=10,r=5 (transparent white)
- 2x4 buttons: w=68,h=84,r=16; xs=96,180,264,348; y=178 and 292 (white)

Generates source SVGs + PNGs of all sizes under out/.
"""

import os
import subprocess
import sys

BASE = os.path.dirname(os.path.abspath(__file__))
SIZES = (22, 24, 32, 48, 64, 128)

STATES = {
    "connected": dict(
        name="octopad-tray",
        pad_fill="#22c55e", pad_stroke="#15803d",
        toksvg="tray-icon-connected.svg"),
    "checking": dict(
        name="octopad-tray-checking",
        pad_fill="#f59e0b", pad_stroke="#b45309",
        toksvg="tray-icon-checking.svg"),
    "off": dict(
        name="octopad-tray-off",
        pad_fill="#ef4444", pad_stroke="#b91c1c",
        toksvg="tray-icon-off.svg"),
}


def tiles_svg():
    out = []
    for x in (96, 180, 264, 348):
        for y in (178, 292):
            out.append(f'  <rect x="{x}" y="{y}" width="68" height="84" rx="16" fill="#ffffff"/>\n')
    return "".join(out)


def build_svg(p):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="100%" height="100%">
  <g transform="translate(256 256) scale(1.24) translate(-256 -256)">
    <rect x="88" y="120" width="336" height="272" rx="48" fill="{p['pad_fill']}" stroke="{p['pad_stroke']}" stroke-width="10"/>
    <rect x="132" y="150" width="248" height="10" rx="5" fill="#ffffff" opacity="0.85"/>
{tiles_svg()}  </g>
</svg>
'''


def render_png(svg, px, state):
    src = os.path.join(BASE, svg)
    dst = os.path.join(BASE, "out", f"{state}-n{px}.png")
    subprocess.run(
        ["magick", "-background", "none", "-density", str(px * 8), src,
         "-resize", f"{px}x{px}", dst], check=True)
    return dst


def main() -> int:
    os.makedirs(os.path.join(BASE, "out"), exist_ok=True)
    for state, p in STATES.items():
        toksvg = p["toksvg"]
        open(os.path.join(BASE, toksvg), "w").write(build_svg(p))
        print(f"[gen] SVG {toksvg} written")
        for px in SIZES:
            render_png(toksvg, px, state)
        print(f"[gen] PNG {SIZES} for {toksvg}")
    print("[gen] done")
    return 0


if __name__ == "__main__":
    sys.exit(main())