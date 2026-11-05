//! A photo's star: XMP `xmp:Rating` (0 unrated, 1–5 stars, -1 rejected), the tag darktable,
//! gThumb, digiKam and Lightroom share. One star marks a photo worth keeping; any rating of 1
//! or more counts as starred.
//!
//! JPEG and WebP keep it in the photo file: only `xmp:Rating` in its XMP packet changes (the
//! rest of the packet stays, the image data is copied byte for byte, the file keeps its
//! modification time). No backup: nothing else changes, and setting the star again undoes
//! it. Other formats get a sidecar next to the photo, `IMG_1234.HEIC.xmp` (darktable's name);
//! one that exists already is patched the same way.
//!
//! Reading: the photo's own packet, else the sidecar, else EXIF `Rating` (Windows writes it).

use crate::meta::{self, Exif, Head, Ifd};
use crate::source::one_at_a_time;
use crate::{Error, Result, store};
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// The namespace of `xmp:Rating`.
pub const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";

/// The star a rating shows as.
pub fn is_starred(rating: i8) -> bool {
    rating >= 1
}

/// The photo's rating (0 if it has none).
pub fn read(photo: &Path) -> Result<i8> {
    let embedded = {
        let _one = one_at_a_time(photo);
        match crate::decode::read_jpeg_head(photo)? {
            Some(head) => {
                let h = head.parsed();
                let exif = h.exif.and_then(Exif::parse).and_then(|e| e.number(Ifd::Main, 0x4746));
                (h.xmp.and_then(|x| xmp_rating(&String::from_utf8_lossy(x))), exif)
            }
            None => (read_webp_xmp(photo)?.and_then(|x| xmp_rating(&String::from_utf8_lossy(&x))), None),
        }
    };
    if let (Some(r), _) = embedded {
        return Ok(r);
    }
    let side = sidecar(photo);
    if side.is_file()
        && let Some(r) = xmp_rating(&store::read_text(&side)?)
    {
        return Ok(r);
    }
    Ok(embedded.1.map_or(0, |r| r.round().clamp(-1.0, 5.0) as i8))
}

/// Give the photo `rating`, unless it has it already (returns whether it changed). Saved
/// edits stay valid ([`store::keep_stamp`]).
pub fn set(photo: &Path, rating: i8) -> Result<bool> {
    if read(photo)? == rating {
        return Ok(false);
    }
    store::keep_stamp(photo, || write(photo, rating))?;
    Ok(true)
}

/// Star (`true`) or unstar the photo; see [`set`].
pub fn star(photo: &Path, on: bool) -> Result<bool> {
    set(photo, if on { 1 } else { 0 })
}

/// Give a new file of the photo (a save or revert just wrote it) the rating the old one had.
/// Best effort: the photo itself is written already.
pub(crate) fn carry(photo: &Path, rating: i8) {
    if read(photo).unwrap_or(0) != rating
        && let Err(e) = write(photo, rating)
    {
        eprintln!("photo-cruiser: {}: the star was lost: {e}", photo.display());
    }
}

/// `IMG_1234.HEIC.xmp` for `IMG_1234.HEIC`.
pub fn sidecar(photo: &Path) -> PathBuf {
    let mut name = photo.file_name().unwrap_or_default().to_os_string();
    name.push(".xmp");
    photo.with_file_name(name)
}

/// Write the rating where it belongs: into a JPEG or WebP, else into the sidecar.
fn write(photo: &Path, rating: i8) -> Result<()> {
    let bytes = crate::source::read(photo)?;
    let unreadable = |what: &str| Error::decode(photo, format!("can't add the star: {what}"));
    // A JPEG broken before its frame can't be patched safely: it gets a sidecar, as [`read`] expects.
    let jpeg = match meta::jpeg_head(&bytes) {
        Head::Done(h) => Some(h),
        _ => None,
    };
    let out = if let Some(head) = jpeg {
        let packet = with_rating(head.xmp, rating, false).ok_or_else(|| unreadable("unreadable XMP"))?;
        meta::jpeg_with_xmp(&bytes, packet.as_bytes()).ok_or_else(|| unreadable("XMP too big"))?
    } else if bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        let packet =
            with_rating(meta::webp_xmp(&bytes), rating, false).ok_or_else(|| unreadable("unreadable XMP"))?;
        meta::webp_with_xmp(&bytes, packet.as_bytes()).ok_or_else(|| unreadable("broken WebP"))?
    } else {
        return write_sidecar(photo, rating);
    };
    store::replace_file(photo, &out)
}

fn write_sidecar(photo: &Path, rating: i8) -> Result<()> {
    let side = sidecar(photo);
    let old = match side.is_file() {
        true => Some(store::read_text(&side)?),
        false if rating == 0 => return Ok(()),
        false => None,
    };
    let text = with_rating(old.as_deref().map(str::as_bytes), rating, true)
        .ok_or_else(|| Error::decode(&side, "can't add the star: unreadable XMP"))?;
    store::write_text(&side, &text)
}

