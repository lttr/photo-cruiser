# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11.3", "pillow-heif>=0.21"]
# ///
"""Generate the small synthetic fixtures in fixtures/ (committed). Run: `just fixtures`.

Natural order of the photo entries (what `pcr list fixtures` must print):
  img1.png, img2.jpg, img3-exif6.jpg, img4.AVIF, img5.HEIC, img9.webp, img10.gif, img11.bmp, img12-broken.jpg
Ignored: .hidden.jpg, notes.txt, subdir/, previews/, save/

save/: inputs for the Save tests (they copy them to a temp folder first):
  q98.jpg (quality 98, 4:2:0, 300 dpi, sRGB ICC profile), q75.jpg (quality 75),
  anim.gif (2 frames), lossless.webp, gray16.png (16-bit)

previews/: photos with embedded thumbnails (80×60, a downscaled copy):
  thumb.HEIC (HEIF thumbnail item), thumb-exif6.jpg (EXIF IFD1 thumbnail, orientation 6),
  letterbox.jpg (16:9 photo, 4:3 thumbnail: aspect mismatch → no preview),
  rounded.jpg (4:3 photo, 80×64 thumbnail like a phone's: no preview, but a grid thumbnail)

adjust/: inputs for the adjustment tools:
  document.jpg (photo of a page: yellowish paper with a shadow in a corner and grey-brown
  text, on a dark desk at the left; the page is x 60..320),
  warm-cast.jpg (a neutral scene, grey ramps and colour patches, under warm tungsten-like light:
  linear R × 1, G × 0.8, B × 0.55),
  window.jpg (a dark room with a bright window on the right: shelves and a table in the
  shadows, sky and a tree outside; the window is x 200..300, y 30..170),
  woods-haze.jpg (wooded hills in haze, nearer ones clearer: sky y < 50, far ridge to
  y 110, middle ridge to y 170, a clear foreground below; for Dehaze)
"""

import io
import struct
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


def exif_with_thumbnail(thumb: Image.Image, orientation: int) -> bytes:
    """APP1 payload: IFD0 (orientation) + IFD1 pointing at a JPEG thumbnail (little endian)."""
    buf = io.BytesIO()
    thumb.save(buf, "JPEG", quality=85)
    jpeg = buf.getvalue()
    ifd0 = struct.pack("<H", 1) + struct.pack("<HHIHH", 0x0112, 3, 1, orientation, 0) + struct.pack("<I", 26)
    data_at = 26 + 2 + 2 * 12 + 4
    ifd1 = struct.pack("<H", 2) + struct.pack("<HHII", 0x0201, 4, 1, data_at)
    ifd1 += struct.pack("<HHII", 0x0202, 4, 1, len(jpeg)) + struct.pack("<I", 0)
    return b"Exif\x00\x00" + b"II*\x00" + struct.pack("<I", 8) + ifd0 + ifd1 + jpeg


previews = out / "previews"
previews.mkdir(exist_ok=True)
heic = marked(checker(), "heic")
heic.save(previews / "thumb.HEIC", quality=80, thumbnails=[80])
jpg = marked(gradient(), "exif6")
jpg.save(previews / "thumb-exif6.jpg", quality=90, exif=exif_with_thumbnail(jpg.resize((80, 60)), 6))
wide = marked(gradient(320, 180), "16:9")
wide.save(previews / "letterbox.jpg", quality=90, exif=exif_with_thumbnail(gradient(80, 60), 1))
rounded = marked(gradient(), "4:3")
rounded.save(previews / "rounded.jpg", quality=90, exif=exif_with_thumbnail(rounded.resize((80, 64)), 1))



def document(w=W, h=H, desk=60):
    """A weak-contrast photo of a page: yellowish paper, a little darker to the right and
    in a shadow at the bottom right corner, lines of grey-brown "words", and a dark desk
    left of x = desk."""

    def light(x, y):
        corner = ((w - x) ** 2 + (h - y) ** 2) ** 0.5 / 110
        return 1.0 - 0.03 * (x - desk) / (w - desk) - 0.25 * max(0.0, 1.0 - corner)

    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            px[x, y] = tuple(round(c * light(x, y)) for c in (205, 192, 150))
    d = ImageDraw.Draw(img)
    for row, y in enumerate(range(24, h - 20, 18)):
        x = desk + 20
        for i in range(12):
            word = 12 + (row * 7 + i * 13) % 26
            if x + word > w - 20:
                break
            d.rectangle([x, y, x + word, y + 7], fill=tuple(round(c * light(x, y)) for c in (85, 76, 62)))
            x += word + 7
    d.rectangle([0, 0, desk - 1, h - 1], fill=(38, 34, 32))
    return img


