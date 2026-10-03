## §118 — The KASAN pre-shadow window is raw assembly, and the invariant is checked by a build gate rather than by review

**Date:** 2026-08-12

**Decided by:** Claude (operator-approved scope). §107 was the operator's call
to build the instrumented kernel; every decision below is a specific call made
while making it actually boot, and is mine to revisit.

**Context — the failure that forced this.** Between the kernel entry point and
the moment `mm::kasan::early_init` finishes installing the zero shadow there is
no shadow to read *and* no IDT to catch the resulting page fault. One
instrumented memory access in that window is a triple fault: QEMU resets, the
kernel prints **nothing at all**, and `boot-test.sh` reports the same "no
BOOT_OK" it would report for any other boot failure. Two of these were hit for
real before the gate below existed, each costing a `-d int,cpu_reset` run and a
symbol lookup to localize.

**The discovery that makes this non-obvious.** A module-level
`#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]` does **not**
establish that the module's code is uninstrumented. `sanitize` is a per-function
LLVM attribute, so it covers functions that are *ours*. Generic `core`
functions monomorphise into the kernel crate's codegen units, are emitted
out-of-line at `-O0`, and carry the default (instrumented) attribute — and they
dereference the pointers we hand them, so the shadow probe lands in `core`'s
frame where our exemption has no reach. The two real faults:

- `serial::init` takes a spinlock → `core::sync::atomic::atomic_compare_exchange_weak::<u8>`
  probes the shadow of `serial::SERIAL`.
- `for i in 0..512` hands `&mut Range<usize>` to
  `core::iter::range::RangeIteratorImpl::spec_next`, which probes the shadow of
  *this* frame's stack slot. (`-asan-stack=0` suppresses instrumentation of a
  function's own allocas — not of a pointer parameter that happens to point at
  a caller's alloca.)

Neither is visible by reading the source of the exempt module.

**Decision 1 — the window issues its own loads and stores via `asm!`.**
`mm::kasan::raw_load_u64` / `raw_store_u64` / `raw_shr_u64` are the audited
primitives; `limine::LimineRequest::<HhdmResponse>::offset_raw` and
`boot::hhdm_offset_early` are built on them, and the idempotency flag is a plain
`static mut EARLY_SHADOW_DONE: u64` read with `raw_load_u64` rather than an
`AtomicU64` (whose `load` is a `core` generic, hence instrumented).

*For:* inline assembly is opaque to LLVM, so it cannot be instrumented,
elided or reordered — the last property is what the previous `read_volatile`
was there for anyway. *Against:* it is more code than `*ptr`, and the safety
argument moves from the type system into `// SAFETY:` comments. Accepted
because there is no attribute that reaches into a monomorphised `core` generic;
keeping the access in our own frame is the only mechanism available.

**Decision 2 — debug arithmetic checks count as instrumented code.** In a debug
build `+` branches to `core::panicking::panic_const_add_overflow` and `>>` to
`panic_const_shr_overflow`, both instrumented panic machinery reachable from
the window. `wrapping_shr` is *not* a fix: it forwards to
`core::num::<u64>::unchecked_shr`, whose `ub_checks` precondition check is
itself a `core` generic that monomorphises in with instrumentation. So the
window uses `wrapping_add`/`saturating_add` for arithmetic and `raw_shr_u64`
(an `asm!` `shr`) for shifts, and `while` loops with plain counters instead of
`for`-over-`Range`.

**Decision 3 — `early_init` splits into two named phases.** `install_zero_shadow`
runs with no shadow and returns a `ShadowRoots`; `publish_shadow_roots` runs
after the TLB flush and does the ordinary atomic stores. `early_init` is a
two-statement wrapper.

*For:* the phase boundary was previously a comment in the middle of one
function, which no tool can reference. A call-graph walk cannot express "the
first half of this function", so the *code* draws the line instead, and the
gate below can name `install_zero_shadow` as a root. *Against:* it is a split
made for the benefit of tooling rather than for the code's own structure.
Accepted: the boundary is real regardless, and naming it also stopped the
gate's false positive on the post-shadow atomic stores.

**Decision 4 — the invariant is a build gate, not a review item.**
`scripts/kasan-check-preshadow.py` disassembles the built kernel, walks the
call graph breadth-first from the pre-shadow roots, and fails on any reachable
`__asan*` call or any indirect call it cannot prove exempt. `kasan-build.sh`
runs it immediately after the build. A `check_entry_order` pass separately
verifies that `kernel_main` still calls nothing but the allowed set before
`serial::init`, so inserting a new call ahead of the window's end is *reported*
rather than silently escaping the walk.

*For:* the whole point of the section above is that source review cannot
establish this property, and the failure mode gives you no evidence to work
from. Failing here costs a message; failing at boot costs a debugging session.
*Against:* it is a disassembly-based check, so it is coupled to symbol mangling
(`ROOT_SUBSTRINGS` carries v0 length prefixes such as `19install_zero_shadow`)
and needs updating when those functions are renamed. Mitigated by the root-not-
found and entry-order checks, which turn a stale list into a hard failure
instead of a silent pass.

**Where it lives.** `kernel/src/mm/kasan.rs` (the raw primitives, the phase
split, `early_translate`), `kernel/src/limine.rs` (`offset_raw`),
`kernel/src/boot.rs` (`hhdm_offset_early`), `kernel/src/main.rs` (the shadow
install is now statement zero of `kernel_main`, before `serial::init`),
`scripts/kasan-check-preshadow.py`, `scripts/kasan-build.sh`.

**How to reverse.** All of it is inert in the ordinary build: the raw
primitives compile to the same loads and stores, the phase split is a
refactor, and the checker exits 2 ("not an instrumented build") when handed a
kernel with no `__asan` symbols.
