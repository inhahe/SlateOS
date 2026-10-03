## TD-C-CONTEXT-MENU-EXTENSIONS-ARE-IMPLEMENTED-TWICE-AND-REACHED-NEITHER-TIME

**Date:** 2026-09-14. **Lane:** C.

**In short:** `design.txt` specifies that programs can add items to context
menus -- "Open with…", "Compress to .zip" -- behind a capability, loaded lazily
so that showing the menu does not start the program, with a settings page to
turn individual ones off. That feature has been written **twice**, in two
crates, by two different pieces of work, and neither copy is reached by
anything. About 3 650 lines between them.

| | lines | tests | what it says it is |
|---|---|---|---|
| `gui/toolkit/src/context_ext.rs` | 1 603 | 53 | "capability-gated, lazy-loading context menu extension system" |
| `gui/desktop/src/context_ext.rs` | 2 052 | (12 public items) | "allows applications to register context menu items with the desktop shell" |

Both cite the same three design constraints, in the same order. Neither is
named by any file outside itself. `scripts/scan-orphan-modules.py` reports both
and says so out loud -- each is listed as *"also spelled in"* the other, which
is the scan's way of saying two modules model one noun.

**Which should survive is an architectural question, not a quality one.** A
context-menu *widget* belongs in the toolkit and is already there
(`guitk::menu`). An extension *registry* -- who may add an item, which
capability it needs, when the program behind it is loaded, which ones the user
has disabled -- is policy about programs on this machine, and the shell is what
owns that. On that reading the desktop's copy is the one in the right place and
the toolkit's is the one to delete.

That reading is not obviously right, and nothing should be deleted on the
strength of a paragraph: the toolkit's copy has 53 tests and may simply be the
better code, in which case the right move is to move *it* into the shell and
delete the other. The deciding work is a read of both, which nobody has done.

**Why it is worth doing rather than leaving.** Two implementations of one
specified feature is the defect this file has more instances of than any other
-- two snap implementations, two taskbars, two `same_device`, two clocks -- and
every one of them was found by accident, late, after work had been spent on the
copy nobody used. Here both copies are already known and neither is wired, so
the cost of choosing now is a read; the cost of choosing later is another
sweep polishing the wrong one.
