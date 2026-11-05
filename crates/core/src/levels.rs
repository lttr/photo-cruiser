//! Halved copies of an image, for drawing it small without aliasing.
//!
//! A GPU's bilinear filter reads only the 2×2 texels nearest each screen pixel. Shrinking
//! by more than 2:1 skips texels, and edges and noise come out jagged ("too sharp").
//! Without mipmaps (iced has none), the fix is the same as theirs: draw from the copy
//! that is at most twice the size on screen.

use image::RgbaImage;

/// No copies with a longer side below this: smaller than any useful view.
pub const MIN_SIDE: u32 = 256;

/// The image halved again and again (each copy half the previous one, rounded up), while
/// the longer side stays at least [`MIN_SIDE`]. Largest first; empty for small images.
pub fn halvings(img: &RgbaImage) -> Vec<RgbaImage> {
    let mut out: Vec<RgbaImage> = Vec::new();
    loop {
        let last = out.last().unwrap_or(img);
        if last.width().max(last.height()).div_ceil(2) < MIN_SIDE {
            return out;
        }
        let next = halve(last);
        out.push(next);
    }
}

/// Each output pixel is the average of the 2×2 pixels it covers (fewer at an odd edge),
/// weighted by alpha so transparent pixels don't darken the colour.
pub fn halve(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (nw, nh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
    let src = img.as_raw();
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    let stride = w as usize * 4;
    for y in 0..nh as usize {
        let rows = [2 * y, (2 * y + 1).min(h as usize - 1)];
        let rows = if rows[0] == rows[1] { &rows[..1] } else { &rows[..] };
        for x in 0..nw as usize {
            let cols = [2 * x, (2 * x + 1).min(w as usize - 1)];
            let cols = if cols[0] == cols[1] { &cols[..1] } else { &cols[..] };
            let (mut rgb, mut a, mut n) = ([0u32; 3], 0u32, 0u32);
            for &r in rows {
                for &c in cols {
                    let p = &src[r * stride + c * 4..][..4];
                    let pa = p[3] as u32;
                    for i in 0..3 {
                        rgb[i] += p[i] as u32 * pa;
                    }
                    a += pa;
                    n += 1;
                }
            }
            let o = &mut out[(y * nw as usize + x) * 4..][..4];
            for (o, c) in o.iter_mut().zip(rgb) {
                *o = (c + a / 2).checked_div(a).unwrap_or(0) as u8;
            }
            o[3] = ((a + n / 2) / n) as u8;
        }
    }
    RgbaImage::from_raw(nw, nh, out).expect("buffer matches size")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn halve_averages_blocks() {
        let img = RgbaImage::from_fn(4, 2, |x, _| match x {
            0 => Rgba([0, 0, 0, 255]),
            1 => Rgba([200, 100, 50, 255]),
            _ => Rgba([10, 20, 30, 255]),
        });
        let h = halve(&img);
        assert_eq!(h.dimensions(), (2, 1));
        assert_eq!(h.get_pixel(0, 0), &Rgba([100, 50, 25, 255]));
        assert_eq!(h.get_pixel(1, 0), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn halve_odd_edges_and_alpha() {
        // 3×3: the last column and row average only what they cover.
        let img = RgbaImage::from_fn(3, 3, |x, y| Rgba([(x * 100) as u8, (y * 100) as u8, 0, 255]));
        let h = halve(&img);
        assert_eq!(h.dimensions(), (2, 2));
        assert_eq!(h.get_pixel(1, 1), &Rgba([200, 200, 0, 255]));
        assert_eq!(h.get_pixel(1, 0), &Rgba([200, 50, 0, 255]));
        // A transparent pixel doesn't pull the colour towards its own (black).
        let img = RgbaImage::from_fn(2, 1, |x, _| if x == 0 { Rgba([200, 0, 0, 255]) } else { Rgba([0; 4]) });
        assert_eq!(halve(&img).get_pixel(0, 0), &Rgba([200, 0, 0, 128]));
        // Fully transparent stays so.
        assert_eq!(halve(&RgbaImage::new(2, 2)).get_pixel(0, 0), &Rgba([0; 4]));
    }

    #[test]
    fn halvings_stop_at_min_side() {
        let sizes = |w, h| halvings(&RgbaImage::new(w, h)).iter().map(|i| i.dimensions()).collect::<Vec<_>>();
        assert_eq!(sizes(2000, 1001), vec![(1000, 501), (500, 251)]);
        assert_eq!(sizes(510, 300), vec![]);
        assert_eq!(sizes(512, 300), vec![(256, 150)]);
    }
}
