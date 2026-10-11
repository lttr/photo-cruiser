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
