//! Edge-aware smoothing: a self-guided filter (He, Sun and Tang, "Guided Image
//! Filtering", 2010), for tools that change a photo by its local brightness (Shadows &
//! highlights; later local contrast and dehaze).
//!
//! Inside a window, the output is a linear fit of the input: flat where the input varies
//! less than `eps` (it averages), the input itself where it varies much more (an edge
//! stays sharp). So smoothed brightness follows strong edges, and changing a photo by it
//! leaves no halos along them. Costs a few box filters, whatever the radius.

/// A single-channel image, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(width: usize, height: usize, data: Vec<f32>) -> Self {
        assert_eq!(data.len(), width * height, "plane size");
        Plane { width, height, data }
    }

    fn map(&self, f: impl Fn(f32) -> f32) -> Plane {
        Plane { data: self.data.iter().map(|v| f(*v)).collect(), ..*self }
    }

    fn zip(&self, other: &Plane, f: impl Fn(f32, f32) -> f32) -> Plane {
        Plane { data: self.data.iter().zip(&other.data).map(|(a, b)| f(*a, *b)).collect(), ..*self }
    }
}

/// The mean of each `(2r + 1)²` window; windows are cut at the edges, not padded.
pub fn box_mean(p: &Plane, r: usize) -> Plane {
    let (w, h) = (p.width, p.height);
    // Window bounds [lo, hi) of position `i` along an axis of `len`.
    let window = |i: usize, len: usize| (i.saturating_sub(r), (i + r + 1).min(len));
    // Along rows, a running sum per row.
    let mut rows = vec![0f32; w * h];
    for (src, out) in p.data.chunks_exact(w).zip(rows.chunks_exact_mut(w)) {
        let mut sum = 0f64;
        let (mut lo, mut hi) = (0, 0);
        for (i, o) in out.iter_mut().enumerate() {
            let (want_lo, want_hi) = window(i, w);
            for v in &src[hi..want_hi] {
                sum += f64::from(*v);
            }
            for v in &src[lo..want_lo] {
                sum -= f64::from(*v);
            }
            (lo, hi) = (want_lo, want_hi);
            *o = (sum / (hi - lo) as f64) as f32;
        }
    }
    // Along columns, a running sum per column, a whole row at a time (cache friendly).
    let mut out = vec![0f32; w * h];
    let mut sums = vec![0f64; w];
    let (mut lo, mut hi) = (0, 0);
    for y in 0..h {
        let (want_lo, want_hi) = window(y, h);
        for row in rows[hi * w..want_hi * w].chunks_exact(w) {
            sums.iter_mut().zip(row).for_each(|(s, v)| *s += f64::from(*v));
        }
        for row in rows[lo * w..want_lo * w].chunks_exact(w) {
            sums.iter_mut().zip(row).for_each(|(s, v)| *s -= f64::from(*v));
        }
        (lo, hi) = (want_lo, want_hi);
        let n = (hi - lo) as f64;
        out[y * w..(y + 1) * w].iter_mut().zip(&sums).for_each(|(o, s)| *o = (s / n) as f32);
    }
    Plane::new(w, h, out)
}

/// `p` smoothed by a self-guided filter of radius `r` and edge threshold `eps` (a variance
/// in the units of `p` squared): regions varying less than about `√eps` are averaged,
/// stronger edges kept.
///
/// Large planes are filtered fast (He and Sun, "Fast Guided Filter", 2015): the fit is
/// found on a copy shrunk so its longer side is at most [`FAST_SIDE`], and applied to `p`
/// at full size. Edges stay as sharp as `p`'s.
pub fn guided(p: &Plane, r: usize, eps: f32) -> Plane {
    let s = p.width.max(p.height).div_ceil(FAST_SIDE).max(1);
    let small = if s > 1 { shrink(p, s) } else { p.clone() };
    let r_small = (r as f32 / s as f32).round().max(1.0) as usize;
    let mean = box_mean(&small, r_small);
    let var = box_mean(&small.map(|v| v * v), r_small).zip(&mean, |sq, m| (sq - m * m).max(0.0));
    let a = var.map(|v| v / (v + eps));
    let b = mean.zip(&a, |m, a| m * (1.0 - a));
    let (a, b) = (box_mean(&a, r_small), box_mean(&b, r_small));
    let (a, b) = if s > 1 { (grow(&a, p.width, p.height), grow(&b, p.width, p.height)) } else { (a, b) };
    Plane { data: (0..p.data.len()).map(|i| a.data[i] * p.data[i] + b.data[i]).collect(), ..*p }
}

/// Planes with a longer side above this are filtered on a shrunk copy.
pub const FAST_SIDE: usize = 1024;

