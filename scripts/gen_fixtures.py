# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11.3", "pillow-heif>=0.21"]
# ///
"""Generate the small synthetic fixtures in fixtures/ (committed). Run: `just fixtures`.

Natural order of the photo entries (what `pcr list fixtures` must print):
  img1.png, img2.jpg, img3-exif6.jpg, img4.AVIF, img5.HEIC, img9.webp, img10.gif, img11.bmp, img12-broken.jpg
Ignored: .hidden.jpg, notes.txt, subdir/
"""

import sys
from pathlib import Path

import pillow_heif
from PIL import Image, ImageDraw

pillow_heif.register_heif_opener()

out = Path(sys.argv[1] if len(sys.argv) > 1 else "fixtures")
out.mkdir(parents=True, exist_ok=True)
W, H = 320, 240


def gradient(w=W, h=H):
    img = Image.new("RGB", (w, h))
    img.putdata([(x * 255 // (w - 1), y * 255 // (h - 1), 128) for y in range(h) for x in range(w)])
    return img


def checker(w=W, h=H, s=20):
    img = Image.new("RGB", (w, h), "white")
    d = ImageDraw.Draw(img)
    for y in range(0, h, s):
        for x in range(0, w, s):
            if (x // s + y // s) % 2:
                d.rectangle([x, y, x + s - 1, y + s - 1], fill=(30, 60, 200))
    return img


def marked(img, label):
    """Red square in the top-left corner so orientation is visible."""
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, 39, 39], fill=(230, 20, 20))
    d.text((50, 10), label, fill=(255, 255, 255))
    return img


marked(gradient(), "png").save(out / "img1.png")
marked(checker(), "jpg").save(out / "img2.jpg", quality=90)

# Stored landscape 320x240 with EXIF orientation 6 (rotate 90° CW) → displayed 240x320.
exif = Image.Exif()
exif[0x0112] = 6
marked(gradient(), "exif6").save(out / "img3-exif6.jpg", quality=90, exif=exif)

marked(gradient(), "avif").save(out / "img4.AVIF", quality=80)
marked(checker(), "heic").save(out / "img5.HEIC", quality=80)
marked(gradient(160, 120), "webp").save(out / "img9.webp", quality=90)
marked(checker(160, 120), "gif").save(out / "img10.gif")
marked(gradient(64, 48), "bmp").save(out / "img11.bmp")

(out / "img12-broken.jpg").write_bytes(b"\xff\xd8\xff\xe0 this is not really a jpeg")
marked(gradient(32, 32), "hidden").save(out / ".hidden.jpg")
(out / "notes.txt").write_text("not a photo\n")
(out / "subdir").mkdir(exist_ok=True)
marked(gradient(32, 32), "sub").save(out / "subdir" / "nested.jpg")
print(f"fixtures written to {out}")
