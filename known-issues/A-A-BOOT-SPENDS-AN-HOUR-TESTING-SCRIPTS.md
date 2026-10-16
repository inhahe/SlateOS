### A-A-BOOT-SPENDS-AN-HOUR-TESTING-SCRIPTS -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- a cost on every lane's boot, measured on lane A's.

**In short:** a boot test of a kernel change spends about an hour before
QEMU starts, almost all of it re-testing the boot test's own scripts, which
the change did not touch. Boot rq31 (2026-10-02, lane A) took 4036 s: gates
3736 s (93%), build 221 s, QEMU 30 s. The largest gates were script test
suites: `test-selftests-are-repo-safe.py` 769 s, `test-checkers-honour-head.py`
495 s, `test-boot-test.py` 256 s, `test-pre-push-fmt-gate.py` 139 s,
`test-pre-push-doclinks-gate.py` 136 s -- each run untraced ("gate cache:
SKIP") because it starts other processes, whose reads the cache cannot see.
36,353 processes in all. Each such boot also blocks the machine for other
work: a compile run beside the gates made one time out (rq30).

**Where:** `scripts/boot-test.sh` (the gate sequence) and the gate cache
(`scripts/run-checker.sh`, design-decisions 979); the suites themselves,
owned by lanes A and B.

**Proper fix**, to be decided with the suites' owners: either (a) a suite
declares the files it exercises -- the scripts it tests, discovered as it
already discovers them -- and the cache skips it while those are unchanged
since a pass (a declared input missed is a stale pass, which is the risk the
cache's rule exists to avoid, so the declaration would need its own check),
or (b) the independent suites run in parallel, each in its own temp tree,
which keeps every run and spends cores instead of wall time.
