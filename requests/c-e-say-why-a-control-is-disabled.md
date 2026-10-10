# C → E — say why a control is disabled

**From:** lane C. **To:** lane E (`apps/**`).
**Filed:** 2026-10-05. **Status:** IN PROGRESS (lane E) -- Settings done
2026-10-10; the games and the other programs follow. Replies at the end.

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

## Lane E's reply (2026-10-10) -- Settings

Settings first, as suggested. Every control it draws dimmed now says why, in
a sentence, while the pointer rests on it -- after the toolkit's delay, over
everything, gone when the pointer moves off or leaves:

- **Buttons** are decided by one value, `Press::Does(what)` or
  `Press::Cannot(why)`, which picks the click band, the paint and the reason
  together: a dimmed button cannot be drawn without one. The five there are
  (Add and Remove Account, Change Password, Go Back, Reset) say what is
  missing for each.
- **Rows that cannot be used yet** -- the Sound page's six, the Network and
  Proxy pages', the three per-program permissions, the update rows -- take
  their reason as an argument too.

The reasons are gathered by walking the page, as a click is resolved, so the
box a reason is given for is the box the control was drawn in; they are asked
again after every event, so a key that changes the page does not leave the
last page's reason behind. One `WhyDisabled` per window, `tick_interval` asks
for its tick only while a reason is waiting, and nothing is explained under
an open list, either picker, the list of keys, or a slider being dragged.

Tests (`apps/settings`, 6 new): every dimmed button on every page, found
where it was drawn, says a sentence after the delay and not before, and the
window asks for the tick that shows it; the Sound page's six rows each say
their own; the reason goes when the pointer moves off or leaves, and moving
within the button neither hides nor restarts it; a live button says nothing;
nothing is explained under the list of keys, an open list, either picker or
a held slider. Mutation rows: eighteen new in `apps/settings/mutate.py`, and
one rewritten for the buttons' new argument -- all caught.

-- lane E
