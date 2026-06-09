# ternary-antidote

CRDTs for GPU cluster state with ternary merge outcomes. {+1=converged, 0=pending, -1=conflict}. G-Counter, LWW-Register, OR-Set with ternary semantics.

## Overview

# ternary-antidote

CRDTs for GPU cluster state with ternary merge outcomes.

## Architecture

This crate sits within the **five-layer Oxide Stack**:

| Layer | Crate | Role |
|-------|-------|------|
| 1 | open-parallel | Async runtime (tokio fork) |
| 2 | pincher | "Vector DB as runtime, LLM as compiler" |
| 3 | flux-core | Bytecode VM + A2A agent protocol |
| 4 | cuda-oxide | Flux→MIR→Pliron→NVVM→PTX compiler |
| 5 | cudaclaw | Persistent GPU kernels, warp consensus, SmartCRDT |

The key insight: **ternary values {-1, 0, +1} map directly to GPU compute**. They pack 16× denser than FP32, enable XNOR+popcount matmul, and conservation laws become compile-time checks.

## Stats

| Metric | Value |
|--------|-------|
| Tests | 8 |
| Lines of Code | 186 |
| Public API Surface | 18 items |
| License | Apache-2.0 |

## Installation

```toml
[dependencies]
ternary-antidote = "0.1.0"
```

## Usage

```rust
use ternary_antidote::*;
// See src/lib.rs tests for complete working examples
```

### Key Types

```
- pub enum MergeOutcome { Converged = 1, Pending = 0, Conflict = -1 }
- pub struct GCounter {
    pub fn new() -> Self { Self { counts: HashMap::new() } }
    pub fn increment(&mut self, node: &str, delta: u64) { *self.counts.entry(node.into()).or_insert(0) += delta; }
    pub fn value(&self) -> u64 { self.counts.values().sum() }
    pub fn merge(&mut self, other: &GCounter) -> MergeOutcome {
- pub struct LwwRegister<T: Clone> {
    pub fn new(value: T, timestamp: u64, node: &str) -> Self {
    pub fn set(&mut self, value: T, timestamp: u64, node: &str) {
    pub fn get(&self) -> &T { &self.value }
```

## Design Philosophy

This crate uses **ternary algebra** (Z₃) where every value is {-1, 0, +1}:

- **+1** → positive signal (healthy, allocated, converged, ready)
- **0** → neutral (pending, balanced, monitoring, degraded)
- **-1** → negative signal (failed, free, diverged, overloaded)

This isn't arbitrary — ternary is the natural encoding for:
1. **BitNet b1.58** (Microsoft) — ternary neural networks at 60% less power
2. **GPU warp voting** — hardware ballot instructions return ternary consensus
3. **Conservation laws** — {-1, 0, +1} preserves quantity (what goes in must come out)

## Testing

```bash
git clone https://github.com/SuperInstance/ternary-antidote.git
cd ternary-antidote
cargo test
```

## License

Apache-2.0
