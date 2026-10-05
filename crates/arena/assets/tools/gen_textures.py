#!/usr/bin/env python3
"""Procedural textures for the arena: card back, playmats, mana/keyword icons.
Run from anywhere: python3 gen_textures.py   (needs numpy + Pillow)."""
import math, os
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "textures")
os.makedirs(os.path.join(OUT, "icons"), exist_ok=True)
rng = np.random.default_rng(7)


def fbm(h, w, octaves=6, base=4, persistence=0.5, seed=0):
    r = np.random.default_rng(seed)
    out = np.zeros((h, w), np.float32)
    amp, tot = 1.0, 0.0
    for o in range(octaves):
        n = base * 2 ** o
        g = r.random((n + 1, max(2, int(n * w / h)) + 1)).astype(np.float32)
        im = Image.fromarray((g * 255).astype(np.uint8)).resize((w, h), Image.BICUBIC)
        out += amp * (np.asarray(im, np.float32) / 255)
        tot += amp
        amp *= persistence
    return out / tot


def save(arr, name):
    arr = np.clip(arr, 0, 1)
    Image.fromarray((arr * 255).astype(np.uint8)).save(os.path.join(OUT, name), optimize=True)
    print("wrote", name)


def lerp(a, b, t):
    return a + (b - a) * t[..., None]


# --------------------------------------------------------------------------- card back
def card_back():
    W, H = 488, 680
    S = 2  # supersample for edges
    w, h = W * S, H * S
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    u, v = (x / w - 0.5) * 2, (y / h - 0.5) * 2
    # rounded rect mask
    rad = 0.075 * w
    qx = np.abs(x - w / 2) - (w / 2 - rad)
    qy = np.abs(y - h / 2) - (h / 2 - rad)
    d = np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - rad
    mask = np.clip(0.5 - d / S, 0, 1)
    # frame = dark leathery brown
    n = fbm(h, w, 5, 8, 0.6, 1)
    frame = lerp(np.array([0.12, 0.06, 0.035]), np.array([0.2, 0.1, 0.055]), n)
    # inner panel inset
    inset = 0.055 * w
    qx2 = np.abs(x - w / 2) - (w / 2 - inset - rad * 0.4)
    qy2 = np.abs(y - h / 2) - (h / 2 - inset - rad * 0.4)
    d2 = np.hypot(np.maximum(qx2, 0), np.maximum(qy2, 0)) + np.minimum(np.maximum(qx2, qy2), 0) - rad * 0.4
    inner = np.clip(0.5 - d2 / S, 0, 1)
    # swirl pattern
    r = np.hypot(u * 0.72, v * 0.5) * 1.0
    th = np.arctan2(v * 0.5, u * 0.72)
    spiral = np.sin(5 * th + 9 * np.log(r + 0.05) - 1.2) * 0.5 + 0.5
    spiral2 = np.sin(3 * th - 6 * np.log(r + 0.08) + 1.0) * 0.5 + 0.5
    glow = np.exp(-r * 2.2)
    amber = np.array([0.95, 0.5, 0.1])
    deep = np.array([0.35, 0.12, 0.03])
    t = np.clip(glow * (0.35 + 0.65 * spiral) + 0.18 * spiral2 * np.exp(-r * 1.2), 0, 1)
    t = t ** 1.15
    base = lerp(np.array([0.16, 0.07, 0.04]), deep, np.clip(0.4 + 0.6 * n, 0, 1) * np.exp(-r * 0.9))
    inner_col = lerp(base, amber, t * 0.95)
    # concentric ring ornament + central oval
    ring = np.exp(-((r - 0.62) / 0.012) ** 2) * 0.5 + np.exp(-((r - 0.18) / 0.012) ** 2) * 0.6
    inner_col += ring[..., None] * np.array([0.9, 0.6, 0.2])
    # corner ornaments: small gold diamonds
    for cx, cy in [(0.17, 0.12), (0.83, 0.12), (0.17, 0.88), (0.83, 0.88)]:
        dd = np.abs(x / w - cx) + np.abs(y / h - cy) * (h / w)
        inner_col += (np.exp(-(dd / 0.018) ** 2))[..., None] * np.array([0.8, 0.55, 0.2])
    # gold hairline around inner panel
    line = np.exp(-((d2 + 2 * S) / (1.3 * S)) ** 2)
    col = frame * (1 - inner[..., None]) + inner_col * inner[..., None]
    col += line[..., None] * np.array([0.85, 0.58, 0.2]) * 0.9
    # outer bevel highlight
    bev = np.exp(-((d + 3 * S) / (1.5 * S)) ** 2) * 0.12
    col += bev[..., None]
    # vignette
    col *= (1 - 0.35 * (np.abs(u) ** 3 + np.abs(v) ** 3))[..., None]
    rgba = np.dstack([np.clip(col, 0, 1), mask])
    im = Image.fromarray((rgba * 255).astype(np.uint8), "RGBA").resize((W, H), Image.LANCZOS)
    im.save(os.path.join(OUT, "card_back.png"), optimize=True)
    print("wrote card_back.png")


