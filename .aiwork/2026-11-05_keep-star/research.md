---
references:
  - https://raw.githubusercontent.com/darktable-org/darktable/master/src/common/exif.cc
  - https://raw.githubusercontent.com/darktable-org/darktable/master/src/common/image.c
  - https://docs.darktable.org/usermanual/4.6/en/preferences-settings/storage/
  - https://discuss.pixls.us/t/darktable-3-8-1-will-not-read-star-ratings-created-in-adobe-bridge-2022/30595
  - https://gitlab.gnome.org/GNOME/gthumb/-/issues/227
---

# Research: rating compatibility

The common tag is XMP `xmp:Rating`: 0 = unrated, 1–5 stars, -1 = rejected. One star = `xmp:Rating 1`.

## darktable (from source)

- Reads `Xmp.xmp.Rating` from XMP embedded in the image and from its sidecar. Falls back to
  `Exif.Image.Rating`, then `Exif.Image.RatingPercent * 5 / 100`, unless "ignore EXIF rating" is set.
- Sidecar name keeps the extension: `IMG_1234.jpg.xmp`.
- Never writes to the original image, only to its sidecar and exports.
- Reads the rating at import ("add to library"; "copy & import" reportedly skips sidecars).
  Later changes reach it only through the sidecar with "look for updated XMP files on startup"
  turned on (off by default, slow on big libraries). Once darktable has written its own sidecar,
  ratings changed elsewhere are reportedly ignored.

## gThumb 3.12.6 (from the installed libraries; source not read)

- `libexiv2_tools.so` and `libedit_metadata.so` reference `Xmp::xmp::Rating`, `Xmp::Embedded`, `Xmp::Sidecar`.
- `org.gnome.gthumb.comments synchronize` is `true` here, which should mean gThumb writes the
  rating into the file. Which sidecar name it reads is unverified.
- Issue #227: gThumb doesn't show `Xmp.xmp.Rating` set by exiftool/exiv2, only one it set itself.
  Cause and fix status unknown. Test: `exiftool -XMP-xmp:Rating=1` on a temp copy, open in gThumb.

## Direction discussed

- One key toggles the star; writes immediately, undoable, then advances to the next photo.
- Embedded XMP for JPEG/WebP (APP1 `http://ns.adobe.com/xap/1.0/` segment, `XMP ` chunk),
  patching only the rating and keeping the rest of the packet. Metadata-only, no re-encode.
- `IMG_1234.jpg.xmp` sidecar for HEIC and read-only/gvfs folders.
- A per-folder rating index in `.photo-cruiser/`, checked against mtimes, so filtering doesn't read every file.
- Filter: all / starred only.
- Open: write into the original or sidecar only; whether a metadata-only write needs a backup;
  hand-written XMP patching vs an exiv2/XMP toolkit binding (crates not checked).
