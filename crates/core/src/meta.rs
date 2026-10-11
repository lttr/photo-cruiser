//! Small parsers for what Save needs to keep a photo's quality and metadata: JPEG
//! markers (quantisation tables, sampling, density), EXIF (a TIFF structure) and WebP
//! chunks. They read and patch bytes; decoding pixels is [`crate::decode`]'s job.

/// What a JPEG was encoded with, to encode it again without adding visible loss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JpegSettings {
    /// Luma and chroma quantisation tables, natural (row-major) order. `None` if they
    /// couldn't be read.
    pub tables: Option<[[u16; 64]; 2]>,
    /// Luma sampling factors (h, v) relative to chroma: (2, 2) is 4:2:0, (1, 1) is 4:4:4.
    pub sampling: (u8, u8),
    /// JFIF density: unit (0 = aspect ratio only, 1 = dpi, 2 = dots per cm), x, y.
    pub density: Option<(u8, u16, u16)>,
}

impl Default for JpegSettings {
    fn default() -> Self {
        JpegSettings { tables: None, sampling: (1, 1), density: None }
    }
}

impl JpegSettings {
    /// Chroma has fewer samples than luma (4:2:0, 4:2:2, ...).
    pub fn subsampled(&self) -> bool {
        self.sampling != (1, 1)
    }
}

/// Zigzag position → natural index (`jpeg_natural_order` of libjpeg).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7,
    14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39,
    46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// The example tables of ITU-T T.81 Annex K, natural order (what IJG libjpeg scales).
const ANNEX_K_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17,
    22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78,
    87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const ANNEX_K_CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// The IJG (libjpeg) luma and chroma tables at `quality` 1–100, natural order.
pub fn ijg_tables(quality: u8) -> [[u16; 64]; 2] {
    let q = quality.clamp(1, 100) as u32;
    let scale = if q < 50 { 5000 / q } else { 200 - q * 2 };
    let scaled = |t: &[u16; 64]| t.map(|v| ((v as u32 * scale + 50) / 100).clamp(1, 255) as u16);
    [scaled(&ANNEX_K_LUMA), scaled(&ANNEX_K_CHROMA)]
}

/// Read quantisation tables, sampling factors and density from a JPEG's markers (up to
/// the first scan). `None` if it isn't a JPEG. 16-bit table entries are clamped to 255.
pub fn jpeg_settings(b: &[u8]) -> Option<JpegSettings> {
    if b.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut dqt: [Option<[u16; 64]>; 4] = [None; 4];
    let mut comps: Vec<(u8, u8)> = Vec::new(); // (sampling HV, table)
    let mut density = None;
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1; // fill byte
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let seg = b.get(i + 4..i + 2 + len)?;
        match marker {
            0xDB => {
                let mut j = 0;
                while j < seg.len() {
                    let (wide, id) = (seg[j] >> 4 != 0, (seg[j] & 0x0F) as usize);
                    j += 1;
                    let mut t = [0u16; 64];
                    for (k, natural) in ZIGZAG.iter().enumerate() {
                        t[*natural] = match wide {
                            true => {
                                u16::from_be_bytes([*seg.get(j + 2 * k)?, *seg.get(j + 2 * k + 1)?]).min(255)
                            }
                            false => *seg.get(j + k)? as u16,
                        };
                    }
                    j += if wide { 128 } else { 64 };
                    *dqt.get_mut(id)? = Some(t);
                }
            }
            0xC0..=0xC2 => {
                let n = *seg.get(5)? as usize;
                comps = (0..n)
                    .map(|c| Some((*seg.get(7 + 3 * c)?, *seg.get(8 + 3 * c)?)))
                    .collect::<Option<_>>()?;
            }
            0xE0 if seg.starts_with(b"JFIF\0") && seg.len() >= 12 => {
                let at = |o: usize| u16::from_be_bytes([seg[o], seg[o + 1]]);
                density = Some((seg[7], at(8), at(10)));
            }
            0xDA => break,
            _ => {}
        }
        i += 2 + len;
    }
    let luma = *comps.first()?;
    let chroma = comps.get(1).copied();
    let table = |c: (u8, u8)| dqt.get(c.1 as usize).copied().flatten();
    let tables = match chroma {
        Some(ch) => table(luma).zip(table(ch)).map(|(l, c)| [l, c]),
        None => table(luma).map(|l| [l, l]),
    };
    let sampling = match chroma {
        Some((hv, _)) => {
            let (lh, lv, ch, cv) = (luma.0 >> 4, luma.0 & 0x0F, (hv >> 4).max(1), (hv & 0x0F).max(1));
            if lh % ch == 0 && lv % cv == 0 { (lh / ch, lv / cv) } else { (1, 1) }
        }
        None => (1, 1),
    };
    Some(JpegSettings { tables, sampling, density })
}

