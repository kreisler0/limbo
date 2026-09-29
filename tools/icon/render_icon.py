"""Renders the Limbo app icon.

A dark grey tile with a white grid. In the middle floats a liquid-glass blob, not
quite circular, that refracts the grid behind it: magnified in the middle,
bent hard at the rim, with chromatic fringes (red, green and blue refract by
slightly different amounts).

    tools/icon/.venv/bin/python tools/icon/render_icon.py

Writes src-tauri/icons/* and ui/src/assets/limbo-mark.png.
"""

from __future__ import annotations

import math
import os
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
ICONS = ROOT / "src-tauri" / "icons"
UI_ASSETS = ROOT / "ui" / "src" / "assets"


def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def grid(x, y, cells, line):
    """White grid lines on black, anti-aliased. x, y in [0, 1] tile units."""
    fx = np.abs(((x * cells) + 0.5) % 1.0 - 0.5) / cells  # distance to nearest line
    fy = np.abs(((y * cells) + 0.5) % 1.0 - 0.5) / cells
    d = np.minimum(fx, fy)
    aa = 1.0 / 900.0
    return 1.0 - smoothstep(line * 0.5 - aa, line * 0.5 + aa, d)


def blob_radius(theta):
    """Organic, slightly lopsided outline (radius in tile units)."""
    r = 0.285
    r *= 1.0 + 0.060 * np.cos(2 * theta - 0.9) + 0.035 * np.cos(3 * theta + 1.7) + 0.014 * np.cos(5 * theta - 0.4)
    return r


def render(size: int, cells: int, line: float, ss: int = 3) -> Image.Image:
    n = size * ss
    coords = (np.arange(n) + 0.5) / n
    x, y = np.meshgrid(coords, coords)

    # Tile: rounded square (squircle-ish) with transparent corners.
    corner = 0.225
    qx = np.maximum(np.abs(x - 0.5) - (0.5 - corner), 0.0)
    qy = np.maximum(np.abs(y - 0.5) - (0.5 - corner), 0.0)
    tile_d = (qx ** 4 + qy ** 4) ** 0.25 - corner  # superellipse corners
    tile_alpha = 1.0 - smoothstep(-1.2 / n, 1.2 / n, tile_d)

    # Blob geometry: slightly above center, like a drop about to fall.
    cx, cy = 0.5, 0.49
    dx, dy = x - cx, y - cy
    rho = np.sqrt(dx * dx + dy * dy)
    theta = np.arctan2(dy, dx)
    R = blob_radius(theta)
    s = rho / R  # 0 at center, 1 on the outline
    inside = 1.0 - smoothstep(0.995, 1.005, s)

    # Lens height field: a soft dome (used for shading).
    t = np.clip(s, 0.0, 0.9999)
    dome = np.sqrt(1.0 - t ** 2.2)

    def sample(dispersion):
        # Radial lens profile: magnified in the middle, background compressed
        # toward the rim (you see "around" the blob there). Monotonic, so
        # lines bend but never fold into rings. Each color channel refracts a
        # little differently: chromatic fringes where the lines are compressed.
        g = 0.62 * t + (1.05 + dispersion) * t ** 4
        scale = np.where(t > 1e-6, g / np.maximum(t, 1e-6), 0.62)
        sx = cx + dx * scale
        sy = cy + dy * scale
        # A gentle twist so it reads as liquid rather than a plain lens.
        swirl = 0.16 * t ** 2
        cs, sn = np.cos(swirl), np.sin(swirl)
        rx, ry = sx - cx, sy - cy
        sx, sy = cx + rx * cs - ry * sn, cy + rx * sn + ry * cs
        return grid(sx, sy, cells, line)

    base = grid(x, y, cells, line)
    r = sample(-0.07)
    g = sample(0.0)
    b = sample(0.08)

    # Glass body: darker inside so the grid glows through; faint blue-cyan tint.
    # Dark grey tile (not pure black) with white lines.
    bg = np.array([0.125, 0.125, 0.13])
    body_dark = 0.80
    rgb = bg + (1.0 - bg) * np.stack([base, base, base], axis=-1)
    # Inside the glass the grey reads a touch deeper, so the drop has body.
    lens_bg = bg * 0.72
    lens = lens_bg + (body_dark - lens_bg) * np.stack([r, g, b * 1.03], axis=-1)
    lens += np.stack([0.02, 0.035, 0.06], axis=0)[None, None, :] * (0.4 + 0.6 * dome[..., None])

    # Light passing through the glass pools at the bottom: a faint cool glow.
    pool = np.exp(-(((x - cx) / 0.20) ** 2 + ((y - (cy + 0.17)) / 0.11) ** 2)) * inside
    lens += pool[..., None] * np.array([0.10, 0.16, 0.24])

    # Fresnel rim: bright where the glass turns edge-on.
    rim = smoothstep(0.80, 0.995, s) * inside
    light_dir = np.array([-0.62, -0.78])
    nx, ny = dx / (rho + 1e-9), dy / (rho + 1e-9)
    facing = np.clip(-(nx * light_dir[0] + ny * light_dir[1]), -1.0, 1.0)
    rim_light = rim * (0.35 + 0.65 * np.clip(-facing, 0.0, 1.0) ** 1.5)
    # Opposite rim catches a cooler, dimmer reflection.
    rim_back = rim * 0.35 * np.clip(facing, 0.0, 1.0) ** 2
    lens += rim_light[..., None] * np.array([1.0, 1.0, 1.0])
    lens += rim_back[..., None] * np.array([0.55, 0.75, 1.0])

    # Thin crisp outline.
    edge = np.exp(-((s - 0.985) / 0.012) ** 2) * inside
    lens += edge[..., None] * 0.55

    # Specular highlight: a soft sliver near the top-left.
    hx, hy = cx - 0.105, cy - 0.125
    hd = ((x - hx) / 0.105) ** 2 + ((y - hy) / 0.052) ** 2
    rot = ((x - hx) * 0.8 + (y - hy) * 0.6)
    spec = np.exp(-hd * 2.2) * (1.0 - smoothstep(-0.02, 0.06, rot * 0.0)) * inside
    lens += spec[..., None] * 0.85
    # A small secondary glint on the lower right.
    gx, gy = cx + 0.13, cy + 0.15
    glint = np.exp(-(((x - gx) / 0.030) ** 2 + ((y - gy) / 0.018) ** 2)) * inside
    lens += glint[..., None] * 0.35

    # Soft shadow/caustic under the blob on the grid.
    sh = np.exp(-(((x - cx) / 0.33) ** 2 + ((y - (cy + 0.33)) / 0.07) ** 2))
    rgb *= (1.0 - 0.35 * sh)[..., None]
    caustic = np.exp(-(((x - cx - 0.03) / 0.12) ** 2 + ((y - (cy + 0.26)) / 0.025) ** 2)) * (1.0 - inside)
    rgb += caustic[..., None] * np.array([0.20, 0.26, 0.34])

    out = rgb * (1.0 - inside[..., None]) + lens * inside[..., None]
    out = np.clip(out, 0.0, 1.0)

    # Subtle vignette so the tile has depth.
    vig = 1.0 - 0.12 * smoothstep(0.40, 0.75, np.sqrt((x - 0.5) ** 2 + (y - 0.5) ** 2))
    out *= vig[..., None]

    rgba = np.concatenate([out, tile_alpha[..., None]], axis=-1)
    img = Image.fromarray((rgba * 255.0 + 0.5).astype(np.uint8), "RGBA")
    return img.resize((size, size), Image.LANCZOS)


