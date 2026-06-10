#!/usr/bin/env python3
"""Marketing hero: header (icon + name + tagline + badges) above a 3D fan of the
app screenshots — perspective + rotation so each window frame is clearly tilted,
and the side cards stay ~80% visible."""
from PIL import Image, ImageDraw, ImageFilter, ImageFont

SHOTS = "/Users/ganin/Work/trashly/docs/screenshots"
ICON = "/tmp/trashly-icon.png"
OUT = f"{SHOTS}/hero.png"
W, H = 3000, 1700

TITLE = "Trashly"
TAGLINE = "Reclaim gigabytes on your Mac — clean, uninstall, optimize & monitor."
BADGES = ["AGPLv3", "macOS", "Free & open-source"]


def sf(size, weight):
    f = ImageFont.truetype("/System/Library/Fonts/SFNS.ttf", size)
    try:
        f.set_variation_by_name(weight)
    except Exception:
        pass
    return f


# ── perspective (no numpy) ──────────────────────────────────────────────────
def _solve(A, b):
    n = len(b)
    M = [row[:] + [b[i]] for i, row in enumerate(A)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(M[r][c]))
        M[c], M[p] = M[p], M[c]
        pv = M[c][c]
        for j in range(c, n + 1):
            M[c][j] /= pv
        for r in range(n):
            if r != c and M[r][c]:
                f = M[r][c]
                for j in range(c, n + 1):
                    M[r][j] -= f * M[c][j]
    return [M[i][n] for i in range(n)]


def _coeffs(rect, quad):
    A, B = [], []
    for (rx, ry), (qx, qy) in zip(rect, quad):
        A.append([rx, ry, 1, 0, 0, 0, -qx * rx, -qx * ry]); B.append(qx)
        A.append([0, 0, 0, rx, ry, 1, -qy * rx, -qy * ry]); B.append(qy)
    return _solve(A, B)


def card(name, scale, lean):
    """A screenshot turned into a 3D-tilted card: draw a light window-edge
    border, foreshorten with perspective, then rotate so the verticals slant."""
    im = Image.open(f"{SHOTS}/{name}").convert("RGBA")
    im = im.resize((round(im.width * scale), round(im.height * scale)), Image.LANCZOS)
    w, h = im.size
    ImageDraw.Draw(im).rounded_rectangle(
        [1, 1, w - 2, h - 2], radius=16, outline=(255, 255, 255, 95), width=3)
    if lean == 0:
        return im
    k, inset = 0, w * 0
    rect = [(0, 0), (w, 0), (w, h), (0, h)]
    if lean < 0:  # outer (left) edge recedes
        quad = [(inset, h * k), (w, 0), (w, h), (inset, h * (1 - k))]
    else:         # outer (right) edge recedes
        quad = [(0, 0), (w - inset, h * k), (w - inset, h * (1 - k)), (0, h)]
    im = im.transform((w, h), Image.PERSPECTIVE, _coeffs(rect, quad), Image.BICUBIC)
    return im.rotate(8 if lean < 0 else -8, expand=True, resample=Image.BICUBIC)


# ── background: dark gradient + soft blue glow ──────────────────────────────
TOP, BOT = (16, 18, 24), (9, 10, 14)
bg = Image.new("RGB", (W, H))
px = bg.load()
for y in range(H):
    t = y / (H - 1)
    row = tuple(round(TOP[i] + (BOT[i] - TOP[i]) * t) for i in range(3))
    for x in range(W):
        px[x, y] = row
bg = bg.convert("RGBA")
glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
ImageDraw.Draw(glow).ellipse([W * 0.24, H * 0.04, W * 0.76, H * 0.72], fill=(70, 120, 240, 95))
hero = Image.alpha_composite(bg, glow.filter(ImageFilter.GaussianBlur(200)))
draw = ImageDraw.Draw(hero)

# ── header ──────────────────────────────────────────────────────────────────
icon = Image.open(ICON).convert("RGBA").resize((190, 190), Image.LANCZOS)
title_font = sf(150, "Heavy")
tw = draw.textlength(TITLE, font=title_font)
gap = 34
gx = round((W - (icon.width + gap + tw)) / 2)
cy = 185
hero.alpha_composite(icon, (gx, cy - icon.height // 2))
draw.text((gx + icon.width + gap, cy), TITLE, font=title_font, fill=(245, 247, 250, 255), anchor="lm")
draw.text((W // 2, 352), TAGLINE, font=sf(52, "Medium"), fill=(170, 180, 196, 255), anchor="mm")

bfont = sf(38, "Bold")
pad_x, pad_y, bgap = 36, 20, 26
sizes = [draw.textlength(b, font=bfont) for b in BADGES]
bh = 38 + pad_y * 2
widths = [s + pad_x * 2 for s in sizes]
bx = round((W - (sum(widths) + bgap * (len(BADGES) - 1))) / 2)
by = 446
for b, bw in zip(BADGES, widths):
    draw.rounded_rectangle([bx, by, bx + bw, by + bh], radius=bh / 2,
                           fill=(22, 26, 34, 235), outline=(255, 255, 255, 55), width=2)
    draw.text((bx + bw / 2, by + bh / 2), b, font=bfont, fill=(246, 248, 252, 255), anchor="mm")
    bx += bw + bgap

# ── 3D fan ──────────────────────────────────────────────────────────────────
center = card("dashboard.png", 0.56, 0)
clean = card("clean.png", 0.52, -1)
uninstall = card("uninstall.png", 0.52, 1)

cx = (W - center.width) // 2
placements = [
    (clean, cx + 150 - clean.width, 690),
    (uninstall, cx + center.width - 150, 690),
    (center, cx, 615),  # Dashboard, centre, on top
]


def drop_shadow(c, x, y):
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    layer.paste(Image.new("RGBA", c.size, (0, 0, 0, 150)), (x, y + 36), c)
    return layer.filter(ImageFilter.GaussianBlur(60))


for c, x, y in placements:
    hero = Image.alpha_composite(hero, drop_shadow(c, x, y))
for c, x, y in placements:
    hero.alpha_composite(c, (x, y))

hero.convert("RGB").save(OUT)
print("wrote", OUT, hero.size)
