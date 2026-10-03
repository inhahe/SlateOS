## §923 — The 70 ms per file was the disk after all, and the E: move fixed it; do not ask for an antivirus exclusion

**Date:** 2026-09-09. **Decided by:** Claude (autonomous), closing the
re-measurement §921 recorded as pending. **Lane:** A.

**In short:** six days ago this project asked the operator to turn off virus
scanning for the project folder, on the grounds that opening a file here cost
about 70 milliseconds and that the antivirus was the only explanation left.
That was wrong. Re-measuring now — same machine, same files, both drives — the
cost is the *disk*: reading the 807 kernel source files costs 19.3 s on the old
hard drive when they are not already in memory, and 0.27 s on the SSD. Once
they are in memory both drives cost the same. Moving the project to `E:` in
September already fixed it, and **no exclusion should be requested**, for `E:`
or anywhere else — it would weaken a system-wide security setting to buy
nothing.

### What was measured

`bench/file-read-latency.py`, 807 `.rs` files under `kernel/src`, the same
content present on both drives, trees alternated pass to pass so neither gets a
systematic warming advantage:

| | not in memory | already in memory |
|---|---|---|
| `D:` (hard drive) | 19.3 s total, **20.9 ms** median per file | 0.19 s, **0.20 ms** |
| `E:` (SSD) | 0.27 s total, **0.21 ms** median per file | 0.22 s, **0.17 ms** |

Cold, the SSD is **71× faster**. Warm, the two drives are indistinguishable —
which is the point: the cost tracks *the device*, and vanishes entirely once
the device is out of the loop.

### Why the original conclusion was wrong

A-Q7 ruled out the disk and the cache on a single measurement: *"A second full
pass over all 805 files, immediately: still 61.8 s."* If a warm pass really
cost the same as a cold one, the storage could not be responsible and something
inspecting each open would be the natural suspect.

**That measurement does not reproduce.** The same second pass over the same
files on the same drive costs **0.19 s** — 325× less than 61.8 s. A warm cache
helps by about 100×, so the one observation that eliminated the disk was the
one that was wrong, and everything downstream of it followed.

Two other things independently falsify the antivirus story, and both were
available at the time:

- The operator reported that `D:\visual studio projects` **was already
  excluded** from scanning. The cost was being paid inside an exclusion.
- 37 MB of source cannot fail to stay in a cache on a machine with this much
  memory, so "a warm cache changes nothing" should not have been believed
  without asking why it was not warm.

I cannot say what the 61.8 s actually was. The plausible candidate is the
condition the operator described — CPU saturation plus four continuous backup
jobs (two local, two cloud) whose filesystem filters and read traffic would
both add latency and evict cache — against a single-actuator hard drive already
serialising three lanes. That is a hypothesis; unlike the original one, it is
not being acted on.

### What follows

1. **Do not request an antivirus exclusion.** The evidence for it is gone. This
   matters beyond the performance question: the request would have spent the
   operator's administrator access and weakened a security boundary on the
   strength of a number nobody could re-run.
2. **The `E:` migration already collected the win** — 71× on cold reads. The
   `D:` trees remain only as a fallback.
3. **The claim is now a script, not a paragraph.**
   `bench/file-read-latency.py` (19 self-tests) reproduces the table above in
   about a second. The reason A-Q7 stood unchallenged for six days is that
   re-running it meant reconstructing an ad-hoc measurement from prose.

### The general lesson, which is the reason this entry is long

This is the same failure this lane has now recorded several times: **a verdict
issued from an observation that was never actually made.** The specific shape
here is worth naming, because it is the most expensive variant — a *control*
that did not control. The warm-pass measurement existed to eliminate a
hypothesis, and eliminating a hypothesis is exactly where a bad measurement
does the most damage: a wrong positive result gets checked by the next person
who tries to use it, whereas a wrong *negative* closes a door and nobody opens
it again. The disk was ruled out on day one and never reconsidered, and the
conclusion that survived was the one that asked a human to reduce their own
security.

The habit that catches it: when a measurement's job is to *rule something out*,
that is the measurement to re-run, not the one that produced the headline
number. And any performance claim that will be acted on should ship as
something runnable — the cost of writing `bench/file-read-latency.py` was a few
minutes, against six days of a wrong conclusion sitting in the operator's
decision queue.
