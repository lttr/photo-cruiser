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

use std::collections::HashMap;

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
}
