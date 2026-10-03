### TD-OILS-THE-JOBS-LIFETIME-CORPUS-CASE-IS-FLAKY-UNDER-LOAD. `jobs` read `Running` where bash read `Done` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/tests/corpus/jobs-lifetime.sh`.

**What.** One full corpus sweep reported `482 matched, 1 failed`, the failure
being line 19 of this case: bash printed `[1]+ Done sleep 0.05` and osh printed
`[1]+ Running sleep 0.05 &`. The case passed alone immediately afterwards
(`1 matched, 0 failed`). Same shape as the unit-test flake above, and the same
cause: a sweep runs 483 cases through *two* shells, so the machine is saturated
with short-lived processes and a spawn can take longer than the margin allowed.

**Why.** The case's setup is timed even though its subject is not. `sleep 0.05 &
sleep 0.3; jobs` is a gate asserting "that job is finished by now", and its
margin was only 0.25 s — but the thing that consumes the margin is not the
sleeps, it is the *process spawns* between them. The file already carried this
diagnosis for two of its lines (a comment sizing them at 1.2 s for exactly this
reason); it was simply never applied to the other fifteen, and the header still
claimed "Nothing here turns on timing".

**Fixed by** sizing every gate for ≥0.5 s of margin on each side it asserts: the
one-sided gates went 0.3 s → 0.6 s (15 lines); the two-sided gate that needs a
second job still running went 0.2/0.4 s → 0.6/1.5 s; and the `wait %2`-gated
line's surviving job went 0.9 s → 1.5 s. The header now states the assumption
instead of denying it, and each two-sided gate names its margins. Cost: the case
runs ~16 s per shell rather than ~11 s, well inside its `# TIMEOUT: 60`.

**Not fixed by** a harness retry. `measure()` already re-runs a case that times
out or is refused a process — neither of which is an answer — but a *content*
mismatch is an answer, and retrying it would mask real divergences.
