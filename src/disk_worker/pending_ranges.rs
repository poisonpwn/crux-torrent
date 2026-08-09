use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tokio_util::bytes::Bytes;

struct PendingChunk {
    data: Bytes,
    arrival: Instant,
}

// contiguous run of bytes that can be flushed to a file at an offset using normal sequential io
// storing an arrival time for checking staleness later.
pub struct Run {
    pub start: usize,
    chunks: Vec<Bytes>,
    oldest_arrival: Instant,
}

impl Run {
    pub fn chunks(&self) -> &[Bytes] {
        &self.chunks
    }

    pub fn len(&self) -> usize {
        self.chunks.iter().map(Bytes::len).sum()
    }

    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn age(&self) -> Duration {
        self.oldest_arrival.elapsed()
    }
}

// summary of a run found while scanning left to right on the pending chunks,
// based on the summary found, the largest/oldest of group can be flushed
struct GroupSummary {
    start: usize,
    end: usize,
    len: usize,
    oldest_arrival: Instant,
}

type StartOffset = usize;

pub struct PendingRanges {
    chunks: BTreeMap<StartOffset, PendingChunk>,
}

impl PendingRanges {
    pub fn new() -> Self {
        Self {
            chunks: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, offset: usize, data: Bytes) {
        let end = offset + data.len();
        debug_assert!(end > offset, "inserted an empty chunk into PendingRanges");

        debug_assert!(
            self.chunks
                .range(..offset)
                .next_back()
                .is_none_or(|(&prev_start, prev)| prev_start + prev.data.len() <= offset),
            "inserted a chunk at [{offset}, {end}) that overlaps its predecessor \
             (the same bytes must have been routed to this file twice)"
        );
        debug_assert!(
            self.chunks
                .range(offset..)
                .next()
                .is_none_or(|(&next_start, _)| next_start >= end),
            "inserted a chunk at [{offset}, {end}) that overlaps its successor \
             (the same bytes must have been routed to this file twice)"
        );

        self.chunks.insert(
            offset,
            PendingChunk {
                data,
                arrival: Instant::now(),
            },
        );
    }

    pub fn take_largest(&mut self) -> Option<Run> {
        let group = self.scan_groups().into_iter().max_by_key(|g| g.len)?;
        Some(self.extract(group))
    }

    /// convenience method for extracting the largest one if it's bigger than the threshold
    /// provided.
    pub fn take_largest_if_at_least(&mut self, min_len: usize) -> Option<Run> {
        let group = self.scan_groups().into_iter().max_by_key(|g| g.len)?;
        if group.len < min_len {
            return None;
        }
        Some(self.extract(group))
    }

    pub fn take_older_than(&mut self, max_age: Duration) -> Option<Run> {
        let now = Instant::now();
        let group = self
            .scan_groups()
            .into_iter()
            .find(|g| now.duration_since(g.oldest_arrival) >= max_age)?;
        Some(self.extract(group))
    }

    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    #[allow(unused)]
    pub fn total_pending_bytes(&self) -> usize {
        self.chunks.values().map(|c| c.data.len()).sum()
    }

    /// scan the pending chunks and create the group summaries for ranges without holes
    fn scan_groups(&self) -> Vec<GroupSummary> {
        let mut groups: Vec<GroupSummary> = Vec::new();

        for (&offset, chunk) in &self.chunks {
            let end = offset + chunk.data.len();
            match groups.last_mut() {
                // merge with the previous group that touches this chunk
                Some(group) if group.end == offset => {
                    group.end = end;
                    group.len += chunk.data.len();
                    group.oldest_arrival = group.oldest_arrival.min(chunk.arrival);
                }
                // start a new group
                _ => groups.push(GroupSummary {
                    start: offset,
                    end,
                    len: chunk.data.len(),
                    oldest_arrival: chunk.arrival,
                }),
            }
        }

        groups
    }

    /// removes every chunk in `group`'s range from the map and assembles them into a [`Run`].
    fn extract(&mut self, group: GroupSummary) -> Run {
        let mut in_range = self.chunks.split_off(&group.start);
        let mut after = in_range.split_off(&group.end);
        self.chunks.append(&mut after);

        let chunks = in_range.into_values().map(|c| c.data).collect();

        Run {
            start: group.start,
            chunks,
            oldest_arrival: group.oldest_arrival,
        }
    }
}

impl Default for PendingRanges {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(byte: u8, len: usize) -> Bytes {
        Bytes::from(vec![byte; len])
    }

    fn run_contents(run: &Run) -> Vec<u8> {
        run.chunks().iter().flat_map(|b| b.to_vec()).collect()
    }

    #[test]
    fn insert_into_empty_creates_a_single_run() {
        let mut ranges = PendingRanges::new();
        ranges.insert(10, chunk(1, 10));

        let run = ranges.take_largest().unwrap();
        assert_eq!(run.start, 10);
        assert_eq!(run.len(), 10);
        assert!(ranges.is_empty());
    }

