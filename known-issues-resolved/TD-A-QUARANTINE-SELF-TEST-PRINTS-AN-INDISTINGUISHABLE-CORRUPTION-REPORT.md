## TD-A-QUARANTINE-SELF-TEST-PRINTS-AN-INDISTINGUISHABLE-CORRUPTION-REPORT (lane A, 2026-08-21) — FIXED 2026-08-22

**In short:** the memory-corruption detector runs a self-test at boot that
deliberately corrupts its own scratch buffer to prove the detector works. The
report it prints when it finds that deliberate corruption is worded and
formatted exactly like a real one — `*** CORRUPTION ***` with an address — so
anyone grepping a serial log for corruption finds a false positive every boot.

**Where:** `kernel/src/mm/quarantine.rs`, the self-test path. Cost about an
hour during the `B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE` diagnosis: the reported
slot was inside `KERNEL_BOOT_STACK`, which looked like the allocator had handed
out a slot inside the live boot stack — a spectacular bug, and not a real one.
The giveaway is only in the surrounding lines (`scan_all (corrupted): OK (1
found)` before it, `Self-test PASSED` after).

**Proper fix:** the self-test should pass a flag that makes the report print
under a distinct, obviously-synthetic prefix — `[quarantine] (self-test)
expected corruption at …` — so the real string `*** CORRUPTION ***` appears in
a log only when something is actually wrong. A log line that means two
different things is not a diagnostic.

### Fixed 2026-08-22 — and there was a second, worse half nobody had noticed

Fixed in `0e28b9d4a` as described: an `Origin` enum (`Real` / `SelfTest`) is
threaded through `report()`, and the `SelfTest` line deliberately does not
contain the string `*** CORRUPTION ***`, so grepping a log for it now finds
only the real thing.

**But the log line was the half that had been noticed.** While making the
change it turned out the synthetic find also did
`CORRUPTIONS.fetch_add(1, …)` — the same counter `stats().corruptions` exposes,
and the one `kernel/src/main.rs`'s Path-Z hunt checkpoint prints as
`corruptions={}`. That is the number a soak run reads to decide whether a boot
was clean. So the self-test did not merely print a false positive once per
boot; it made **every healthy boot report a nonzero corruption count**, forever,
to an automated consumer that has no surrounding lines to disambiguate from.

A health counter that is nonzero on a healthy boot is not a health signal —
it trains its reader to ignore it, which is the failure mode the counter exists
to prevent. Worth recording because of how it was found: not by looking for it,
but because fixing the cosmetic half required touching the one function that
did both. **The visible symptom of a bad signal is often the cheaper half of
it.** When you fix an ambiguous diagnostic, follow every consumer of the value
it reports, not just the humans reading the log.

**Regression test:** `self_test()` test 6 asserts `stats().corruptions` is
unchanged across the whole self-test. Its assert message names *both* causes it
can have — regressed tagging, or a genuine corruption found while the self-test
ran — rather than asserting a single meaning for an ambiguous signal, which is
the mistake the entry above is about.

**Design note on the mechanism:** the origin is a function argument, not a
global `IN_SELF_TEST` flag. A flag is readable by a *different* CPU sweeping
the quarantine ring at the same moment, and would silently downgrade that CPU's
genuine find to a self-test line — turning a false positive into a false
negative, which is the strictly worse trade.
