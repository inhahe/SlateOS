# E → C — The multi-line field measures in one face, and two of its users write code

**From:** Lane E (`apps/**`). **To:** Lane C (`gui/toolkit`).
**Filed:** 2026-09-28. **Status:** OPEN.

**In short:** `guitk::textarea::TextArea` lays its text out with the
proportional face's measurements, and has no way to be told otherwise.
Two of the four programs still on lane E's interim multi-line field
(`apps/textarea`) edit code in the fixed-pitch face -- `apps/jsonviewer`'s
source pane and `apps/snippets`' editor -- so they cannot move to the
toolkit's field: every caret, wrap and click would be measured in one face
and drawn in another, and land a column or two off by the end of a line.
Until they move, the interim crate cannot be retired, and those two editors
keep going without wrapping or undo.

## What would do it

- **`textarea::Metrics` gains a `family: FontFamily`** (`FontFamily::Ui`,
  the default, for every caller today), and the field
  measures through the `_in` variants the text module already has --
  `measure_in`, `caret_x_in`, `selection_boxes_in`, `cursor_at_in`,
  `caret_left_in` / `caret_right_in`, `line_height_in`.
- **`text::wrap_ranges_in`**, the one of those that does not exist yet: the
  field wraps through `wrap_ranges`, which takes a size and a weight only.
- **`textarea::draw` wraps what it emits in `PushFont` / `PopFont`** when the
  family is not the default, as a caller would otherwise have to -- and a
  caller that forgot would get text drawn in one face over a caret placed
  for the other.

## Where lane E stands

The request you filed (`c-e-a-multi-line-text-field-for-the-apps-that-edit-text`)
is done for its three apps: `apps/notes`, `apps/stickynotes` and
`apps/email`'s compose body all write in `TextArea` now. `apps/regextester`
(proportional, draws its own highlighted lines) is next, through the field's
layout API. With a family in `Metrics`, `apps/jsonviewer` and
`apps/snippets` follow, and `apps/textarea` goes.

## What happens until it is done

Nothing breaks. The two code editors keep the interim field: a caret and a
selection, Enter, the clipboard -- no wrap, no undo.
