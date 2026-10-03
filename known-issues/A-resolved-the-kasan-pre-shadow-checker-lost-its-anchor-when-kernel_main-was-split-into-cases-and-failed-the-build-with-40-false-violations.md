### [A] RESOLVED — the KASAN pre-shadow checker lost its anchor when `kernel_main` was split into cases, and failed the build with ~40 false violations — 2026-08-19

**In short:** The KASAN-instrumented build has been failing since `86a56f78c`, on a
check that was wrong rather than on anything wrong with the kernel. Because that build
is run by hand and not by the routine boot gate, nothing noticed. The instrumented
kernel itself was fine the whole time.

**Symptom.** `./scripts/kasan-build.sh --boot` compiles successfully and then dies at
the invariant check, before ever booting:

```
=== KASAN uninstrumented-path invariants are NOT satisfied ===
  STRUCTURE: kernel_main
    calls _RNvNvCs..._6kernel11kernel_main4case
    before 6serial4init - it runs in the pre-shadow window but is not in ENTRY_ALLOWED_CALLS
  ... ~40 more ...
  STRUCTURE: no call to '6serial4init' found in kernel_main; the pre-shadow window's
  end can no longer be located
```

**Cause.** `check_entry_order` walks `kernel_main`'s calls in program order and stops
at `serial::init`, which marks the point where the KASAN shadow is live; everything
before it must be in `ENTRY_ALLOWED_CALLS`. Commit `86a56f78c` split `kernel_main` into
36 nested `#[inline(never)] fn case()` helpers to cut its peak stack frame from 19 776
to 10 976 bytes. That moved the pre-shadow statements — *and* the `serial::init` call
that ends the window — into the first helper. The scan then saw a call to
`kernel_main::case` where it expected `serial::init`, never found the anchor, and so
treated the entire function as pre-shadow.

The refactor was correct and the checker's expectation was the stale part: it was
anchored to how `kernel_main` happened to be carved into functions rather than to what
it does.

**Fix, part 1 — follow the body.** The scan now descends into nested helpers in
flattened program order. A v0-mangled symbol nests as `...11kernel_main4case` /
`...11kernel_mains0_4case`, so `NESTED_ENTRY_SUBSTRING` recognises them as
`kernel_main`'s own body rather than as calls out of it. A helper with no disassembly
is reported rather than skipped, since an unfollowed helper is a hole in the window.

**Fix, part 2 — do not bury the real finding.** When the anchor cannot be located the
checker now reports *that alone*. Without the anchor the scan never learned where the
window ends, so every violation it emits is an artefact of the unbounded window; the
real run printed ~40 such lines above the one accurate diagnosis, each inviting the
reader to go exempt a call that was never in the window. The message also names the two
ways the anchor goes missing (moved into an unrecognised helper, or inlined away).

**Verified.** Against the instrumented kernel the checker now reports OK: 14 functions
reachable before the shadow is installed, 21 on the check path, 45 from the poisoned
roots, none instrumented. Teeth confirmed by emptying `ENTRY_ALLOWED_CALLS`, which
yields exactly the two genuine pre-shadow calls (`boot::hhdm_offset_early`,
`mm::kasan::early_init`) attributed to `kernel_main::case` — so the descent reaches
them rather than passing vacuously. The anchor-missing path was exercised by pointing
`WINDOW_END_SUBSTRING` at a symbol that does not exist: one message, not forty.

**Why it went unnoticed, and what would catch it next time.** The instrumented build is
not part of `boot-test.sh`, so a check that only this build runs can rot for a week
without a red gate. Nothing here changes that; it remains a manual build.
