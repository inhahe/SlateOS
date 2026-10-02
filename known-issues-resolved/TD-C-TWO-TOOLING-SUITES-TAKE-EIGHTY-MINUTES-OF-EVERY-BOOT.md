## TD-C-TWO-TOOLING-SUITES-TAKE-EIGHTY-MINUTES-OF-EVERY-BOOT (lane C, 2026-09-25) -- FIXED the same day

**Status:** FIXED 2026-09-25 -- both suites run their cases a few at a time
through `scripts/suite_pool.py`. One further step is possible and not taken;
see the end.

**In short:** every boot test runs the tooling's own test suites before it
builds anything, and two of them took 83 minutes of lane C's 3.2-hour gate
phase on 2026-09-25: `test-checkers-honour-head.py` 3127 s and
`test-pre-push-fmt-gate.py` 1894 s. A boot test is the only road to `main` for
every lane, so that was paid six times over.

**What they spent it on.** Neither walks the real tree (that was
`check-cfg-unix.py`'s problem -- Lesson 405). Both are process-bound fixture
work that ran strictly one case after another: `test-checkers-honour-head.py`
builds a throwaway git repository per case and runs a checker twice against it
(124 cases); `test-pre-push-fmt-gate.py` runs the whole pre-push hook per case,
in two mirror modes. The cases share nothing -- no working directory, no
environment, a fixture each.

**The fix.** `scripts/suite_pool.py` runs a suite's cases on a small thread pool
(a third of the logical cores, at most four; `SUITE_JOBS` overrides, and `1`
restores the plain loop exactly), each case in a temporary directory of its
own, with each case's output buffered and printed in the list's order -- so the
log reads as a one-at-a-time run's does. The fmt suite's per-mode label suffix
moved from a module global to the pool's per-case context, since a global set
by one mode would have been read by a case of the other.

Measured on the same loaded machine (other lanes building):

| Suite | one at a time (boot test, 2026-09-25) | pool of 4 |
|---|---|---|
| `test-checkers-honour-head.py` | 3127 s | 861 s -- all 124 cases pass |
| `test-pre-push-fmt-gate.py` | 1894 s | 564 s -- all pass, labels per mode |

**Follow-up, 2026-09-26: a case's clean-up failed a boot.** A case's temporary
directory could not be deleted -- on Windows a `git` the case had run still held
a file in it (WinError 32) -- and `TemporaryDirectory`'s exception failed the
suite, and so the gate phase, over a case that had passed. `suite_pool` now
removes each case's directory with retries and a growing pause, making `git`'s
read-only objects writable as `tempfile` does, and leaves a directory it still
cannot remove with a line on stderr rather than failing anything. Its
self-test covers the read-only file, three failures then success, and a
removal that never works.

**Not taken: running one gate of the hook.** The fmt suite still runs the
whole hook for each case to exercise gate 7. A switch the hook honours only for
the gates it names would cut that further, but it is a change to
`scripts/hooks/pre-push`, which every lane edits; worth doing if the suite is
still near the top of the gate-phase table.
