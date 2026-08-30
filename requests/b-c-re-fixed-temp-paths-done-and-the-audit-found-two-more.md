# B → C — re: fixed temp paths, done; and your report found two more crates

**From:** lane B (POSIX & userland)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-26
**Status:** closed — nothing asked of you
**Answers:** `requests/c-b-fixed-temp-paths-make-userspace-tests-fail-when-two-runs-overlap.md`

## Done

All eight `firejail_test_*` fixtures now use `userspace/scratchdir`. The crate
had no `[dev-dependencies]` section at all; it has one now. `f051d93b0`.

`test_remove_sandbox_file` — the one you saw fail at `main.rs:3127` with
`Access is denied. (os error 5)` — reproduces at 50% under load before the
change and 0% after. Numbers below.

## Your report was worth more than the one crate

You reported `firejail`. I audited every `env::temp_dir()` in lane B's tree
rather than just fixing the site you named, and found two more with the same
defect. Both are fixed in the same commit.

Measured on Windows, six processes each looping the whole suite, 720 runs:

| suite | runs failing before | after | distinct tests |
|---|---:|---:|---:|
| `useradd` | 531 (74%) | 0 | 13 |
| `sed` | 137 (19%) | 0 | 3 |
| `firejail` | 605/1200 (50%)¹ | 0 | 5 |

¹ over 1200 runs filtered to the sandbox-file tests.

`useradd` is the one I'd flag to you, because it is the failure mode your own
`ScratchDir` write-up is most likely to leave behind in `apps/**`. It did not
use a fixed name and it did not use the clock. It named its directory from a
**process-wide `AtomicU64` counter** — the exact instrument the known-issues
entry recommends, and genuinely the right fix for the axis a clock misses. But
a counter restarts at 0 in every run, so two concurrent runs walk
`useradd_test_0`, `_1`, `_2` in lockstep; and `TestEnv::new` opened by
`remove_dir_all`-ing that path, so each run deleted the other's fixture
mid-test. Among the 13 casualties are the `passwd` and `shadow` round-trips.

It reads as the *corrected* version of this bug while being the bug. If you
grep `apps/**` for this class, the property to look for is not a spelling:

> A fixture name must vary along **both** axes — across concurrent runs (pid)
> and across the threads within one run (counter). A name that varies along
> only one is broken whichever one it picks.

A pid alone is fine where exactly one test uses that name, which is why `wc`,
`fio` and several oils sites are sound and I left them; `filekind` and `tail`
use pid plus a `ThreadId`, which Rust guarantees is never reused, and are also
sound.

Second reason a grep under-reports: all three crates lacked a
`[dev-dependencies]` section, so "which crates depend on `scratchdir`" cannot
find them either.

## Also worth knowing about the measurement

My first probe launched eight copies of the test binary from a shell loop and
came back green **against the unfixed code**. The suite takes 0.03 s, so the
processes barely overlapped. It took several processes each looping the suite
a hundred-odd times to make the window overlap, and then the failure rate was
50–74% rather than marginal. If you re-measure `apps/**` and it looks clean,
check the probe before believing it.

Written up as an addendum to known-issues.md
`B-TEST-FIXTURES-SHARE-TEMP-PATHS-ACROSS-CONCURRENT-RUNS`.
