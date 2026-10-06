## 1453. A ribbon in the toolkit: tabs of grouped commands, deliberately not Office's

**Date:** 2026-09-30 &middot; **Decided by:** Claude (autonomous), within the
operator's rule in `roadmap-detailed.md` → *Ribbon Widget* (implement the
general tabbed pattern; stop short of the arrangements Microsoft licenses)
&middot; **Lane:** C

**In short:** An application with many commands -- a word processor, an
image editor, the file manager -- can now put them in a ribbon: a row of tabs
(Home, View, ...), each showing its commands under it in named groups, as big
buttons, small ones, on/off switches, buttons with a menu, dropdown lists and
rows of picture choices. When the window is too narrow, whole groups fold
into single buttons that show the group when pressed. It is part of the
toolkit (`guitk::ribbon`); applications adopt it when they choose to.

**The shape** is the dock's (`guitk::dock`, 2026-09-28): state and functions over
it, every case testable without a window. The application owns the
`Ribbon`, calls `layout` each frame, hands pointer and key events to it and
gets back a `RibbonEvent` naming its own command numbers, and calls `draw`.

**Deliberately not Office's.** The roadmap entry's caution, applied:

| Office | Here |
|---|---|
| contextual tabs under a coloured header naming their set, in the title bar | a band of the user's accent along each contextual tab's own top edge; nothing above the strip |
| groups shrink by stages (large buttons to medium to small) before collapsing | a group is whole or folded into one button, nothing in between |
| a gallery previews a choice on the document under the pointer | a gallery chooses on a click and never previews |
| key tips come in layers: Alt, letters on the tabs, then a tab's own letters | F10 shows one layer: a digit on each tab and a letter on each of the front tab's commands at once; a digit brings its tab forward and the letters follow |

**Choices made within that, each easy to change:**

| Question | Chosen | Instead |
|---|---|---|
| Which group folds first | the application's per-group priority, lowest first, the rightmost among equals | by width (folds whatever is biggest, which is often what users reach for most) |
| What fits nowhere, even folded | behind a `»` button whose menu holds each group as a submenu of its controls | clipped (commands unreachable); scroll arrows (a second way to move along the ribbon) |
| A gallery's full list | the toolkit's menu, the chosen row checked | a grid of pictures (nicer for styles; a later refinement) |
| Minimizing | a double click on a tab, or Ctrl+F1; a tab pressed then shows its commands over the page until one is used | a pin button (more chrome for the same thing) |
| The strip's colour | the focused title bar's role, so a ribbon under a title bar reads as one piece | its own role (the join shows) |

A press on a face acts on release over the same face, as every button does;
an arrow opens its menu on the press, as a menu opener does. A press outside
an open menu or panel closes it and does nothing else.

**Key tips** (keyboard access without the pointer): a word's first letter
where it is free, then another of its letters, then any -- Cut `c`, Copy
`o` -- and past twenty-six things two letters for every one, so that no tip
is the start of another. A button or toggle's letter does it; a dropdown's,
gallery's or split button's opens its menu with the arrows ready (a split
button's menu gains its face as the first row, so both are in reach); a
folded group's opens its panel and the letters move onto it. Escape steps
back -- out of a panel, then out of the tips -- and any other key, or a
press of the pointer, leaves them and is the application's.

**Tooltips**: the toolkit's own (`menu::Tooltip`), after 600 ms at rest: a
control's name, then what more it does -- or, for one that cannot be used,
why, as `design.txt` asks of every disabled control. A pointer event carries
no time, so the ribbon is told the time (`tick`) rather than keeping a
second clock, and says when it next needs telling (`tooltip_due_in`).

**The user's changes.** A right-click offers them where they apply: a
command onto the Quick Access Toolbar or off it, out of its group, or a
command into a group ("Add a command to this group": every command the
ribbon has that the group has not, and any the application offers for the
purpose); a tab hidden or moved; and anywhere, the hidden tabs back, the
toolbar over or under the ribbon, collapsing, and undoing every change.

| Question | Chosen | Instead |
|---|---|---|
| What the ribbon keeps | the application's tabs and the user's changes, the shown tabs built from the two | the changed tabs alone (a new version of the application's tabs could not be told from the user's changes) |
| A command put in a group | at its end, a row high | where the user drops it (needs dragging, which nothing else in the ribbon needs) |
| A command taken out and put back | back where it was | at the end, like any other |
| A toggle's state, a list's choice | the command's: Bold on two tabs is on in both | each button's own (the first version did this; a click on one left the other stale) |
| Where the changes live | one line of text the application keeps in its settings file, `ribbon1;min=1;qat=2,10;...`; a change naming what the ribbon no longer has is dropped on reading | a file of the ribbon's own (a second settings file per application) |
| The last tab | cannot be hidden | can (and the ribbon has nowhere to show a command) |
| Undo every change | undoes all but minimizing, which is how the ribbon is looked at | minimizing too |

**The strip follows the title bar, accent and all.** The toolkit's palette
carries whether the user asked for accented title bars
(`Palette::accent_titlebars`), and `Palette::title_bar` / `title_text` are
the one answer both the window manager's bar (`DecorationColors`, which
until now applied the accent on top of the palette from the settings) and a
ribbon's strip are drawn from -- a test holds the two equal for every accent
and mode. One consequence, chosen: in high contrast an accented bar is the
scheme's accent, picked to read on its background, where it was the user's
raw accent. On an accented strip a contextual tab's band is the ink that
reads on the accent, since a band of the accent would vanish into it.

**The dialog** ("Customize the ribbon…", the last row of the right-click
menu) shows the same changes all at once: every command on the left; every
tab on the right -- a check box to show or hide it, and under a shown tab
its groups and their commands; Add and Remove between them, Move up and
Move down for a tab, Undo all changes, Close. A double click adds or takes
out, and the keyboard works it (the arrows in the list that has the keys,
Tab between the two, Space for a tab's check box, Enter to add or take
out). Modal over the window while it is open. It holds nothing but what is
chosen in it: every change is the ribbon's own method, so the dialog and
the right-click menus cannot disagree. A tab moved and moved back leaves no
trace -- an order equal to the application's is not kept.
