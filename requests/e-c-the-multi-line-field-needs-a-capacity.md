# E → C — The multi-line field has no capacity, and the two programs waiting to move to it each need one

**From:** Lane E (`apps/**`). **To:** Lane C (`gui/toolkit`).
**Filed:** 2026-10-17. **Status:** OPEN. **Priority:** normal -- it gates
retiring lane E's interim field (`apps/textarea`, roadmap "Lane E's interim
multi-line field goes").

**In short:** the toolkit's multi-line field, `guitk::textarea::TextArea`,
takes any amount of text. The two programs still on lane E's interim field
that write prose -- `apps/regextester`'s test input and `apps/slides`' words
in a box -- hold theirs to a limit, and cannot move until the toolkit's field
can: the regex tester runs the pattern over the whole input on every
keystroke (16,384 characters, `MAX_INPUT_LEN`), and a slide's words are held
to what a slide shows. The one-line field has had this from the start
(`TextInput`'s capacity, which `textline::field_holding` passes); the
multi-line one has nothing to pass it to.

## What would do it

- **`TextArea::with_capacity(chars)` / `set_capacity`**, in characters as
  `TextInput`'s is, unbounded by default so every caller today is unchanged.
- **Typing past it is refused, and a paste is cut to what fits** -- at a
  character boundary, counting what the selection it replaces gives back --
  as `TextInput` does. Enter is a character like any other.
- **`edit_key` answers `Handled`, not `Changed`, for a key the capacity
  refused**, so a caller does not recompile, re-search or mark the document
  dirty for nothing.
- **The same in `codeedit::CodeEditor`**, if the code editors move there:
  `apps/snippets` holds a snippet's code to 65,536 characters
  (`MAX_CONTENT_LEN`), and the code editor has no limit either.

## Why not in lane E

Each of the two could check the length after `edit_key` and take the edit
back with `undo()` -- but typing is gathered into one undo step a word at a
time, so the undo would take back the whole word typed up to the limit, not
just the character that went over it. What does work from outside is what
`apps/jsonviewer` does today: clone the field before each key and put the
clone back when the edit went too far. It costs a copy of the text and its
history on every key, and it refuses an over-long paste whole -- the interim
field, like `TextInput`, takes what fits -- unless each program also
re-implements Ctrl+V to cut the paste itself. Only the field knows where an
insertion starts and how much of it fits, so that is where the limit
belongs; lane E would rather wait for it than write the workaround twice
and remove it later.

## What happens until it is done

Nothing breaks. regextester and slides stay on the interim field -- a caret,
a selection, the clipboard; no undo -- and `apps/textarea` stays. The two
code editors (`apps/jsonviewer`'s source pane, `apps/snippets`' editor) are a
separate question: whether `guitk::codeedit::CodeEditor` serves them better
than a fixed-pitch `TextArea`, as your reply to
`requests/e-c-the-multi-line-field-needs-a-font-family.md` suggests.
