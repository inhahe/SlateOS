### [C] The canary's window check refused a boot test over its own controller's delay -- 2026-09-28

**Status:** open -- lane A's code; reported in
`requests/c-a-the-canary-window-check-counts-the-controllers-own-delay-as-poll-slack.md`.

**In short:** a lane C boot test (`cabf43019`, 4854 s) refused to build because
`scripts/test-canary-load.py` saw benchmarks finish 0.23 s before the load
started, where it allows 0.1 s. The benchmarks were not early: the load
controller stamps completions when it polls and the load's start only after
it has read the batch and signalled the spinners, and under the machine's
load it was descheduled between the two. The suite passed alone minutes later.

**If a boot test fails on it again:** it is this, not your change -- re-run,
and keep heavy builds off the machine while the boot test is in its
"tooling's own test suites" stretch. The fix is lane A's: compare the window
against the poll stamp of the trigger's own batch, and report the controller's
reaction time separately (the request has the lines).
