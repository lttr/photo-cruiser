//! Geometry of the photo grid: rows of cells. Shared by the view (layout) and the state
//! (row moves, scrolling, which thumbnails are visible), so both agree on where every
//! photo is.
//!
//! Three layouts (`Z` cycles): small or big square cells, as many columns as fit the
//! width; or justified rows, each photo at its own aspect ratio and every row filling the
//! width, about as high as the big cells. Justified rows have only thin gaps (no cell
//! padding), the file names over the photos.

use photo_cruiser_core::justified;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// File name under the thumbnail (only while names are shown).
pub const LABEL: f32 = 18.0;

/// Space around and between the cells of a layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    /// Around the grid.
    pub pad: f32,
    /// Between cells.
    pub gap: f32,
    /// Inside a cell, around the thumbnail (the selection shows there).
    pub cell: f32,
    /// Kept free on the right for the scrollbar.
    pub scrollbar: f32,
}

/// Width of iced's scrollbar (it floats over the content).
const SCROLLBAR: f32 = 10.0;
const CELLS: Margins = Margins { pad: 16.0, gap: 6.0, cell: 6.0, scrollbar: SCROLLBAR + 2.0 };
/// Justified: thin gaps, the photos fill their cells. The scrollbar is still kept free:
/// its rail is opaque and would hide photos.
const THIN: Margins = Margins { pad: 4.0, gap: 4.0, cell: 0.0, scrollbar: SCROLLBAR };
/// Aspect ratio assumed while a photo's size isn't known (justified layout).
pub const DEFAULT_ASPECT: f32 = 1.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Small,
    /// Cells twice as big.
    Big,
    /// Rows of photos at their aspect ratio, as high as the big cells.
    Justified,
}

impl Layout {
    /// The one after this (`Z`).
    pub fn next(self) -> Self {
        match self {
            Layout::Small => Layout::Big,
            Layout::Big => Layout::Justified,
            Layout::Justified => Layout::Small,
        }
    }

    /// The square thumbnails are fitted into, or the target row height.
    pub fn side(self) -> f32 {
        if self == Layout::Small { 180.0 } else { 360.0 }
    }

    /// Longest side of a decoded thumbnail (about 1.4× the box, for HiDPI screens).
    pub fn thumb_side(self) -> u32 {
        if self == Layout::Small { 256 } else { 512 }
    }

    pub fn margins(self) -> Margins {
        if self == Layout::Justified { THIN } else { CELLS }
    }

    /// File names go under the thumbnails, not over them.
    pub fn label_below(self) -> bool {
        self != Layout::Justified
    }

    /// Menu item that switches to this layout.
    pub fn label(self) -> &'static str {
        match self {
            Layout::Small => "Small thumbnails",
            Layout::Big => "Big thumbnails",
            Layout::Justified => "Justified layout",
        }
    }
}

/// A row of cells. Sizes are outer (padding and the name label included).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub items: Range<usize>,
    /// In content coordinates.
    pub top: f32,
    pub h: f32,
    /// One per item.
    pub widths: Vec<f32>,
}

