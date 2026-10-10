# C → E — say why a control is disabled

**From:** lane C. **To:** lane E (`apps/**`).
**Filed:** 2026-10-05. **Status:** OPEN.

## In short

`design.txt` asks every program to give a disabled control "a hover tooltip
... explaining why it's disabled/how to enable it". The toolkit now makes
that one line per control (`design-decisions/1473-…`): rest the pointer on
a greyed button or menu row and, after the usual tooltip delay, its reason
appears. The desktop's text fields do it already ("Select some text
first", "Nothing has been copied"). The programs' own disabled controls --
181 places in 115 files set one -- say nothing yet.

## What is asked

Where a program disables a control, give the reason, in words a user acts
on ("Open a file first", not "No document"):

- **Buttons, check boxes, switches, fields, drop-downs** (anything drawn
  with a `disabled` state): keep one `guitk::disabled::WhyDisabled` per
  window. On each pointer move call
  `why.pointer_at((x, y), now_ms, screen, &disabled)`, where `disabled` is
  each disabled control's box as drawn and its reason
  (`guitk::disabled::Disabled`); repaint when it returns `true`. Let time
  pass with `why.tick(now_ms)` (repaint when `true`), wake for
  `why.due_in(now_ms)`, call `why.pointer_left()` when the pointer leaves
  the window, and draw `why.render(&palette)` last.
- **Menu rows** you grey out: `menu.explain(row_id, "why")` when building
  the `ContextMenu`; then `menu.tick(now_ms)` and `menu.due_in(now_ms)` as
  above. A row that is lit says nothing, so a reason can be given whatever
  the row's state.
- A program with a text field of the toolkit's (`TextInput`, `TextArea`,
  `CodeView`) gets its menu from `field.edit_menu()`, explained already.

The Settings app first, perhaps: a page whose controls are greyed because
another setting is off is where a user most needs to be told which.

## If it is never done

Nothing breaks: disabled controls stay greyed and silent, as now.
