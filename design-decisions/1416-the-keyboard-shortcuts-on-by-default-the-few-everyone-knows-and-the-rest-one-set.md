## 1416. The keyboard shortcuts on by default: the few everyone knows, and the rest one setting away

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended option B; the operator chose a set close to it, and added a few) &middot; **Lane:** C, with E

**In short:** `design.txt` asks for very few shortcuts turned on, and the desktop
turned on 31. The operator chose which stay on. On by default: closing a
window (Alt+F4), switching windows (Alt+Tab), the Super key for the start menu,
Super+R for the Run box, Print Screen and its variants, and the dedicated
volume keys -- plus, inside programs, the editing keys everyone knows. The rest
(window snapping, desktop switching, and the shell's other panels) stay
available and are easy to find, but start unbound. Super+Tab goes: what it
opened becomes a choice of what Alt+Tab shows.

**The question:** `open-questions.md` C-Q24 (now resolved).

| Group | Keys | Default |
|---|---|---|
| The window | Alt+F4 close, Alt+Tab and Alt+Shift+Tab switch | **on** |
| The shell | Super (start menu), Super+R (Run box) | **on** |
| Screenshots | Print Screen: the whole screen; Alt+Print Screen: the current window (both to the clipboard, as on Windows); Ctrl+Print Screen and Ctrl+Alt+Print Screen: the same two, asking where to save them as a file | **on** |
| Dedicated keys | volume up, down, mute; brightness up, down where the hardware sends a key | **on** -- they are keys of their own, usually in the top row or behind Fn, where nobody presses one by accident |
| Inside programs | Ctrl+C copy, Ctrl+X cut, Ctrl+V paste, Ctrl+Z undo, Ctrl+Shift+Z redo, Ctrl+F4 close the current tab or document | **on** -- each program's own, never a desktop-wide grab, since they mean something different in each program |
| Inside programs, moving (the operator's addition, later the same day) | Page Up, Page Down, Home, End, Ctrl+Home, Ctrl+End, "for relevant apps" | **on** -- wherever there is a list, a text or a view to move through; each program's own, like the row above |
| Available, off by default | snap left and right, maximise, minimise, show desktop, the zone overlay, next and previous desktop, notifications, task manager, settings, lock, the shortcut card, the keyboard-layout switch (Super+Space), the file explorer (Super+E), a region screenshot (Super+Shift+S), and a new one: put the monitor to sleep | off, one binding away in the shortcut settings |
| Gone | Super+Tab | what it opens (the overview) becomes a setting: whether Alt+Tab shows the switcher or the overview |

**Two additions the operator made, recorded as work:**
- **A redo *tree* wherever something can be undone and redone:** an undo that
  keeps the branches a user undid away from, rather than discarding them the
  moment something new is done. Today each of 39 programs keeps its own
  straight-line history, and the toolkit's text area its own; the toolkit gets
  one tree the rest can adopt.
- **Print Screen saving to a file**, one key for the window and one for the
  screen, each asking where to save.

**Judgment calls inside the operator's answer**, easy to change: the exact
save-to-file keys (Ctrl+ and Ctrl+Alt+Print Screen), and treating the editing
keys as each program's own rather than the desktop's. And which Print Screen is
which: the operator wrote "Alt+PrtScrn for the entire screen and Ctrl+PrtScrn
for the current window, like on Windows?" -- but Windows has it the other way
round (Print Screen the screen, Alt+Print Screen the focused window). The
defaults follow Windows, as the "like on Windows" asked, which is also what
frees Ctrl+ for the save-to-file pair.

**The moving keys, as built (2026-09-27, lane C):** the toolkit reads them
one way (`guitk::listview::ListKey`): in a list, Home and End with or without
Ctrl are its ends -- a list has no line for plain Home to start -- and Page Up
and Page Down move a windowful; Ctrl+Page Up/Down are left alone, since they
change tab in a program with tabs. Menus and the menu bar, the path bar's
completions, the text views (plain Home/End as well as Ctrl), the start menu,
the overview, the notification pane, the shortcut card and its action picker,
the login screen's users, the Run box and the desktop's icons all answer them;
the text area, grid, tree and file list already did. Judgment calls, easy to
change:
- **A text field over a list** (the start menu's search, the card's picker, the
  Run box, the path bar): Home and End are the text's while it has text to move
  through, and the list's when it is empty or Ctrl is held; the page keys are
  the list's.
- **A list that is not drawn** (the Run box's history): Page Up goes to its
  oldest entry and Page Down back to what was typed.
- **A surface that does not scroll** (the desktop's icons, the overview, the
  login screen): a page is what is on the screen -- the icons' column, all the
  cards, all the users.

**As built, 2026-09-27 (lane C):**
- `hotkeys::register_defaults` holds the operator's set and nothing else --
  fourteen bindings, down from thirty-one -- pinned whole by
  `the_defaults_are_the_operators_set_and_nothing_else`. Three actions are new:
  `ScreenshotWindow`, `ScreenshotToFile` and `ScreenshotWindowToFile`.
- **The shortcut card lost its chord, so it gained a door:** a Keyboard
  Shortcuts place in the start menu's places column, which opens it. A card
  only a shortcut could open would be one nobody could reach to bind the
  shortcut. The places tighten a little (32 units each down to 26) before the
  lowest is left out, so an eighth place costs nothing on a small screen.
- **Switching keyboard layouts still works out of the box:** Alt+Shift is the
  layout switcher's own setting (`keyboard.layout_switch` in `input.yaml`),
  never a hotkey-table entry, so taking Super+Space out removed a second way,
  not the only one.
- **Waiting on other lanes:** Alt+Print Screen arrives as an unknown key on
  PS/2 keyboards and in QEMU, which send a different scancode while Alt is held
  (lane A, `requests/c-a-keys-that-never-reach-the-desktop.md`, which also
  records that a USB keyboard reaches no window at all); the screenshot tool
  does not read `--save` yet (lane E, `requests/c-e-print-screen-can-save-to-a-file.md`);
  putting the monitor to sleep needs a compositor verb that does not exist
  (lane F, `requests/c-f-a-way-for-the-shell-to-put-the-display-to-sleep.md`).
- **Still lane C's to do:** the monitor-sleep action, once lane F's verb
  lands. *Done 2026-10-06 (§1487): `SleepDisplay`, unbound, and "Sleep the
  display" in the power menu.* (What Alt+Tab shows was done the same day, as an action rather than a
  setting: §1419.)
