//! Geometry of the photo grid: fixed-size cells in rows, as many columns as fit the
//! width. Shared by the view (layout) and the state (row moves, scrolling, which
//! thumbnails are visible), so both agree on where every photo is.

use iced::Size;
use std::ops::Range;

/// Longest side of a decoded thumbnail (about 2× the box, for HiDPI screens).
pub const THUMB_SIDE: u32 = 256;
/// The square the thumbnail is fitted into.
pub const BOX: f32 = 180.0;
/// File name under the thumbnail (only while names are shown).
pub const LABEL: f32 = 18.0;
pub const CELL_PAD: f32 = 6.0;
pub const CELL_W: f32 = BOX + 2.0 * CELL_PAD;

/// Height of a cell, with or without the file name.
pub fn cell_h(names: bool) -> f32 {
    BOX + 2.0 * CELL_PAD + if names { LABEL } else { 0.0 }
}
/// Between cells.
pub const GAP: f32 = 6.0;
/// Around the grid.
pub const PAD: f32 = 16.0;
/// Kept free on the right for the scrollbar.
const SCROLLBAR: f32 = 12.0;

/// Columns that fit `width` (at least one).
pub fn columns(width: f32) -> usize {
    (((width - 2.0 * PAD - SCROLLBAR + GAP) / (CELL_W + GAP)).floor() as usize).max(1)
}

/// Top of `row` in content coordinates (cells `cell_h` high).
pub fn row_top(row: usize, cell_h: f32) -> f32 {
    PAD + row as f32 * (cell_h + GAP)
}

/// Indices of the photos in rows that overlap the viewport (`offset` = scroll position).
pub fn visible(body: Size, cell_h: f32, offset: f32, len: usize) -> Range<usize> {
    let cols = columns(body.width);
    let step = cell_h + GAP;
    let first = ((offset - PAD) / step).floor().max(0.0) as usize;
    let last = ((offset + body.height - PAD) / step).ceil().max(0.0) as usize;
    (first * cols).min(len)..(last * cols).min(len)
}

/// The scroll offset that brings `row` fully into view, if it isn't.
pub fn reveal(row: usize, cell_h: f32, offset: f32, height: f32) -> Option<f32> {
    let (top, bottom) = (row_top(row, cell_h) - PAD, row_top(row, cell_h) + cell_h + PAD);
    if top < offset {
        Some(top.max(0.0))
    } else if bottom > offset + height {
        Some((bottom - height).max(0.0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fit_the_width() {
        assert_eq!(columns(100.0), 1);
        let five = 2.0 * PAD + SCROLLBAR + 5.0 * CELL_W + 4.0 * GAP;
        assert_eq!((columns(five - 1.0), columns(five)), (4, 5));
    }

    #[test]
    fn visible_rows_and_reveal() {
        for c in [cell_h(false), cell_h(true)] {
            let body = Size::new(columns_width(3), 2.0 * (c + GAP));
            assert_eq!(visible(body, c, 0.0, 100), 0..6, "rows 0 and 1");
            assert_eq!(visible(body, c, row_top(1, c), 100), 3..9, "rows 1 and 2");
            assert_eq!(visible(body, c, 0.0, 4), 0..4);
            let h = body.height;
            assert_eq!(reveal(0, c, 0.0, h), None);
            assert_eq!(reveal(5, c, 0.0, h), Some(row_top(5, c) + c + PAD - h));
            assert_eq!(reveal(0, c, 500.0, h), Some(0.0));
            assert_eq!(reveal(3, c, row_top(3, c) - PAD, h), None);
        }
    }

    fn columns_width(n: usize) -> f32 {
        2.0 * PAD + SCROLLBAR + n as f32 * (CELL_W + GAP)
    }
}
