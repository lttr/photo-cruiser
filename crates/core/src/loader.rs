//! Scheduling of background decodes for a browser over a list of photos.
//!
//! "Ignore stale results" alone is not enough: every queued decode still runs and the UI
//! falls behind on a held arrow key. So jobs are never queued: a small, fixed number run
//! at once, and each free slot takes the most wanted missing item *at the time it frees
//! up*. Wanted, in order: a quick embedded preview of the current photo (own slot, so it
//! never waits behind full decodes), then full decodes of the current photo and its
//! neighbours, biased towards the browsing direction (+1, +2, −1, +3 when going forward).
//! A generation counter drops results from an old folder. The loader does no I/O itself;
//! the caller runs each [`Job`] wherever it likes and reports back with [`Loader::finish`].
//!
//! [`Thumbs`] does the same for the thumbnails of a grid: the visible ones first, then a
//! margin around them, with their own slots so they never hold up a full decode.
//! [`Sizes`] probes every photo's size (headers only, in chunks) for layouts that need
//! all aspect ratios up front.

use std::collections::HashMap;
use std::ops::Range;

/// Full decodes running at once. Two keep the current photo from waiting long behind a
/// stale neighbour on a held key, without starving the UI of CPU.
pub const FULL_SLOTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Small embedded thumbnail (EXIF / HEIF), shown until the full decode is ready.
    Preview,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub generation: u64,
    pub index: usize,
    pub kind: Kind,
}

#[derive(Debug)]
pub struct Loader<T> {
    generation: u64,
    cache: HashMap<(usize, Kind), T>,
    in_flight: Vec<Job>,
    /// +1 or −1: the last move between distinct indices.
    forward: bool,
    last: Option<usize>,
}

impl<T> Default for Loader<T> {
    fn default() -> Self {
        Loader { generation: 0, cache: HashMap::new(), in_flight: Vec::new(), forward: true, last: None }
    }
}

impl<T> Loader<T> {
    /// Forget everything (new folder). Results of older jobs will be ignored.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.cache.clear();
        self.in_flight.clear();
        self.forward = true;
        self.last = None;
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.cache.get(&(index, Kind::Full))
    }

    pub fn preview(&self, index: usize) -> Option<&T> {
        self.cache.get(&(index, Kind::Preview))
    }

    pub fn in_flight(&self) -> &[Job] {
        &self.in_flight
    }

    /// Drop everything outside the wanted window and return a decode to start, if a slot
    /// is free and something wanted is missing. Call until it returns `None`.
    pub fn next_job(&mut self, current: usize, len: usize) -> Option<Job> {
        if let Some(last) = self.last
            && last != current
        {
            self.forward = current > last;
        }
        self.last = Some(current);
        let wanted = self.wanted(current, len);
        self.cache.retain(|k, _| wanted.contains(k) || *k == (current, Kind::Preview));
        let busy = |kind| self.in_flight.iter().filter(|j| j.kind == kind).count();
        let (previews, fulls) = (busy(Kind::Preview), busy(Kind::Full));
        let (index, kind) = wanted.into_iter().find(|&(i, kind)| {
            let free = match kind {
                Kind::Preview => previews == 0,
                Kind::Full => fulls < FULL_SLOTS,
            };
            free && !self.cache.contains_key(&(i, kind))
                && !self.in_flight.iter().any(|j| (j.index, j.kind) == (i, kind))
        })?;
        let job = Job { generation: self.generation, index, kind };
        self.in_flight.push(job);
        Some(job)
    }

    /// Store a finished job's result. Returns false (and drops it) if the job is stale.
    pub fn finish(&mut self, job: Job, result: T) -> bool {
        if job.generation != self.generation {
            return false;
        }
        self.in_flight.retain(|j| *j != job);
        self.cache.insert((job.index, job.kind), result);
        true
    }

    /// Nothing running and every wanted full decode around `current` is loaded.
    pub fn is_idle(&self, current: usize, len: usize) -> bool {
        self.in_flight.is_empty()
            && self
                .wanted(current, len)
                .into_iter()
                .all(|k| k.1 == Kind::Preview || self.cache.contains_key(&k))
    }

    /// Wanted items, most wanted first.
    fn wanted(&self, current: usize, len: usize) -> Vec<(usize, Kind)> {
        let at = |steps: isize| {
            let d = if self.forward { steps } else { -steps };
            current.checked_add_signed(d).filter(|i| *i < len)
        };
        let preview = (current < len && !self.cache.contains_key(&(current, Kind::Full)))
            .then_some((current, Kind::Preview));
        let full = [0, 1, 2, -1, 3].into_iter().filter_map(at).map(|i| (i, Kind::Full));
        preview.into_iter().chain(full).collect()
    }
}