    #[test]
    fn non_touching_inserts_stay_separate_runs() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 10));
        ranges.insert(20, chunk(2, 10));

        assert_eq!(ranges.total_pending_bytes(), 20);

        // largest is a tie in size; either could come out first, but each must be a standalone
        // run of length 10 (i.e. they never merged).
        let first = ranges.take_largest().unwrap();
        assert_eq!(first.len(), 10);
        let second = ranges.take_largest().unwrap();
        assert_eq!(second.len(), 10);
        assert!(ranges.is_empty());
    }

    #[test]
    fn merges_with_predecessor() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 10));
        ranges.insert(10, chunk(2, 5)); // touches predecessor's end at 10

        let run = ranges.take_largest().unwrap();
        assert_eq!(run.start, 0);
        assert_eq!(run.len(), 15);
        assert_eq!(run_contents(&run), [vec![1; 10], vec![2; 5]].concat());
        assert!(ranges.is_empty());
    }

    #[test]
    fn merges_with_successor() {
        let mut ranges = PendingRanges::new();
        ranges.insert(10, chunk(1, 5));
        ranges.insert(0, chunk(2, 10)); // touches successor's start at 10

        let run = ranges.take_largest().unwrap();
        assert_eq!(run.start, 0);
        assert_eq!(run.len(), 15);
        assert_eq!(run_contents(&run), [vec![2; 10], vec![1; 5]].concat());
    }

    #[test]
    fn bridges_a_gap_between_two_runs() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 10));
        ranges.insert(20, chunk(3, 10));
        ranges.insert(10, chunk(2, 10)); // bridges both

        assert_eq!(ranges.total_pending_bytes(), 30);
        let run = ranges.take_largest().unwrap();
        assert_eq!(run.start, 0);
        assert_eq!(run.len(), 30);
        assert_eq!(
            run_contents(&run),
            [vec![1; 10], vec![2; 10], vec![3; 10]].concat()
        );
        assert!(ranges.is_empty());
    }

    #[test]
    fn take_largest_prefers_the_biggest_run() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 5)); // small, standalone run
        ranges.insert(100, chunk(2, 50)); // big, standalone run

        let run = ranges.take_largest().unwrap();
        assert_eq!(run.start, 100);
        assert_eq!(run.len(), 50);

        // the small run is still there afterwards.
        assert_eq!(ranges.total_pending_bytes(), 5);
    }

    #[test]
    fn take_largest_if_at_least_only_takes_when_big_enough() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 5));

        assert!(ranges.take_largest_if_at_least(6).is_none());
        assert_eq!(
            ranges.total_pending_bytes(),
            5,
            "must not remove a run it declined to take"
        );

        let run = ranges.take_largest_if_at_least(5).unwrap();
        assert_eq!(run.len(), 5);
        assert!(ranges.is_empty());
    }

    #[test]
    fn take_largest_on_empty_map_is_none() {
        let mut ranges = PendingRanges::new();
        assert!(ranges.take_largest().is_none());
    }

    #[test]
    fn take_older_than_only_returns_stale_runs() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 5));
        std::thread::sleep(Duration::from_millis(20));
        ranges.insert(100, chunk(2, 5));

        // only the first run is older than 15ms at this point.
        let run = ranges.take_older_than(Duration::from_millis(15)).unwrap();
        assert_eq!(run.start, 0);
        assert_eq!(run.len(), 5);

        assert!(ranges.take_older_than(Duration::from_millis(15)).is_none());
    }

    #[test]
    fn merging_keeps_the_earliest_arrival_time_of_the_group() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 5)); // old
        std::thread::sleep(Duration::from_millis(20));
        ranges.insert(5, chunk(2, 5)); // fresh, merges with the old run

        // the merged run's age should still be dominated by the older chunk.
        let run = ranges.take_older_than(Duration::from_millis(15)).unwrap();
        assert_eq!(run.start, 0);
        assert_eq!(run.len(), 10);
    }

    #[test]
    fn extracting_a_run_leaves_the_rest_of_the_map_intact() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 10));
        ranges.insert(10, chunk(2, 10)); // merges with the first -> run [0, 20)
        ranges.insert(50, chunk(3, 10)); // separate run

        let run = ranges.take_largest().unwrap(); // takes [0, 20)
        assert_eq!(run.start, 0);
        assert_eq!(run.len(), 20);

        // the untouched run at 50 must still be there, and must not have been disturbed by the
        // split/append used to remove the [0, 20) run.
        assert_eq!(ranges.total_pending_bytes(), 10);
        let remaining = ranges.take_largest().unwrap();
        assert_eq!(remaining.start, 50);
        assert_eq!(remaining.len(), 10);
    }

    #[test]
    #[should_panic]
    fn overlapping_insert_panics_in_debug_builds() {
        let mut ranges = PendingRanges::new();
        ranges.insert(0, chunk(1, 10));
        ranges.insert(5, chunk(2, 10)); // overlaps [0, 10) at bytes [5, 10)
    }
}