def cast(w=W, h=H, gains=(1.0, 0.8, 0.55)):
    """A neutral scene (a grey wall getting lighter to the right, a darker grey floor, and
    patches of red, green, blue and skin) under light with linear `gains` per channel."""

    def lin(v):
        v /= 255
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    def enc(v):
        v = min(max(v, 0.0), 1.0)
        return round(255 * (v * 12.92 if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055))

    patches = [(180, 50, 40), (60, 140, 60), (50, 70, 160), (220, 170, 140)]
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            if y >= h * 2 // 3:
                c = (90 + x // 8,) * 3
            else:
                c = (110 + x * 100 // w,) * 3
                i, r = divmod(x, w // 4)
                if 30 <= y < 100 and 15 <= r < w // 4 - 15:
                    c = patches[i]
            px[x, y] = tuple(enc(lin(v) * g) for v, g in zip(c, gains))
    return img


def window(w=W, h=H):
    """A backlit room: dark walls and furniture with some texture, and a bright window
    with sky and a tree, for Shadows & highlights."""
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            grain = (x * 7 + y * 13) % 5
            if y >= 180:
                c = (34 + grain + (x // 6) % 2 * 4, 26 + grain, 20)  # floorboards
            else:
                c = (30 + grain, 28 + grain, 34 + grain)  # wall
            if 200 <= x < 300 and 30 <= y < 170:
                c = (205 + y // 8, 225 + y // 12, 248)  # sky, lighter at the bottom
            px[x, y] = c
    d = ImageDraw.Draw(img)
    d.ellipse([235, 60, 290, 120], fill=(150, 185, 140))  # tree crown
    d.rectangle([258, 115, 266, 169], fill=(120, 105, 90))  # trunk
    d.rectangle([248, 30, 252, 169], fill=(60, 52, 46))  # window bar
    d.rectangle([20, 40, 120, 150], fill=(48, 36, 28))  # shelf
    for i, y in enumerate(range(52, 140, 22)):
        d.rectangle([24, y, 116, y + 3], fill=(62, 48, 36))
        for j in range(7):
            x = 30 + j * 12
            d.rectangle([x, y - 14, x + 8, y - 1], fill=((90, 30, 30), (30, 60, 90), (70, 70, 30))[(i + j) % 3])
    d.rectangle([60, 170, 190, 182], fill=(70, 50, 34))  # table top
    return img


def woods_haze(w=W, h=H):
    """Wooded hills in haze: each ridge is the same textured green, seen through more haze
    the farther it is (linear light: scene × t + airlight × (1 − t))."""
    import math

    img = Image.new("RGB", (w, h))
    px = img.load()
    air = (0.55, 0.58, 0.63)  # an overcast, hazy morning

    def lin(v):
        v /= 255
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    def enc(v):
        v = min(max(v, 0.0), 1.0)
        v = v * 12.92 if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
        return round(v * 255)

    for y in range(h):
        for x in range(w):
            far = 60 + 12 * math.sin(x / 37)
            mid = 120 + 15 * math.sin(x / 23 + 1)
            near = 185 + 8 * math.sin(x / 17 + 2)
            t = 0.0 if y < far else 0.3 if y < mid else 0.55 if y < near else 0.9
            tex = ((x * 7 + y * 13) % 9) / 8  # tree tops
            scene = (40 + 30 * tex, 70 + 40 * tex, 35 + 20 * tex)
            if y >= near:
                scene = (90 + 40 * tex, 75 + 30 * tex, 50 + 20 * tex)  # a dry meadow
            px[x, y] = tuple(enc(lin(s) * t + a * (1 - t)) for s, a in zip(scene, air))
    return img


(out / "adjust").mkdir(exist_ok=True)
document().save(out / "adjust" / "document.jpg", quality=95, subsampling=0)
cast().save(out / "adjust" / "warm-cast.jpg", quality=95, subsampling=0)
window().save(out / "adjust" / "window.jpg", quality=95, subsampling=0)
woods_haze().save(out / "adjust" / "woods-haze.jpg", quality=95, subsampling=0)

from PIL import ImageCms

save = out / "save"
save.mkdir(exist_ok=True)
icc = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
marked(gradient(), "q98").save(save / "q98.jpg", quality=98, subsampling=2, dpi=(300, 300), icc_profile=icc)
marked(gradient(), "q75").save(save / "q75.jpg", quality=75, subsampling=2)
frames = [marked(checker(64, 48, 8), "1"), marked(gradient(64, 48), "2")]
frames[0].save(save / "anim.gif", save_all=True, append_images=frames[1:], duration=100, loop=0)
marked(gradient(160, 120), "lossless").save(save / "lossless.webp", lossless=True)
Image.new("I;16", (32, 24), 40000).save(save / "gray16.png")
print(f"fixtures written to {out}")