impl Row {
    /// Horizontal centre of the `k`th cell, from the row's left edge.
    fn center(&self, k: usize, gap: f32) -> f32 {
        self.widths[..k].iter().map(|w| w + gap).sum::<f32>() + self.widths[k] / 2.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub rows: Vec<Row>,
    pub margins: Margins,
}

impl Grid {
    /// The grid of `len` photos in an area `width` wide. `aspect(i)` is photo `i`'s width
    /// / height (used by the justified layout only).
    pub fn new(layout: Layout, width: f32, names: bool, len: usize, aspect: impl Fn(usize) -> f32) -> Grid {
        let m = layout.margins();
        let width = width - 2.0 * m.pad - m.scrollbar;
        let pad = 2.0 * m.cell;
        let label = if names && layout.label_below() { LABEL } else { 0.0 };
        // Items, image height, outer widths.
        let lines: Vec<(Range<usize>, f32, Vec<f32>)> = match layout {
            Layout::Justified => {
                let aspects: Vec<f32> = (0..len).map(aspect).collect();
                // Cells are images plus padding: count the padding as part of the gaps.
                let rows = justified::rows(&aspects, width - pad, layout.side(), m.gap + pad);
                let full = rows.len().saturating_sub(1);
                rows.into_iter()
                    .enumerate()
                    .map(|(k, r)| {
                        let w = |i: usize| (justified::clamp(aspects[i]) * r.height).floor() + pad;
                        let mut widths: Vec<f32> = r.items.clone().map(w).collect();
                        // Widths are whole pixels: the last photo of a full row takes the rest,
                        // so the row ends exactly at the edge.
                        if k < full {
                            let n = widths.len();
                            let rest = width - widths[..n - 1].iter().sum::<f32>() - (n - 1) as f32 * m.gap;
                            widths[n - 1] = rest.floor();
                        }
                        (r.items, r.height.floor(), widths)
                    })
                    .collect()
            }
            Layout::Small | Layout::Big => {
                let (side, cols) = (layout.side(), columns(width, layout.side() + pad, m.gap));
                let row = |start: usize| {
                    let items = start..(start + cols).min(len);
                    let widths = vec![side + pad; items.len()];
                    (items, side, widths)
                };
                (0..len).step_by(cols).map(row).collect()
            }
        };
        let mut top = m.pad;
        let rows = lines
            .into_iter()
            .map(|(items, image_h, widths)| {
                let row = Row { items, top, h: image_h + pad + label, widths };
                top += row.h + m.gap;
                row
            })
            .collect();
        Grid { rows, margins: m }
    }

    /// The row holding photo `index`.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        let r = self.rows.partition_point(|r| r.items.end <= index);
        (r < self.rows.len()).then_some(r)
    }

    /// Indices of the photos in rows that overlap the viewport (`offset` = scroll position).
    pub fn visible(&self, offset: f32, height: f32) -> Range<usize> {
        let first = self.rows.partition_point(|r| r.top + r.h <= offset);
        let end = self.rows.partition_point(|r| r.top < offset + height);
        match (self.rows.get(first), end.checked_sub(1).and_then(|l| self.rows.get(l))) {
            (Some(a), Some(b)) if first < end => a.items.start..b.items.end,
            _ => 0..0,
        }
    }

    /// The scroll offset that brings photo `index`'s row fully into view, if it isn't.
    pub fn reveal(&self, index: usize, offset: f32, height: f32) -> Option<f32> {
        let row = &self.rows[self.row_of(index)?];
        let (top, bottom) = (row.top - self.margins.pad, row.top + row.h + self.margins.pad);
        if top < offset {
            Some(top.max(0.0))
        } else if bottom > offset + height {
            Some((bottom - height).max(0.0))
        } else {
            None
        }
    }

    /// The photo in the row above (`down` false) or below `index` that is closest
    /// horizontally.
    pub fn vertical(&self, index: usize, down: bool) -> Option<usize> {
        let r = self.row_of(index)?;
        let row = &self.rows[r];
        let target = self.rows.get(if down { r + 1 } else { r.checked_sub(1)? })?;
        let gap = self.margins.gap;
        let x = row.center(index - row.items.start, gap);
        let distance = |k: &usize| (target.center(*k, gap) - x).abs();
        let k = (0..target.widths.len()).min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
        Some(target.items.start + k)
    }
}