# --------------------------------------------------------------------------- playmats
def vignette(h, w, k=0.55):
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    u, v = (x / w - 0.5) * 2, (y / h - 0.5) * 2
    return 1 - k * np.clip(np.hypot(u * 0.9, v) - 0.25, 0, 1) ** 1.6, u, v


def playmat_sanctum():
    W, H = 2048, 1152
    vig, u, v = vignette(H, W, 0.6)
    n = fbm(H, W, 6, 3, 0.55, 11)
    gold = lerp(np.array([0.55, 0.33, 0.07]), np.array([0.85, 0.58, 0.16]), n)
    # big mirrored swooshes (like the arena table's ornate ribbon channels)
    X = np.abs(u)
    swoosh = 0
    for off, wd in [(0.62, 0.012), (0.70, 0.02), (0.40, 0.008)]:
        curve = off + 0.35 * np.sin(v * 2.2 + 0.4) * (0.5 + 0.5 * np.cos(v * 1.3))
        swoosh += np.exp(-((X - curve) / wd) ** 2)
    swoosh = np.clip(swoosh, 0, 1)
    edge_dark = np.exp(-((X - 0.97) / 0.05) ** 2)
    col = gold * (0.88 + 0.12 * swoosh[..., None])
    col += swoosh[..., None] * np.array([0.12, 0.08, 0.02])
    # central lighter lens in the middle, subtle hex pattern
    centre = np.exp(-((u / 0.8) ** 2 + (v / 0.9) ** 2) * 1.5)
    col = col * (0.82 + 0.25 * centre[..., None])
    # fine stone seams
    seam = (np.abs(np.sin(u * 22 + n * 3)) < 0.01) * 0.04 + (np.abs(np.sin(v * 12 + n * 2)) < 0.01) * 0.04
    col -= seam[..., None]
    # centre line
    col -= (np.exp(-(v / 0.004) ** 2) * 0.08)[..., None]
    # green-ish dark edges (mossy corners)
    col = lerp(col, col * np.array([0.35, 0.4, 0.2]), np.clip(edge_dark + np.clip(X - 0.9, 0, 1) * 6, 0, 0.9))
    col *= vig[..., None]
    save(col, "playmat_sanctum.png")


def playmat_jungle():
    W, H = 2048, 1152
    vig, u, v = vignette(H, W, 0.55)
    n = fbm(H, W, 7, 3, 0.55, 21)
    n2 = fbm(H, W, 5, 6, 0.5, 22)
    grass = lerp(np.array([0.13, 0.32, 0.08]), np.array([0.42, 0.55, 0.14]), n)
    # orange earth patches
    patch = np.clip((n2 - 0.52) * 4, 0, 1)
    col = lerp(grass, grass * 0.4 + np.array([0.5, 0.3, 0.1]) * 0.6, patch * 0.7)
    # river (meandering)
    cx = 0.05 * np.sin(v * 5 + 1) + 0.2 * np.sin(v * 1.7) - 0.55
    river = np.clip(1 - np.abs(u - cx) / 0.045, 0, 1)
    river2 = np.clip(1 - np.abs(u + 0.15 * np.sin(v * 2.4 + 2) - 0.7) / 0.03, 0, 1)
    rv = np.clip(river + river2, 0, 1) ** 0.7
    col = lerp(col, np.array([0.1, 0.35, 0.5]) + n[..., None] * 0.1, rv * 0.75)
    # darken centre-ish slightly for contrast with cards? keep low contrast
    col = col * 0.8 + 0.1
    col *= vig[..., None]
    save(col, "playmat_jungle.png")


