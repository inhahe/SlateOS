## TD-A-THE-HEAD-OF-LINE-WITNESS-CANNOT-RED-THE-BOOT (lane A, 2026-09-14) -- **FIXED 2026-09-25**

**Status:** FIXED (stamped 2026-09-25) -- `proc::spawn::run_persistent_netstack` now routes the witness's `Err` through `selftest::dispatch_debug("net::socket head-of-line", Severity::Diagnostic, ..)`, which prints the `self-test failed` marker the harness fails a run on, exactly as proposed below. Whether the witness *runs* rather than declines is a separate question its `Ok(None)` arm answers on every boot.

**In short:** the concurrency witness written today for `D-NETSOCK-SYNC` -- the
second witness 932 requires -- reports a failure into a channel nothing reads.
If the head-of-line bug came back, the boot would print a warning and pass.

**The chain, and it breaks at the last link.**

1. `net::socket::self_test_no_head_of_line()` returns `Err` on regression. Good.
2. `proc::spawn::run_persistent_netstack()` **propagates** it rather than warning
   -- which was the deliberate part, and is not enough.
3. `kernel/src/main.rs:9457` catches it:
   `WARNING: persistent userspace netstack (ring 3) startup failed: {:?}`
4. `scripts/boot-test.sh:353` fails a run by grepping the serial log for
   **`self-test failed`**, case-insensitive. The string above does not contain
   it. The run stays green.

**Why the mistake survived review -- mine.** The commit message states the intent
exactly: *"a regression in it must red the boot rather than print a line nobody
greps for"*. I verified step 2, that this dispatch propagates on `Err` unlike its
warning-only neighbours, and treated propagating-further-than-the-neighbours as
equivalent to reaching the top. It travels exactly one frame further and lands in
a `WARNING` with different wording. The neighbours warn because their failures
are *environmental* -- no DHCP lease, no IPv6 peer -- and that call site is right
to be lenient about daemon startup; the error just has no way to say "this one is
different".

**This is 937 substitution, self-inflicted.** A green boot would be a true report
about a real run that genuinely exercised the code -- and would say nothing about
the property the witness exists for. Worse than the original gap, because
`known-issues` would record the second witness as landed.

**The fix**, after the current boot frees `kernel/src`: dispatch the witness
through `selftest::dispatch_debug("Socket head-of-line", Severity::Diagnostic,
result)` at the call site in `run_persistent_netstack`. That prints
`WARNING: Socket head-of-line self-test failed: ...`, which carries the marker
the harness greps, so the run fails -- and unlike `Severity::Integrity` it does
not `halt_loop()`, so the remaining self-tests still run and one boot yields the
whole picture. Do **not** escalate `main.rs:9457` itself: that would turn every
environmental daemon-startup failure into a red boot, which is the lenience the
neighbours were right about.

**Checked whether this is systemic, and it is not -- population of about three.**
Searching `kernel/src` for `WARNING:` lines that report a failure without the
`self-test failed` marker returns two, one of which (`Buffer cache flush
failed`) is a legitimate environmental warning. So no gate is warranted: a
checker built for a population of one is a maintenance cost with nothing to
catch.

Two limits on that measurement, stated because the number is the whole basis
for not building a gate. The grep is single-line and **missed this very bug**,
whose `serial_println!` spans four lines -- so the true count is about three,
not two. And the first search tried, `if let Err(..) = ..self_test`, could
never have found it either: the swallowing frame is `run_persistent_netstack`,
whose name contains no `self_test`. The defect is a dataflow property -- an
`Err` crossing into a function whose own failure is worded differently -- and
grep matches text. Anyone revisiting this should not read "two" as a measured
population; it is a lower bound from a search that demonstrably misses the
shape it is looking for.
**Until then, `D-NETSOCK-SYNC` still has ONE witness.** The entry must not be
updated to say otherwise on the strength of this run passing.