/// Thumbnail decodes running at once: all cores but two (for the UI and the photo
/// loader), between 2 and 8.
pub fn thumb_slots() -> usize {
    static SLOTS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SLOTS.get_or_init(|| {
        std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(2)).clamp(2, 8)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThumbJob {
    pub generation: u64,
    pub index: usize,
    /// Longest side to decode at.
    pub side: u32,
}

/// Thumbnails of one folder. Wanted: the visible indices in order, then outwards (one
/// below, one above, ...) for as many again on each side. Finished thumbnails are kept,
/// with the side they were decoded at, until [`Thumbs::reset`]; asking for a bigger side
/// decodes them again but keeps the smaller ones available meanwhile.
#[derive(Debug)]
pub struct Thumbs<T> {
    generation: u64,
    done: HashMap<usize, (u32, T)>,
    in_flight: Vec<usize>,
}

impl<T> Default for Thumbs<T> {
    fn default() -> Self {
        Thumbs { generation: 0, done: HashMap::new(), in_flight: Vec::new() }
    }
}

impl<T> Thumbs<T> {
    /// Forget everything (new folder). Results of older jobs will be ignored.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.done.clear();
        self.in_flight.clear();
    }

    /// The thumbnail of `index`, whatever its side.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.done.get(&index).map(|(_, t)| t)
    }

    /// Decoded at `side` or bigger.
    fn has(&self, index: usize, side: u32) -> bool {
        self.done.get(&index).is_some_and(|(s, _)| *s >= side)
    }

    /// A thumbnail to decode at `side`, if a slot is free and something wanted is missing
    /// (or smaller). Call until it returns `None`.
    pub fn next_job(&mut self, visible: Range<usize>, len: usize, side: u32) -> Option<ThumbJob> {
        if self.in_flight.len() >= thumb_slots() {
            return None;
        }
        let index = wanted(visible, len).find(|i| !self.has(*i, side) && !self.in_flight.contains(i))?;
        self.in_flight.push(index);
        Some(ThumbJob { generation: self.generation, index, side })
    }

    /// Store a finished job's result. Returns false (and drops it) if the job is stale.
    pub fn finish(&mut self, job: ThumbJob, result: T) -> bool {
        if job.generation != self.generation {
            return false;
        }
        self.in_flight.retain(|i| *i != job.index);
        self.done.insert(job.index, (job.side, result));
        true
    }

    /// Nothing running and every visible thumbnail is done at `side`.
    pub fn is_idle(&self, visible: Range<usize>, len: usize, side: u32) -> bool {
        self.in_flight.is_empty() && (visible.start..visible.end.min(len)).all(|i| self.has(i, side))
    }
}

fn wanted(visible: Range<usize>, len: usize) -> impl Iterator<Item = usize> {
    let (start, end) = (visible.start.min(len), visible.end.min(len));
    let margin = end - start;
    let around = (0..margin).flat_map(move |k| [end.checked_add(k), start.checked_sub(k + 1)]);
    (start..end).chain(around.flatten().filter(move |i| *i < len))
}

/// Size probes running at once (each a chunk of [`SIZE_CHUNK`] photos).
pub const SIZE_SLOTS: usize = 2;
/// Photos per size probe job.
pub const SIZE_CHUNK: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeJob {
    pub generation: u64,
    pub items: Range<usize>,
}

