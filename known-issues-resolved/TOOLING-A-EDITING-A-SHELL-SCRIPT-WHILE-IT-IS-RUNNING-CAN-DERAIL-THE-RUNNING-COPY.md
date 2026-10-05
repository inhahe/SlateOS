### [A] TOOLING-A-EDITING-A-SHELL-SCRIPT-WHILE-IT-IS-RUNNING-CAN-DERAIL-THE-RUNNING-COPY — ⚠️ HAZARD, avoided 2026-08-15

**Status:** not a bug in the repo; a standing hazard in how we work. Recorded
because it was very nearly hit today and the failure would have been baffling.

**The hazard.** `bash` does not read a script into memory and then run it. It
reads it *incrementally*, remembering a byte offset into the file. If the file
changes underneath a running shell, the shell resumes at its saved offset in
the **new** bytes — which no longer mean what they did. The result is not a
clean error: the shell executes whatever fragment now lives at that offset,
producing syntax errors from lines that are perfectly valid, or, much worse,
silently running a *different* command than the one on that line.

**How it nearly happened.** `./scripts/boot-test.sh --bench` was running in the
background (a ~17 min job). The next queued task was to add a free-space floor
to `scripts/boot-test.sh` — i.e. to rewrite, in place, the exact file a live
bash was mid-way through interpreting. Whether it corrupts depends on an
implementation detail nobody should have to reason about: an editor that writes
to a temp file and `rename()`s over the target is safe (the running shell keeps
the old inode), while one that truncates and rewrites in place is not. Our
editing tool's behaviour on Windows is not guaranteed to be the former.

**What makes it nasty.** The damage lands in the *long-running background job*,
not in the edit, so the two are separated by minutes and look unrelated. A
boot test that dies with a syntax error at line 900 of a script that `bash -n`
declares perfectly valid is a genuinely hard thing to diagnose, and the natural
first hypothesis — "the script is broken" — is wrong.

**Prescription.** Before editing any script, check whether a background task is
currently executing it, and wait. This is cheap: the queued edit loses a few
minutes; the alternative costs a confusing debugging session and a wasted run.
The same applies to `scripts/run-timeout.py` and any other harness file, and it
is *most* dangerous for exactly the files worth improving — the long-running
harnesses, which are running precisely when you have the idle window that
tempts you to improve them.

#### P16 control arm — second reading, now spanning both build profiles; and why today's two failing boots do NOT grade it

The debug boot at `6deaa847e` (2026-08-15) printed:

```
[spawn]   sleep clocks: HPET 119276000 ns vs TSC/clock_realtime 120088295 ns
          across the child's lifetime (HPET/TSC = 0.99x)
```

**This is a control reading, not a grading run** — the child reported 120 ms,
far above P16's < 40 ms trigger, so nothing anomalous existed for the clocks to
disagree about.

What it adds is that the control arm now spans **both build profiles**: 1.00x on
a release boot (84.3 ms child) and 0.99x on a debug boot (120.1 ms child). The
child's lifetime differs by 43% between the two — debug is slower, as expected —
and the two oscillators tracked each other through both. That is a stronger
statement than one profile could make: HPET-vs-TSC agreement is not an artefact
of a particular build's timing, so a divergence on a failing boot remains
attributable to the failure.

**Two boots failed today and neither one grades P16.** Break-tests #1 and #2
(deliberate mutations of `test_valid_entries` and `CapEntryInfo`) both died in
the capability self-test at serial line ~1071 — roughly 5 500 lines *before* the
spawn phase, so neither boot ever ran the sleep test at all. Counting them as
"failing boots" toward P16 would be precisely the error this entry already
records against the P20 load run: treating a failure as evidence about the
hypothesis it happens to sit near, rather than the one it actually exercises.
P16 needs a boot where **the sleep test itself** fails; a boot that failed
earlier for an unrelated, self-inflicted reason is not a sample of that
population.

P16 accordingly remains **unresolved and awaiting a qualifying failing boot**.
