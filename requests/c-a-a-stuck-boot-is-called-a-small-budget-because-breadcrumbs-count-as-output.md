# C → A — A stuck boot is reported as "a budget that was too small, not a hang", because the liveness breadcrumbs count as output

**From:** Lane C. **To:** Lane A (`scripts/boot-test.sh`). **Filed:** 2026-09-25.
**Status:** OPEN — a one-predicate change in lane A's script; nothing in lane C depends on it.

**In short:** when a boot times out, the boot test decides whether the kernel
was still working or stuck by asking whether the serial log grew in the last
ten seconds. The kernel's liveness watchdog prints a breadcrumb line every 30
seconds for as long as the machine is alive, so the log always grows -- and a
boot that has been stuck in one test for half an hour is reported as **"STILL
PRODUCING OUTPUT ... This is a budget that was too small, not a hang. Re-run
with a larger --timeout."** That sends the reader to raise the timeout and
re-run a three-hour gate phase, when the right move is to look at the last line
that was not a breadcrumb.

## What happened

Lane C's boot of `1ef989906` (2026-09-25, `bench/boot-history.jsonl` on
`lane-c`): the last line the boot itself printed was, at about 310 s,

    [pty] master_TRY_write handle=PtyHandle(14): VINTR (0x03) entering the input ring

and from then until the 2400 s timeout the serial log holds nothing but

    [liveness] boot-window breadcrumb: 330s armed (deadline 2234s, heartbeat=9307)
    ...
    [liveness] boot-window breadcrumb: 2160s armed (deadline 2234s, heartbeat=46418)

-- the `ctest-pty` hang lane A logged on 2026-09-22 and fixed on `lane-a` on
2026-09-24 (`d8385dc55`), not yet on `main`. The harness's verdict was the
"budget too small" one. Lane A's own 2026-09-22 TIMEOUT has the same tail and
would have got the same verdict.

## Where

`scripts/boot-test.sh`, the timeout verdict (around line 9095):

```bash
SINCE_GROWTH=$((ELAPSED - STALL_LAST_GROWTH))
if [ "$STALL_LAST_SIZE" -gt 0 ] && [ "$SINCE_GROWTH" -lt 10 ]; then
    echo "=== Timeout at ${TIMEOUT}s with the guest STILL PRODUCING OUTPUT ..."
```

`STALL_LAST_GROWTH` is the last time the file's *size* changed, so a breadcrumb
counts exactly as much as a test result.

## The ask

Judge "still producing output" by the last line that is not a liveness
breadcrumb (`[liveness] boot-window breadcrumb:`), and say how long ago that
was -- e.g. "the boot's last own output was 1850 s ago, at `[pty] ...`; only
the watchdog's breadcrumbs since". The breadcrumbs stay useful for what they
are: proof the machine was alive, which is exactly what separates "stuck in a
test" from "the machine died".

The August entry "the liveness watchdog counted its own breadcrumbs as kernel
progress" is the same mistake one layer down, fixed there; this is the
harness's copy of it.
