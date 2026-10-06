## 1473. A disabled control says why, from a reason its owner gives -- kept beside the menu's rows, not in them

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a greyed-out button or menu row can now say why it is greyed
out: rest the pointer on it and, after the usual tooltip delay, a tooltip
gives the reason -- "Select some text first", "Nothing has been copied",
"This text cannot be changed". `design.txt` asks programs to do this for
every disabled control; the toolkit now makes it a line of code. The
desktop's text fields already do it in their right-click menus. Programs
adopt it for their own controls (lane E).

**Where:** `gui/toolkit/src/menu.rs` (`ContextMenu::explain`, `tick`,
`due_in`, `showing_reason`), `gui/toolkit/src/editmenu.rs` (`why_dimmed`,
`menu`), `gui/toolkit/src/disabled.rs` (`WhyDisabled`, `Disabled`), the
fields' `edit_menu`, and `gui/desktop/src/lib.rs` (`advance_osd`,
`tooltip_due_in`); asked for by `design.txt` (the disabled-control tooltip)
and `roadmap-detailed.md` §3.5 (*Enable/disable controls API*).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A menu's reasons kept on the menu, by row id** (`ContextMenu::explain`) | a field in `MenuItem::Action` and `Submenu` | `MenuItem`'s variants are built field by field in 94 places, 25 of them in lane E's programs: a new field would break every one of them at once, across a lane boundary. A side table breaks none, and a row is named by the id it already has. | Two places say what a row is. A later `MenuItem` that grows a builder could fold it in. |
| **The reason only while the row is greyed** | whenever explained | A reason can be given once, whatever the row's state; a row that lights up stops explaining itself without anyone remembering to clear it. | -- |
| **Shown after the tooltip delay, the wait started by the first `tick`** | at once; or a timestamp on every pointer move | `handle_mouse_move` takes no time and lane E's programs call it; a menu whose owner never ticks behaves exactly as before. At once would flash a reason over every greyed row a pointer crosses. | An owner must tick, and wake for `due_in`, to see reasons -- as for any tooltip. |
| **One `WhyDisabled` per window, handed the disabled controls' boxes and reasons on each move** | a reason inside each control's state | The toolkit's controls are drawn by their owner each frame from small `Copy` states; a `String` would end that, and only the owner knows which control is where. | The owner repeats its disabled controls' boxes; it has them, having just drawn them. |
| **`DisabledOverlay` and `render_reason_tooltip` removed** | kept beside the new | Neither had a caller anywhere. The first promised a "not-allowed" cursor and blocked input it did not provide; the second was a second tooltip renderer, without the screen edges or the delay, beside `menu::Tooltip`. A third would have joined them. | -- |

The edit menu's dimming and its reasons are one function (`why_dimmed`):
`rows` dims exactly the rows it explains, so a menu cannot grey a row it
does not explain, nor explain one it does not grey.

**What is not done.** The programs' own controls and menus are lane E's to
explain (`requests/c-e-say-why-a-control-is-disabled.md`). A disabled
control cannot take the keyboard, so its reason is the pointer's alone; a
screen reader would need it spoken, which waits on an accessibility tree.
