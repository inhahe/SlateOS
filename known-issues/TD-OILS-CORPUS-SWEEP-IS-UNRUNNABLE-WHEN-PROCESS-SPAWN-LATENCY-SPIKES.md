### TD-OILS-CORPUS-SWEEP-IS-UNRUNNABLE-WHEN-PROCESS-SPAWN-LATENCY-SPIKES. A full sweep reports a cascade of `bash=-1` timeouts that are the machine's, not the shell's — 2026-08-06 — OPEN (environmental)

**Where:** `scripts/osh-bash-diff.py` (`CASE_TIMEOUT = 20`, the `# TIMEOUT: N`
per-case override), and any corpus case that spawns a lot of externals.

**What:** the sweep of 2026-08-06 05:58 recorded seven failures —
`a-a-collection-declaration-is-split-into-words`,
`a-a-name-enumeration-passes-over-a-bare-declaration`,
`local-dash-binds-a-variable-no-listing-reports`, `local-outside-function`,
`logout`, `nameref-declare`, `noassign-vars` — every one of them of the shape

```
status: bash=-1 osh=0
bash stderr: <timed out after 20s>
```

i.e. the *reference* shell timed out while osh completed with correct output.
None is a shell bug and none is a regression: the timings below were taken with
osh not running at all.

**Measured (idle machine, 2026-08-06 06:20–06:35):**

| what | wall | note |
|---|---|---|
| `noassign-vars.sh` under bash | 29.4 s | but only 0.59 s user + 1.24 s sys |
| `local-outside-function.sh` | 13.4 s | |
| `local-dash-binds-a-variable-…` | 22.6 s | already over the 20 s default |
| `a-a-collection-declaration-…` | 41.9 s | |
| `a-a-name-enumeration-…` | 42.8 s | |
| `nameref-declare.sh` | 44.4 s | |
| `logout.sh` | 53.2 s | |
| `bash -c 'for i in $(seq 1 100); do /usr/bin/true; done'` | 36.1 / 41.4 / 38.4 s | **~390 ms per spawn** |

The 29 s wall against 1.8 s of CPU is the whole story: bash is not computing,
it is waiting on process creation. ~390 ms per `fork`+`exec` through the MSYS
runtime is roughly **20× the normal cost on this machine**, and it is
reproducible across back-to-back runs, so it is a standing condition and not a
momentary spike. A sweep is ~444 cases; at that spawn cost it neither finishes
in reasonable time nor produces trustworthy results.

**Why it cascades.** Each timed-out case leaves its bash tree behind — the
run of 05:58 had grown to 16 live `bash.exe` processes, of which 8 went away
the moment the sweep's own PID tree was killed. So the failures are
progressively *more* likely the longer the sweep runs: the alphabetical run of
consecutive victims from `local-*` through `noassign-*` is that, not a family
resemblance between the cases.

**Not the cause (checked):** Defender real-time protection is on, but it was
equally on during the green 444-case sweep at 05:15 the same morning; the
`ftrace.exe` GPU render belonging to a concurrent session started at 06:22,
*after* the first timeouts at 05:58; and no process was consuming meaningful
CPU during the 390 ms/spawn measurement (top consumer ~13%).

**What to do:**

1. **Do not read a `bash=-1` failure as a regression.** Discriminate first:
   `status: bash=-1` with osh's stdout present and correct means the harness
   timed the *reference* out. Confirm by timing the case under bash alone,
   with osh uninvolved.
2. **Re-measure spawn cost before trusting a sweep.** The one-liner in the
   table above is the canary; under ~50 ms/spawn the 20 s default is fine.
3. **The proper fix is not to raise `CASE_TIMEOUT`.** Papering over a 20×
   environment regression with a bigger budget makes every genuine hang cost
   minutes instead of seconds. `# TIMEOUT: N` remains right for cases that are
   intrinsically long (the TD-OILS-CORPUS-LOAD-SENSITIVE-CASES precedent) —
   `local-dash-binds-a-variable-no-listing-reports` at 22.6 s is arguably one
   even on a healthy machine — but the seven above are not intrinsically long.
4. **The environment fix needs the operator**, since it is system-wide and
   needs admin: add Defender process/path exclusions for
   `C:\Program Files\Git\usr\bin\`, the repo's `target\` tree and `osh.exe`.
   Logged rather than done for that reason.