/// Cells `cell_w` wide, `gap` apart, that fit `width` (at least one).
fn columns(width: f32, cell_w: f32, gap: f32) -> usize {
    (((width + gap) / (cell_w + gap)).floor() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: f32 = CELLS.pad;
    const GAP: f32 = CELLS.gap;
    const CELL_PAD: f32 = CELLS.cell;

    /// Body width for `n` columns of `layout`.
    fn columns_width(n: usize, layout: Layout) -> f32 {
        2.0 * PAD + CELLS.scrollbar + n as f32 * (layout.side() + 2.0 * CELL_PAD + GAP) - GAP
    }

    fn fixed(layout: Layout, cols: usize, names: bool, len: usize) -> Grid {
        Grid::new(layout, columns_width(cols, layout), names, len, |_| 1.0)
    }

    #[test]
    fn columns_fit_the_width() {
        let cols = |width| Grid::new(Layout::Small, width, false, 100, |_| 1.0).rows[0].items.len();
        assert_eq!(cols(100.0), 1);
        let five = columns_width(5, Layout::Small);
        assert_eq!((cols(five - 1.0), cols(five)), (4, 5));
        let big = fixed(Layout::Big, 2, true, 3);
        let small = fixed(Layout::Small, 2, true, 3);
        assert_eq!(big.rows[0].widths, [372.0, 372.0]);
        assert_eq!(big.rows[1].items, 2..3, "short last row");
        assert_eq!(big.rows[0].h - 360.0, small.rows[0].h - 180.0);
    }

    #[test]
    fn visible_rows_and_reveal() {
        for (layout, names) in [(Layout::Small, false), (Layout::Small, true), (Layout::Big, true)] {
            let g = fixed(layout, 3, names, 100);
            let (c, h) = (g.rows[0].h, 2.0 * (g.rows[0].h + GAP));
            let top = |r: usize| g.rows[r].top;
            assert_eq!(top(1), PAD + c + GAP);
            assert_eq!(g.visible(0.0, h), 0..6, "rows 0 and 1");
            assert_eq!(g.visible(top(1), h), 3..9, "rows 1 and 2");
            assert_eq!(fixed(layout, 3, names, 4).visible(0.0, h), 0..4);
            assert_eq!(Grid::default().visible(0.0, h), 0..0);
            assert_eq!(g.reveal(0, 0.0, h), None);
            assert_eq!(g.reveal(15, 0.0, h), Some(top(5) + c + PAD - h));
            assert_eq!(g.reveal(3, top(3), h), Some(top(1) - PAD));
            assert_eq!(g.reveal(6, top(2) - PAD, h), None);
        }
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let g = fixed(Layout::Small, 4, false, 9); // rows 0–3, 4–7, 8
        assert_eq!(g.vertical(1, true), Some(5));
        assert_eq!(g.vertical(7, true), Some(8), "into the short last row");
        assert_eq!(g.vertical(8, true), None);
        assert_eq!(g.vertical(8, false), Some(4));
        assert_eq!(g.vertical(2, false), None);
    }

    #[test]
    fn justified_rows_fill_the_width() {
        let aspects = [1.5, 0.67, 1.5, 1.0, 2.5, 1.5, 0.75, 1.33, 1.5, 1.5];
        let g = Grid::new(Layout::Justified, 1200.0, false, aspects.len(), |i| aspects[i]);
        let (last, full) = g.rows.split_last().unwrap();
        let m = THIN;
        assert_eq!(g.rows[0].top, m.pad);
        for (r, next) in full.iter().zip(&g.rows[1..]) {
            let inner = 1200.0 - 2.0 * m.pad - SCROLLBAR - (r.widths.len() - 1) as f32 * m.gap;
            assert_eq!(r.widths.iter().sum::<f32>(), inner, "{r:?}");
            assert_eq!(r.top + r.h + m.gap, next.top);
            for (i, cw) in r.items.clone().zip(&r.widths) {
                assert!((cw / r.h - aspects[i]).abs() < 0.02, "photo {i}: {}", cw / r.h);
            }
        }
        assert_eq!(last.h, 360.0, "the last row isn't stretched");
        // Names go over the photos: rows don't grow.
        assert_eq!(Grid::new(Layout::Justified, 1200.0, true, aspects.len(), |i| aspects[i]), g);
        // Down goes to the photo below, wherever it is.
        let (a, b) = (&g.rows[0], &g.rows[1]);
        let below = g.vertical(a.items.end - 1, true).unwrap();
        assert!(b.items.contains(&below) && below > b.items.start);
        assert_eq!(g.vertical(b.items.start, false), Some(0));
    }
}
