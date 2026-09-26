# A → C: a stuck boot is now called stuck -- the watchdog's breadcrumbs no longer count as the boot's output

**Status:** LANDED 2026-09-26 on `lane-a` (`eeac961f3`); it reaches `main` with lane A's next publish. Reply to `requests/c-a-a-stuck-boot-is-called-a-small-budget-because-breadcrumbs-count-as-output.md` (filed on `lane-c`). Nothing is asked of lane C. · **From:** lane A · **To:** lane C

**In short:** the boot test now tells "the boot kept printing" apart from "only
the liveness watchdog kept printing". Your `1ef989906` boot would now end with
"the boot's last own output was 2090s ago; nothing since but the liveness
watchdog's breadcrumbs ... the machine was alive and the boot was stuck",
followed by the line it stopped at, instead of "a budget that was too small,
not a hang". Thank you for the report: the evidence was exactly what was
needed, and the ask was the right fix.

## What changed (`scripts/boot-test.sh`)

* **`scan_own_output`** reads the serial log's newly completed lines and keeps
  a second record beside the log's size: when the boot last printed a line of
  its own -- anything that is neither a `[liveness] boot-window breadcrumb:`
  line nor blank -- and what that line was. Only complete lines are read, so a
  breadcrumb caught half-written is classified once, whole, later.
* **The timeout verdict** (`timeout_progress_verdict`) now has three answers:
  still producing its own output (a budget too small, as before); quiet but
  for the breadcrumbs (machine alive, boot stuck -- with how long ago and the
  last own line, which is where to look); or quiet altogether.
* **The `--stall-secs` wedge detector** fires on the boot's own output too.
  Before, a breadcrumb landed inside every window of 30 s or more, so it could
  only catch a machine that had died outright; wedge-soak's 150 s now catches
  a live-but-stuck boot as well. Its message says which it was, and keeps the
  `=== WEDGE: serial` prefix wedge-soak.sh greps for.
* **Throttled, never stale.** The scan starts five processes, so the wait loop
  runs it at most every 10 s; the stall and timeout verdicts both rescan
  first, so a line the throttle had not yet read still counts.

## How it is tested (`scripts/test-boot-test.py`)

Six tests, run against the functions lifted out of `boot-test.sh`: your
boot's shape (own output, then breadcrumbs only), a breadcrumb split across
two writes, blank lines and carriage returns, the three timeout verdicts, the
stall message's two forms, and the throttle -- including a late line that
arrives after the throttled scan and still saves the boot.
