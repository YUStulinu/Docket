"""Turns a clean page image into a phone photo of it: on a table, at an angle, unevenly lit, a bit
blurred and noisy, JPEG-compressed. The corners of the page in the photo are returned, so the
document detection can be scored against them."""

from __future__ import annotations

import io
import math

import numpy as np
from PIL import Image, ImageDraw, ImageFilter


def _coeffs(dst: list[tuple[float, float]], src: list[tuple[float, float]]) -> list[float]:
    """PIL's perspective coefficients: for each output pixel (x, y) in `dst`, where to sample in `src`."""
    rows = []
    rhs = []
    for (x, y), (u, v) in zip(dst, src):
        rows.append([x, y, 1, 0, 0, 0, -u * x, -u * y])
        rows.append([0, 0, 0, x, y, 1, -v * x, -v * y])
        rhs.extend([u, v])
    return list(np.linalg.solve(np.array(rows, dtype=float), np.array(rhs, dtype=float)))


def _background(rng: np.random.Generator, w: int, h: int) -> Image.Image:
    """A table: wood, a dark desk, or cloth."""
    kind = rng.integers(3)
    base = [np.array([120, 85, 55]), np.array([45, 48, 52]), np.array([150, 150, 140])][kind]
    base = base + rng.integers(-20, 20, 3)
    small = rng.normal(0, 1, (h // 8 + 1, w // 8 + 1))
    noise = np.kron(small, np.ones((8, 8)))[:h, :w]
    if kind == 0:  # wood: grain across the width
        noise = noise * 0.5 + np.sin(np.linspace(0, rng.uniform(20, 60), w))[None, :] * 0.8
    img = np.clip(base[None, None, :] + noise[..., None] * 12, 0, 255).astype(np.uint8)
    return Image.fromarray(img, "RGB").filter(ImageFilter.GaussianBlur(2))


def photograph(page: Image.Image, rng: np.random.Generator, flat: bool = False, canvas: tuple[int, int] | None = None):
    """Returns (photo JPEG bytes, corners [(x, y)] clockwise from the top left, photo size)."""
    pw, ph = page.size
    landscape = pw > ph
    if canvas is None:
        canvas = (2400, 1800) if landscape else (1800, 2400)
    cw, ch = canvas
    if flat:
        # A flatbed scan: the page fills the frame, square, evenly lit.
        scale = min(cw / pw, ch / ph) * 0.97
        w, h = int(pw * scale), int(ph * scale)
        x0, y0 = (cw - w) // 2, (ch - h) // 2
        corners = [(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)]
        bg = Image.new("RGB", canvas, (245, 245, 243))
    else:
        fill = rng.uniform(0.62, 0.86)
        scale = min(cw / pw, ch / ph) * fill
        w, h = pw * scale, ph * scale
        cx, cy = cw / 2 + rng.uniform(-0.06, 0.06) * cw, ch / 2 + rng.uniform(-0.05, 0.05) * ch
        angle = math.radians(rng.uniform(-9, 9))
        pts = []
        for dx, dy in [(-w / 2, -h / 2), (w / 2, -h / 2), (w / 2, h / 2), (-w / 2, h / 2)]:
            x = cx + dx * math.cos(angle) - dy * math.sin(angle)
            y = cy + dx * math.sin(angle) + dy * math.cos(angle)
            pts.append((x, y))
        # Perspective: the camera is not square to the page.
        jitter = 0.07 * min(w, h)
        corners = [(x + rng.uniform(-jitter, jitter), y + rng.uniform(-jitter, jitter)) for x, y in pts]
        bg = _background(rng, cw, ch)
    coeffs = _coeffs(corners, [(0, 0), (pw, 0), (pw, ph), (0, ph)])
    warped = page.convert("RGB").transform(canvas, Image.PERSPECTIVE, coeffs, Image.BICUBIC, fillcolor=(0, 0, 0))
    mask = Image.new("L", canvas, 0)
    ImageDraw.Draw(mask).polygon([tuple(c) for c in corners], fill=255)
    if not flat:
        # The page's own shadow on the table.
        shadow = Image.new("L", canvas, 0)
        off = rng.uniform(8, 25)
        ImageDraw.Draw(shadow).polygon([(x + off, y + off) for x, y in corners], fill=110)
        shadow = shadow.filter(ImageFilter.GaussianBlur(18))
        bg = Image.composite(Image.new("RGB", canvas, (0, 0, 0)), bg, shadow)
    img = Image.composite(warped, bg, mask)
    arr = np.asarray(img).astype(np.float32)
    if not flat:
        # Uneven light: a gradient from one side, and the shadow of the phone or a hand.
        yy, xx = np.mgrid[0:ch, 0:cw].astype(np.float32)
        a = rng.uniform(0, 2 * math.pi)
        grad = (xx * math.cos(a) + yy * math.sin(a)) / max(cw, ch)
        light = 1.0 - rng.uniform(0.12, 0.35) * (grad - grad.min()) / (np.ptp(grad) + 1e-6)
        arr *= light[..., None]
        if rng.random() < 0.6:
            sh = Image.new("L", canvas, 0)
            sx, sy = rng.uniform(0, cw), rng.uniform(0, ch)
            r = rng.uniform(0.25, 0.5) * cw
            ImageDraw.Draw(sh).ellipse([sx - r, sy - r * 0.7, sx + r, sy + r * 0.7], fill=255)
            sh = np.asarray(sh.filter(ImageFilter.GaussianBlur(r * 0.35))).astype(np.float32) / 255
            arr *= (1.0 - rng.uniform(0.15, 0.3) * sh)[..., None]
        # A warm or cold cast.
        arr *= np.array([1.0 + rng.uniform(-0.05, 0.05), 1.0, 1.0 + rng.uniform(-0.06, 0.04)])[None, None, :]
        arr += rng.normal(0, rng.uniform(2, 5), arr.shape)
    img = Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "RGB")
    if not flat:
        img = img.filter(ImageFilter.GaussianBlur(rng.uniform(0.4, 1.1)))
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=int(rng.integers(72, 88)) if not flat else 90)
    return buf.getvalue(), [(round(float(x), 1), round(float(y), 1)) for x, y in corners], canvas
