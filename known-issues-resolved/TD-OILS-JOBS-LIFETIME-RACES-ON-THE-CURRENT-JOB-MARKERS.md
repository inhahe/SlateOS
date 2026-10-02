### TD-OILS-JOBS-LIFETIME-RACES-ON-THE-CURRENT-JOB-MARKERS. `jobs-lifetime.sh` disagrees about `+`/`-` about 1 run in 5 — 2026-08-08 — ✅ FIXED 2026-08-08 (the case's margin, not the marker code)

**Where:** `userspace/oils/tests/corpus/jobs-lifetime.sh` line 68. The marker
code in `userspace/oils/src/interp.rs` — `note_new_job`, `newest_running_before`
and `reset_job_markers` — was suspected and is exonerated; it is unchanged.

**What.** A full sweep failed this one case where the immediately preceding
sweep, on the *same binary*, passed it. Re-run in isolation: **1 failure in 5**.
The divergence is entirely in the markers — every line of text is otherwise
identical:

```text
line 68  `sleep 0.05 & sleep 0.06 & sleep 0.6; jobs %1; echo "--"; jobs; disown -a`
           bash: [1]-  Done … sleep 0.05      then  [2]   Done … sleep 0.06
           osh : [1]   Done … sleep 0.05      then  [2]+  Done … sleep 0.06
```

So bash had job 1 marked `-` and, after `jobs %1` dropped it, showed job 2 with
*no* marker; osh had job 1 unmarked and job 2 still `+`. The case's own comment
states the rule being tested: "Dropping the `-` job leaves the `+` job in place,
yet the listing that follows shows no marker at all: with nothing running there
is nothing to re-mark."

**First diagnosis (2026-08-08) — wrong, and recorded here because the way it
was wrong is worth keeping.** It read: *this is osh's bug, not the case's*, on
the grounds that bash was stable under the same perturbation while osh flipped
~15% of the time, always to the same wrong answer — "a case with margins too
thin for the host would make *both* shells wobble", and "an always-identical
wrong answer is not the signature of jitter, it is a second code path". Both
halves are bad reasoning. The observable here is **binary** — job 1 is either
alive when job 2 is created or it is not — so jitter can only ever produce one
wrong answer, and identity proves nothing. And two shells with different spawn
latencies do not have to wobble at the same margin, or at all.

**What it actually is: the case's margin, and it was measured.** The line was
instrumented with `$EPOCHREALTIME` on both sides of each `&`, and the failing
run was caught in the act:

```text
   t1-t0    t2-t1   the listing taken immediately after both spawns
     5.1    145.8   [1]  Done sleep 0.05  /  [2]+ Running sleep 0.06 &
     …        7-10  [1]- Running sleep 0.05 & / [2]+ Running sleep 0.06 &   (48 runs)
```

`t2-t1` is the cost of creating job 2. It is normally 7–10 ms, and in the one
failing run of 49 it was **145.8 ms** — nearly three times job 1's entire 50 ms
life. So job 1 really had finished before job 2 existed, `newest_running_before`
really had nothing to return, and osh's `note_new_job` did exactly what bash's
`set_current_job` does with that table (jobs.c:3425). No second code path.

bash's stability has a dull explanation too, also measured: over 40 runs its
*second* spawn cost 12–28 ms with no tail at all, while its *first* cost 92–162
ms every time. Both shells pay ~150 ms for a process on this host; bash happens
to pay it where nothing is timing it, osh pays it where something is.

**Fixed 2026-08-08** by giving the line the margin its neighbours already have:
`sleep 0.6 & sleep 0.61 & sleep 1.5` in place of `0.05 / 0.06 / 0.6`. 20/20
identical in both shells afterwards, where the old spelling was ~15% divergent.
The overlap the line asserts — job 1 running when job 2 is created — is now
0.6 s against a ~150 ms worst-case spawn, and the gate keeps 0.89 s on the other
side.

**This exact failure mode was already documented**, in `jobs-options.sh` lines
45–52: "At `sleep 0.05` the margin was the 50 ms between the two forks, and a
loaded machine spent that too — bash printed `[1] ` where osh printed `[1]-`."
That line was widened to 0.4 s at the time and `jobs-lifetime.sh` line 68 was
missed. `jobs-lifetime.sh`'s own header already states the rule ("what eats
margin is not the sleeps but the **process spawns** between them"); line 68 was
simply the one place that did not follow it. **Rule for new timing cases: an
assertion about one job being alive when another is *created* needs ≥ 0.5 s,
because that margin is spent by a process spawn, not by anything in the script.**

**Found by** the post-Defender-exclusion timing sweep, 2026-08-08 — not by any
code change; the binary was byte-identical to the one that had just passed a
full sweep.