/// `packet` (or a new one) with `rating`. `None` if it can't be patched safely.
fn with_rating(packet: Option<&[u8]>, rating: i8, sidecar: bool) -> Option<String> {
    match packet {
        Some(p) => xmp_with_rating(std::str::from_utf8(p).ok()?, rating),
        None if sidecar => Some(new_sidecar(rating)),
        None => Some(new_packet(rating)),
    }
}

/// The `XMP ` chunk of a WebP, reading only chunk headers up to it. `None` if it has none
/// or isn't a WebP. Hold [`one_at_a_time`] while calling.
fn read_webp_xmp(path: &Path) -> Result<Option<Vec<u8>>> {
    let io = |e| Error::io(path, e);
    let mut f = std::fs::File::open(path).map_err(io)?;
    let mut head = [0u8; 12];
    if f.read_exact(&mut head).is_err() || &head[..4] != b"RIFF" || &head[8..] != b"WEBP" {
        return Ok(None);
    }
    let mut chunk = [0u8; 8];
    while f.read_exact(&mut chunk).is_ok() {
        let len = u32::from_le_bytes(chunk[4..].try_into().expect("4 bytes")) as u64;
        if &chunk[..4] == b"XMP " {
            let mut data = Vec::new();
            (&mut f).take(len).read_to_end(&mut data).map_err(io)?;
            return Ok(Some(data));
        }
        f.seek(SeekFrom::Current((len + len % 2) as i64)).map_err(io)?;
    }
    Ok(None)
}

/// The rating in an XMP packet, if it has one.
pub fn xmp_rating(x: &str) -> Option<i8> {
    let v: f64 = x[find_rating(x)?].trim().parse().ok()?;
    Some(v.round().clamp(-1.0, 5.0) as i8)
}

/// The packet with `xmp:Rating` set to `rating`: the old value replaced, or the property
/// added to the first `rdf:Description`. Trailing padding absorbs the change in length, so
/// a padded packet keeps its size. `None` if it has no place for it.
pub fn xmp_with_rating(x: &str, rating: i8) -> Option<String> {
    let out = match find_rating(x) {
        Some(r) => format!("{}{rating}{}", &x[..r.start], &x[r.end..]),
        None => insert_rating(x, rating)?,
    };
    Some(keep_size(x.len(), out))
}

