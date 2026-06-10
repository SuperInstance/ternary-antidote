# ternary-antidote

**CRDTs where merge tells you what happened: converged, pending, or conflict.**

Distributed state is hard. When two GPU nodes independently modify the same piece of cluster state — a counter, a register, a set — they need to reconcile without a central authority. CRDTs (Conflict-free Replicated Data Types) solve this by guaranteeing that merges always converge.

But "converged" isn't the whole story. Sometimes the merge is trivial (both nodes had the same value). Sometimes it's interesting (different values, resolved by rule). Sometimes it reveals a genuine conflict that needs human attention. Traditional CRDTs return a merged value and leave you guessing which case happened.

This crate returns a `MergeOutcome` from every merge: **Converged (+1)**, **Pending (0)**, or **Conflict (-1)**. You know immediately whether the merge was clean or needs intervention.

## The Insight

In a GPU cluster managing thousands of state updates per second, you don't have time to inspect every merge. You need the merge function to classify its own outcome. A ternary return value does exactly that:

- **+1 Converged** — both replicas agreed, or the merge rule resolved cleanly. No action needed.
- **0 Pending** — the merge is incomplete, waiting for more replicas. Keep collecting.
- **-1 Conflict** — two replicas disagree and the resolution is ambiguous. Flag for review.

This turns a silent merge into an actionable signal. Your monitoring dashboard shows "+1 everywhere" (healthy) or "scattered -1s" (investigate).

## Quick Start

```toml
[dependencies]
ternary-antidote = "0.1.0"
```

```rust
use ternary_antidote::*;

// === G-Counter (grow-only counter) ===
let mut c1 = GCounter::new();
let mut c2 = GCounter::new();
c1.increment("gpu-0", 100);
c1.increment("gpu-0", 50);
c2.increment("gpu-1", 200);

let outcome = c1.merge(&c2);
assert_eq!(outcome, MergeOutcome::Converged); // G-Counter always converges
assert_eq!(c1.value(), 350); // max per node, sum across nodes

// === LWW-Register (last-writer-wins) ===
let mut r1 = LwwRegister::new("config_v1", 1, "gpu-0");
let mut r2 = LwwRegister::new("config_v2", 2, "gpu-1");
assert_eq!(r1.merge(&r2), MergeOutcome::Converged);
assert_eq!(r1.get(), &"config_v2"); // higher timestamp wins

// Same timestamp, different nodes → conflict
let mut r3 = LwwRegister::new("alpha", 5, "gpu-0");
let r4 = LwwRegister::new("beta", 5, "gpu-1");
assert_eq!(r3.merge(&r4), MergeOutcome::Conflict);

// === OR-Set (observed-remove set) ===
let mut s1: OrSet<&str> = OrSet::new();
let mut s2: OrSet<&str> = OrSet::new();
s1.add("node-a", (1, "gpu-0".into()));
s2.add("node-b", (2, "gpu-1".into()));
assert_eq!(s1.merge(&s2), MergeOutcome::Converged);
assert!(s1.contains(&"node-a"));
assert!(s1.contains(&"node-b"));
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  MergeOutcome                                        │
│  Converged = +1  │  Pending = 0  │  Conflict = -1   │
└──────────┬──────────────┬──────────────┬────────────┘
           │              │              │
    ┌──────▼──────┐ ┌─────▼──────┐ ┌────▼───────┐
    │  G-Counter  │ │ LWW-Register│ │  OR-Set    │
    │  (always +1)│ │ (+1 or -1) │ │  (always +1)│
    └─────────────┘ └────────────┘ └────────────┘
```

Each CRDT type has a deterministic merge outcome:

- **G-Counter**: Always `Converged`. The merge rule is `max` per node — it's monotonically increasing and commutative.
- **LWW-Register**: `Converged` when one timestamp is strictly higher. `Conflict` when timestamps are equal but values differ (same timestamp, different authors). This is the only type that can return `Conflict`.
- **OR-Set**: Always `Converged`. Observed-remove semantics guarantee convergence — adds are unique-tagged, and removes only affect previously-seen tags.

## API Reference

### MergeOutcome

```rust
pub enum MergeOutcome {
    Converged = 1,  // Clean merge
    Pending = 0,    // Incomplete merge
    Conflict = -1,  // Ambiguous merge — needs attention
}
```