/// Displayed sizes (width, height) of the photos of one folder, probed from the start in
/// chunks. A size can also be filled in early (from a decoded thumbnail). Unreadable
/// photos stay `None`.
#[derive(Debug, Default)]
pub struct Sizes {
    generation: u64,
    sizes: Vec<Option<(u32, u32)>>,
    /// Start of the first chunk not yet handed out.
    next: usize,
    in_flight: usize,
}

impl Sizes {
    /// Forget everything (new folder of `len` photos). Results of older jobs will be ignored.
    pub fn reset(&mut self, len: usize) {
        self.generation += 1;
        self.sizes = vec![None; len];
        (self.next, self.in_flight) = (0, 0);
    }

    pub fn get(&self, index: usize) -> Option<(u32, u32)> {
        self.sizes.get(index).copied().flatten()
    }

    pub fn set(&mut self, index: usize, size: (u32, u32)) {
        if let Some(s) = self.sizes.get_mut(index) {
            *s = Some(size);
        }
    }

    /// The next chunk to probe, if a slot is free and photos are left. Call until `None`.
    pub fn next_job(&mut self) -> Option<SizeJob> {
        if self.in_flight >= SIZE_SLOTS || self.next >= self.sizes.len() {
            return None;
        }
        let items = self.next..(self.next + SIZE_CHUNK).min(self.sizes.len());
        (self.next, self.in_flight) = (items.end, self.in_flight + 1);
        Some(SizeJob { generation: self.generation, items })
    }

    /// Store a finished chunk (one result per item; `None` = unreadable, a size already
    /// known stays). Returns false (and drops it) if the job is stale.
    pub fn finish(&mut self, job: &SizeJob, results: &[Option<(u32, u32)>]) -> bool {
        if job.generation != self.generation {
            return false;
        }
        self.in_flight -= 1;
        for (i, r) in job.items.clone().zip(results) {
            if let Some(size) = r {
                self.sizes[i] = Some(*size);
            }
        }
        true
    }

