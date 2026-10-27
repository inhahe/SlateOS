## A-PUSH-SUITE-GATE-RUNS-NO-SUITE-FOR-A-CHANGED-SUITE-OR-SHARED-HELPER (lane A, 2026-10-21) — OPEN
**Status:** OPEN

**In short:** when a push changes a script under `scripts/`, the push hook runs
that script's own test suite (`test-<name>.py`) and nothing else. So a push that
changes a test suite runs no suite at all, and a push that changes a helper
other suites import (`gitenv.py`, `srcload.py`, `msysbash.py`, `shellfunc.py`)
runs only the helper's own small suite, never the suites that depend on it. A
mistake there is caught later, by the boot test, which runs every suite.

**Where:** `scripts/hooks/pre-push`, gate 20's loop over `changed_scripts`:
`test-*) continue ;;   # a suite is not its own subject`, and the pairing by stem
(`scripts/test-<stem>.py`).

**Seen:** the review of `ebcc9ca3e` (2026-10-21), which moved six suites onto
the new `scripts/shellfunc.py`: none of the six ran at push time, and a later
change to `shellfunc.py` would run only `test-shellfunc.py`.

**Fix:** in gate 20, run a changed suite itself, and for a changed module also
every `scripts/test-*.py` that imports it (`^import <stem>\b` or
`^from <stem> import`), each once. Measure the push-time cost first: `gitenv`
is imported by most suites, so a change to it would run most of them -- which
may be the right answer for a module that many suites trust.
