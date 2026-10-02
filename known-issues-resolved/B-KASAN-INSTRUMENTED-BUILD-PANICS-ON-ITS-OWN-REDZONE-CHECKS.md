## B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS

**Status:** ✅ **FIXED 2026-08-12** — the flood and the panic are both gone.
Found 2026-08-12 by the first instrumented boot that got far enough to reach the
self-tests.

**Verification.** The instrumented boot of 2026-08-12 ran 5560 lines — well past
the self-tests that previously flooded and panicked — with **zero `[kasan]
CRITICAL` reports**. The 64-report cap is untouched at the point the hunt
window opens, which was consequence (1) below and the reason this blocked the
§107 escalation.

**That boot did not reach `BOOT_OK`**, but for an unrelated reason: it later
wedged mid-print on a page fault. That is tracked separately as
`B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT`; it is a different
failure (a deadlock, not a report flood) at a different place, and it is still
open. So the §107 escalation is unblocked *by this entry* but still gated on
that wedge.

**What the fix was.** Exactly the "proper fix" described below, implemented as
`kernel/src/mm/rawmem.rs` (`read_u8`/`write_u8`/`fill_u8` in inline `asm!`) with
every deliberate poisoned-memory touch in `heap.rs`/`poison.rs`/`quarantine.rs`
routed through it, plus walk 3 of `scripts/kasan-check-preshadow.py` as the
build gate, plus `rawmem::self_test()` at boot ahead of the poison/kasan/
quarantine self-tests it underpins. The design and the reasoning behind walk 3's
accessor-based violation rule are recorded in `design-decisions.md` §120.

`kasan_rt::self_test`'s `before` snapshot also moved to *after*
`self_test_freed_address()` returns, as described below. That was independently
worth doing: it is what made the assertion count setup traffic as the thing
under test, and it would have kept the measurement window fragile even with the
flood gone.

---

*Original report follows.*

**What happens.** The compiler-instrumented kernel (`scripts/kasan-build.sh`)
does **not** reach `BOOT_OK`. It runs almost the whole boot, then dies in the
`mm::kasan_rt` self-test:

```
[kasan] CRITICAL: out-of-bounds (heap redzone) on read of 1 bytes @ 0xffff80007eb05268 (shadow=0xfb)
   ... x64, then: [kasan] further reports suppressed after 64
[kasan] Self-test PASSED
[kasan-rt] Running self-test...
!!! KERNEL PANIC !!!
panicked at kernel\src\mm\kasan_rt.rs:386:13:
assertion `left == right` failed: kasan-rt: outlined check did not report
  left: 217
 right: 101
```

**Root cause — a third instance of the §118/§119 hazard.** The symbolized
backtrace of the flood is unambiguous:

```
#0 kernel::mm::kasan_rt::report
#1 __asan_load1_noabort
#2 core::ptr::read_volatile::<u8>        <-- instrumented
#3 kernel::mm::heap::check_redzone
#4 <mm::heap::KernelHeap as GlobalAlloc>::dealloc
#5 __rust_dealloc  #6 alloc::alloc::dealloc  #7 mm::kasan::self_test
```

`mm/heap.rs`, `mm/poison.rs`, `mm/quarantine.rs` and `mm/kasan.rs` **all**
carry the module-level `#![cfg_attr(kasan_instrumented, sanitize(address =
"off"))]`. It does not help them, for the reason already written down twice:
a module-level `sanitize` **cannot exempt a generic `core` function**, because
the monomorphisation is emitted into the *kernel* crate with the default
(instrumented) attribute. Every one of these modules does its actual byte
touching through `core::ptr::{read_volatile, write_volatile, write_bytes}` —
so the exemption is **cosmetic on exactly the operations that matter**.

These modules are the ones whose *entire job* is to read and write memory
KASAN has deliberately poisoned:

| Site | What it touches |
|---|---|
| `mm/heap.rs:138-141,305-308,1737-1740` | free-magic reads on a freed block |
| `mm/heap.rs:160-170` | free-magic + `FREE_POISON` writes |
| `mm/heap.rs:218,322` | redzone verify (`check_redzone`, the flood above) |
| `mm/poison.rs:138,157,174` | `poison_free`/`poison_alloc`/`poison_redzone` fills |
| `mm/quarantine.rs:145` | parking a freed slot poisoned — the hunt's core mechanism |

So the instrumented build reports a violation for every byte of every redzone
check and every poison fill. That is *correct* by ASan's rules and useless by
ours: the access is the detector, not the bug.

**Why the panic follows.** `kasan_rt::self_test` snapshots `report_count()`
into `before`, then calls `kasan::self_test_freed_address()`, which does a real
`alloc`/`dealloc` — and that `dealloc` runs `check_redzone`, generating ~116
reports of its own before the assertion is reached. `before + 1` is then wrong
by exactly that flood (217 vs 101). The snapshot is in the wrong place *and*
the flood should not exist; fixing the flood fixes both.

**Why this is worse than a failed boot.** Two consequences that would have
silently wrecked the hunt even if the assertion were relaxed:

1. **The 64-report cap is spent before the hunt window.** The self-tests run at
   reference line ~21662; the Path-Z checkpoint the hunt watches is at ~19579
   but the *armed* run poisons throughout. Once suppressed, a genuine
   B-KNULLJUMP report would never print.
2. **Armed, the flood is enormous.** This validation boot ran *without*
   `mm.corruption_hunt`, so KASAN was disabled (`ENABLED` defaults false) and
   only the self-test poisoned anything — stats `poisoned=112B,
   shadow_frames=3`. The reference armed boot reports `poisoned=133859680B,
   shadow_frames=197` with `total_parked=55372`. Armed *and* instrumented,
   every one of those parks and every redzone check on 133 MB reports. (Noting
   this because the tiny stats line looks alarming on its own and is not: it is
   the unarmed default, not a shadow-coverage failure.)

**Proper fix.** Add a small uninstrumented raw-access primitive — byte
load/store/fill implemented with `core::arch::asm!` — and route every
deliberate poisoned-memory touch in `heap.rs`/`poison.rs`/`quarantine.rs`
through it. `asm!` is the right tool rather than another `sanitize` attribute:
LLVM does not instrument inline-asm memory operands, so the guarantee holds
whether or not the helper is inlined, and it cannot be re-broken by someone
later calling a `core` generic from an "exempt" module. Then extend
`scripts/kasan-check-preshadow.py` with a **third root set** ("functions that
deliberately access poisoned memory must be uninstrumented"), so this class of
regression is caught by the build gate rather than by a 2.7-hour boot — the
existing gate walks only the pre-shadow root and the check-path root, which is
why it passed this binary. Separately, move `kasan_rt::self_test`'s `before`
snapshot to *after* `self_test_freed_address()` returns, so setup can never be
counted as the thing under test.

**Reproduce.** `./scripts/kasan-build.sh` then boot; ~2.7 h to reach the panic
(see Q43 on the cost). Partial evidence kept in `build/serial-test.txt` from
the 2026-08-12 run; symbol table for that binary in `build/kernsyms.txt`.

**Related.** `design-decisions.md` §118 and §119 (the same hazard from the
pre-shadow and check-path roots), `open-questions.md` Q43 (the profile's cost),
and `B-KNULLJUMP-SIGNAL` (what the profile was built to hunt).
