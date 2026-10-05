## 1467. A window minimised to the tray is an entry the shell adds to the tray itself

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** `design.txt` lets a user make any program "always start in
system tray, always in taskbar, or neither". That is now a window rule
(`window-rules.yaml`, §1465): `tray: true` sends a program's windows to the
system tray when they are minimised, and with `state: minimized` starts them
there; `tray: false` keeps them in the taskbar; `taskbar: false` gives them
neither. A window in the tray has no taskbar button and no place in Alt+Tab;
it has an icon in the tray -- its program's picture, its title as the
tooltip -- which brings it back when clicked and opens its menu on a right
click. The window itself stays what it was, a minimised window: only where
the shell lists it changes.

**Where:** `gui/windowrules/src/lib.rs` (`RuleActions::to_tray`),
`gui/windowrules/src/file.rs` (the `tray` key), `gui/desktop/src/lib.rs`
(`ManagedWindow::in_tray`, `sync_window_tray`, `rebuild_tray`,
`click_tray_icon`, `SHELL_TRAY_OWNER`), tests in
`gui/desktop/src/tray_window_tests.rs`.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The shell keeps the entries, in the same list as the programs' icons**, owned by `SHELL_TRAY_OWNER` (0, which the compositor gives no connection) | asking the compositor to register a tray icon on the window's behalf | Nothing crosses a lane or the wire: the shell already knows which windows are minimised and what each rule said. One list means the tray's order, its overflow behind the chevron, its dragging and its tooltips treat a window's entry like any icon, with nothing to keep in step. | The shell's entries are gone if the shell restarts -- until the next window list, which rebuilds them, as it rebuilds the taskbar. |
| **The window stays minimised in the compositor** | hiding (unmapping) it | Minimised is what it is: the overview still finds it, `Activate` brings it back in the state it was minimised from (maximised stays maximised, as on the taskbar button), and a window hidden by the shell could not be told from one its program hid. | The overview -- which shows every window, as it does a rule's `taskbar: false` ones -- shows a window the taskbar does not. |
| **An entry's id is the window's for as long as it is open** | a fresh id each time it goes to the tray | A window retitling itself while in the tray is relabelled where it is rather than moved to the end. | A small map from window to id, pruned as windows close. |
| **The tray holds every desktop's windows** | only the showing desktop's, as the taskbar does | A program's tray icon is every desktop's; a window put away in the tray is reached the same way, and clicking it switches to it as the overview does. | A window put away on desktop 2 shows in the tray on desktop 1. |
| **A right click opens the window's menu; the middle button does nothing** | sending clicks on to the program, as a program's own icon's are | The entry is the shell's, standing in for the taskbar button, so it answers as the button does. | -- |

**A program's own wish** -- "minimise me to the tray" -- needs a flag on its
window carried in the window list, which is lane F's wire
(`requests/c-f-let-a-window-say-it-goes-to-the-tray.md`). A rule's `tray`,
either way, is meant to overrule it; that is why the rule is a three-way
`Option` and not a switch.
