//! # ternary-antidote
//!
//! CRDTs for GPU cluster state with ternary merge outcomes.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome { Converged = 1, Pending = 0, Conflict = -1 }

// === G-Counter (grow-only) ===
#[derive(Debug, Clone)]
pub struct GCounter {
    counts: HashMap<String, u64>,
}

impl GCounter {
    pub fn new() -> Self { Self { counts: HashMap::new() } }
    pub fn increment(&mut self, node: &str, delta: u64) { *self.counts.entry(node.into()).or_insert(0) += delta; }
    pub fn value(&self) -> u64 { self.counts.values().sum() }
    pub fn merge(&mut self, other: &GCounter) -> MergeOutcome {
        for (k, &v) in &other.counts {
            let current = self.counts.entry(k.clone()).or_insert(0);
            *current = (*current).max(v);
        }
        MergeOutcome::Converged // G-Counter always converges
    }
}

impl Default for GCounter { fn default() -> Self { Self::new() } }

// === LWW-Register (last-writer-wins) ===
#[derive(Debug, Clone)]
pub struct LwwRegister<T: Clone> {
    value: T,
    timestamp: u64,
    node: String,
}

impl<T: Clone> LwwRegister<T> {
    pub fn new(value: T, timestamp: u64, node: &str) -> Self {
        Self { value, timestamp, node: node.into() }
    }

    pub fn set(&mut self, value: T, timestamp: u64, node: &str) {
        if timestamp >= self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
            self.node = node.into();
        }
    }

    pub fn get(&self) -> &T { &self.value }

    pub fn merge(&mut self, other: &LwwRegister<T>) -> MergeOutcome {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
            self.node = other.node.clone();
            MergeOutcome::Converged
        } else if other.timestamp == self.timestamp && other.node != self.node {
            MergeOutcome::Conflict // same timestamp, different nodes
        } else {
            MergeOutcome::Converged
        }
    }
}

// === OR-Set (observed-remove) ===
#[derive(Debug, Clone)]
pub struct OrSet<T: Clone + Eq + std::hash::Hash> {
    elements: HashMap<T, HashSet<(u64, String)>>, // element -> set of unique tags
    tombstones: HashSet<(u64, String)>,
}

impl<T: Clone + Eq + std::hash::Hash> OrSet<T> {
    pub fn new() -> Self { Self { elements: HashMap::new(), tombstones: HashSet::new() } }

    pub fn add(&mut self, element: T, tag: (u64, String)) {
        if !self.tombstones.contains(&tag) {
            self.elements.entry(element.clone()).or_default().insert(tag);
        }
    }

    pub fn remove(&mut self, element: &T) -> bool {
        if let Some(tags) = self.elements.remove(element) {
            for tag in tags { self.tombstones.insert(tag); }
            return true;
        }
        false
    }

    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains_key(element)
    }

    pub fn merge(&mut self, other: &OrSet<T>) -> MergeOutcome {
        self.tombstones.extend(other.tombstones.clone());
        // Remove tombstoned elements
        let tombstones = &self.tombstones;
        for tags in self.elements.values_mut() {
            tags.retain(|t| !tombstones.contains(t));
        }
        self.elements.retain(|_, tags| !tags.is_empty());
        // Add other's elements
        for (elem, tags) in &other.elements {
            let our_tags = self.elements.entry(elem.clone()).or_default();
            for tag in tags {
                if !tombstones.contains(tag) { our_tags.insert(tag.clone()); }
            }
        }
        MergeOutcome::Converged
    }

    pub fn len(&self) -> usize { self.elements.len() }
}

impl<T: Clone + Eq + std::hash::Hash> Default for OrSet<T> { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcounter_increment() {
        let mut c = GCounter::new();
        c.increment("a", 5);
        c.increment("b", 3);
        assert_eq!(c.value(), 8);
    }

    #[test]
    fn test_gcounter_merge() {
        let mut c1 = GCounter::new(); c1.increment("a", 5);
        let mut c2 = GCounter::new(); c2.increment("b", 3);
        assert_eq!(c1.merge(&c2), MergeOutcome::Converged);
        assert_eq!(c1.value(), 8);
    }

    #[test]
    fn test_lww_set() {
        let mut r = LwwRegister::new("old", 1, "a");
        r.set("new", 2, "b");
        assert_eq!(r.get(), &"new");
    }

    #[test]
    fn test_lww_old_timestamp_ignored() {
        let mut r = LwwRegister::new("new", 10, "a");
        r.set("old", 5, "b");
        assert_eq!(r.get(), &"new");
    }

    #[test]
    fn test_lww_merge_converge() {
        let mut r1 = LwwRegister::new("a", 1, "x");
        let r2 = LwwRegister::new("b", 2, "y");
        assert_eq!(r1.merge(&r2), MergeOutcome::Converged);
        assert_eq!(r1.get(), &"b");
    }

    #[test]
    fn test_lww_merge_conflict() {
        let mut r1 = LwwRegister::new("a", 5, "x");
        let r2 = LwwRegister::new("b", 5, "y");
        assert_eq!(r1.merge(&r2), MergeOutcome::Conflict);
    }

    #[test]
    fn test_orset_add_remove() {
        let mut s: OrSet<&str> = OrSet::new();
        s.add("x", (1, "a".into()));
        assert!(s.contains(&"x"));
        s.remove(&"x");
        assert!(!s.contains(&"x"));
    }

    #[test]
    fn test_orset_merge() {
        let mut s1: OrSet<&str> = OrSet::new();
        let mut s2: OrSet<&str> = OrSet::new();
        s1.add("a", (1, "x".into()));
        s2.add("b", (2, "y".into()));
        assert_eq!(s1.merge(&s2), MergeOutcome::Converged);
        assert_eq!(s1.len(), 2);
    }
}
