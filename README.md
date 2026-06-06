# ternary-antidote

**CRDTs (Conflict-free Replicated Data Types) for GPU cluster state with ternary merge outcomes: Converged (+1), Pending (0), Conflict (-1).**

## Background

In distributed systems, CRDTs (introduced by Marc Shapiro et al. in 2011) provide **eventual consistency without coordination**. Each replica can update independently, and merges are guaranteed to converge. Amazon DynamoDB, Riak, and Redis CRDT modules all use variants of these data structures.

`ternary-antidote` adapts CRDTs for GPU cluster state management. The key innovation is that every `merge()` operation returns a **ternary outcome**:

| Value | Outcome | Meaning |
|-------|---------|---------|
| +1 | `Converged` | States merged cleanly, no conflict |
| 0 | `Pending` | Merge in progress (for async merges) |
| -1 | `Conflict` | Irreconcilable conflict detected |

This ternary feedback lets the GPU cluster's control plane **react to merge quality in real-time**: converged merges require no action, pending merges can be scheduled for retry, and conflicts trigger resolution protocols.

The crate implements three foundational CRDTs: G-Counter (grow-only), LWW-Register (last-writer-wins), and OR-Set (observed-remove).

## How It Works

### G-Counter (Grow-Only Counter)

Each node maintains its own counter. The global value is the sum of all per-node counters. Merge takes the element-wise maximum. **Always converges** — returns `MergeOutcome::Converged`.

```
Node A: {a: 5}  +  Node B: {b: 3}  →  {a: 5, b: 3}  →  value = 8  →  Converged
```

### LWW-Register (Last-Writer-Wins)

Each register carries a value, timestamp, and originating node. On merge, the higher-timestamp value wins. If timestamps are equal but nodes differ, returns `Conflict (-1)`.

```
Register(x, ts=1, node="a")  merge  Register(y, ts=2, node="b")  →  y wins  →  Converged
Register(x, ts=5, node="a")  merge  Register(y, ts=5, node="b")  →  tie!    →  Conflict
```

### OR-Set (Observed-Remove Set)

Elements are tagged with unique (counter, node-id) pairs. Add creates a new tag; remove tombstones all current tags. Merge unions elements and tombstones, then filters tombstoned entries. **Always converges** due to the observed-remove semantics.

### Ternary Mapping

```rust
enum MergeOutcome { Converged = 1, Pending = 0, Conflict = -1 }
```

The `Pending` state is reserved for asynchronous merge scenarios (e.g., when a merge requires fetching additional state from another replica before completing).

## Experimental Results

The test suite validates:

- **G-Counter**: Incrementing per-node counters and merging across nodes yields correct totals. Merge always returns `Converged`.
- **LWW-Register**: Higher timestamps win. Older writes are ignored. Equal timestamps with different nodes produce `Conflict`.
- **OR-Set**: Add, remove, and merge operations produce correct membership. Tombstoned elements stay removed after merge. Cross-node merge preserves both nodes' additions.

## Impact for GPU Cluster Computing

CRDTs are essential for GPU clusters because:

- **No coordination overhead**: GPU nodes can update state locally without network round-trips. This is critical when nodes are busy running kernels and can't afford synchronous coordination.
- **Partition tolerance**: If the network between GPU racks fails, each rack continues operating independently. When connectivity resumes, CRDTs merge cleanly.
- **Ternary feedback**: The merge outcome tells the control plane whether to trigger conflict resolution. In traditional CRDTs, merges silently succeed — there's no way to detect when something unusual happened.

## Use Cases

1. **GPU Resource Accounting**: A G-Counter tracks total GPU-hours consumed across a federated cluster. Each data center increments its local counter and merges periodically.
2. **Model Version Registry**: An LWW-Register stores the current "production" model version. Any data center can update it, and the latest update wins globally.
3. **Active Job Set**: An OR-Set tracks which inference jobs are currently active across the fleet. Jobs are added when started and removed (observed-remove) when completed, with clean cross-region merging.
4. **Configuration Distribution**: GPU kernel configurations (block sizes, shared memory allocations) are distributed as CRDTs, allowing any node to propose updates that automatically converge.

## Open Questions

1. **Conflict resolution policy**: When an LWW-Register merge returns `Conflict`, what deterministic resolution policy should be used? Node-ID ordering? Application-level merge?
2. **Custom CRDT composition**: Can ternary CRDTs be composed (e.g., a Map of LWW-Registers) while preserving the ternary merge outcome contract?
3. **GPU-accelerated merge**: Can CRDT merge operations be compiled to GPU kernels for sub-millisecond state reconciliation across thousands of replicas?

## Connection to Oxide Stack

`ternary-antidote` is the **state layer** that all other components build on:

| Layer | Crate | Dependency |
|-------|-------|-----------|
| 1 — State | **`ternary-antidote`** | Foundation: all state is CRDTs |
| 2 — Versioning | `ternary-version` | Version vectors built on CRDT primitives |
| 3 — Propagation | `ternary-epidemic` | CRDT state is gossiped between nodes |
| 4 — Consensus | `ternary-consensus` | Consensus decisions are stored as CRDTs |
| 5 — Replication | `ternary-mirror` | Mirror consistency verified against CRDT state |

Every piece of distributed state in the GPU runtime is a CRDT. This crate is the foundation.
