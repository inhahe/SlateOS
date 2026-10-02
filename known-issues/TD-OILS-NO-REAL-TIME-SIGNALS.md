### TD-OILS-NO-REAL-TIME-SIGNALS. osh's signal table stops at 31; bash exposes SIGRTMIN..SIGRTMAX (34–64) — 2026-08-25 — OPEN

**In short:** POSIX systems have two kinds of signal. The classic ones
(`SIGINT`, `SIGTERM`, …) are numbered 1–31 and osh handles all of them
correctly. Linux adds a second block, the *real-time* signals — 31 extra
numbered slots (34–64) that programs use to send each other application-defined
notifications. osh does not know they exist. A script that says `kill -RTMIN+2
$pid` works under bash and fails under osh.

**Found by** the reference-shell migration: this was invisible while the corpus
measured against MSYS bash, whose own table is BSD-shaped and disagreed with
osh's in so many places that the real-time block was lost in the noise. Against
glibc bash the 1–31 range agrees exactly, which is what makes the tail visible.

**Where:** `userspace/oils/src/interp.rs` — the signal name/number table behind
`kill`, `trap` and `$?`'s 128+n encoding.

**Measured:**

```
$ osh  -c 'kill -l RTMIN'   →  kill: RTMIN: invalid signal specification
$ bash -c 'kill -l RTMIN'   →  34
```

`kill -l` output is identical up to `31) SIGSYS`, after which bash continues
with 34–64 and osh stops.

**Proper fix.** Extend the table with `SIGRTMIN` (34) through `SIGRTMAX` (64),
and accept the `RTMIN+n` / `RTMAX-n` name forms in `kill`, `trap` and `kill -l`,
which is how they are almost always written. Note that glibc reserves 32 and 33
for its own threading use and bash lists neither, so the block genuinely starts
at 34 — the numbers are not simply "32 onwards".

**Risk of leaving it.** Low but real: real-time signals are rare in shell
scripts, but a script that uses one fails with a confusing "invalid signal
specification" rather than doing nothing, and the failure is at the point of
use rather than at startup.
