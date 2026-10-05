## 1445. An installed program replaces SlateOS's own by desktop file ID, not by the program it starts

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C &middot; asked by lane E (`e-c-the-installed-and-built-in-programs-belong-in-gui-programs`)

**In short:** the start menu, Settings' Default Apps page and the file
manager each list "the programs this machine has": the installed ones, with
SlateOS's own behind them. They disagreed about when an installed program
*replaces* one of SlateOS's. The start menu matched by the program an entry
starts (an entry running `calculator` replaced SlateOS's Calculator); the
others by the entry's name on disk, its desktop file ID
(`org.slateos.Calculator.desktop`). Now there is one rule, in
`gui/programs` (`known`, `known_in`, `with_built_in`), and it is the ID: an
entry replaces SlateOS's only when it is the same entry.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **By desktop file ID** (chosen) | the freedesktop rule for "the same entry" -- a user's copy of a system entry replaces it by its ID, and so do SlateOS's own entries once the image installs them as files; a launcher someone makes for a program, `Exec=calculator --scientific` under an ID of its own, is a second entry beside the first, as it is on every other desktop | an entry that starts one of SlateOS's programs under another ID is listed beside SlateOS's, not instead of it |
| By the program's file name -- the start menu's rule until now | a third-party entry for SlateOS's calculator hides SlateOS's | another program that happens to share a name -- `/opt/foo/bin/editor` -- takes SlateOS's editor off the menu; a launcher a person made for SlateOS's calculator hides the stock one; and `defaults.list`, which names defaults by ID, cannot say which of the two it meant |
| Either | catches both | the false matches of the second, and two rules to explain |

**What counts as the same ID.** Any file the scan finds for it, used or not
(`Scan::claims`): a `Hidden=true` copy -- the freedesktop way to remove an
entry -- takes SlateOS's off the list too, and a copy that does not parse is
reported rather than quietly passed over for the compiled-in one. The start
menu holds installed entries to their `TryExec` and `NoDisplay` as before;
SlateOS's own are not held to `TryExec`, because the image does not install
their entries yet and a menu without them would be empty.

**Order:** the installed first, SlateOS's behind -- `Role::filled_by` breaks
ties by order, and a program the machine has installed is asked first.

**Revisit if** SlateOS's own entries ship installed as files (their
`TryExec` then applies like anyone's), or a real case turns up of a package
shipping one of SlateOS's programs under an ID of its own.
