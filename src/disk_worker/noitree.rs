use std::collections::BTreeMap;
use tokio::time::Instant;

struct NOITreeMap<T: Merge> {
    pub(super) inner: BTreeMap<(u32, u32), T>,
}

impl<T: Merge + Default> NOITreeMap<T> {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    pub fn insert_merge_touching(&mut self, interval: (u32, u32), element: <T as Merge>::Inner) {
        let (mut qs, mut qe) = interval;
        debug_assert!(qe > qs);
        const MESG: &str = "interval item disappeared between check and fetch-remove";

        // panic if asked to insert an overlapping interval
        debug_assert!(!self.inner.keys().any(|&(is, ie)| (is < qs && ie > qs)
            || (is < qe && ie > qe)
            || (is >= qs && ie <= qe)));

        let mut items = match self.inner.range(..=(qs, u32::MAX)).next_back() {
            Some((&(ps, pe), _)) if pe == qs => {
                qs = qs.min(ps);
                self.inner.remove_entry(&(ps, pe)).expect(MESG).1
            }
            _ => T::default(),
        };

        items.push(element);

        if let Some((&(ns, ne), _)) = self.inner.range((qe, 0)..).next() {
            qe = qe.max(ne);
            items.merge(self.inner.remove_entry(&(ns, ne)).expect(MESG).1);
        }

        self.inner.insert((qs, qe), items);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&(u32, u32), &T)> {
        self.inner.iter()
    }

    /// only removes exact ranges, it's just to expose the inner remove method.
    pub fn remove_range(&mut self, interval: impl AsRef<(u32, u32)>) -> Option<((u32, u32), T)> {
        self.inner.remove_entry(interval.as_ref())
    }
}

pub struct SliceEntry<T> {
    pub entry_time: Instant,
    pub data: Vec<T>,
}

pub trait Merge {
    type Inner;
    fn merge(&mut self, other: Self);
    fn push(&mut self, inner: Self::Inner);
}

impl<T> Merge for SliceEntry<T> {
    type Inner = T;

    fn merge(&mut self, other: Self) {
        self.entry_time = self.entry_time.min(other.entry_time);
        self.data.extend(other.data);
    }

    fn push(&mut self, element: Self::Inner) {
        self.data.push(element);
    }
}

impl<T> Default for SliceEntry<T> {
    fn default() -> Self {
        Self {
            entry_time: Instant::now(),
            data: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<T> Merge for Vec<T> {
        type Inner = T;
        fn merge(&mut self, other: Self) {
            self.extend(other);
        }

        fn push(&mut self, inner: Self::Inner) {
            self.push(inner);
        }
    }

    #[test]
    fn insert_into_empty_creates_interval() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((10, 20), 1);

        assert!(m.inner.contains_key(&(10, 20)));
        assert_eq!(m.inner.get(&(10, 20)).unwrap().as_slice(), &[1]);
    }

    #[test]
    fn insert_non_touching_creates_second_interval() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((0, 10), 1);
        m.insert_merge_touching((20, 30), 2);

        assert_eq!(m.inner.len(), 2);
        assert_eq!(m.inner.get(&(0, 10)).unwrap().as_slice(), &[1]);
        assert_eq!(m.inner.get(&(20, 30)).unwrap().as_slice(), &[2]);
    }

    #[test]
    fn merge_with_predecessor_only() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((0, 10), 1);
        m.insert_merge_touching((10, 20), 2); // touches predecessor at 10

        assert_eq!(m.inner.len(), 1);
        assert!(m.inner.contains_key(&(0, 20)));
        // Order: old predecessor items then new element (based on typical implementation)
        assert_eq!(m.inner.get(&(0, 20)).unwrap().as_slice(), &[1, 2]);
    }

    #[test]
    fn merge_with_successor_only() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((10, 20), 1);
        m.insert_merge_touching((0, 10), 2); // touches successor at 10

        assert_eq!(m.inner.len(), 1);
        assert!(m.inner.contains_key(&(0, 20)));
        // New element inserted into newly created/merged vec before successor items (depends on your code)
        assert_eq!(m.inner.get(&(0, 20)).unwrap().as_slice(), &[2, 1]);
    }

    #[test]
    fn merge_both_sides_bridge_gap() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((0, 10), 1);
        m.insert_merge_touching((20, 30), 3);
        m.insert_merge_touching((10, 20), 2); // bridges both, should become (0,30)

        assert_eq!(m.inner.len(), 1);
        assert!(m.inner.contains_key(&(0, 30)));
        assert_eq!(m.inner.get(&(0, 30)).unwrap().as_slice(), &[1, 2, 3]);
    }

    #[test]
    #[should_panic]
    fn overlap_with_predecessor_panics() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((0, 10), 1);
        m.insert_merge_touching((9, 20), 2); // overlaps: predecessor end 10 > qs 9
        dbg!(m.inner);
    }

    #[test]
    #[should_panic]
    fn overlap_with_successor_panics() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((10, 20), 1);
        m.insert_merge_touching((0, 11), 2); // overlaps successor: succ_start 10 < qe 11
        dbg!(m.inner);
    }

    #[test]
    fn touching_is_not_overlap_half_open() {
        let mut m: NOITreeMap<Vec<_>> = NOITreeMap::new();
        m.insert_merge_touching((0, 1), 1);
        m.insert_merge_touching((1, 2), 2); // touching at 1 is valid half-open adjacency

        assert_eq!(m.inner.len(), 1);
        assert!(m.inner.contains_key(&(0, 2)));
        assert_eq!(m.inner.get(&(0, 2)).unwrap().as_slice(), &[1, 2]);
    }
}