def design_for(size: int):
    """Fewer, bolder grid lines at small sizes so they stay crisp."""
    if size <= 16:
        return 4, 0.060
    if size <= 24:
        return 5, 0.045
    if size <= 32:
        return 6, 0.034
    if size <= 64:
        return 7, 0.022
    return 8, 0.012


def main():
    ICONS.mkdir(parents=True, exist_ok=True)
    UI_ASSETS.mkdir(parents=True, exist_ok=True)
    cache = {}

    def icon(size):
        if size not in cache:
            cells, line = design_for(size)
            ss = 8 if size <= 64 else (3 if size <= 256 else 2)
            cache[size] = render(size, cells, line, ss)
        return cache[size]

    master = icon(1024)
    master.save(ICONS / "icon.png")
    icon(32).save(ICONS / "32x32.png")
    icon(128).save(ICONS / "128x128.png")
    icon(256).save(ICONS / "128x128@2x.png")
    icon(64).save(UI_ASSETS / "limbo-mark.png", optimize=True)

    sizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
    frames = [icon(s) for s in sizes]
    frames[-1].save(ICONS / "icon.ico", format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[:-1])

    # Preview sheet for review (not shipped).
    sheet = Image.new("RGBA", (1024 + 40 + 256 + 40, 1024), (245, 245, 243, 255))
    sheet.alpha_composite(master, (0, 0))
    y = 0
    for s in [256, 128, 64, 48, 32, 24, 16]:
        sheet.alpha_composite(icon(s), (1024 + 40, y))
        y += s + 16
    preview = Path(os.environ.get("ICON_PREVIEW", ROOT / "tools" / "icon" / "preview.png"))
    sheet.save(preview)
    print("wrote", ICONS, "and", preview)


if __name__ == "__main__":
    main()
