## A-FIXTURE-CLEANUP-LEAVES-EMPTY-DIRECTORIES-IN-BUILD-AND-CANNOT-TELL-YOU (lane A, 2026-09-04) — FIXED 2026-09-04

**Status: FIXED the same day it was filed; see "Fixed, and the retry count settles the diagnosis" at the end. Filed as:** Cosmetic today, but the mechanism is not, and the mechanism is
this file's recurring one.

`build/` in the lane-A worktree currently holds **fourteen leaked directories**:

```
build/tmpbn11j9gi  build/tmpjdvgdtgm  build/tmpk7eblvhs  build/tmplcrx43y1
build/tmpp2zjzoj6  build/tmptsejfhu0  build/tmptvcr4ibw          (7, 2026-09-04 00:44)
build/tmp_huu_e07  build/tmpdbdgw0hw  build/tmpgi8c5lb2  build/tmphzx24y10
build/tmpiev_ii77  build/tmpnssaex3m  build/tmpusyrju0j          (7, 2026-09-04 01:31)
```

Two runs, seven each. **Every one of them is empty.** That is the diagnosis,
not an aside: `shutil.rmtree` deleted the contents successfully and failed only
on the final `os.rmdir` of the directory itself — the classic Windows transient
sharing violation, an indexer or scanner still holding the directory handle a
few milliseconds after its last child went away. The retry that would fix it is
one loop; what there is instead is `ignore_errors=True`.

**Where.** Three sites in `scripts/test-boot-test.py`, all the same shape:

| Create | Clean up |
|---|---|
| `:534` `tempfile.mkdtemp(dir=fixture_root)` | `:586` `shutil.rmtree(tmp, ignore_errors=True)` |
| `:708` `tempfile.mkdtemp(dir=fixture_root)` | `:745` `shutil.rmtree(tmp, ignore_errors=True)` |
| `:842` `tempfile.mkdtemp(dir=fixture_root)` | `:872` `shutil.rmtree(tmp, ignore_errors=True)` |

with `fixture_root = os.path.join(REPO_ROOT, "build")` at `:532`, `:706`, `:840`.

**Two defects, and the second is the interesting one.**

1. **No `prefix=`.** These are the only three `mkdtemp` calls in the file that
   omit one; the other three (`:166`, `:199`, `:351`) pass
   `slateos-bash-probe-`, `slateos-boot-test-`, `slateos-elsewhere-` and go to
   the system temp directory. Without a prefix the leak is named `tmpXXXXXXXX`,
   which is (a) unattributable — nothing in the name says which script made it
   or why — and (b) unsweepable, because `build/tmp` is a *real* directory in
   this tree, created 2026-08-29 and unrelated to these fixtures, so the obvious
   `rm -rf build/tmp*` deletes it too. A leak you cannot safely glob for is a
   leak nobody will ever clean; nothing in `scripts/` sweeps these by name, and
   neither `prune-build-trees.py` nor `prune-build-cache.py` mentions `tmp` at
   all.

2. **`ignore_errors=True` makes a failed cleanup indistinguishable from a
   successful one.** This is the same shape as the five sightings already in
   this file of *a gate that discovers nothing reports no failures, which reads
   exactly like a pass*, moved from a gate into a teardown. The suite has passed
   every time while leaving fourteen directories behind, because the only signal
   was thrown away at the point it was produced. And the flag is not merely
   noisy-suppressing: it would equally swallow a fixture that failed to clean
   with its *contents* intact — a real disk leak, on a drive whose space is a
   standing project constraint — and report it identically to this harmless one.
   The empty directories are the benign end of a range the code cannot
   distinguish.

**Why it is worth fixing even though the leak is 0 bytes.** Because `build/` is
the first place anyone looks when a boot test misbehaves, and fourteen
identically-shaped mystery directories are exactly the kind of noise that makes
a real artifact hard to see; because the count grows monotonically, seven per
suite run, with no upper bound; and because the failure is silent by
construction, so the day it starts leaking something that is *not* empty, it
will report that in precisely the same way it reports today: not at all.

**The proper fix**, in the order it should be done:

1. Give all three sites `prefix="slateos-boot-test-fixture-"`. This is what
   makes every later step possible, and it is what distinguishes the fixtures
   from `build/tmp`.
2. Replace `ignore_errors=True` with a bounded retry — remove the tree, then
   retry the final `os.rmdir` a handful of times with a short sleep, since the
   handle is released within milliseconds. A directory that survives the retries
   is a real finding: **print it and fail the case**, do not swallow it.
3. Sweep the recognisable prefix at suite startup, so that a fixture orphaned by
   a killed run (Ctrl-C, `run-timeout.py` firing) is collected by the next run
   rather than accumulating forever. Step 1 is what makes this safe to write.
4. Delete the fourteen existing directories once step 1 has landed, not before —
   deleting them first only hides the evidence that the fix has to be verified
   against.