/// An EXIF block (a TIFF structure, without the `Exif\0\0` header) to read and patch.
struct Tiff {
    b: Vec<u8>,
    le: bool,
}

impl Tiff {
    fn new(b: &[u8]) -> Option<Tiff> {
        let le = match b.get(..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        Some(Tiff { b: b.to_vec(), le })
    }

    fn u16(&self, o: usize) -> Option<u16> {
        let b: [u8; 2] = self.b.get(o..o + 2)?.try_into().ok()?;
        Some(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn u32(&self, o: usize) -> Option<u32> {
        let b: [u8; 4] = self.b.get(o..o + 4)?.try_into().ok()?;
        Some(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }

    fn set_u16(&mut self, o: usize, v: u16) -> Option<()> {
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.b.get_mut(o..o + 2)?.copy_from_slice(&b);
        Some(())
    }

    fn set_u32(&mut self, o: usize, v: u32) -> Option<()> {
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.b.get_mut(o..o + 4)?.copy_from_slice(&b);
        Some(())
    }

    fn ifd0(&self) -> Option<usize> {
        Some(self.u32(4)? as usize)
    }

    /// Offsets of the 12-byte entries of the IFD at `ifd`, with their tags.
    fn entries(&self, ifd: usize) -> Option<Vec<(u16, usize)>> {
        let n = self.u16(ifd)? as usize;
        (0..n).map(|i| ifd + 2 + 12 * i).map(|e| Some((self.u16(e)?, e))).collect()
    }

    /// Where the IFD at `ifd` stores the offset of the next one.
    fn next_pos(&self, ifd: usize) -> Option<usize> {
        Some(ifd + 2 + 12 * self.u16(ifd)? as usize)
    }

    /// A SHORT or LONG value (from the entry's value field).
    fn value(&self, entry: usize) -> Option<u32> {
        match self.u16(entry + 2)? {
            3 => self.u16(entry + 8).map(u32::from),
            _ => self.u32(entry + 8),
        }
    }

    /// Store one SHORT (if it fits and the entry is one) or LONG in an entry.
    fn set_value(&mut self, entry: usize, v: u32) -> Option<()> {
        self.set_u32(entry + 4, 1)?;
        self.set_u32(entry + 8, 0)?;
        match (self.u16(entry + 2)?, u16::try_from(v)) {
            (3, Ok(short)) => self.set_u16(entry + 8, short),
            _ => {
                self.set_u16(entry + 2, 4)?;
                self.set_u32(entry + 8, v)
            }
        }
    }
}

/// The JPEG thumbnail referenced by IFD1 of an EXIF block.
pub fn exif_thumbnail(exif: &[u8]) -> Option<&[u8]> {
    let t = Tiff::new(exif)?;
    let ifd1 = t.u32(t.next_pos(t.ifd0()?)?)? as usize;
    if ifd1 == 0 {
        return None;
    }
    let (mut offset, mut length) = (None, None);
    for (tag, e) in t.entries(ifd1)? {
        match tag {
            0x0201 => offset = Some(t.value(e)? as usize),
            0x0202 => length = Some(t.value(e)? as usize),
            _ => {}
        }
    }
    let (offset, length) = (offset?, length?);
    exif.get(offset..offset.checked_add(length)?)
}

/// EXIF for a saved photo of `width`×`height` whose pixels are already turned upright:
/// Orientation 1, no IFD1 thumbnail (it would show the photo as it was), and
/// PixelXDimension / PixelYDimension updated. Resolution tags (the print size) are kept.
/// `None` if the block can't be parsed: writing it unpatched could turn the photo twice.
pub fn patch_exif(exif: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut t = Tiff::new(exif)?;
    let ifd0 = t.ifd0()?;
    let mut sub = None;
    for (tag, e) in t.entries(ifd0)? {
        match tag {
            0x0112 => t.set_value(e, 1)?,
            0x8769 => sub = Some(t.value(e)? as usize),
            _ => {}
        }
    }
    let next = t.next_pos(ifd0)?;
    t.set_u32(next, 0)?;
    if let Some(sub) = sub {
        for (tag, e) in t.entries(sub)? {
            match tag {
                0xA002 => t.set_value(e, width)?,
                0xA003 => t.set_value(e, height)?,
                _ => {}
            }
        }
    }
    Some(t.b)
}

/// The EXIF Orientation of a block (1 if absent).
pub fn exif_orientation(exif: &[u8]) -> Option<u16> {
    let t = Tiff::new(exif)?;
    let e = t.entries(t.ifd0()?)?.into_iter().find(|(tag, _)| *tag == 0x0112);
    Some(e.and_then(|(_, e)| t.value(e)).unwrap_or(1) as u16)
}

/// The chunks of a WebP file (RIFF): FourCC and payload.
fn webp_chunks(b: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    if b.get(..4)? != b"RIFF" || b.get(8..12)? != b"WEBP" {
        return None;
    }
    let mut chunks = Vec::new();
    let mut i = 12;
    while i + 8 <= b.len() {
        let id: [u8; 4] = b[i..i + 4].try_into().ok()?;
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        chunks.push((id, b.get(i + 8..i + 8 + len)?));
        i += 8 + len + len % 2;
    }
    Some(chunks)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebpInfo {
    pub lossless: bool,
    pub animated: bool,
}

/// Lossless (`VP8L`) or lossy (`VP8 `), and animated (the `VP8X` flag). `None` if it
/// isn't a WebP.
pub fn webp_info(b: &[u8]) -> Option<WebpInfo> {
    let chunks = webp_chunks(b)?;
    let animated = chunks.iter().any(|(id, d)| id == b"VP8X" && d.first().is_some_and(|f| f & 0x02 != 0));
    let lossless = chunks.iter().any(|(id, _)| id == b"VP8L");
    Some(WebpInfo { lossless, animated })
}

/// Add an ICC profile and EXIF to an encoded still WebP (libwebp's simple encoder writes
/// neither): an extended (`VP8X`) header, `ICCP` before the image, `EXIF` after it.
pub fn webp_with_metadata(
    webp: &[u8],
    icc: Option<&[u8]>,
    exif: Option<&[u8]>,
    alpha: bool,
    (width, height): (u32, u32),
) -> Option<Vec<u8>> {
    if icc.is_none() && exif.is_none() {
        return Some(webp.to_vec());
    }
    let chunks = webp_chunks(webp)?;
    let alpha = alpha || chunks.iter().any(|(id, _)| id == b"ALPH");
    let mut flags = 0u8;
    for (on, bit) in [(icc.is_some(), 0x20), (alpha, 0x10), (exif.is_some(), 0x08)] {
        if on {
            flags |= bit;
        }
    }
    let mut vp8x = vec![flags, 0, 0, 0];
    vp8x.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    let image = chunks.iter().filter(|(id, _)| !matches!(id, b"VP8X" | b"ICCP" | b"EXIF")).copied();
    let all: Vec<([u8; 4], &[u8])> = std::iter::once((*b"VP8X", vp8x.as_slice()))
        .chain(icc.map(|d| (*b"ICCP", d)))
        .chain(image)
        .chain(exif.map(|d| (*b"EXIF", d)))
        .collect();
    let mut body = b"WEBP".to_vec();
    for (id, data) in all {
        body.extend_from_slice(&id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Some(out)
}

/// The ICC profile and EXIF of a WebP.
pub fn webp_metadata(b: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let chunks = webp_chunks(b).unwrap_or_default();
    let find = |want: &[u8; 4]| chunks.iter().find(|(id, _)| id == want).map(|(_, d)| d.to_vec());
    (find(b"ICCP"), find(b"EXIF"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Little-endian EXIF: IFD0 with Orientation 6 and an Exif IFD pointer; Exif IFD with
    /// PixelX/YDimension (SHORT); IFD1 with a thumbnail.
    fn exif() -> Vec<u8> {
        let mut b = b"II*\0".to_vec();
        b.extend(8u32.to_le_bytes());
        let entry = |tag: u16, typ: u16, v: u32| {
            let mut e = tag.to_le_bytes().to_vec();
            e.extend(typ.to_le_bytes());
            e.extend(1u32.to_le_bytes());
            e.extend(v.to_le_bytes());
            e
        };
        // IFD0 at 8: 2 entries (8 + 2 + 24 + 4 = 38).
        b.extend(2u16.to_le_bytes());
        b.extend(entry(0x0112, 3, 6));
        b.extend(entry(0x8769, 4, 38));
        b.extend(64u32.to_le_bytes()); // IFD1 at 64
        // Exif IFD at 38: 2 entries (38 + 2 + 24 = 64), no next.
        b.extend(2u16.to_le_bytes());
        b.extend(entry(0xA002, 3, 320));
        b.extend(entry(0xA003, 3, 240));
        // (next pointer of the Exif IFD overlaps IFD1's count: unused)
        // IFD1 at 64: offset/length of 3 bytes at 94.
        b.extend(2u16.to_le_bytes());
        b.extend(entry(0x0201, 4, 94));
        b.extend(entry(0x0202, 4, 3));
        b.extend(0u32.to_le_bytes());
        b.extend(b"JPG");
        b
    }

    #[test]
    fn patch_exif_orientation_ifd1_dimensions() {
        let e = exif();
        assert_eq!(exif_thumbnail(&e), Some(&b"JPG"[..]));
        assert_eq!(exif_orientation(&e), Some(6));
        let p = patch_exif(&e, 70000, 50).unwrap();
        assert_eq!(exif_orientation(&p), Some(1));
        assert_eq!(exif_thumbnail(&p), None);
        let t = Tiff::new(&p).unwrap();
        let dims: Vec<_> = t.entries(38).unwrap().iter().map(|(_, e)| t.value(*e).unwrap()).collect();
        assert_eq!(dims, [70000, 50], "a width over 65535 becomes a LONG");
        assert_eq!(patch_exif(b"nonsense", 1, 1), None);
    }

    #[test]
    fn ijg_quality_tables() {
        let [l, c] = ijg_tables(50);
        assert_eq!((l, c), (ANNEX_K_LUMA, ANNEX_K_CHROMA));
        let [l, _] = ijg_tables(95);
        assert_eq!(&l[..4], [2, 1, 1, 2]);
        assert!(ijg_tables(100)[1].iter().all(|v| *v == 1));
    }

    #[test]
    fn webp_metadata_round_trip() {
        // A minimal lossy WebP: RIFF header and one (fake) VP8 chunk of 3 bytes (padded).
        let mut w = b"RIFF".to_vec();
        w.extend(16u32.to_le_bytes());
        w.extend(b"WEBPVP8 ");
        w.extend(3u32.to_le_bytes());
        w.extend([1, 2, 3, 0]);
        assert_eq!(webp_info(&w), Some(WebpInfo { lossless: false, animated: false }));
        let out = webp_with_metadata(&w, Some(b"icc"), Some(b"II*\0"), false, (5, 4)).unwrap();
        let ids: Vec<_> = webp_chunks(&out).unwrap().iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [*b"VP8X", *b"ICCP", *b"VP8 ", *b"EXIF"]);
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize, out.len() - 8);
        assert_eq!(webp_metadata(&out), (Some(b"icc".to_vec()), Some(b"II*\0".to_vec())));
        assert_eq!(out[20], 0x28, "ICC and EXIF flags");
    }
}
