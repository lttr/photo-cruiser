//! Justified layout: photos in rows of equal height that fill the width exactly, each
//! photo at its own aspect ratio (like Flickr's `justified-layout`).
//!
//! Rows are filled greedily: photos are added until the row, stretched to the width,
//! would be lower than the target height; then the row ends with or without the last
//! photo, whichever height is closer to the target. The last row is not stretched.

use std::ops::Range;

/// Aspect ratios (width / height) are clamped to this, so a sliver or a giant panorama
/// can't make a row absurdly tall or low.
pub const ASPECT_RANGE: (f32, f32) = (0.25, 8.0);

/// The aspect ratio the layout uses for `aspect` (within [`ASPECT_RANGE`]).
pub fn clamp(aspect: f32) -> f32 {
    aspect.clamp(ASPECT_RANGE.0, ASPECT_RANGE.1)
}

/// One row: the photos `items`, all `height` high, each `clamp(aspect) * height` wide.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub items: Range<usize>,
    pub height: f32,
}

/// Break photos with the given aspect ratios into rows `width` wide (photo widths plus
/// `gap` between neighbours), about `target` high.
pub fn rows(aspects: &[f32], width: f32, target: f32, gap: f32) -> Vec<Row> {
    let aspect = |i: usize| clamp(aspects[i]);
    // Height of `items` stretched to the width.
    let fit = |sum: f32, n: usize| (width - (n - 1) as f32 * gap).max(1.0) / sum;
    let mut rows = Vec::new();
    let mut start = 0;
    while start < aspects.len() {
        let (mut sum, mut end) = (0.0, start);
        let mut done = None;
        while end < aspects.len() {
            let before = (end > start).then(|| fit(sum, end - start));
            sum += aspect(end);
            end += 1;
            let h = fit(sum, end - start);
            if h <= target {
                // Closer to the target (as a ratio): with or without the photo just added.
                done = Some(match before {
                    Some(b) if b / target < target / h => Row { items: start..end - 1, height: b },
                    _ => Row { items: start..end, height: h },
                });
                break;
            }
        }
        let row = done.unwrap_or(Row { items: start..end, height: target });
        start = row.items.end;
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(aspects: &[f32], row: &Row, gap: f32) -> f32 {
        aspects[row.items.clone()].iter().map(|a| a * row.height).sum::<f32>()
            + (row.items.len() - 1) as f32 * gap
    }

    #[test]
    fn rows_fill_the_width_near_the_target() {
        let aspects = [1.5, 0.75, 1.5, 1.0, 2.0, 1.5, 0.66, 1.33, 1.5, 1.5, 0.8];
        let rows = rows(&aspects, 1000.0, 200.0, 4.0);
        let (last, full) = rows.split_last().unwrap();
        for r in full {
            assert!((width(&aspects, r, 4.0) - 1000.0).abs() < 0.01, "{r:?}");
            assert!((150.0..=270.0).contains(&r.height), "{r:?}");
        }
        assert_eq!(last.height, 200.0, "the last row isn't stretched");
        assert!(width(&aspects, last, 4.0) <= 1000.0);
        // Every photo exactly once, in order.
        let items: Vec<usize> = rows.iter().flat_map(|r| r.items.clone()).collect();
        assert_eq!(items, (0..aspects.len()).collect::<Vec<_>>());
    }

    #[test]
    fn picks_the_closer_height() {
        // Two 1.5 photos are 333 high, three 222. Target 300: 333/300 = 1.11 beats
        // 300/222 = 1.35, so two.
        let r = rows(&[1.5; 4], 1000.0, 300.0, 0.0);
        assert_eq!(r[0], Row { items: 0..2, height: 1000.0 / 3.0 });
        // Target 250: three at 222 (ratio 1.125) beats two at 333 (1.33).
        let r = rows(&[1.5; 4], 1000.0, 250.0, 0.0);
        assert_eq!(r[0].items, 0..3);
    }

    #[test]
    fn edge_cases() {
        assert!(rows(&[], 1000.0, 200.0, 4.0).is_empty());
        // A panorama wider than the row: alone, lower than the target.
        let r = rows(&[20.0, 1.0], 1000.0, 200.0, 4.0);
        assert_eq!(r[0], Row { items: 0..1, height: 1000.0 / ASPECT_RANGE.1 });
        // No usable width: one photo per row, nothing panics.
        assert_eq!(rows(&[1.0, 1.0], 0.0, 200.0, 4.0).len(), 2);
    }
}