    /// Every chunk probed.
    pub fn is_done(&self) -> bool {
        self.next >= self.sizes.len() && self.in_flight == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run every job the loader hands out, finishing them in order, while at `current`.
    fn drain(l: &mut Loader<&'static str>, current: usize, len: usize) -> Vec<(usize, Kind)> {
        let mut order = vec![];
        while let Some(j) = l.next_job(current, len) {
            order.push((j.index, j.kind));
            l.finish(j, "x");
        }
        order
    }

    #[test]
    fn preview_first_then_direction_biased_window() {
        use Kind::*;
        let mut l = Loader::default();
        assert_eq!(
            drain(&mut l, 3, 10),
            [(3, Preview), (3, Full), (4, Full), (5, Full), (2, Full), (6, Full)]
        );
        assert!(l.is_idle(3, 10));
        // Step back: now biased backwards; 2..=0 wanted, 3 kept (one behind), 4.. dropped.
        assert_eq!(drain(&mut l, 2, 10), [(1, Full), (0, Full)]);
        assert!(l.get(3).is_some() && l.get(5).is_none() && l.get(6).is_none());
        assert!(l.is_idle(2, 10));
        // Clamped at the ends.
        let mut l = Loader::default();
        assert_eq!(drain(&mut l, 0, 2), [(0, Preview), (0, Full), (1, Full)]);
        assert!(l.is_idle(0, 2) && l.is_idle(0, 0));
    }

    #[test]
    fn slots_limit_and_latest_wins() {
        let mut l = Loader::<&str>::default();
        let p0 = l.next_job(0, 10).unwrap();
        let f0 = l.next_job(0, 10).unwrap();
        let f1 = l.next_job(0, 10).unwrap();
        assert_eq!([p0.kind, f0.kind, f1.kind], [Kind::Preview, Kind::Full, Kind::Full]);
        assert_eq!(l.next_job(0, 10), None, "all slots busy");
        // User holds → while those decode, no new jobs pile up.
        for cur in 1..=5 {
            assert_eq!(l.next_job(cur, 10), None);
        }
        // The preview slot frees first: its next job is the preview of where the user is *now*.
        assert!(l.finish(p0, "p0"));
        let p5 = l.next_job(5, 10).unwrap();
        assert_eq!((p5.index, p5.kind), (5, Kind::Preview));
        assert!(l.preview(0).is_none(), "out of the window");
        l.finish(f0, "zero");
        assert_eq!(l.next_job(5, 10).unwrap().index, 5);
        assert!(l.get(0).is_none());
        assert!(!l.is_idle(5, 10));
    }

    #[test]
    fn thumbs_visible_first_then_around() {
        let mut t = Thumbs::default();
        let mut order = vec![];
        while let Some(j) = t.next_job(4..6, 9, 256) {
            order.push(j.index);
            t.finish(j, ());
        }
        assert_eq!(order, [4, 5, 6, 3, 7, 2]);
        assert!(t.is_idle(4..6, 9, 256) && !t.is_idle(0..2, 9, 256));
        // Clamped to the folder.
        assert_eq!(wanted(0..20, 3).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn thumbs_slots_and_stale() {
        let mut t = Thumbs::<u8>::default();
        let jobs: Vec<_> = std::iter::from_fn(|| t.next_job(0..10, 10, 256)).collect();
        assert_eq!(jobs.len(), thumb_slots());
        assert!(!t.is_idle(0..1, 10, 256));
        t.reset();
        assert!(!t.finish(jobs[0], 1));
        assert!(t.get(0).is_none());
        let j = t.next_job(0..1, 10, 256).unwrap();
        assert!(t.finish(j, 2));
        assert_eq!(t.get(0), Some(&2));
    }

    #[test]
    fn thumbs_bigger_side_redecodes() {
        let mut t = Thumbs::<u32>::default();
        let j = t.next_job(0..1, 1, 256).unwrap();
        t.finish(j, 256);
        assert!(t.is_idle(0..1, 1, 256) && !t.is_idle(0..1, 1, 512));
        // Bigger: decoded again, the small one is shown meanwhile.
        let j = t.next_job(0..1, 1, 512).unwrap();
        assert_eq!((j.index, j.side, t.get(0)), (0, 512, Some(&256)));
        t.finish(j, 512);
        // Smaller again: the big one will do.
        assert!(t.next_job(0..1, 1, 256).is_none());
        assert_eq!(t.get(0), Some(&512));
    }

    #[test]
    fn stale_generation_dropped() {
        let mut l = Loader::<u8>::default();
        let old = l.next_job(0, 3).unwrap();
        l.reset();
        let new = l.next_job(0, 3).unwrap();
        assert!(!l.finish(old, 1));
        assert!(l.preview(0).is_none());
        assert!(l.finish(new, 2));
        assert_eq!(l.preview(0), Some(&2));
    }

    #[test]
    fn sizes_in_chunks_stale_dropped() {
        let mut s = Sizes::default();
        s.reset(SIZE_CHUNK + 5);
        s.set(1, (3, 2));
        let a = s.next_job().unwrap();
        let b = s.next_job().unwrap();
        assert_eq!((a.items.clone(), b.items.clone()), (0..SIZE_CHUNK, SIZE_CHUNK..SIZE_CHUNK + 5));
        assert_eq!(s.next_job(), None);
        assert!(!s.is_done());
        let mut results = vec![Some((4, 3)); SIZE_CHUNK];
        results[1] = None;
        assert!(s.finish(&a, &results));
        assert_eq!((s.get(0), s.get(1), s.get(SIZE_CHUNK)), (Some((4, 3)), Some((3, 2)), None));
        s.reset(2);
        assert!(!s.finish(&b, &[Some((1, 1)); 5]), "stale");
        assert_eq!(s.get(0), None);
        let c = s.next_job().unwrap();
        assert!(s.finish(&c, &[None, Some((1, 1))]));
        assert!(s.is_done() && s.next_job().is_none());
    }
}
