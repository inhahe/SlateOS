### B-BENCH-RECORDER-CRASHED-FOR-FOUR-COMMITS-AND-THE-BOOT-GATE-PRINTED-PASSED — 2026-08-14 — ✅ FIXED 2026-08-14 (`scripts/bench-history.py`, `scripts/boot-test.sh`, `scripts/test-bench-history.py`)

**Symptom.** Every `--bench` boot from `368c128fd` onward wrote **no history
record at all**. The kernel measured correctly, the serial log was complete, the
tool printed its full comparison and a correct canary summary — and then died:

```
Traceback (most recent call last):
  File "scripts\bench-history.py", line 1233, in <module>
    sys.exit(main())
  File "scripts\bench-history.py", line 1222, in main
    record["canary_verdict"] = verdict
                               ^^^^^^^
NameError: name 'verdict' is not defined
=== Boot test PASSED ===
```

Note the last line. That is the whole bug.

**Cause 1 — the extraction took a binding with it.** `368c128fd` moved 55 lines
of canary-summary printing out of `main()` into `print_canary_summary()`, to make
the wording assertable from the test suite. The moved block began
`verdict = canary_verdict(canary)`, so the binding left with it — while `main()`
went on referencing `verdict` 250 lines further down. Python does not diagnose
this until the line runs.

**Why the refactor's own evidence could not see it.** That commit justified
itself with a measured behaviour-preservation check: assertions went 106 → 117,
none lost. That check was sound and remains true. It simply could not cover this,
because it can only cover functions that *have* tests, and `main()` had none —
the only code path in the tool that actually writes to `history.jsonl` was the
one path with no test. Extracting code out of an untested caller moves the
tested part and leaves the untested part holding a dangling reference; a test
suite that grows in the extracted half reports success either way.

**Cause 2, and the worse one — the boot gate discarded the exit status.**
`boot-test.sh` invoked the recorder as `python bench-history.py … || true`,
reasoning in a comment that "a missing python or a write failure must not turn a
healthy boot into a failed one". Both of those are true and both are already
handled elsewhere: python's absence by the `command -v` branch, a write failure
by the tool reporting it without exiting non-zero. What `|| true` actually
suppressed was the case nobody had in mind — the recorder *crashing*. So the
traceback scrolled past and `=== Boot test PASSED ===` was printed directly over
it, four commits running.

This is the project's recurring shape, one level up from where it usually
appears: **a check that cannot fail is indistinguishable from a check that
passes.** Here the check was the tool's own exit status, and it had been
explicitly disarmed.

**Cause 3, found by the new test on its first run.** `main()` finished by
printing `os.path.relpath(args.history, REPO_ROOT)`. On Windows `relpath` raises
`ValueError` when the two paths are on different drives, so any `--history`
outside the checkout's volume aborted the tool *after* the record had already
been appended — a traceback and a non-zero exit on a run that had in fact
succeeded. Cosmetic path-prettifying must not be able to fail the run it reports
on.

**Fix.**

* `print_canary_summary` returns its verdict on all four paths, and `main()`
  takes the value from it rather than recomputing `canary_verdict(canary)`. The
  coupling is now explicit: there is no way to use the printer's output without
  receiving the value `main()` needs, and no second call site where the printed
  prose and the stored verdict could drift apart.
* `display_path()` falls back to the path as given when no relative form exists.
* `boot-test.sh` captures the recorder's status into `BENCH_RECORDER_STATUS`.
  This invocation passes no `--fail-on-regression`, so the tool has no legitimate
  non-zero exit here — any non-zero status is a fault in the tooling.
* New `finish_pass()` is the **only** place that prints the PASSED banner. The
  two success paths (the poll loop spotting the marker; the post-loop check
  finding it after QEMU exited) each carried their own verbatim copy of the
  pass sequence, so a condition added to one would silently not apply to the
  other. A run whose recorder failed now ends `=== Boot test INCOMPLETE ===`
  with **exit 3** — distinct from 1 (kernel/self-test failure) and 2 (wedge),
  because conflating "the kernel is broken" with "our tooling is broken" sends
  the reader to the wrong tree.
* `boot-test.sh`'s header now documents exit codes 2 and 3. 2 has existed since
  the stall detector landed while the header still claimed only 0 and 1 were
  possible; a status a caller cannot know about is a status the caller cannot
  handle.
* `test_main_records_end_to_end` drives `main()` to an appended record and
  asserts the record's contents, both canary paths, and all four of the
  printer's return values.

**Positive controls, because a regression test that cannot fail is the same bug
again.** Re-deleting the `verdict =` assignment reproduces the identical
`NameError` through the new test. Driving the real `finish_pass()` text with a
recorder stubbed to crash yields exit 3 and no PASSED banner; with a healthy
stub it yields exit 0 and PASSED. One further control was needed on the test
itself: the four return-path assertions were originally written *inside* the
`redirect_stdout` block that silences the printer, which posted their own
PASS/FAIL lines into the discarded buffer — four assertions running and
reporting nothing. They now collect under the redirect and assert outside it.

