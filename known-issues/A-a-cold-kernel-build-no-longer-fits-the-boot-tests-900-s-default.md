## `[A]` A cold kernel build no longer fits the boot test's 900 s default

**Status:** OPEN (2026-08-20)

**What happens.** `python scripts/run-timeout.py 900 ./scripts/boot-test.sh` —
the budget most of this project's history used — now times out *during the
build*, before QEMU is ever launched, whenever the rustc fingerprint cache is
cold. It was hit on 2026-08-20 while gating the ZFS gang-block work: the run
died at 900 s with the log's last line still `Compiling kernel v0.1.0`. The
re-run with `--poll 60 2700` finished the same work in 1349 s, of which 677 s
(11m17s) was the build alone.

**Why the cache is cold more often than it looks.** Nothing dramatic is needed
to invalidate it. A `cargo clippy` run immediately before the boot test does
it, because clippy and rustc write different fingerprints into the same target
directory and each evicts the other. So the ordinary "lint, then boot-test"
sequence is exactly the sequence that guarantees a cold build.

**Two things that made it worse than a plain timeout.** The run was piped
through `| tail -60`, which buffered the child's entire output, so the log file
sat at 0 bytes for the full 900 s and the job looked wedged rather than busy.
And the pipeline's exit status is `tail`'s, so the harness reported **0** while
`run-timeout.py` had in fact returned 124. Both are avoidable: never pipe
`run-timeout.py`, and read its log incrementally instead.

**Proper fix.** Either raise `run-timeout.py`'s recommended boot-test budget in
`CLAUDE.md`/`scripts` to ~2700 s, or split the build out of `boot-test.sh` so
the timeout covers only the QEMU phase and a slow build cannot be mistaken for
a hang. The second is better — the two phases fail for unrelated reasons and
deserve unrelated budgets — but the first is what any run should do today.

**Workaround until then.** `python scripts/run-timeout.py --poll 60 2700
./scripts/boot-test.sh`, backgrounded via the Bash tool's `run_in_background`,
with no pipe on the command.

### CORRECTION 2026-08-31 — the fix landed, and the stated cause was wrong

**Status: CLOSED (fixed), with one paragraph above retracted.**

Two separate things need saying, because only one of them is "this got fixed".

**The fix.** `boot-test.sh` now documents and budgets the outer window itself
(`scripts/boot-test.sh:30-57`): **7200 s**, derived as ~3000 s observed pre-QEMU
+ 2400 s inner QEMU timeout + headroom. The entry's "Proper fix" offered a
choice between raising the budget to ~2700 s and splitting build from boot; the
budget was raised, to nearly 3× what this entry proposed, because the pre-QEMU
phase was re-measured on 2026-08-31 at ~3000 s with another lane building
concurrently — so 2700 s would itself have been too tight. The comment there
also records the reason a *generous* outer budget is not laziness: an outer
timeout that fires first replaces the inner one's SYSTEM HANG diagnostic
(faulting RIP over HMP, task table, marker) with an anonymous exit 124, on
exactly the runs where the diagnostic matters. That destroyed two runs on
2026-08-31 before the comment was rewritten.

**The retraction.** The paragraph above titled *"Why the cache is cold more
often than it looks"* — claiming a preceding `cargo clippy` evicts the build's
fingerprints, so that "lint, then boot-test" guarantees a cold build — **is
false, and was reasoning rather than measurement.** `cargo clippy` sets
`RUSTC_WORKSPACE_WRAPPER`, which is hashed into every workspace unit's
fingerprint, so clippy's artifacts occupy their own cache entries and leave the
build's untouched. Measured 2026-08-24 and recorded at
`scripts/boot-test.sh:4055-4067`: a `cargo build` run *immediately after* a cold
200 s clippy took **4.7 s** — i.e. not invalidated at all.

That retraction matters beyond this entry, because the same false belief had
already been used once to *defer* adding the clippy gate on cost grounds. A
plausible mechanism, asserted without measurement, cost a real gate and then
survived long enough to be written down here as an explanation for an unrelated
timeout. The actual cause of the 2026-08-20 overrun was simply that the build
is long and the budget was short — no eviction required.
