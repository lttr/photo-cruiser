//! Scheduling of background decodes for a browser over a list of photos.
//!
//! "Ignore stale results" alone is not enough: every queued decode still runs and the UI
//! falls behind on a held arrow key. So: at most one decode in flight, always for the
//! most wanted missing index *at the time the previous one finished* (current, then
//! current + 1, then current − 1). A generation counter drops results from an old folder.
//! The loader does no I/O itself; the caller runs each [`Job`] wherever it likes and
//! reports back with [`Loader::finish`].

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub generation: u64,
    pub index: usize,
}

#[derive(Debug)]
pub struct Loader<T> {
    generation: u64,
    cache: HashMap<usize, T>,
    in_flight: Option<Job>,
}

impl<T> Default for Loader<T> {
    fn default() -> Self {
        Loader { generation: 0, cache: HashMap::new(), in_flight: None }
    }
}

impl<T> Loader<T> {
    /// Forget everything (new folder). Results of older jobs will be ignored.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.cache.clear();
        self.in_flight = None;
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.cache.get(&index)
    }

    pub fn in_flight(&self) -> Option<Job> {
        self.in_flight
    }

    /// Drop everything outside `current ± 1` and return the next decode to start, if
    /// none is running and something nearby is missing.
    pub fn next_job(&mut self, current: usize, len: usize) -> Option<Job> {
        let keep = current.saturating_sub(1)..=current.saturating_add(1);
        self.cache.retain(|i, _| keep.contains(i));
        if self.in_flight.is_some() {
            return None;
        }
        let index = wanted(current).find(|i| *i < len && !self.cache.contains_key(i))?;
        let job = Job { generation: self.generation, index };
        self.in_flight = Some(job);
        Some(job)
    }

    /// Store a finished job's result. Returns false (and drops it) if the job is stale.
    pub fn finish(&mut self, job: Job, result: T) -> bool {
        if job.generation != self.generation {
            return false;
        }
        if self.in_flight == Some(job) {
            self.in_flight = None;
        }
        self.cache.insert(job.index, result);
        true
    }

    /// Nothing running and everything around `current` is loaded.
    pub fn is_idle(&self, current: usize, len: usize) -> bool {
        self.in_flight.is_none() && wanted(current).all(|i| i >= len || self.cache.contains_key(&i))
    }
}

fn wanted(current: usize) -> impl Iterator<Item = usize> {
    [Some(current), current.checked_add(1), current.checked_sub(1)].into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_in_flight_latest_wins() {
        let mut l = Loader::<&str>::default();
        let j0 = l.next_job(0, 10).unwrap();
        assert_eq!(j0.index, 0);
        // User holds → while 0 decodes: no new jobs pile up.
        for cur in 1..=5 {
            assert_eq!(l.next_job(cur, 10), None);
        }
        assert!(l.finish(j0, "zero"));
        // The next job targets where the user is *now*; 0 is out of the window.
        let j = l.next_job(5, 10).unwrap();
        assert_eq!(j.index, 5);
        assert!(l.get(0).is_none());
        l.finish(j, "five");
        assert_eq!(l.next_job(5, 10).unwrap().index, 6);
    }

    #[test]
    fn preload_order_and_idle() {
        let mut l = Loader::<u8>::default();
        let mut order = vec![];
        while let Some(j) = l.next_job(3, 5) {
            order.push(j.index);
            l.finish(j, 0);
        }
        assert_eq!(order, [3, 4, 2]);
        assert!(l.is_idle(3, 5));
        assert!(l.is_idle(4, 5));
        assert!(!l.is_idle(1, 5));
        assert!(l.is_idle(0, 0));
    }

    #[test]
    fn stale_generation_dropped() {
        let mut l = Loader::<u8>::default();
        let old = l.next_job(0, 3).unwrap();
        l.reset();
        let new = l.next_job(0, 3).unwrap();
        assert!(!l.finish(old, 1));
        assert!(l.get(0).is_none());
        assert!(l.finish(new, 2));
        assert_eq!(l.get(0), Some(&2));
    }
}