**Cost.** Four commits of `--bench` boots produced no data: roughly 9 minutes of
QEMU each, and — more expensively — the P21 baseline measured during this thread
had to be re-measured, because the run that produced it recorded nothing.

---

#### CORRECTION to P21(b), written BEFORE the measurement — the clause is not gradeable

Registered wording:

> **P21(b)** — `vfs_stat_breakdown_prologue` drops by **less** than
> `vfs_stat_breakdown_ns` does in absolute cycles, because the prologue's own
> `normalize_path` still allocates and is untouched. *Falsified if the prologue
> drops by as much or more*, which would mean the two benchmarks are not
> measuring the nested quantities the breakdown claims they are.

Reading the code the two benchmarks actually call (`kernel/src/fs/vfs.rs:1555`):

```rust
pub(crate) fn resolve_prologue(path: &Path) -> KernelResult<PathBuf> {
    let ns_path = crate::ipc::namespace::resolve_path(path)?;   // == the _ns benchmark
    let path = ns_path.as_path();
    validate_path(path)?;
    Ok(normalize_path(path))
}
```

`prologue` **strictly contains** `ns`: it is `ns` plus `validate_path` plus
`normalize_path`. So if the `Cow` removes one allocation from inside
`namespace::resolve_path`, and A2 is untouched, both benchmarks lose *the same
absolute amount*. Equal absolute drops are what correct nesting **predicts**.

The registered clause has this exactly backwards. It calls the equal-drop case a
falsification and names "the two benchmarks are not measuring the nested
quantities" as the thing that case would demonstrate — when in fact an equal drop
is the *signature* of the nesting being right, and a prologue drop substantially
**smaller** than the `ns` drop would be the anomaly needing explanation.

What the wording was probably reaching for is the *percentage*: the same absolute
saving is a smaller fraction of the prologue's ~580 ns than of the `ns` phase's
~261 ns. That claim is true, but it is arithmetic, not a prediction — it follows
from the two baselines alone and cannot fail.

So P21(b) does not separate any outcomes. "Less" and "equal" are divided by a
boundary that run-to-run noise straddles, and both readings are consistent with
the code being correct. **It will be graded UNGRADEABLE, not confirmed and not
falsified** — and that grade is recorded here *before* the measuring boot
finishes, because a prediction rewritten after its number is known is worth
nothing.

The replacement, for the next time this decomposition is touched: the prologue's
absolute drop should land **within measurement noise of the `ns` phase's**
(baselines: 261/262 ns across two idle runs, so noise is well under 5 ns on that
phase). A prologue drop materially *smaller* than the `ns` drop would mean the
saving is being partly re-absorbed inside A2 — plausibly by allocator ordering,
since removing one allocation changes what state the next one meets — and that is
a real, falsifiable claim about a real mechanism.

**The lesson, which is the point of registering these at all:** P21(a) was
derived from a mechanism (an allocation on a no-op path) and is gradeable.
P21(b) was derived from a *feeling* that the nested benchmark ought to move
less, and the "because" clause attached to it was never checked against the four
lines of code it describes. A prediction whose stated falsification condition is
its own confirmation is the same defect this file keeps recording one level down:
**a check that cannot fire is indistinguishable from a check that passes.**

---

#### P16 instrument landed — first reading, and why it does NOT grade P16

`59d7cfc61` added the HPET-vs-TSC discriminator. Its first output, from the
release boot at `24a3407cc`:

```
[spawn]   sleep clocks: HPET 84338300 ns vs TSC/clock_realtime 84001888 ns across
          the child's lifetime (HPET/TSC = 1.00x)
[spawn]   -> AGREE within 5%: both oscillators saw the same interval ...
[spawn]   fastpy-on-SlateOS `sleep` ... : OK
```

**This run passed.** P16 is registered against "a boot where the child reports
< 40 ms", and this child reported well over it. So the reading is *not* evidence
for cause (1) and the "AGREE" line it printed must not be read as a verdict on
the bug — on a passing run the clocks agreeing is unremarkable, because nothing
was anomalous for them to disagree about. Banking it as a confirmation would be
the identical error recorded a few sections up for the P20 load run: reading a
result as confirming the hypothesis it happens to sit next to.

What it *does* establish is the **control arm**, which the instrument previously
had none of: on a healthy boot the two oscillators agree to within 0.4%
(84 338 300 vs 84 001 888 ns). That matters, because it rules out the boring
explanation in advance — the two clocks are not chronically skewed on this host,
so if a *failing* boot shows them diverging, the divergence is specific to the
failure rather than a standing property of the machine. Without this reading, a
1.36x ratio on a failing boot could not have been distinguished from a machine
where the ratio is always 1.36x.

P16 therefore remains **unresolved and awaiting a failing boot**. The test is
intermittent, so this is a matter of accumulating boots, not of doing anything
further to the instrument. Both branches are now reachable and both say which
subsystem to open.

Baselines for P21(a) now stand at three idle release runs — `vfs_stat_breakdown_ns`
= 262, 261, 262 ns — putting run-to-run noise on that phase under 0.5% and the
20% threshold far outside it. `vfs_stat_breakdown_prologue` = 580, 568 ns across
the two runs that recorded it (noise ~2%).
