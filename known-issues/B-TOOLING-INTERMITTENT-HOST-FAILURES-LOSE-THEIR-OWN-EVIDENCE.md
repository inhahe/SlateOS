## B-TOOLING-INTERMITTENT-HOST-FAILURES-LOSE-THEIR-OWN-EVIDENCE — OPEN 2026-09-01

**In short:** two different pieces of the build tooling failed once each on
2026-09-01, and both passed on an immediate retry with no change in between. Both
were *host-side* failures rather than anything in the code under test. Neither
left usable evidence, which is the part that makes this an entry rather than a
shrug: a flake you cannot inspect is indistinguishable from a real bug that only
fires sometimes, and re-running until green is how the second kind gets shipped.

**Failure 1 — `scripts/test-build-usb-image.py`.** During a full
`./scripts/boot-test.sh` run it exited non-zero, and the harness correctly
refused to build:

```
ERROR: refusing to build.  1 tooling test suite(s) failed:
    test-build-usb-image.py
```

Run directly afterwards it passed all 6 groups, including under the exact
conditions `boot-test.sh` uses (`PYTHONIOENCODING=:replace python -u`, stdout a
pipe rather than a console).

*Why this should not be able to happen.* The suite is deterministic by
construction. `scripts/build-usb-image.py` pins every FAT directory entry to a
fixed epoch (`FIXED_FAT_TIME`/`FIXED_FAT_DATE`) and derives the disk GUID,
partition GUID and volume serial from a SHA-256 of the staged content, precisely
so the image is a pure function of its inputs. The test's `make_tree` is entirely
synthetic — no repo file, no clock, no randomness — and totals about 400 KiB.
Disk space is not a plausible cause either: the images go to `%TEMP%` on `C:`,
which had 502 GB free.

*Why the evidence is gone.* `boot-test.sh` does print the whole failing output
(`--- <name> FAILED (exit N) ---` followed by the captured stdout+stderr, and
deliberately not a tail, "these suites print one line per assertion and the
failing one is rarely last"). The harness did its job. The loss was downstream:
the suites run in alphabetical order, `test-build-usb-image.py` is near the
front, and the background-task output file this run was read from keeps only the
last N lines — so the block had scrolled off by the time the run ended. The
suite's `main` also `rmtree`s its temp directory in a `finally`, so nothing
survived on disk to inspect.

**Failure 2 — rustc crashed compiling the kernel.**
`cargo check --workspace --target x86_64-pc-windows-gnu` died with

```
process didn't exit successfully: `rustc.exe --crate-name kernel ...`
  (exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN)
```

An immediate re-run of the same command finished clean in 7m51s. `0xc0000409` is
a compiler crash, not a diagnostic — the code cannot cause it and cannot fix it.

**Why they are filed together — and the mechanism, now confirmed.** The initial
guess here was "probably environmental, probably memory pressure", written down
as a hypothesis. It was confirmed within the hour by a third failure that named
the cause outright. `cargo test -p coreutils --target x86_64-pc-windows-gnu`,
run while a `cargo clippy` on the same crate was still going, died with:

```
error[E0786]: found invalid metadata files for crate `coreutils`
  = note: failed to mmap file '...\libcoreutils-....rlib':
          The paging file is too small for this operation to complete. (os error 1455)
...
rustc-LLVM ERROR: out of memory
Allocation failed
```

So the host exhausts memory when two rustc-heavy jobs overlap, and the failure
surfaces as whatever the loser happened to be doing: an mmap that cannot be
backed, an LLVM allocation that returns null, or — failure 2 above —
`STATUS_STACK_BUFFER_OVERRUN`, which is what a rustc dying of memory exhaustion
looks like from outside the process. Failure 1 fits the same shape: it builds a
dozen 384 MiB images while a full workspace build is running.

**The operational rule this yields: do not overlap rustc-heavy jobs on this
host.** Run `cargo build`, `cargo test`, `cargo clippy` and `boot-test.sh` one
at a time, even though each is slow and the temptation to background one and
start the next is constant. Two of the three failures above were caused by
exactly that temptation, including by the session that wrote this entry. A
backgrounded `cargo` is not free concurrency here — it is a second job competing
for a page file that cannot cover both, and it corrupts the result of the job
you were actually waiting on.

**Failure 4 — `git push` refused by gate 8, and the same tree passed it minutes
later.** Same day, same session. `git push origin lane-b` was backgrounded
*while the boot test was still building* — the exact thing the rule above had
just been written to forbid — and exited 1 with gate 8's refusal:

```
pre-push: REFUSING to push — a diagnostic above puts a file name straight into
its message.
...
To push anyway:
    ALLOW_UNQUOTED_NAMES=1 git push origin ...
```

Run afterwards on the unchanged tree, `scripts/quote-names.py --selftest`
passed 56/56 and `--check` reported `ok -- 0 known sites in 0 files`; the push
then succeeded with all ten gates green. Nothing was edited in between.

*The cause could not be determined, and that is the finding.* The background
task's output file keeps only its tail, so `head -60` and `tail -25` returned
the same block of boilerplate — the gate's standing explanation of what
unquoted names are — and the lines that would have said *which file and which
site* had scrolled off. Two hypotheses remain live and the evidence to separate
them is gone: either the checker crashed (a `MemoryError` or a failed read under
the same host pressure as failures 1–3), or it genuinely found a violation in a
source file that a concurrent process had left half-written when
`Path.read_text` reached it. The cost was not the lost push: it was two hours
spent designing a fix for a defect in newly-written `cp` code that, it turned
out, was never there.

*One real defect did fall out of the post-mortem, independent of which
hypothesis is right.* Every gate asks its checker `if ! "$py" "$script"
--check`, and a Python script that dies of an uncaught exception exits 1 —
the same code it uses for "I found violations." So a checker that **crashes**
is reported to the operator as a **defect in their code**, in the gate's own
confident wording, and the only remedy the message offers is the bypass
(`ALLOW_UNQUOTED_NAMES=1`), which would push with that gate disabled. A gate
that cannot tell "your code is wrong" from "I fell over" spends its credibility
on the wrong verdict and trains its reader toward the bypass — which is
precisely the failure the hook's own comments elsewhere say a gate must not
have. Fixed by `scripts/hooks/pre-push`'s `run_checker` helper, tested by
`scripts/test-pre-push-run-checker.py`; see design-decisions.md 746.

**What to do when any of these recurs.** Do *not* re-run until green and move on.

1. Capture the run's output to a file in full rather than reading a truncated
   background-task tail — `./scripts/boot-test.sh 2>&1 | tee /tmp/boot.log` —
   so the `--- FAILED ---` block survives.
2. For the image suite specifically, comment out the `shutil.rmtree` in
   `main`'s `finally` (`scripts/test-build-usb-image.py`) for the reproducing
   run and diff the images it left behind. `test_reproducible_and_sensitive`
   writes `a.img` and `b.img` from one tree; if those two differ, the builder
   has a nondeterminism the pinning above was supposed to have removed and that
   is a real bug in `build-usb-image.py`.
3. Note whether the host was building concurrently, and how much RAM was free.

**Not blocking.** Both retried green, and the boot test's refusal-to-build on a
tooling failure is the correct behaviour — it is what surfaced this at all,
rather than letting a run produce numbers from a harness that had just failed
its own test.