/// `p` shrunk by `s`: each pixel the mean of the `s × s` block it covers (fewer at the
/// edges).
fn shrink(p: &Plane, s: usize) -> Plane {
    let (w, h) = (p.width.div_ceil(s), p.height.div_ceil(s));
    let mut sum = vec![0f64; w * h];
    let mut n = vec![0u32; w * h];
    for (y, row) in p.data.chunks_exact(p.width).enumerate() {
        let base = (y / s) * w;
        for (x, v) in row.iter().enumerate() {
            sum[base + x / s] += f64::from(*v);
            n[base + x / s] += 1;
        }
    }
    Plane::new(w, h, sum.iter().zip(&n).map(|(s, n)| (s / f64::from(*n)) as f32).collect())
}

/// `p` grown to `w × h` by bilinear interpolation, block centres to pixel centres.
fn grow(p: &Plane, w: usize, h: usize) -> Plane {
    let (kx, ky) = (p.width as f32 / w as f32, p.height as f32 / h as f32);
    // For each output column (row): the two source columns (rows) and the weight of the second.
    let taps = |len: usize, k: f32, src_len: usize| -> Vec<(usize, usize, f32)> {
        (0..len)
            .map(|i| {
                let f = ((i as f32 + 0.5) * k - 0.5).clamp(0.0, (src_len - 1) as f32);
                let i0 = f.floor() as usize;
                (i0, (i0 + 1).min(src_len - 1), f - i0 as f32)
            })
            .collect()
    };
    let (xs, ys) = (taps(w, kx, p.width), taps(h, ky, p.height));
    let mut out = Vec::with_capacity(w * h);
    for &(y0, y1, fy) in &ys {
        let (r0, r1) = (&p.data[y0 * p.width..][..p.width], &p.data[y1 * p.width..][..p.width]);
        for &(x0, x1, fx) in &xs {
            let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
            let bottom = r1[x0] + (r1[x1] - r1[x0]) * fx;
            out.push(top + (bottom - top) * fy);
        }
    }
    Plane::new(w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_mean_cuts_windows_at_edges() {
        let p = Plane::new(4, 1, vec![0.0, 4.0, 8.0, 12.0]);
        let m = box_mean(&p, 1);
        assert_eq!(m.data, [2.0, 4.0, 8.0, 10.0]);
        assert_eq!(box_mean(&p, 0), p);
        let tall = Plane::new(1, 3, vec![3.0, 6.0, 9.0]);
        assert_eq!(box_mean(&tall, 5).data, [6.0, 6.0, 6.0]);
    }

    #[test]
    fn shrink_and_grow() {
        let p = Plane::new(5, 2, vec![0.0, 2.0, 4.0, 6.0, 8.0, 0.0, 2.0, 4.0, 6.0, 8.0]);
        let s = shrink(&p, 2);
        assert_eq!((s.width, s.height, s.data.clone()), (3, 1, vec![1.0, 5.0, 8.0]));
        let g = grow(&Plane::new(2, 1, vec![0.0, 4.0]), 4, 2);
        assert_eq!(g.data, [0.0, 1.0, 3.0, 4.0, 0.0, 1.0, 3.0, 4.0]);
    }

    #[test]
    fn fast_path_keeps_edges() {
        // A 3000-wide step with texture, on the fast path.
        let (w, h) = (3000, 40);
        let data = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let tex = if (x / 3 + y / 3) % 2 == 0 { 0.1 } else { -0.1 };
                if x < 1700 { -4.0 + tex } else { tex }
            })
            .collect();
        let p = Plane::new(w, h, data);
        let s = guided(&p, 75, 0.25);
        let at = |x: usize| s.data[20 * w + x];
        // The step stays a step: under 0.4 EV bleeds across, and only near the edge.
        for x in (1650..1700).chain(1700..1750) {
            let want = if x < 1700 { -4.0 } else { 0.0 };
            assert!((at(x) - want).abs() < 0.4, "{x}: {}", at(x));
        }
        for x in [100, 1000, 1600, 1800, 2500] {
            let want = if x < 1700 { -4.0 } else { 0.0 };
            assert!((at(x) - want).abs() < 0.05, "{x}: {}", at(x));
        }
    }

    #[test]
    fn guided_smooths_texture_and_keeps_edges() {
        // Left half around −4 with ±0.1 texture, right half around 0 with the same.
        let (w, h) = (40, 10);
        let data = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let tex = if (x + y) % 2 == 0 { 0.1 } else { -0.1 };
                if x < w / 2 { -4.0 + tex } else { tex }
            })
            .collect();
        let s = guided(&Plane::new(w, h, data), 4, 0.25);
        let at = |x: usize| s.data[5 * w + x];
        // Texture averaged away from the edge; at the edge, at most a pixel or two bleeds a
        // little across.
        for x in [0, 5, 10] {
            assert!((at(x) + 4.0).abs() < 0.03, "dark side smooth: {x}: {}", at(x));
        }
        for x in [29, 35, 39] {
            assert!(at(x).abs() < 0.03, "bright side smooth: {x}: {}", at(x));
        }
        for x in [18, 19] {
            assert!((at(x) + 4.0).abs() < 0.3, "dark up to the edge: {x}: {}", at(x));
        }
        for x in [20, 21] {
            assert!(at(x).abs() < 0.3, "bright from the edge: {x}: {}", at(x));
        }
    }
}
