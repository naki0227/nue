"""Regenerates the bundled sticker icons in data/icons (dev-only helper, needs Pillow).

The PNGs are committed, so the renderer never needs Python. Add your own
<name>.png (transparent background) to data/icons to extend the set; the
file name is the overlay "asset" in the edit plan.
"""
import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw

SIZE = 512
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent / "data" / "icons"
OUTLINE = (255, 255, 255, 255)
INK = (20, 20, 28, 255)


def canvas():
    return Image.new("RGBA", (SIZE * 2, SIZE * 2), (0, 0, 0, 0))


def save(img, name):
    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    img.save(OUT / f"{name}.png")


def star_points(cx, cy, r_out, r_in, n, rot=-math.pi / 2):
    pts = []
    for i in range(n * 2):
        r = r_out if i % 2 == 0 else r_in
        a = rot + i * math.pi / n
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def outlined_polygon(d, pts, fill, width=36):
    d.polygon(pts, fill=OUTLINE)
    # Shrink toward the centroid to fake an outline.
    cx = sum(p[0] for p in pts) / len(pts)
    cy = sum(p[1] for p in pts) / len(pts)
    inner = []
    for x, y in pts:
        dx, dy = x - cx, y - cy
        dist = math.hypot(dx, dy) or 1
        inner.append((x - dx / dist * width, y - dy / dist * width))
    d.polygon(inner, fill=fill)


def bar(d, x0, y0, x1, y1, fill):
    d.rounded_rectangle((x0, y0, x1, y1), radius=(x1 - x0) // 2, fill=fill)


def exclaim():
    img = canvas(); d = ImageDraw.Draw(img)
    outlined_polygon(d, star_points(512, 512, 500, 330, 12), (255, 59, 59, 255))
    bar(d, 452, 250, 572, 640, OUTLINE)
    d.ellipse((452, 680, 572, 800), fill=OUTLINE)
    save(img, "exclaim")


def question():
    img = canvas(); d = ImageDraw.Draw(img)
    d.ellipse((40, 40, 984, 984), fill=OUTLINE)
    d.ellipse((76, 76, 948, 948), fill=(61, 140, 255, 255))
    d.arc((322, 200, 702, 560), start=180, end=90, fill=OUTLINE, width=110)
    bar(d, 457, 500, 567, 660, OUTLINE)
    d.ellipse((452, 700, 572, 820), fill=OUTLINE)
    save(img, "question")


def heart():
    img = canvas(); d = ImageDraw.Draw(img)
    def heart_pts(scale):
        pts = []
        for i in range(200):
            t = i / 200 * 2 * math.pi
            x = 16 * math.sin(t) ** 3
            y = 13 * math.cos(t) - 5 * math.cos(2 * t) - 2 * math.cos(3 * t) - math.cos(4 * t)
            pts.append((512 + x * scale, 470 - y * scale))
        return pts
    d.polygon(heart_pts(29), fill=OUTLINE)
    d.polygon(heart_pts(26), fill=(255, 79, 154, 255))
    d.ellipse((300, 280, 400, 380), fill=(255, 190, 220, 255))
    save(img, "heart")


def star():
    img = canvas(); d = ImageDraw.Draw(img)
    outlined_polygon(d, star_points(512, 540, 480, 200, 5), (255, 205, 40, 255), width=44)
    save(img, "star")


def sparkle():
    img = canvas(); d = ImageDraw.Draw(img)
    for cx, cy, r, fill in [(560, 470, 420, (255, 236, 120, 255)), (250, 760, 180, (255, 255, 255, 255))]:
        outlined_polygon(d, star_points(cx, cy, r, r * 0.22, 4), fill, width=26)
    save(img, "sparkle")


def sweat():
    img = canvas(); d = ImageDraw.Draw(img)
    def drop(scale):
        pts = []
        for i in range(200):
            t = i / 200 * 2 * math.pi
            x = math.sin(t) * (1 - math.cos(t)) * 0.5
            y = -math.cos(t)
            pts.append((512 + x * scale * 1.3, 500 + y * scale))
        return pts
    d.polygon(drop(430), fill=OUTLINE)
    d.polygon(drop(380), fill=(90, 200, 255, 255))
    d.ellipse((580, 560, 670, 700), fill=(200, 240, 255, 255))
    save(img, "sweat")


def check():
    img = canvas(); d = ImageDraw.Draw(img)
    d.ellipse((40, 40, 984, 984), fill=OUTLINE)
    d.ellipse((76, 76, 948, 948), fill=(46, 204, 113, 255))
    d.line([(280, 530), (450, 700), (760, 360)], fill=OUTLINE, width=120, joint="curve")
    save(img, "check")


def cross():
    img = canvas(); d = ImageDraw.Draw(img)
    d.ellipse((40, 40, 984, 984), fill=OUTLINE)
    d.ellipse((76, 76, 948, 948), fill=(235, 64, 52, 255))
    d.line([(320, 320), (704, 704)], fill=OUTLINE, width=120)
    d.line([(704, 320), (320, 704)], fill=OUTLINE, width=120)
    save(img, "cross")


def arrow():
    img = canvas(); d = ImageDraw.Draw(img)
    pts = [(120, 400), (560, 400), (560, 200), (920, 512), (560, 824), (560, 624), (120, 624)]
    d.polygon(pts, fill=OUTLINE)
    inner = [(160, 440), (600, 440), (600, 290), (860, 512), (600, 734), (600, 584), (160, 584)]
    d.polygon(inner, fill=(255, 140, 0, 255))
    save(img, "arrow")


def fire():
    img = canvas(); d = ImageDraw.Draw(img)
    def flame(scale, cx, cy, sway):
        pts = []
        for i in range(240):
            t = i / 240 * 2 * math.pi
            x = math.sin(t) * (1 - math.cos(t)) * 0.5
            y = -math.cos(t)
            # Bend the tip sideways like a flickering flame.
            bend = sway * (1 - y) ** 2
            pts.append((cx + (x * 1.25 + bend) * scale, cy + y * scale))
        return pts
    d.polygon(flame(440, 512, 540, 0.12), fill=OUTLINE)
    d.polygon(flame(400, 512, 550, 0.12), fill=(255, 90, 30, 255))
    d.polygon(flame(220, 512, 720, -0.1), fill=(255, 210, 60, 255))
    save(img, "fire")


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for make in (exclaim, question, heart, star, sparkle, sweat, check, cross, arrow, fire):
        make()
    print(f"Generated icons in {OUT}")
