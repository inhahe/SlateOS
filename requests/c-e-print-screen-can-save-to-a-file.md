# C -> E -- Print Screen can save to a file: the screenshot tool learns `--save`

**From:** Lane C. **To:** Lane E (`apps/screenshot`).
**Filed:** 2026-09-27. **Status:** OPEN -- the shell already sends the flag;
the tool ignores it.

**In short:** the operator asked for Print Screen shortcuts that save the
capture as a file, asking where -- one for the whole screen, one for the
focused window (`design-decisions.md` §1416, answering C-Q24). The desktop now
binds four Print Screen chords by default, all starting `/usr/bin/screenshot`.
Two of them pass a second word, `--save`, which the tool does not read yet, so
today they behave exactly like their plain twins.

## What the shell starts

| Chord | Command line |
|---|---|
| Print Screen | `screenshot --fullscreen` |
| Alt+Print Screen | `screenshot --window` |
| Ctrl+Print Screen | `screenshot --fullscreen --save` |
| Ctrl+Alt+Print Screen | `screenshot --window --save` |

(`gui/desktop/src/hotkeys.rs`, `HotkeyAction::launch`; pinned by
`each_print_screen_starts_the_tool_in_its_own_mode`.) The mode is always the
first word, as `apply_command_line` expects; `--save` only ever follows it.

## What is asked

In `ScreenshotApp::apply_command_line`, accept `--save` after the mode word:
capture as that mode says, then open the toolkit's Save dialog
(`guitk::dialog::FilePicker`, in save mode, starting in the folder the tool
already saves to and with its usual generated name filled in) and write the
file where the user chooses. Cancelling the dialog discards the capture. A
test in the style of the existing flag tests (the table at
`main.rs` ~2705) can pin both words reaching their effect.

Without `--save`, §1416 records the plain chords as copying the capture to the
clipboard, as Windows does. That half waits on the clipboard reaching other
programs at all (open question C-Q29, `known-issues.md`
`TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO`), so it is not
asked for here; until then the plain chords keep doing what the tool does now.

The tool cannot capture anything yet (`CANNOT_CAPTURE`), so none of this
produces a file until capture works; the flag handling and the dialog can land
first and be tested on their own.

## If this is never done

Ctrl+Print Screen and Ctrl+Alt+Print Screen do what Print Screen and Alt+Print
Screen do, and the operator's save-to-file shortcuts do not exist in practice.