**How to confirm the fix.** Run `scripts/test-boot-test.py`, then check that
`ls -d build/slateos-boot-test-fixture-*` reports nothing and that the count of
`build/tmp*` entries is exactly one (`build/tmp` itself). Before the fix, the
same run adds seven.

**Related:** this is the same transient-file-handle class as **A-Q7** (the
antivirus exclusion question in `open-questions.md`). An exclusion for the
worktree would likely make the `rmdir` stop failing in the first place — but the
retry is correct regardless, since the fix must not depend on an operator having
configured a scanner.

### Watched it happen, 2026-09-04 03:27 — it is deterministic, not intermittent

The entry above was written from fourteen directories found after the fact. The
boot test running at the time then reached `test-boot-test.py` in its self-test
sweep, which gave a live observation instead of a reconstruction:

| | before | after |
|---|---|---|
| leaked fixture directories | 14 | **21** |
| all empty | yes | yes, 21 of 21 |
| `build/tmp` (the real, unrelated one) | 1 entry | 1 entry, untouched |

**Exactly seven more, all empty, in one run.** Three suite runs have now each
leaked exactly seven, so this is not an intermittent race that occasionally
loses — every fixture teardown in the file fails its final `os.rmdir`, every
time, and has been doing so for as long as anyone has looked. The suite reported
`PASSED` while doing it, which is the whole complaint.

That also sharpens the fix. A failure rate of 100% is not "the handle is
occasionally still open"; it is "the handle is *always* still open at the moment
we ask". So the bounded retry in step 2 must actually sleep between attempts
rather than spin — a zero-delay retry loop would fail all its attempts just as
reliably as the single try does now — and while the fix is being developed it
should print how many attempts it actually took. If that number turns out to be
larger than a handful, the assumption that this is a milliseconds-long window is
wrong, and the diagnosis needs revisiting before the retry is called a fix.

The three creation sites sit in helpers — `_run_clippy_gate` (`:506`),
`_run_prune_hook` (`:694`) and `_run_python_suites` (`:829`) — each invoked once
per test case, so the seven leaks are seven cases spread across the three, and
repairing the three `mkdtemp`/`rmtree` pairs covers all of them. There is no
fourth site: these are the only `mkdtemp(dir=…)` calls anywhere in `scripts/`.

### Fixed, and the retry count settles the diagnosis — 2026-09-04

All four steps landed, in the order the entry set out.

1. `FIXTURE_PREFIX = "slateos-boot-test-fixture-"`, applied through a single
   `new_fixture()` helper that replaces the three bare `mkdtemp(dir=build)`
   calls.
2. `drop_fixture()` replaces `shutil.rmtree(tmp, ignore_errors=True)`. It
   retries the whole `rmtree` — not just the final `os.rmdir` — so a fixture
   stuck on a *file* is retried too, sleeps `0.05 s × attempt` between tries,
   bounds at ten, and records what each teardown cost. It still never raises:
   a teardown that threw would fail a test that had already passed, which is
   the one thing `ignore_errors=True` got right. The difference is that a
   refusal is now recorded rather than discarded.
3. `sweep_stale_fixtures()` runs first in `main()`, collecting anything an
   earlier run could not — a suite killed by Ctrl-C or by `run-timeout.py`
   firing never reaches its own `finally`. It announces itself only when it
   finds something: a line that prints "0" on every clean run is a line nobody
   reads on the run where it says 3.
4. The twenty-one pre-fix leftovers are deleted. `build/tmp` — the real,
   unrelated directory — was checked and left alone, which is the whole reason
   step 1 had to come first.

**The measurement the entry asked for.** The prediction was that a survivor
after a sleeping retry would mean the milliseconds-long-window diagnosis was
wrong. The first run under the fix reports:

```
fixture teardown: 11 removed, 7 needed a retry, worst 2 of 10 attempts
```

**Seven needed a retry, and the worst case was the second attempt** — one 50 ms
sleep. Seven is exactly the number that used to leak per run, and eleven minus
seven is exactly the four teardowns that never had a problem. So the diagnosis
holds precisely: the handle is always still open when we first ask, and always
released well before we ask again. Nothing leaked; the run left zero
directories behind.

**What did not change, deliberately.** A fixture that survives all ten attempts
is reported as a named `WARNING` with its path, whether it still has contents,
and the underlying `OSError` — but it does **not** fail the suite. It is not a
`boot-test.sh` defect, and a red suite for a held file handle would be a flaky
red that gets bypassed by habit, which is how this tree loses gates. The
distance between that and the `ignore_errors=True` it replaces is the whole
point: the old code produced *no output at all*, so twenty-one directories
accumulated under a suite that printed `PASSED`. A named warning plus a sweep
on the next run is visible, greppable, and self-clearing.

**Still open, and out of scope here:** the other three `mkdtemp` calls in the
file (`:166`, `:199`, `:351`) also pass `ignore_errors=True`, but they write to
the *system* temp directory with explicit prefixes, so a leak there is the
platform's to collect rather than debris in a directory people read. They were
left alone rather than swept up silently, because whether they leak at all has
not been measured and a fix to something unmeasured is a guess.
