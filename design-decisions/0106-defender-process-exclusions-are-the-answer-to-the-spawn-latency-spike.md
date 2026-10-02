## §106 — Defender process exclusions are the answer to the spawn-latency spike

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended this option). This was Q38 "Add
antivirus exclusions so the osh corpus sweep is runnable again?" in
`open-questions.md`.

**Context.** Process creation on the development host costs **~360–390 ms per
spawn** through the MSYS runtime — roughly 20× normal, stable across
back-to-back measurements over four days (`bash -c 'for i in $(seq 1 100); do
/usr/bin/true; done'` takes 36–41 s; re-measured 2026-08-07 at 36.1 s). That
makes `scripts/osh-bash-diff.py` unusable as a gate: cases that should take a
second take 13–53 s against a 20 s budget, and the *reference* shell is what
times out. A sweep on 2026-08-07 returned 41 failures, every one of them a
never-finished case rather than a real diff.

**Decision.** Add Windows Defender **process** exclusions for `bash.exe` and
`osh.exe`, scoped as narrowly as they will go, rather than blanket path
exclusions over the Git and `target\` trees — that keeps ordinary file scanning
intact.

**Action still outstanding — this needs the operator.** Defender's exclusion
list cannot be read or written without elevation, and the automation runs
unelevated (`Add-MpPreference` returns "You don't have enough permissions").
The exact command, to run once from an **Administrator** PowerShell:

```powershell
Add-MpPreference -ExclusionProcess 'bash.exe','osh.exe'
```

Until that is run the sweep stays a soft gate: timeouts are discriminated from
real regressions by timing the case under bash alone.

**Alternatives considered.**
- *B — diagnose further before excluding anything.* The cause is not proven:
  Defender was equally on during a green 444-case sweep on 2026-08-06 at 05:15,
  so something changed. Rejected because it costs operator time and the sweep
  stays unusable meanwhile; the exclusion is cheap to undo if it turns out not
  to be the cause.
- *C — live with it,* relying on the unit suite plus targeted single-case runs.
  This is what was happening meanwhile. Rejected as the standing answer: the
  unit suite does not compare against real bash at all, and single-case runs
  cannot catch a regression in a case you did not think to run.

**Where it lives.** `scripts/osh-bash-diff.py` (`CASE_TIMEOUT = 20` and the
`# TIMEOUT: N` per-case override). Note the fix is *not* raising `CASE_TIMEOUT`:
that would make every genuine hang cost minutes instead of seconds.

**How to reverse.** `Remove-MpPreference -ExclusionProcess 'bash.exe','osh.exe'`
from an elevated shell.