/// Prefixes bound to [`XMP_NS`] in `x`.
fn prefixes(x: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (i, _) in x.match_indices("xmlns:") {
        let rest = &x[i + 6..];
        let Some(eq) = rest.find('=') else { continue };
        let name = rest[..eq].trim();
        let value = rest[eq + 1..].trim_start();
        let Some(q) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        if value[1..].split(q).next() == Some(XMP_NS) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Where the value of `xmp:Rating` is (an attribute's or an element's text).
fn find_rating(x: &str) -> Option<Range<usize>> {
    for p in prefixes(x) {
        let name = format!("{p}:Rating");
        for (i, _) in x.match_indices(&name) {
            let before = x[..i].chars().next_back();
            let after = &x[i + name.len()..];
            let start = i + name.len();
            match before {
                Some('<') if after.starts_with('>') => {
                    let end = after.find("</")?;
                    return Some(start + 1..start + end);
                }
                Some(c) if c.is_whitespace() => {
                    let rest = after.trim_start();
                    let Some(rest) = rest.strip_prefix('=') else { continue };
                    let rest = rest.trim_start();
                    let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
                    let at = x.len() - rest.len() + 1;
                    let end = x[at..].find(q)?;
                    return Some(at..at + end);
                }
                _ => {}
            }
        }
    }
    None
}

/// Add `xmp:Rating` as an attribute of the first `rdf:Description` (or a new one).
fn insert_rating(x: &str, rating: i8) -> Option<String> {
    let Some(start) = x
        .find("<rdf:Description")
        .filter(|&i| x[i + 16..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/'))
    else {
        // No description at all: a new one in the rdf:RDF element.
        let at = x.find("<rdf:RDF")?;
        let end = at + tag_end(&x[at..])?;
        if x[..end].ends_with('/') {
            return None;
        }
        let desc =
            format!("\n  <rdf:Description rdf:about=\"\" xmlns:xmp=\"{XMP_NS}\" xmp:Rating=\"{rating}\"/>");
        return Some(format!("{}{desc}{}", &x[..=end], &x[end + 1..]));
    };
    let mut end = start + tag_end(&x[start..])?;
    if x[..end].ends_with('/') {
        end -= 1;
    }
    let tag = &x[start..end];
    let attr = match prefixes(tag).first() {
        Some(p) => format!(" {p}:Rating=\"{rating}\""),
        // `xmp` bound to something else here: leave it alone.
        None if tag.contains("xmlns:xmp=") => return None,
        None => format!(" xmlns:xmp=\"{XMP_NS}\" xmp:Rating=\"{rating}\""),
    };
    Some(format!("{}{attr}{}", &x[..end], &x[end..]))
}

/// Offset of the `>` closing the start tag at the beginning of `x`, skipping quoted values.
fn tag_end(x: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in x.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Take a change in length out of (or give it to) the padding before `<?xpacket end`.
fn keep_size(len: usize, mut s: String) -> String {
    let Some(end) = s.rfind("<?xpacket end") else { return s };
    let pad_start = s[..end].trim_end().len();
    let pad = end - pad_start;
    if s.len() > len {
        let d = s.len() - len;
        // Keep a newline before the end.
        if pad > d {
            s.replace_range(pad_start..pad_start + d, "");
        }
    } else if pad > 0 {
        s.insert_str(end, &" ".repeat(len - s.len()));
    }
    s
}

const PACKET_PADDING: usize = 512;

/// A packet to embed: the XMP wrapper, the rating, a little padding.
fn new_packet(rating: i8) -> String {
    let pad = " ".repeat(PACKET_PADDING);
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n{}\n{pad}\n<?xpacket end=\"w\"?>",
        xmpmeta(rating)
    )
}

/// A sidecar file.
fn new_sidecar(rating: i8) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}\n", xmpmeta(rating))
}

fn xmpmeta(rating: i8) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
         <rdf:Description rdf:about=\"\" xmlns:xmp=\"{XMP_NS}\" xmp:Rating=\"{rating}\"/>\n \
         </rdf:RDF>\n</x:xmpmeta>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARKTABLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 4.4.0-Exiv2">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:darktable="http://darktable.sf.net/"
    xmp:Rating="3"
    darktable:xmp_version="5">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

    #[test]
    fn reads_and_replaces_an_attribute() {
        assert_eq!(xmp_rating(DARKTABLE), Some(3));
        let out = xmp_with_rating(DARKTABLE, 0).unwrap();
        assert_eq!(out, DARKTABLE.replace(r#"xmp:Rating="3""#, r#"xmp:Rating="0""#));
        assert_eq!(xmp_rating(&out), Some(0));
    }

    #[test]
    fn element_form_other_prefix_and_lookalikes() {
        let x = r#"<rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:xap="http://ns.adobe.com/xap/1.0/"
            xmlns:MicrosoftPhoto="http://ns.microsoft.com/photo/1.0/" MicrosoftPhoto:Rating="75">
            <xap:RatingPercent>1</xap:RatingPercent><xap:Rating>4</xap:Rating></rdf:Description></rdf:RDF>"#;
        assert_eq!(xmp_rating(x), Some(4));
        let out = xmp_with_rating(x, 1).unwrap();
        assert!(out.contains("<xap:Rating>1</xap:Rating>") && out.contains(r#"MicrosoftPhoto:Rating="75""#));
        assert_eq!(xmp_rating(r#"<x xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating='-1'/>"#), Some(-1));
        assert_eq!(xmp_rating(r#"<x xmlns:xmp="other" xmp:Rating="2"/>"#), None, "not the XMP namespace");
    }

    #[test]
    fn added_to_the_first_description_keeping_the_rest() {
        let x = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r">
<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" dc:note="a > b"/>
</rdf:RDF></x:xmpmeta>"#;
        let out = xmp_with_rating(x, 1).unwrap();
        assert_eq!(xmp_rating(&out), Some(1));
        assert!(
            out.contains(r#"dc:note="a > b" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="1"/>"#),
            "{out}"
        );
        // Declared there already: just the attribute.
        let y = r#"<rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreatorTool="cam">"#;
        assert_eq!(
            xmp_with_rating(y, 1).unwrap(),
            r#"<rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreatorTool="cam" xmp:Rating="1">"#
        );
        // No description: a new one.
        let z = r#"<x:xmpmeta><rdf:RDF xmlns:rdf="r"></rdf:RDF></x:xmpmeta>"#;
        assert_eq!(xmp_rating(&xmp_with_rating(z, 1).unwrap()), Some(1));
        assert_eq!(xmp_with_rating("<garbage/>", 1), None);
    }

    #[test]
    fn padding_takes_up_the_change() {
        let p = new_packet(0);
        let starred = xmp_with_rating(&p, 1).unwrap();
        assert_eq!(starred.len(), p.len());
        let head = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description rdf:about=""/></rdf:RDF></x:xmpmeta>"#;
        let x = &format!("{head}\n{}\n<?xpacket end=\"w\"?>", " ".repeat(100));
        let out = xmp_with_rating(x, 1).unwrap();
        assert_eq!((out.len(), xmp_rating(&out)), (x.len(), Some(1)));
        assert!(out.ends_with(" \n<?xpacket end=\"w\"?>"), "{out}");
        assert_eq!(xmp_rating(&new_sidecar(1)), Some(1));
        assert_eq!(sidecar(Path::new("/a/IMG_1.HEIC")), Path::new("/a/IMG_1.HEIC.xmp"));
    }
}
