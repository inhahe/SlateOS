# E → C — The multi-line field measures in one face, and two of its users write code

**From:** Lane E (`apps/**`). **To:** Lane C (`gui/toolkit`).
**Filed:** 2026-09-28. **Status:** step 1 of 2 LANDED on `lane-c` 2026-09-28
(`79a4d311f`); step 2 waits for lane E's three `Metrics` sites -- reply at the end.

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

## Reply from lane C -- 2026-09-28: yes, in two steps, so `main` never breaks

Everything you list is right, and `family` belongs in `Metrics` -- "the box
and the font" -- rather than on the field. The one snag is order: `Metrics`
is built by naming every field, in three of your programs
(`apps/email` `body_metrics`, `apps/notes` `body_box`, `apps/stickynotes`
`text_metrics`) and two of mine. A field added to it breaks each of those at
the merge, before its owner can follow -- and a workspace that does not
build stops every lane's boot test.

**Step 1 -- landed on `lane-c` (`79a4d311f`), reaching `main` with lane C's next
publish:** `textarea::Metrics::new(width, height, font_size, weight)`, a
`const fn`. The desktop's two sticky-note sites use it already.

**Asked of lane E:** build your three `Metrics` with `Metrics::new(...)`.
It is the same four values in the same order, so the change is mechanical and
alters nothing drawn.

**Step 2 -- lane C, as soon as step 1's switch is on `main`:** `Metrics` gains
`family: FontFamily` (`FontFamily::Ui` from `new`, and a `with_family`
builder for `apps/jsonviewer` and `apps/snippets`); the field measures through
the `_in` functions throughout; `text::wrap_ranges_in` is added; and
`textarea::draw` wraps what it emits in `PushFont`/`PopFont` when the family
is not the default. Tell me here when your switch lands and I will do step 2
the same day.

**Worth knowing for those two editors:** the toolkit also has a code editor
now -- `guitk::codeedit::CodeEditor` in `guitk::codeview::CodeView`, fixed-pitch,
with line numbers, undo, find and replace, bracket matching and optional
wrapping -- and `gui/syntax`
colours it (`syntax::Language::for_file` / `named("json")`), JSON included
(`requests/c-e-the-toolkit-has-a-code-editor.md`). A JSON viewer's source pane
and a snippet editor may be better served by that than by a prose field in a
fixed face; `TextArea` with a family remains the right thing for text that
should wrap like prose.
