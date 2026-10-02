## 849. Two taskbars become one: the module keeps the model, the shell keeps the screen

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop has two taskbars. One is on screen and is what you
use; the other is 2,600 lines of finished, tested code that nothing runs, and
it is the one that knows how to pin an application. Rather than throw either
away, the unused one keeps the part that decides *what buttons there are* and
the live one keeps the part that decides *what they look like*. The result is
that pinning starts working and neither implementation is rewritten.

### How there came to be two

`gui/desktop/src/lib.rs` draws the taskbar the user sees: window buttons, the
system tray, the clock, auto-hide, and the tray drag-and-drop added on
2026-09-13. `gui/desktop/src/taskbar.rs` is a separate module -- pinned
shortcuts persisted to config, running-app indicators with window grouping,
drag-to-reorder, drag in and out of the pinned section -- with its own
renderer, its own mouse handling and its own `WindowId`.

Nothing calls it, and nothing noticed: `scripts/scan-orphan-modules.py` was
clearing it, because its `pub struct WindowId` is a name the shell writes
hundreds of times and the file that also declares it (`lib.rs`) was never
collected as an owner. That is fixed; see
`TD-C-THE-ORPHAN-SCAN-CLEARS-A-MODULE-ON-A-NAME-AN-APP-HAPPENS-TO-SHARE`.

### The decision

**`taskbar.rs` becomes the model. `lib.rs` stays the renderer.** The shell
sources its button list from `Taskbar::buttons()` instead of deriving it from
`WindowList`, and `Taskbar::render` -- the part that has never drawn a pixel --
is what gets deleted.

| | adopt the module's model | delete the module, write pinning in the shell |
|---|---|---|
| pinning, grouping, reorder rules | kept, with their tests | rewritten from scratch |
| tray, clock, auto-hide, tray DnD | untouched, they are the renderer's | untouched |
| lines deleted | the module's renderer | 2 600, including 40-odd tests |
| risk | the shell's button list changes shape | the same rules written a second time |
| "two models of one fact" | resolved: one model, one renderer | resolved: one of each, by deletion |

### Why not the other way round

Two arguments were weighed for deleting the module outright and writing
pinning directly into the shell.

The first is that the module has **never run**, so its tests prove it agrees
with itself and nothing more. That is real and is the honest case against
trusting it. It is outweighed by the alternative being to write the same rules
again, untested against anything better, with the same risk and none of the
existing coverage.

The second is that the shell's taskbar carries work the module knows nothing
about -- the tray's width budget, the overflow menu, the drag-and-drop arranger
-- and subordinating the live surface to an unrun module could regress it. This
is why the split is at the model/renderer line rather than wholesale: the
module never learns what a tray is, because it never draws one.

### What this does not decide

Whether `TaskbarConfig`'s *position* and *icon-only* options should be
user-visible settings is left open -- they are model fields today with no page
to set them from, and wiring a settings page for them is the
`TD-C-ELEVEN-SETTINGS-PAGES-SAY-COMING-SOON` question, which is blocked on
consumers existing at all.
