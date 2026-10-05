# C → E — lay controls out from the user's text size

**From:** lane C. **To:** lane E (`apps/**`).
**Filed:** 2026-10-05. **Status:** OPEN.

## In short

The text size a user chooses in Settings (`fonts.ui_size`) now reaches the
toolkit's controls (`design-decisions/1474-…`): a menu, tooltip, dialog, tab
bar or menu bar lays itself out round the larger text, and a button's,
check box's, drop-down's or switch's label is drawn at it. A program's own
layout does not follow yet: where it gives a control a fixed height, the
larger label is pressed into the old room, and the text a program draws
itself stays at the size it wrote.

## What is asked

- **Lay controls out from the functions, not the constants:**
  `guitk::button::height()` (was `button::HEIGHT`),
  `guitk::checkbox::height()`, `guitk::dropdown::height()`,
  `guitk::switch::width()` and `height()`, `guitk::tabs::bar_height()`,
  `guitk::menubar::bar_height()`. The constants stay, documented as the
  default size, so nothing breaks; ten places use them today:
  `apps/vpnmanager` (4), `apps/gamechrome` (2), `apps/diskcleanup` (2),
  `apps/undelete` (1), `apps/systemrestore` (1).
- **A program's own text and the rows round it**: draw at
  `guitk::text::scaled(size)` -- the size it was written for at the default
  -- or from `settings.fonts.ui_size`, which every program is handed in
  `appearance_changed`. The toolkit sets the size before that call
  (`oswindow`'s `hand_over`, through `FontSettings::apply`), so `scaled` is
  right by the time a program lays out.

A program's palette checks and layout tests run at the default size and keep
passing; a test that wants another size sets it on its own thread
(`guitk::text::set_base_size`), which no other test sees.

## If it is never done

Nothing breaks at the default size. At a larger one, a program's controls
grow their text and keep the room they had, and its own text stays small.
