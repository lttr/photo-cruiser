//! What the info panel shows about a photo: the most useful EXIF fields, formatted for
//! people, and a histogram of lightness.

use crate::meta::{Exif, Ifd};
use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

/// One line of the info panel: `"Aperture"`, `"f/2.8"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Field {
    pub label: &'static str,
    pub value: String,
}

/// The photo's fields, in this order, each only if the file has it: Taken, Camera, Lens,
/// Focal length, Aperture, Shutter, ISO, Exposure comp., Program, Metering, Flash, White
/// balance, Location, then always Size (after orientation) and File (format and size).
pub fn fields(path: &Path) -> Result<Vec<Field>> {
    let probe = crate::probe(path)?;
    let bytes = std::fs::metadata(path).map_err(|e| Error::io(path, e))?.len();
    // Unreadable metadata only means fewer fields.
    let exif = crate::decode::metadata(path).ok().and_then(|m| m.exif).and_then(|e| Exif::parse(&e));
    let mut out = exif.as_ref().map(exif_fields).unwrap_or_default();
    let mp = probe.width as f64 * probe.height as f64 / 1e6;
    let size = match mp >= 0.1 {
        true => format!("{} × {} · {} MP", probe.width, probe.height, trim(mp, 1)),
        false => format!("{} × {}", probe.width, probe.height),
    };
    out.push(Field { label: "Size", value: size });
    let file = format!("{} · {}", probe.format.as_str().to_uppercase(), file_size(bytes));
    out.push(Field { label: "File", value: file });
    Ok(out)
}

/// The EXIF part of [`fields`].
pub fn exif_fields(x: &Exif) -> Vec<Field> {
    let mut out = Vec::new();
    let mut push = |label, value: Option<String>| {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            out.push(Field { label, value });
        }
    };
    let taken = x.text(Ifd::Exif, 0x9003).or(x.text(Ifd::Main, 0x0132));
    push("Taken", taken.map(date));
    push("Camera", camera(x.text(Ifd::Main, 0x010F), x.text(Ifd::Main, 0x0110)));
    push("Lens", x.text(Ifd::Exif, 0xA434).map(str::to_string));
    let focal = x.number(Ifd::Exif, 0x920A).filter(|f| *f > 0.0);
    let full_frame = x.number(Ifd::Exif, 0xA405).filter(|f| *f > 0.0);
    push(
        "Focal length",
        match (focal, full_frame) {
            (Some(f), Some(e)) if (f - e).abs() >= 0.5 => {
                Some(format!("{} mm ({} mm full frame)", trim(f, 1), e))
            }
            (Some(f), _) => Some(format!("{} mm", trim(f, 1))),
            (None, Some(e)) => Some(format!("{e} mm full frame")),
            (None, None) => None,
        },
    );
    push("Aperture", x.number(Ifd::Exif, 0x829D).filter(|f| *f > 0.0).map(|f| format!("f/{}", trim(f, 1))));
    push("Shutter", x.number(Ifd::Exif, 0x829A).filter(|t| *t > 0.0).map(shutter));
    push("ISO", x.number(Ifd::Exif, 0x8827).filter(|i| *i > 0.0).map(|i| format!("ISO {i}")));
    push(
        "Exposure comp.",
        x.number(Ifd::Exif, 0x9204).map(|ev| match trim(ev, 1).as_str() {
            "0" | "-0" => "0 EV".to_string(),
            t if ev > 0.0 => format!("+{t} EV"),
            t => format!("{t} EV"),
        }),
    );
    let program = |p| match p {
        1 => "Manual",
        2 => "Program",
        3 => "Aperture priority",
        4 => "Shutter priority",
        5 => "Creative",
        6 => "Action",
        7 => "Portrait",
        8 => "Landscape",
        _ => "",
    };
    push("Program", x.number(Ifd::Exif, 0x8822).map(|p| program(p as u32).into()));
    let metering = |m| match m {
        1 => "Average",
        2 => "Centre-weighted",
        3 => "Spot",
        4 => "Multi-spot",
        5 => "Multi-segment",
        6 => "Partial",
        _ => "",
    };
    push("Metering", x.number(Ifd::Exif, 0x9207).map(|m| metering(m as u32).into()));
    push(
        "Flash",
        x.number(Ifd::Exif, 0x9209).map(|f| f as u32).map(|f| match f {
            _ if f & 0x20 != 0 => "No flash".into(),
            _ if f & 1 != 0 => "Fired".into(),
            _ => "Did not fire".into(),
        }),
    );
    push(
        "White balance",
        x.number(Ifd::Exif, 0xA403).map(|w| if w == 0.0 { "Auto" } else { "Manual" }.into()),
    );
    push("Location", location(x));
    out
}

/// `2024:07:14 18:32:05` → `2024-07-14 18:32:05`.
fn date(t: &str) -> String {
    match t.get(..10) {
        Some(d) if d.as_bytes().get(4) == Some(&b':') => format!("{}{}", d.replace(':', "-"), &t[10..]),
        _ => t.to_string(),
    }
}

