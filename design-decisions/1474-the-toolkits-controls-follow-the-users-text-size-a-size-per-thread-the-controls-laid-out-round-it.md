## 1474. The toolkit's controls follow the user's text size: a size per thread, the controls laid out round it

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the text size a user chooses in Settings now reaches the
toolkit's controls -- the menus, tooltips, buttons, check boxes, drop-downs,
switches, tabs, menu bars, dialogs and the path bar -- which drew at 13
pixels whatever was chosen, so a larger size enlarged the desktop's own text
and the window titles and left every program's controls small. A control
grows with its text: a menu's rows, a button's padding, a check box's box,
not only the label inside them. At the default size nothing moves.

**Where:** `gui/toolkit/src/text.rs` (`DEFAULT_SIZE`, `set_base_size`,
`base_size`, `size_scale`, `scaled`), the toolkit's controls, and
`appearance::FontSettings::apply`, which sets it from `fonts.ui_size` in every
program; asked for by `roadmap-detailed.md` (*Tier 2 -- Font Preferences*:
font sizes) and found broken on the way to the fonts axis (§1472).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **One size, and every dimension round text scaled by it** (`scaled(28.0)`) | text alone | Text larger than the row it sits in spills out or is cut off. A row, a padding, a box laid out for 13 pixels has to grow with the text in it, and a ratio of the default does that for every control at once. Hairlines -- borders, rules -- and decoration -- corners, shadows -- stay as they are. | A layout that was right at 13 can be wrong at 32 in ways no ratio fixes: a dialog of fixed width wraps differently. Each such case is a bug of its own. |
| **Per thread** (`thread_local`) | per process, as the font family is | A program's windows are laid out and drawn on its event loop's thread, so that thread's size is the program's -- and tests, each on a thread of its own, can lay a control out at twice the size without moving one that a test beside them measures. The program's clipboard is per thread for the same reason. | A program that lays a window out on a second thread sees the default there. None does. |
| **The public constants kept, with functions beside them** (`button::HEIGHT` and `button::height()`) | constants turned into functions | Lane E's programs use the constants in ten places; turning them into functions would break their build across a lane boundary. The constants are now documented as the default size, and the functions give the user's. | A program that lays out from a constant lays out for the default size until it moves to the function (`requests/c-e-lay-out-from-the-text-size.md`). |
| **The display's scale left out** | the size and the scale as one factor | Programs do not apply the display's scale at all today (`known-issues/TD-C-PROGRAMS-DRAW-AT-ONE-SCALE-WHATEVER-THE-DISPLAYS-IS.md`), and where it belongs -- the compositor, scaling what a program draws, or every program -- is an open question (C-Q34). Folding it in here would decide it by accident. | -- |

**What is not done.** A program's own text -- the sizes it writes beside the
toolkit's controls -- follows only when the program reads the size
(`guitk::text::scaled`, or `fonts.ui_size` from the settings it is handed):
lane E's (the request above). The text view's default size is document
text, not interface, and is left to the view's caller; code and terminals
have their own size setting (`fonts.mono_size`).
