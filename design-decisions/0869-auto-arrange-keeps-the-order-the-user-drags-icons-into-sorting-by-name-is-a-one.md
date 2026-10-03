## 869. Auto-arrange keeps the order the user drags icons into; sorting by name is a one-off

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** Right-clicking the desktop now offers a View submenu, as other
desktops do: the icon size, "Auto arrange icons" and "Align icons to grid" —
the "snap to grid, or place freely" choice `design.txt` asks for — and a
separate "Sort by name". We had to decide what *auto-arrange* means. It now
means "keep the icons packed together from the top-left, in whatever order
you drag them into", which is how Windows does it. It used to mean "keep them
sorted by name", which is how the Mac's "Sort by Name" works — and under
which dragging an icon does nothing, because it jumps straight back to its
alphabetical place. Sorting is now its own menu item, and it sorts once.
The code is `gui/desktop/src/icons.rs` (`ArrangementMode`, `drop_plan`) and
the menu is in `gui/desktop/src/lib.rs` (`desktop_menu_items`).

### The two candidates

| | the order of the icons | dragging an icon | sorting |
|---|---|---|---|
| **A. Always sorted** (the Mac's "Sort by Name"; what the code did) | by name, always | the icon jumps back: the order is not the user's to change | *is* the mode |
| **B. Packed, in the user's order** (Windows' "Auto arrange") — chosen | whatever the user made it | moves the icon to a new place in the order; the others close up around it | "Sort by name", once |

**Why B.**

- **The menu's words are Windows', because the shell is.** The taskbar, the
  Start menu, the tray and Ctrl+R are all specified in `design.txt` "like on
  Windows", and "Auto arrange icons" beside "Align icons to grid" is Windows'
  own pair. A user reading those words expects B.
- **A makes the one gesture a desktop icon exists for do nothing, silently.**
  The code's version of A re-sorted after every drop, so in one mode of three a
  drag looked like it worked until the mouse was let go.
- **B loses nothing A had.** "Sort by name" produces A's order in one click,
  and under auto-arrange the icons then stay in it until the user drags one
  somewhere else.

**What B costs:** the order is now state the user made, so it has to survive a
restart. It does without a new format: the layout file already stores every
icon's position, and under auto-arrange the order is read back from the
positions — down each column, then the next. An icon the file does not mention
(a default icon added since) goes last.

### Smaller calls made with it

- **On the grid, an icon dropped onto another takes the free cell nearest the
  one it was aimed at.** Not a swap — that moves an icon the user did not touch;
  not a refusal — the drop would look broken; and not an overlap, which is what
  the code did, so that the second icon hid the first. "Nearest" is measured in
  pixels between cells: a cell is taller than it is wide, so the cell beside
  beats the cell below.
- **An icon snaps to the cell its centre is over, not the cell its corner is
  in.** By the corner, an icon had to be dragged a whole cell before it moved
  one. The outline drawn during a drag comes from the same function as the drop
  (`drop_plan`), so it cannot promise a cell the drop then does not use.
- **Placing freely, a dropped icon is drawn on top** of anything it overlaps:
  the last thing put down is the one the user expects to see and to click.
- **The arrangement is saved with the icon positions (`deskicons.yaml`); the
  icon size stays with the appearance settings (`appearance.yaml`).** The size
  is an appearance setting the Settings application edits too, and the file is
  where both read it. The arrangement means nothing without the positions it
  governs, and nothing else edits it. The layout file also records the grid its
  positions were laid out on, since a position is only meaningful at the pitch
  it was saved at and the icon size changes the pitch.
- **Two switches, three states.** "Auto arrange" and "Align to grid" are both
  shown, but arranged-and-not-aligned means nothing, so the model is one
  three-state `ArrangementMode` and the switches map onto it as Windows' do:
  auto-arrange on aligns, auto-arrange off leaves the icons aligned, and
  alignment off stops arranging too.

### How to reverse

A is two lines: `set_arrangement(AutoArrange)` sorts by name instead of into
reading order, and `apply_drop` re-sorts after a reorder. Nothing in the file
format changes either way — the positions are what is saved, in both.
