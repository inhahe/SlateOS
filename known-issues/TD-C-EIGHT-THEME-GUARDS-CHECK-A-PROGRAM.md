## TD-C-EIGHT-THEME-GUARDS-CHECK-A-PROGRAM'S-OPENING-FRAME

**Date:** 2026-09-14. **Lane:** C.

**In short:** twelve applications have a test that checks every colour they draw
comes from the user's chosen theme. Eight of those tests look at the program as
it appears the instant it opens -- nothing selected, no menu open, nothing
running -- and at nothing else. A colour that ignores the theme anywhere else
in the program passes them. The tests are not wrong about what they checked;
they are silent about the rest, and they read as though they covered it.

**Measured, not guessed** (2026-09-14), by reading each guard rather than
grepping for a pattern -- the first two attempts to count this mechanically
were both wrong, in opposite directions, which is itself the lesson:

| app | what its guard renders |
|---|---|
| `procexplorer` | **six named panels with demo data.** The model to copy; it also filters to commands that *carry a colour*, guarding against a vacuous pass |
| `explorer` | nine named states, both modes (fixed 2026-09-14) |
| `benchmark` | six tabs x three phases x hover, both modes (fixed 2026-09-14) |
| `imageviewer` | one scene, but a *maximal* one -- info panel, thumbnails and slideshow all switched on |
| `sysinfo`, `pdfviewer`, `musicplayer`, `speedtest`, `devicemanager`, `pomodoro`, `screenshot`, `mixer` | **one bare default scene.** The only line before the render is the palette assignment |

**Why this is not pedantry.** Two of the three that have been fixed were caught
*failing* once they were widened:

* `benchmark` passed against a hardcoded green pushed deliberately into a
  button's hover branch. Its guard rendered the resting, idle state of one tab
  out of six.
* `explorer` -- the application this roadmap item names as "the visible
  failure" -- rendered a brand-new window on an *empty* directory: the one
  moment it has no rows, nothing selected, no menu open, no transfer running,
  and a toolbar with nothing to grey. A context menu drawn from a fixed light
  palette passed it, because it never opened a menu.

Neither was found by reading. Both were found by *disabling the fix and
watching for a failure*, which is the only method that distinguishes "the test
passes because the code is right" from "the test passes because it is not
looking".

**The fix, per app.** A named list of scenes, in both modes, plus a second test
asserting the scenes are not all the same picture -- without that, an arranger
that quietly stops working (a selection that selects nothing, a menu that does
not open) goes on being swept as a duplicate of the default, and the guard
reports nine states while looking at one. `apps/explorer` has both tests and is
the worked example; `apps/procexplorer` is the older and better-established one.

Cheap where the app already enumerates its own screens: `pomodoro` has
`Screen::ALL` (four) and a `TimerState` (three), `musicplayer` a `Tab` (three),
`devicemanager` three `all()` lists, `speedtest` a phase enum whose `Error`
variant no default render ever reaches.

**Where this sits in the pattern.** It is the same defect as
`TD-C-THE-SCRATCH-CONFIG-GATE-COULD-NOT-SEE-A-STORE` and the orphan scanner's
false clear: *a check reporting success over a population it cannot see*. The
three differ only in what the invisible population is -- crates, module names,
program states. Green and wrong is worse than red, because a green check ends
the investigation.
