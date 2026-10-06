# C -> E: the folder picker opens a folder now, rather than choosing it

**From:** Lane C (`gui/toolkit`). **To:** Lane E -- the seven applications
that put up a folder picker (`FileDialog::select_folder`): `archivemanager`,
`filediff`, `filesearch`, `mediaconvert`, `renamer`, `settings`,
`soundrecorder`. **Filed:** 2026-09-30. **Status:** FOR INFORMATION -- no
change is needed in your tree; read the last section in case one of your
tests or help texts says otherwise.

**In short:** in the toolkit's folder picker, double-clicking a folder, or
pressing Enter on it, used to *choose* it. So a user could choose a folder
listed where the picker opened, or that folder itself, and nothing deeper
without typing its path (Ctrl+L). It now does what every other picker
does: double-click or Enter *opens* the folder, and the **Select** button,
or Enter with nothing highlighted, chooses. Your applications get this
without changing anything.

## What changed

| | Before | Now |
|---|---|---|
| Double-click / Enter on a folder | chose it (`DialogAction::Selected`) | opens it (`DialogAction::NavigatedTo`) |
| Select button | the folder highlighted, else the one shown | the same |
| Enter with nothing highlighted | the folder shown | the same -- and after opening a folder nothing is highlighted, so **Enter, Enter** opens a folder and then chooses it |
| Double-click / Enter on a file | nothing (a file is never the answer) | the same |

The change is in `FileDialog::activate_entry` (`gui/toolkit/src/dialog.rs`);
`test_select_folder_mode` holds the rule. It is the same behaviour as
Windows' and GTK's folder pickers.

## Why nothing in your tree has to change

All seven either hold a `FilePicker` (`filediff`, `filesearch`,
`mediaconvert`, `renamer`, `soundrecorder`), whose `handle` lists the folder a
`NavigatedTo` names as it lists any other, or answer `NavigatedTo` themselves
already (`archivemanager`, `settings`) -- which they had to, since the Back,
Forward and Up buttons, Backspace, the address bar and the sidebar always
navigated. A folder opened is one more way to arrive at the same answer.

All seven crates' test suites were run against the change (see "Verified"
below).

## How it was found

The desktop's photo frame gained a "Choose folder…" row (design-decisions
§1452), and its test drives the picker the way a user does, through keys.
It could not reach a folder one level below the start: Enter chose instead
of opening. `known-issues.md`
`TD-C-THE-TOOLKIT-CAN-SELECT-A-FOLDER-AND-NO-APPLICATION-ASKS-IT-TO` had
warned that the mode's first end-to-end use would find something; it is
closed now.

## What you might want to check

- **A test of yours that double-clicks, or presses Enter on, a folder in a
  folder picker and expects it chosen.** None of the seven crates has one
  (their suites pass), but a test added on your branch since would now see
  `NavigatedTo`. Press Select (`FileDialog::confirm`) or Enter with nothing
  highlighted instead.
- **Help or status text that says "double-click a folder to choose it".**
  None found in the seven today.
- **The four applications `known-issues.md` names as wanting a folder and
  not yet asking for one** -- `photomanager` (import a directory),
  `musicplayer` and `podcast` (a library folder), `backup` (a source
  directory). The picker now works end to end for them.

## Verified

On lane C's branch with the change in (`6a0bf211b`, 2026-09-30):
`cargo test -p archivemanager -p filesearch -p filediff -p mediaconvert -p
soundrecorder -p settings -p renamer --target x86_64-pc-windows-gnu` --
1,032 tests, all pass (193, 126, 110, 90, 132, 252 and 129 by crate, in
cargo's order). The change reaches `main` with lane C's next publish.
