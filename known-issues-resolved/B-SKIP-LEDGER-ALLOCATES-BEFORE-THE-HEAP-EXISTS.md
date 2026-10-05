### B-SKIP-LEDGER-ALLOCATES-BEFORE-THE-HEAP-EXISTS — kernel panic 120 lines into boot — FIXED 2026-08-24 (lane A, `5fbbd539b`)

**Symptom.** Boot test batch39 never reached userspace:

```
[mm]   Double-free detection: OK
!!! KERNEL PANIC !!!
panicked at library/alloc/src/alloc.rs:553:9:
memory allocation of 128 bytes failed
  Task: 0 (""), priority 0, cpu 0
  Heap: slab=0/0 allocs/frees, large=0/0, refills=0, failures=0
```

**Root cause.** `fs::selftest::Skips` — the ledger that §270 introduced so a
self-test's closing line can say how many sections it skipped — stored its
entries in an `alloc::vec::Vec`, and `Skips::suffix()` returned an
`alloc::string::String` built with `alloc::format!`. `mm::frame::self_test()`
runs *between* `[mm] Physical frame allocator initialized` and `[mm] Kernel
heap allocator initialized`, so its first `skips.record(…)` was the first
`Vec::push` of the boot, against an allocator that did not exist yet. The
panic report says so outright — `slab=0/0 allocs/frees, refills=0` — but only
to a reader who already suspects it; nothing in the message or the backtrace
names `fs/selftest.rs`.

The line the sweep replaced (`serial_println!("[mm]   Zeroed frame
allocation: SKIP (HHDM not ready)")`) had worked for months precisely because
printing a `&'static str` allocates nothing.

**Why it was not caught earlier.** Everything that would have caught it was
looking elsewhere:

| Check | Why it was silent |
|---|---|
| `cargo check` / `cargo clippy` | The code is type-correct. "Allocates" is not a type. |
| `scripts/check-selftest-skips.py` | Enforces *how a skip is decided and reported*. It has no notion of boot phase. |
| The previous boot test (batch37) | Predates the change; that boot printed the raw `SKIP` line. |
| Review | The type's own doc comment argued the entries must not allocate — and the `Vec` holding them was on the next line. A comment is not an enforcement mechanism. |

**Fix.** The ledger is now structurally allocation-free: an inline
`[(&'static str, &'static str); MAX_SKIPS]` with a length, and `suffix()`
returns a `Display` adaptor (`SkipSuffix`) instead of a `String`, so all ~26
`serial_println!("… PASSED{}", skips.suffix())` call sites are unchanged.
Overflow past `MAX_SKIPS` (16) is **counted, not dropped** — a dropped skip
would restore exactly the silence §270 exists to prevent, while still printing
a number, and the number would be wrong. See `design-decisions.md` §271 (*the
fixed-capacity skip ledger* — not the framebuffer §271).

**Regression test.** `fs::selftest::self_test()`, called from `main.rs`
immediately after `mm::heap::init` — the earliest point in boot where
`heap::stats()` can observe an allocation at all. It snapshots the slab and
large-allocation counters, exercises `record`/`report`/`suffix` (including the
overflow arm, and `report`'s serial output, because a reporting path that
allocates fails exactly when there is something to report) with interrupts
masked, and fails if either counter moved. It also asserts the rendered
suffixes for the 0 / 1 / overflow cases and that the overflowed skip is still
counted.

**Generalisation worth remembering.** The reporting path of a self-test must
have *weaker* requirements than anything it reports on. A ledger that needs the
heap cannot report on the heap. Anything added to `fs::selftest` in future has
to hold to that, which is what the new self-test is there to enforce.