def playmat_storm():
    W, H = 2048, 1152
    vig, u, v = vignette(H, W, 0.5)
    # lower: mossy stone with circular ring, upper: purple storm clouds
    n = fbm(H, W, 7, 3, 0.58, 31)
    n2 = fbm(H, W, 6, 5, 0.55, 32)
    stone = lerp(np.array([0.1, 0.14, 0.14]), np.array([0.28, 0.34, 0.32]), n)
    ring = np.exp(-((np.hypot(u * 1.0, (v - 0.1) * 1.4) - 0.55) / 0.03) ** 2)
    ring2 = np.exp(-((np.hypot(u * 1.0, (v - 0.1) * 1.4) - 0.35) / 0.015) ** 2)
    stone += ((ring * 0.08) + (ring2 * 0.06))[..., None]
    stone -= (np.exp(-(u / 0.004) ** 2) * 0.05)[..., None]
    cloud_t = np.clip(-v * 1.1 + 0.15, 0, 1)
    clouds = lerp(np.array([0.1, 0.08, 0.18]), np.array([0.5, 0.4, 0.65]), np.clip(n2 * 1.3 - 0.2, 0, 1))
    col = lerp(stone, clouds, cloud_t ** 0.8 * 0.95)
    # a few lightning bolts (faint)
    img = Image.new("L", (W, H), 0)
    dr = ImageDraw.Draw(img)
    r = np.random.default_rng(5)
    for _ in range(4):
        x0, y0 = r.uniform(0.1, 0.9) * W, 0
        pts = [(x0, y0)]
        for i in range(14):
            x0 += r.uniform(-60, 60); y0 += r.uniform(30, 55)
            pts.append((x0, y0))
        dr.line(pts, fill=200, width=3)
    bolt = np.asarray(img.filter(ImageFilter.GaussianBlur(2)), np.float32) / 255
    glowb = np.asarray(img.filter(ImageFilter.GaussianBlur(14)), np.float32) / 255
    col += (bolt * 0.5 + glowb * 0.7)[..., None] * np.array([0.6, 0.6, 1.0]) * 0.6
    col *= vig[..., None]
    save(col, "playmat_storm.png")


# --------------------------------------------------------------------------- icons
SS = 4
SZ = 64


def canvas():
    im = Image.new("RGBA", (SZ * SS, SZ * SS), (0, 0, 0, 0))
    return im, ImageDraw.Draw(im)


def fin(im, name):
    im.resize((SZ, SZ), Image.LANCZOS).save(os.path.join(OUT, "icons", name), optimize=True)


def P(pts):
    return [(x * SS, y * SS) for x, y in pts]


def circle(dr, cx, cy, r, **kw):
    dr.ellipse([(cx - r) * SS, (cy - r) * SS, (cx + r) * SS, (cy + r) * SS], **kw)


MANA = {
    "W": ((249, 243, 214), (170, 150, 90)),
    "U": ((120, 180, 235), (30, 80, 150)),
    "B": ((120, 100, 110), (35, 25, 35)),
    "R": ((240, 120, 90), (160, 35, 25)),
    "G": ((110, 190, 110), (25, 95, 45)),
    "C": ((205, 205, 210), (110, 110, 120)),
}


def glyph(dr, key, col):
    c = col
    if key == "W":  # sun
        circle(dr, 32, 32, 7, fill=c)
        for i in range(8):
            a = i * math.pi / 4
            dr.line(P([(32 + 11 * math.cos(a), 32 + 11 * math.sin(a)), (32 + 17 * math.cos(a), 32 + 17 * math.sin(a))]), fill=c, width=3 * SS)
    elif key == "U":  # drop
        pts = [(32, 13)] + [(32 + 12 * math.sin(a), 38 - 12 * math.cos(a) * -1 * -1 + 0) for a in []]
        pts = [(32, 12), (42, 30)]
        for i in range(0, 181, 15):
            a = math.radians(i)
            pts.append((32 + 10 * math.cos(math.radians(i - 0)) * 1, 36 + 10 * math.sin(a)))
        d = [(32, 12), (43, 31)] + [(32 + 11 * math.cos(math.radians(t)), 36 + 11 * math.sin(math.radians(t))) for t in range(10, 171, 10)] + [(21, 31)]
        dr.polygon(P(d), fill=c)
    elif key == "B":  # skull
        circle(dr, 32, 29, 12, fill=c)
        dr.rounded_rectangle([24 * SS, 34 * SS, 40 * SS, 46 * SS], radius=3 * SS, fill=c)
        for ex in (27, 37):
            circle(dr, ex, 30, 3.4, fill=(20, 12, 20, 255))
        dr.polygon(P([(32, 34), (30, 38), (34, 38)]), fill=(20, 12, 20, 255))
    elif key == "R":  # flame
        d = [(32, 10), (40, 22), (44, 32), (42, 42), (32, 50), (22, 42), (20, 32), (26, 24), (28, 30), (32, 10)]
        dr.polygon(P(d), fill=c)
        dr.polygon(P([(32, 30), (37, 38), (35, 45), (29, 45), (27, 38)]), fill=(255, 225, 150, 255))
    elif key == "G":  # tree
        circle(dr, 32, 25, 11, fill=c)
        circle(dr, 24, 32, 8, fill=c)
        circle(dr, 40, 32, 8, fill=c)
        dr.polygon(P([(29, 36), (35, 36), (36, 52), (28, 52)]), fill=c)
    elif key == "C":
        dr.polygon(P([(32, 14), (46, 32), (32, 50), (18, 32)]), fill=c)


