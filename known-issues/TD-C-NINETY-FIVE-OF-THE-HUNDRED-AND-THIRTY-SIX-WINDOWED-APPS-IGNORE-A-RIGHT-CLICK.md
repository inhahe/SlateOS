## TD-C-NINETY-FIVE-OF-THE-HUNDRED-AND-THIRTY-SIX-WINDOWED-APPS-IGNORE-A-RIGHT-CLICK

**Date:** 2026-09-14. **Lane:** C.

**In short:** right-clicking does nothing in most of the applications. It did
nothing in the file explorer until 2026-09-14 -- no menu, no handler, not a
single mention of the right button anywhere in it -- and asking the same
question of every other application gives a large number. **Not all of those
are defects**, which is why this entry is a measurement and a method rather
than a list of work.

**The measurement.** Of the applications that open a window
(`oswindow::app::launch`):

| | count |
|---|---|
| windowed applications | 136 |
| with no mention of `MouseButton::Right` anywhere | 95 |
| of those, with some notion of a selection | 73 |

**Why the number overstates the problem.** A calculator, a compass, a
metronome and a game of klotski have nothing a context menu would say. Several
of the 73 match on `selection` for something that is not a user's selection of
an item. The honest reading of 95 is *"here is where to look"*, not *"here are
95 bugs"* -- and an entry that claimed the latter would be the sort of list
this file has already learned nobody reads.

**Why it is worth recording at all.** The explorer's gap was invisible for as
long as nobody tried the gesture. Nothing in the toolchain can see it: the
application compiles, its tests pass, every control it *does* have works, and
a missing gesture leaves no trace. The same is true of the other 95, and the
only way any of them will be found is somebody asking.

**How to triage one**, which is the part worth keeping. Ask what the
application's main surface is a list *of*:

* a list of the user's own things -- files, contacts, messages, notes,
  clipboard entries, saved connections -- **almost certainly wants a menu**,
  because every operation it offers is already on a keyboard shortcut nobody
  can discover;
* a list the application generated -- search results, log lines, a disk map --
  wants one only if there is something to *do* to a row;
* a canvas, a game or a single-purpose readout usually wants nothing, and
  giving it a menu is worse than leaving it.

The explorer is the worked example: its menu offers Open, Cut, Copy, Rename
and two deletes for a file, and New folder, Paste and Refresh for the folder
-- every one an operation that already existed and was reachable only from the
keyboard.

**Not a sweep.** Adding a menu to seventy applications is seventy design
questions, and doing it mechanically would produce seventy menus full of rows
that do nothing -- which is the defect this file has the most entries about.
Take them one at a time, when touching the application for another reason.