/// Make and model, the make left out when the model starts with it (`NIKON D750`, not
/// `NIKON CORPORATION NIKON D750`).
fn camera(make: Option<&str>, model: Option<&str>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) => {
            let brand = make.split_whitespace().next().unwrap_or(make);
            match model.to_lowercase().starts_with(&brand.to_lowercase()) {
                true => Some(model.to_string()),
                false => Some(format!("{make} {model}")),
            }
        }
        (make, model) => make.or(model).map(str::to_string),
    }
}

/// `1/250 s`, `0.4 s` (from 1/3 s up, not a round fraction), `2 s`.
fn shutter(t: f64) -> String {
    match t < 0.3 {
        true => format!("1/{} s", (1.0 / t).round()),
        false => format!("{} s", trim(t, 1)),
    }
}

/// GPS position in decimal degrees: `50.07553° N, 14.43780° E`.
fn location(x: &Exif) -> Option<String> {
    let degrees = |tag| match x.get(Ifd::Gps, tag)? {
        crate::meta::Value::Ratio(v) if v.len() == 3 && v.iter().all(|n| n.is_finite()) => {
            Some(v[0] + v[1] / 60.0 + v[2] / 3600.0)
        }
        _ => None,
    };
    let (lat, lon) = (degrees(0x0002)?, degrees(0x0004)?);
    let lat_ref = x.text(Ifd::Gps, 0x0001).unwrap_or("N");
    let lon_ref = x.text(Ifd::Gps, 0x0003).unwrap_or("E");
    Some(format!("{lat:.5}° {lat_ref}, {lon:.5}° {lon_ref}"))
}

/// `8.4 MB`, `512 KB`, `900 B` (decimal units).
fn file_size(b: u64) -> String {
    match b {
        0..1000 => format!("{b} B"),
        1000..1_000_000 => format!("{} KB", (b as f64 / 1e3).round()),
        _ => format!("{} MB", trim(b as f64 / 1e6, 1)),
    }
}

/// `x` with at most `decimals` decimals, without trailing zeros.
fn trim(x: f64, decimals: usize) -> String {
    let s = format!("{x:.decimals$}");
    match s.contains('.') {
        true => s.trim_end_matches('0').trim_end_matches('.').to_string(),
        false => s,
    }
}

/// How many pixels have each lightness 0–255 (Rec. 709 luma of the encoded values, what
/// photo editors call the luminance histogram).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram(pub [u32; 256]);

/// At most this many pixels are counted, evenly spread, so big images stay quick.
const SAMPLES: usize = 1 << 18;

impl Histogram {
    /// Of RGBA pixels (alpha ignored).
    pub fn of(rgba: &[u8]) -> Histogram {
        let n = rgba.len() / 4;
        let step = n.div_ceil(SAMPLES).max(1);
        let mut bins = [0u32; 256];
        for p in rgba.chunks_exact(4).step_by(step) {
            // 0.2126, 0.7152, 0.0722 in 1/256ths.
            let y = (54 * p[0] as u32 + 183 * p[1] as u32 + 19 * p[2] as u32) >> 8;
            bins[y as usize] += 1;
        }
        Histogram(bins)
    }

