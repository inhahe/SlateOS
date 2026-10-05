## §107 — B-KNULLJUMP escalates to a compiler-instrumented KASAN kernel build

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended sequencing A first and then
escalating; the operator agreed the lighter path is exhausted and chose B).
This was Q34 in `open-questions.md`.

**Context.** B-KNULLJUMP is an intermittent (~1-in-120) wild **store** into a
live scheduler BTree node. The Q32→A decision built the two lighter corruption
detectors — a lazily-mapped KASAN shadow (`kernel/src/mm/kasan.rs`) and a slab
free-quarantine (`kernel/src/mm/quarantine.rs`), both boot-green and
self-tested. Neither catches this failure mode *passively*: they only see a
write that lands in a parked or poisoned granule, and B-KNULLJUMP stomps a live
node. A 100-iteration armed hunt (`soak-20260723-190300`) came back
100/100 PASSED with `corruptions=0` — **inconclusive, not exonerating**, since
a clean 100-run is ~43% likely even with the bug fully present.

**Decision.** Wire up full compiler-instrumented KASAN
(`-Zsanitizer=kernel-address`), which auto-instruments every load and store and
flags the faulting instruction directly. Probing confirms the target supports
it (`x86_64-unknown-none` → `supported-sanitizers: ['kcfi', 'kernel-address']`).

**Rationale.** It is the only remaining tool that sees the store. The lighter
path was built, hardened and run at scale without localizing the bug, so
continuing it is the "verification loop that yields no edits" CLAUDE.md warns
against.

**What this commits to — a genuine build fork.** Whole-kernel instrumentation
is a large perf hit, so this lands as a **separate debug profile**, not as the
shipping build. It needs: whole-kernel-VA shadow backing rather than heap-only
(Linux uses a shared zero shadow page for untracked regions), in-kernel
`__asan_*`/`__kasan_*` runtime callbacks, a fixed compile-time shadow offset
matching our layout, and `#[no_sanitize]` plus careful ordering on every
early-boot and shadow-setup path. The main risk is destabilizing boot if the
shadow is not fully ready before the first instrumented code runs — which is
why the shadow-setup path itself must be uninstrumented.

**Alternatives considered.**
- *A — keep working the lighter tools.* Cheap, low-risk, already built, and it
  was the right first move. Rejected as the standing path because it has now
  been run to exhaustion; its structural blind spot (only parked/poisoned
  granules are visible) is exactly where this bug lives.

**Where it lives.** `.cargo/config.toml` rustflags
(`-Zsanitizer=kernel-address`, `-Cllvm-args=-asan-mapping-offset/scale`), a new
`__asan_*` runtime module, whole-VA shadow setup in early boot (`main.rs` mm
init), and the existing `kernel/src/mm/kasan.rs` / `quarantine.rs` /
`heap.rs` hooks.

**How to reverse.** The instrumentation is a build profile; dropping the
rustflags returns the kernel to the current build. The `__asan_*` runtime and
the whole-VA shadow are additive modules that go unused without it.