def mana_icon(k):
    light, dark = MANA[k]
    im, dr = canvas()
    circle(dr, 32, 33, 28, fill=(0, 0, 0, 90))  # shadow
    circle(dr, 32, 31, 28, fill=dark + (255,))
    circle(dr, 32, 31, 25, fill=light + (255,))
    # soft inner gradient
    g = Image.new("RGBA", im.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(g)
    circle(gd, 28, 24, 18, fill=(255, 255, 255, 60))
    g = g.filter(ImageFilter.GaussianBlur(10 * SS / 4))
    im = Image.alpha_composite(im, g)
    dr = ImageDraw.Draw(im)
    glyph(dr, k, dark + (255,) if k not in ("B",) else (200, 185, 200, 255))
    fin(im, f"mana_{k.lower()}.png")


def hexagon(cx, cy, r, rot=math.pi / 6):
    return [(cx + r * math.cos(rot + i * math.pi / 3), cy + r * math.sin(rot + i * math.pi / 3)) for i in range(6)]


def kw_glyph(dr, key, c):
    w = 3 * SS
    if key == "flying":
        dr.polygon(P([(14, 40), (22, 24), (32, 32), (42, 24), (50, 40), (42, 36), (32, 44), (22, 36)]), fill=c)
    elif key == "trample":
        for ox in (-9, 0, 9):
            circle(dr, 32 + ox, 36, 4.2, fill=c)
        dr.polygon(P([(22, 24), (26, 18), (30, 24)]), fill=c)
        dr.polygon(P([(34, 24), (38, 18), (42, 24)]), fill=c)
    elif key == "deathtouch":
        circle(dr, 32, 29, 10, fill=c)
        dr.rounded_rectangle([25 * SS, 33 * SS, 39 * SS, 43 * SS], radius=3 * SS, fill=c)
        for ex in (28, 36):
            circle(dr, ex, 30, 2.8, fill=(25, 20, 28, 255))
    elif key == "lifelink":
        circle(dr, 26, 27, 8, fill=c); circle(dr, 38, 27, 8, fill=c)
        dr.polygon(P([(18.5, 31), (45.5, 31), (32, 47)]), fill=c)
    elif key == "vigilance":
        dr.polygon(P([(12, 32), (22, 24), (32, 21), (42, 24), (52, 32), (42, 40), (32, 43), (22, 40)]), fill=c)
        circle(dr, 32, 32, 6, fill=(25, 20, 28, 255)); circle(dr, 32, 32, 3, fill=c)
    elif key == "haste":
        dr.polygon(P([(36, 12), (20, 34), (30, 34), (26, 52), (44, 28), (33, 28)]), fill=c)
    elif key == "first_strike":
        dr.polygon(P([(44, 14), (48, 18), (28, 38), (24, 34)]), fill=c)
        dr.line(P([(21, 31), (31, 41)]), fill=c, width=w)
        dr.line(P([(22, 42), (16, 48)]), fill=c, width=w)
        dr.polygon(P([(14, 14), (18, 12), (34, 28), (30, 32), (14, 18)]), fill=c)
    elif key == "reach":
        dr.line(P([(32, 50), (32, 20)]), fill=c, width=w)
        dr.polygon(P([(32, 12), (42, 26), (22, 26)]), fill=c)
        dr.line(P([(32, 34), (42, 26)]), fill=c, width=2 * SS); dr.line(P([(32, 34), (22, 26)]), fill=c, width=2 * SS)
    elif key == "menace":
        for ox in (-8, 8):
            circle(dr, 32 + ox, 24, 5, fill=c)
            dr.polygon(P([(32 + ox - 8, 46), (32 + ox + 8, 46), (32 + ox + 5, 31), (32 + ox - 5, 31)]), fill=c)
    elif key == "hexproof":
        dr.polygon(P([(32, 12), (47, 18), (45, 38), (32, 51), (19, 38), (17, 18)]), fill=c)
        dr.line(P([(24, 26), (40, 40)]), fill=(25, 20, 28, 255), width=w)
        dr.line(P([(40, 26), (24, 40)]), fill=(25, 20, 28, 255), width=w)
    elif key == "indestructible":
        dr.polygon(P([(32, 12), (48, 18), (46, 38), (32, 51), (18, 38), (16, 18)]), fill=c)
        dr.polygon(P([(32, 18), (42, 22), (41, 36), (32, 44), (23, 36), (22, 22)]), fill=(25, 20, 28, 255))
        dr.polygon(P([(32, 24), (37, 26), (36, 34), (32, 38), (28, 34), (27, 26)]), fill=c)
    elif key == "defender":
        dr.rectangle([20 * SS, 20 * SS, 44 * SS, 46 * SS], fill=c)
        for x in (20, 29, 38):
            dr.rectangle([x * SS, 14 * SS, (x + 6) * SS, 20 * SS], fill=c)
    elif key == "flash":
        dr.polygon(P([(34, 10), (20, 36), (30, 36), (28, 54), (44, 26), (34, 26)]), fill=c)
    elif key == "double_strike":
        dr.polygon(P([(30, 14), (34, 18), (22, 38), (18, 34)]), fill=c)
        dr.polygon(P([(44, 14), (48, 18), (36, 38), (32, 34)]), fill=c)
        dr.line(P([(14, 34), (26, 44)]), fill=c, width=w); dr.line(P([(28, 34), (40, 44)]), fill=c, width=w)
    elif key == "shroud":
        circle(dr, 32, 32, 14, fill=c); circle(dr, 32, 32, 7, fill=(25, 20, 28, 255))
    else:
        circle(dr, 32, 32, 9, fill=c)


KEYWORDS = ["flying", "trample", "deathtouch", "lifelink", "vigilance", "haste", "first_strike", "reach", "menace",
            "hexproof", "indestructible", "defender", "flash", "double_strike", "shroud", "generic"]


def kw_icon(k):
    im, dr = canvas()
    hx = hexagon(32, 33, 29)
    dr.polygon(P([(x, y + 2) for x, y in hx]), fill=(0, 0, 0, 100))
    dr.polygon(P(hexagon(32, 32, 29)), fill=(212, 165, 70, 255))
    dr.polygon(P(hexagon(32, 32, 26)), fill=(36, 28, 24, 255))
    dr.polygon(P(hexagon(32, 32, 23)), fill=(58, 46, 38, 255))
    kw_glyph(dr, k, (240, 226, 190, 255))
    fin(im, f"kw_{k}.png")


def counter_icon():
    im, dr = canvas()
    circle(dr, 32, 33, 28, fill=(0, 0, 0, 100))
    circle(dr, 32, 32, 28, fill=(212, 165, 70, 255))
    circle(dr, 32, 32, 25, fill=(36, 28, 24, 255))
    fin(im, "counter_badge.png")


def tap_icon():
    im, dr = canvas()
    circle(dr, 32, 32, 28, fill=(30, 30, 34, 220))
    dr.arc([14 * SS, 14 * SS, 50 * SS, 50 * SS], 200, 480, fill=(230, 230, 235, 255), width=5 * SS)
    dr.polygon(P([(44, 14), (53, 26), (38, 26)]), fill=(230, 230, 235, 255))
    fin(im, "tap_badge.png")


if __name__ == "__main__":
    card_back()
    playmat_sanctum(); playmat_jungle(); playmat_storm()
    for k in MANA: mana_icon(k)
    for k in KEYWORDS: kw_icon(k)
    counter_icon(); tap_icon()
