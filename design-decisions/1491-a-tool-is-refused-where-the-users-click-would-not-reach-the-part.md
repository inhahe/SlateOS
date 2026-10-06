## 1491. A tool is refused where the user's click would not reach the part

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a screen reader or a script presses the desktop's parts --
the taskbar's tiles, the start menu's rows, a notification -- through the
shell's own click at the part's middle, so the press does exactly what the
user's would. But the shell has surfaces that take a press meant for
something else: a menu open over everything takes every press, the
notification pane's dimmed backdrop covers the desktop, and an open flyout
or menu closes when the user clicks anywhere off it, without the click
going any further. When a tool asks to press a part a click would not
reach, the shell now refuses ("the widget is hidden") and changes nothing,
rather than clicking anyway and reporting success for a click that went
elsewhere. The tool closes what is in the way first -- it sees the open
surfaces in the same tree, and Escape closes them all -- and presses again.

**Where:** `gui/desktop/src/accessible.rs` -- `takes_every_press`,
`reaches`, `click_part`, `click_icon`, `invoke_pane`; and
`gui/desktop/src/lib.rs` -- `DesktopShell::closed_by_press`, which is now
the one statement of which open surface a press closes instead of reaching
what it landed on, read both by `handle_press_with` and by the automation.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Refuse, and change nothing** | click anyway and answer `Ok` | A tool told "pressed" for a click that only closed a flyout believes a program started that did not; the toolkit's own refusals already mean "its user could not reach it". | A tool must close what is open before pressing; one that does not handles one more refusal. |
| **Refuse, rather than close what is in the way first and then press** | doing the user's first click (the one that closes the flyout) on the tool's behalf, as a row scrolled out of sight is scrolled in first | Scrolling changes only where the user is looking; closing a menu the user opened -- the power choices over the start menu, say -- throws away something they were in the middle of, which the tool did not ask for. | Two steps for the tool where one would do in the common case, and an asymmetry with scrolling that has to be explained (here). |
| **One rule for what a press closes, shared by the shell and its automation** | the automation keeping its own copy of the shell's dismissal rules | The two cannot drift: a new popup's dismissal added to the shell is known to the automation in the same change. | `handle_press_with` reads its dismissals through an extra function and an enum rather than five `if`s in place. |
