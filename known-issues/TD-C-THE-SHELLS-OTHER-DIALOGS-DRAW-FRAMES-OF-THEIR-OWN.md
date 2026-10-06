### [C] TD-C-THE-SHELLS-OTHER-DIALOGS-DRAW-FRAMES-OF-THEIR-OWN -- 2026-10-01

**Status:** OPEN

**In short:** the run box now wears the theme's window frame
(`design-decisions.md` §1461). Two more window-like dialogs in the shell's
tree draw a box and a heading of their own, the same under every theme: the
security prompt (`gui/desktop/src/security_dialog.rs`) and the print dialog
(`print_manager.rs`). Neither is shown today -- both are in the orphan-module
baseline (`scripts/orphan-modules-baseline.txt`); the security prompt waits on
`TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE` -- so nothing a user
sees is wrong yet, but each would come up looking like another desktop under a
theme whose window frames differ from the built-in ones. About (`about.rs`)
goes when lane E's About page reaches `main`, and is not worth converting.
The clipboard history (`clipboard_viewer.rs`) is a flyout and the shortcut
editor (`shortcut_editor.rs`) part of the shortcut card: neither is a window,
and neither should wear a window's frame.

**The proper fix:** when each of the two is wired up, it takes a
`dialog_frame::DialogFrame` from the shell (`set_frame`, as the run box does
in `DesktopShell::set_appearance`), lays its content out in the frame's
`content` rectangle, draws the frame first, and treats the frame's close
button as its own Cancel -- with a test that a theme's taller title bar moves
its content down and its close button closes it.
