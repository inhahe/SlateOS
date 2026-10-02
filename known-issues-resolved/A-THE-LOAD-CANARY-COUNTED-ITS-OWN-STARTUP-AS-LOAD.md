## A-THE-LOAD-CANARY-COUNTED-ITS-OWN-STARTUP-AS-LOAD (lane A) — FIXED 2026-08-31

**Status:** FIXED 2026-08-31, same day as found.

`scripts/canary-load.py`'s `host_occupancy` is labelled in its own record as
"the direct measurement, and the one to believe". It was measuring each
spinner's **Python interpreter startup** as though it were load applied inside
the window.

**How it surfaced.** A boot test failed at 1555 s — *before building anything* —
on the harness self-test `and does not exceed what wall time allows:
occupancy 2.036`. Two single-threaded spinners cannot occupy 2.036 cores; that
is not a flaky threshold but an impossibility, and the run had been declared
red on the strength of it.

**Root cause, in two parts.**

1. `_spin`'s pre-`go` wait loop called `publish()` *only on its way out*:

   ```python
   while not go.is_set():
       if should_quit():
           publish()      # only when quitting
           return
       go.wait(1.0)
   ```

   So the slot held its initial `0.0` for the whole wait. The controller
   snapshots the opening balance *before* setting `go` — deliberately, so no
   in-window burn lands in it — and therefore read `0.0`, while the closing
   snapshot read a real `process_time()` that included the entire interpreter
   startup. The difference was charged to the window.

2. Fixing that alone left a residual (1.82 → 1.14, still impossible), because
   `Process.start()` returns when a process is **spawned**, not when its
   interpreter has booted and reached `_spin`. Under the spawn start method the
   child re-imports the module — hundreds of milliseconds — after `start()` has
   returned. A spinner that had not yet published still contributed a `0.0`
   opening balance.

**The comment asserted the very property that did not hold.** Above the
readiness announcement stood:

> Announce readiness only once every interpreter is up … Announcing earlier
> would move the interpreter startup cost back inside the measurement window,
> **which is the whole thing this program exists to avoid.**

`start()` cannot promise that, and nothing else was enforcing it. This is the
third instance in one day of the same defect shape — a claim in a comment that
reads as a guarantee, is checked by nobody, and is false (cf.
`A-THE-WIRING-GATE-ASKS-A-QUESTION-IT-COULD-ANSWER-ITSELF`, and the
"exercised indirectly by the demand paging self-test" skip in `mm/frame.rs`).

**Why it hid for so long.** The assertion it had to clear was a hardcoded
`occupancy <= 2.0`. A systematic upward bias that happened to land near 1.8
sailed under a bar of 2.0 on every quiet run, and only tipped over when host
load stretched startup. A constant tolerance did not just fail to catch the
bug — it is *why* the bug was invisible.

**What was done.**

| Change | Effect |
|---|---|
| `_spin` publishes before the wait loop and on every wakeup | opening balance reflects real startup CPU, not `0.0` |
| New `ready_count` barrier; controller waits for every spinner to publish before announcing readiness | closes the spawn race; makes the existing comment true |
| Snapshots stamped (`cpu_fire_at`/`cpu_release_at`); `span_s` recorded | the numerator's interval is measured, not assumed equal to the window |
| New `occupancy_measured` = burn ÷ (spinners × span) | a ratio with a real physical ceiling of 1.0 |
| `occupancy_ceiling(span)` = `1 + 15.6 ms / span` | ceiling derived from clock granularity and the span, replacing the constant `2.0` |
| Test asserts `occupancy_measured <= occupancy_ceiling` | tighter on long spans, honest on short ones |

`occupancy` (÷ the *requested* window) is kept unchanged and still backs the
floor check: window is the **intent**, span is the **measurement**, and the two
answer different questions. Only the span-based ratio has a physical ceiling.

**Consequence for existing data.** Every `host_occupancy` figure recorded before
this date is biased upward by roughly one interpreter startup per spinner
divided by the window — negligible for multi-second benchmark windows, large
for short ones. Historical readings near 0.98 were *under*-stating a
correctly-applied load, not over-stating it, so no past grading verdict flips;
but do not mine pre-2026-08-31 occupancy figures for precision.