### G-Counter

```rust
GCounter::new() -> GCounter
counter.increment(node: &str, delta: u64)
counter.value() -> u64
counter.merge(&other: &GCounter) -> MergeOutcome
```

Grow-only counter. Each node maintains its own count. `value()` sums all per-node counts. `merge` takes the element-wise maximum. The value never decreases.

### LWW-Register\<T\>

```rust
LwwRegister::new(value: T, timestamp: u64, node: &str) -> LwwRegister<T>
reg.set(value: T, timestamp: u64, node: &str)
reg.get() -> &T
reg.merge(&other: &LwwRegister<T>) -> MergeOutcome
```

Last-writer-wins register. `set` only updates if the new timestamp ≥ current timestamp. `merge` returns `Conflict` when both registers have the same timestamp but different node IDs (different authors made simultaneous changes).

### OR-Set\<T\>

```rust
OrSet::new() -> OrSet<T>
set.add(element: T, tag: (u64, String))
set.remove(element: &T) -> bool
set.contains(element: &T) -> bool
set.merge(&other: &OrSet<T>) -> MergeOutcome
set.len() -> usize
```

Observed-remove set. Each `add` attaches a unique tag `(timestamp, node)`. A `remove` only deletes elements with tags that the removing node has seen. After merge, concurrent add+remove of the same element preserves the add (because the add has a unique tag the remover didn't know about).

## Real-World Example: GPU Cluster State

```rust
use ternary_antidote::*;

// Track completed batch IDs across nodes
let mut completed: OrSet<u64> = OrSet::new();

// GPU-0 finishes batches 1-5
for id in 1..=5 {
    completed.add(id, (id, "gpu-0".into()));
}

// GPU-1 finishes batches 6-10
let mut remote: OrSet<u64> = OrSet::new();
for id in 6..=10 {
    remote.add(id, (id, "gpu-1".into()));
}

// Merge — no conflicts, all batch IDs preserved
assert_eq!(completed.merge(&remote), MergeOutcome::Converged);
assert_eq!(completed.len(), 10);

// Track total samples processed
let mut samples_local = GCounter::new();
let mut samples_remote = GCounter::new();
samples_local.increment("gpu-0", 50_000);
samples_remote.increment("gpu-1", 48_000);
samples_local.merge(&samples_remote);
assert_eq!(samples_local.value(), 98_000);

// Configuration register with conflict detection
let mut config_local = LwwRegister::new("lr=0.001", 100, "gpu-0");
let config_remote = LwwRegister::new("lr=0.01", 100, "gpu-1");
if config_local.merge(&config_remote) == MergeOutcome::Conflict {
    println!("⚠ Config conflict: two nodes changed learning rate simultaneously");
}
```

## CRDT Properties

| Type | Commutative | Associative | Idempotent | Convergent |
|------|:-----------:|:-----------:|:----------:|:----------:|
| G-Counter | ✓ | ✓ | ✓ | Always |
| LWW-Register | ✓ | ✓ | ✓ | Except timestamp ties |
| OR-Set | ✓ | ✓ | ✓ | Always |

All three are proper state-based CRDTs (CvRDTs): merge is commutative, associative, and idempotent. The only edge case is LWW-Register's conflict when timestamps collide.

## Ecosystem

- **ternary-fault-tree** — model CRDT failure scenarios
- **ternary-shard** — distribute CRDT state across sharded data
- **ternary-intent-cache** — cache CRDT merge outcomes

## Open Questions

- **Conflict resolution policy for LWW**: Currently returns `Conflict` but doesn't resolve. A configurable tiebreaker (e.g., lexicographic node comparison) would make it deterministic.
- **PN-Counter**: G-Counter is grow-only. A PN-Counter (pair of G-Counters) would support decrement.
- **Delta-state CRDTs**: Current implementation ships full state on merge. Delta-state CRDTs send only changes, reducing bandwidth.
- **Vector clocks**: LWW timestamps can collide. Vector clocks provide causal ordering at the cost of larger metadata.

## Stats

| Metric | Value |
|--------|-------|
| Tests | 8 |
| Lines of Rust | 186 |
| Public API | 18 items |

## License

Apache-2.0
