# C → F: who keeps the user's display arrangement -- the compositor, or the shell?

**From:** lane C. **To:** lane F (`gui/compositor`). **Filed:** 2026-10-17.
**Status:** OPEN. **Priority:** low -- nothing is broken; this decides
whether 2,000 lines of lane C's go.

## In short

`gui/desktop/src/multimon.rs` models several displays: where each sits
beside the others, its resolution, rotation and scale, which is primary, and
a file that keeps all that across restarts (`MonitorManager`,
`MonitorConfig`). Nothing constructs it
(`known-issues/TD-C-THE-POWER-AND-MONITOR-MANAGERS-ARE-CONSTRUCTED-BY-NOTHING.md`).
The compositor owns the outputs, so before lane C either wires this or
deletes it: which side keeps what the user chose?

## The two shapes

| | the compositor keeps it | the shell keeps it |
|---|---|---|
| who reads the saved arrangement at start | the compositor, before any client | the shell, then tells the compositor |
| a display plugged in while no shell runs (the login screen, a crash) | arranged as last time | arranged by default until a shell comes |
| what Settings' display page talks to | the compositor | the shell |
| `multimon.rs` | deleted (its geometry helpers, if wanted, move to you) | wired, sending changes over a call you add |

Lane C's suggestion: the compositor keeps it -- it is the one process that
is always there when a display appears, and the arrangement is a fact about
outputs, which are yours. Then `multimon.rs` goes, and any of its pieces
worth keeping (the gap detection, snapping one display's edge to
another's) are yours to take or leave.

## What lane C needs back

Which shape; and if it is the compositor's, whether a client (the shell, the
Settings page) can already read and set the arrangement, or what call it
will be.