    /// The bins scaled to 0–1 for drawing: by the highest one inside the range, so a
    /// spike of clipped pixels at 0 or 255 doesn't flatten the rest (it is cut off).
    pub fn heights(&self) -> [f32; 256] {
        let top = self.0[1..255].iter().copied().max().unwrap_or(0).max(1) as f32;
        self.0.map(|v| (v as f32 / top).min(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A big-endian EXIF block: IFD0 (make, model, Exif and GPS pointers), Exif IFD,
    /// GPS IFD. Out-of-line values go after all IFDs.
    fn exif() -> Vec<u8> {
        enum V<'a> {
            Ascii(&'a str),
            Short(u16),
            Long(u32),
            Ratio(&'a [(u32, u32)]),
            SRatio(i32, i32),
        }
        let ifds: [Vec<(u16, V)>; 3] = [
            vec![
                (0x010F, V::Ascii("NIKON CORPORATION")),
                (0x0110, V::Ascii("NIKON D750")),
                (0x8769, V::Long(0)), // patched below
                (0x8825, V::Long(0)),
            ],
            vec![
                (0x829A, V::Ratio(&[(1, 250)])),
                (0x829D, V::Ratio(&[(28, 10)])),
                (0x8822, V::Short(3)),
                (0x8827, V::Short(400)),
                (0x9003, V::Ascii("2024:07:14 18:32:05")),
                (0x9204, V::SRatio(-2, 3)),
                (0x9207, V::Short(5)),
                (0x9209, V::Short(16)),
                (0x920A, V::Ratio(&[(50, 1)])),
                (0xA403, V::Short(0)),
                (0xA405, V::Short(75)),
                (0xA434, V::Ascii("24-120mm f/4")),
            ],
            vec![
                (0x0001, V::Ascii("N")),
                (0x0002, V::Ratio(&[(50, 1), (4, 1), (3189, 100)])),
                (0x0003, V::Ascii("E")),
                (0x0004, V::Ratio(&[(14, 1), (26, 1), (1608, 100)])),
            ],
        ];
        let ifd_len = |n: usize| 2 + 12 * n + 4;
        let starts: Vec<u32> = ifds
            .iter()
            .scan(8u32, |at, ifd| {
                let s = *at;
                *at += ifd_len(ifd.len()) as u32;
                Some(s)
            })
            .collect();
        let mut extra_at = starts[2] + ifd_len(ifds[2].len()) as u32;
        let (mut b, mut extra) = (b"MM\0\x2a".to_vec(), Vec::new());
        b.extend(8u32.to_be_bytes());
        for ifd in &ifds {
            b.extend((ifd.len() as u16).to_be_bytes());
            for (tag, v) in ifd {
                let (typ, count, data): (u16, u32, Vec<u8>) = match v {
                    V::Ascii(s) => (2, s.len() as u32 + 1, [s.as_bytes(), b"\0"].concat()),
                    V::Short(n) => (3, 1, [n.to_be_bytes().as_slice(), &[0, 0]].concat()),
                    V::Long(_) if *tag == 0x8769 => (4, 1, starts[1].to_be_bytes().to_vec()),
                    V::Long(_) if *tag == 0x8825 => (4, 1, starts[2].to_be_bytes().to_vec()),
                    V::Long(n) => (4, 1, n.to_be_bytes().to_vec()),
                    V::Ratio(r) => (
                        5,
                        r.len() as u32,
                        r.iter().flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()]).flatten().collect(),
                    ),
                    V::SRatio(a, b) => (10, 1, [a.to_be_bytes(), b.to_be_bytes()].concat()),
                };
                b.extend(tag.to_be_bytes());
                b.extend(typ.to_be_bytes());
                b.extend(count.to_be_bytes());
                if data.len() <= 4 {
                    b.extend(&data);
                    b.extend(vec![0; 4 - data.len()]);
                } else {
                    b.extend(extra_at.to_be_bytes());
                    extra_at += data.len() as u32;
                    extra.extend(data);
                }
            }
            b.extend(0u32.to_be_bytes());
        }
        b.extend(extra);
        b
    }

    #[test]
    fn exif_fields_formatted() {
        let x = Exif::parse(&exif()).unwrap();
        let got: Vec<_> = exif_fields(&x).into_iter().map(|f| format!("{}: {}", f.label, f.value)).collect();
        assert_eq!(
            got,
            [
                "Taken: 2024-07-14 18:32:05",
                "Camera: NIKON D750",
                "Lens: 24-120mm f/4",
                "Focal length: 50 mm (75 mm full frame)",
                "Aperture: f/2.8",
                "Shutter: 1/250 s",
                "ISO: ISO 400",
                "Exposure comp.: -0.7 EV",
                "Program: Aperture priority",
                "Metering: Multi-segment",
                "Flash: Did not fire",
                "White balance: Auto",
                "Location: 50.07553° N, 14.43780° E",
            ]
        );
        assert_eq!(Exif::parse(b"nonsense"), None);
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(camera(Some("Apple"), Some("iPhone 13")).unwrap(), "Apple iPhone 13");
        assert_eq!(camera(None, Some("X100V")).unwrap(), "X100V");
        assert_eq!(
            (shutter(1.0 / 8000.0), shutter(0.5), shutter(2.0)),
            ("1/8000 s".into(), "0.5 s".into(), "2 s".into())
        );
        assert_eq!(
            (file_size(900), file_size(51_200), file_size(8_412_000)),
            ("900 B".into(), "51 KB".into(), "8.4 MB".into())
        );
        assert_eq!(date("not a date"), "not a date");
    }

    #[test]
    fn fields_of_a_file_without_exif() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/img1.png");
        let f = fields(&path).unwrap();
        assert_eq!(f[0], Field { label: "Size", value: "320 × 240".into() });
        assert_eq!(f[1].label, "File");
        assert!(f[1].value.starts_with("PNG · ") && f[1].value.ends_with(" KB"), "{}", f[1].value);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn histogram_counts_lightness() {
        let px = [[0, 0, 0, 255], [255, 255, 255, 0], [255, 0, 0, 255], [128, 128, 128, 255]];
        let h = Histogram::of(px.as_flattened());
        assert_eq!((h.0[0], h.0[255], h.0[53], h.0[128]), (1, 1, 1, 1), "red is dark");
        assert_eq!(h.0.iter().sum::<u32>(), 4);
        let heights = h.heights();
        assert_eq!((heights[0], heights[128], heights[100]), (1.0, 1.0, 0.0));
        // Big images: sampled, about SAMPLES pixels.
        let big = vec![200u8; 4 * 3000 * 2000];
        let n: u32 = Histogram::of(&big).0.iter().sum();
        assert!((SAMPLES as u32 / 2..=SAMPLES as u32).contains(&n), "{n}");
    }
}
