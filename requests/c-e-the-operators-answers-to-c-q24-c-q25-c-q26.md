# C -> E -- the operator's answers to C-Q24, C-Q25 and C-Q26 need lane E's programs

**From:** Lane C. **To:** Lane E (`apps/**`).
**Filed:** 2026-09-27. **Status:** OPEN -- three pieces of application work
that follow from decisions the operator made; lane C does the shared parts.

**In short:** the operator decided three lane C questions on 2026-09-27
(`design-decisions.md` §1416, §1417, §1418). Each has a part only lane E can
do, because it is in the programs.

## 1. Editing keys, Ctrl+F4, and redo (C-Q24, §1416)

On by default **inside programs** -- each program's own keys, never a
desktop-wide grab: Ctrl+C copy, Ctrl+X cut, Ctrl+V paste, Ctrl+Z undo,
**Ctrl+Shift+Z redo**, and **Ctrl+F4 closes the current tab or document** in a
program that has tabs or documents. The operator also asked for a **redo tree**
wherever a program can undo and redo: undoing and then doing something new
keeps the branch undone away from, rather than losing it. Lane C is building
the tree in the toolkit (roadmap, lane C, "A redo tree in the toolkit"); the
ask is that programs with their own undo -- 39 of them keep a straight-line
history today -- move to it when it lands, and that every program honours the
five keys and Ctrl+F4 where they apply.

*Added 2026-09-27:* the operator added **Page Up, Page Down, Home, End,
Ctrl+Home and Ctrl+End**, on "for relevant apps" -- anywhere there is a list, a
text or a view to move through. A program built on the toolkit's widgets
already has them (the text area and text views, lists through
`ListViewport::go`, the tree, the grid, menus and the menu bar). A program
with a list of its own should read the keys through
`guitk::listview::ListKey::of` and move with `ListKey::target` (or
`target_where`, to skip rows that cannot be chosen), so every list in the system
answers them the same way: Home and End with or without Ctrl are the ends,
Page Up and Page Down a windowful, and Ctrl+Page Up/Down left to tabs. Where a
text field sits over a list, Home and End are the text's while it has text and
the list's when it is empty or with Ctrl.

## 2. The password manager's two ways out (C-Q25, §1417)

In `apps/credmanager`:
- **A plain-text export**, through the existing `export_csv`, behind a warning
  that says plainly the file will hold every password readable by anyone who
  gets it -- the operator's words: "making it totally clear to the user that A
  is an insecure option".
- **An encrypted backup** that restores everything and is useless to anyone
  without the vault's password. **Not** by connecting `serialize_backup` as it
  stands: it omits the passwords, so it would write a backup that restores
  empty logins.

(The third way the operator added -- a program asking the password manager for
a password, with a capability and a consent prompt -- is lane C's, in
`gui/credentials`.)

## 3. Four programs' settings, kept across restarts (C-Q26, §1418)

Each program keeps its settings in its own `~/.config/<program>.yaml`, written
through `gui/settingsfile` (which exists, preserves comments, and is what the
desktop's own settings use). The four that asked:

| Program | Setting |
|---|---|
| `apps/lockscreen` | `show_clock_seconds`, `show_date` |
| `apps/markdowneditor` | `autosave_enabled` |
| `apps/passwordgen` | the password rules (`PasswordPolicy`) |
| `apps/explorer` | the copy-onto-an-existing-name choice (`ConflictPolicy`) |

Saving stays each program writing its own file; the live-update service lane C
is adding sits beside that, never in front of it.

## 4. A sentence that promises a shortcut no longer on by default (C-Q24, §1416)

*Added 2026-09-27, after the defaults changed.* `apps/settings/src/main.rs`
(~line 4658) tells a user who turns automatic locking off: "It can still be
locked at any time from the start menu or with the lock shortcut." Lock (Super+L)
is no longer bound by default, so on a fresh install there is no lock shortcut.
"from the start menu" is still true (the power menu's Lock). Suggested: drop
"or with the lock shortcut", or say "or with a shortcut, if you set one". A
search of `apps/` found no other text naming a shortcut that is now unbound.

## If this is never done

Nothing gets worse: the editing keys work where they work now, the password
manager still has no way out, and those four settings still reset each run.
