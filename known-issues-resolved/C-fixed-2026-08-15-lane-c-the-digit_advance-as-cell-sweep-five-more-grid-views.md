## FIXED (2026-08-15, lane C) — the `digit_advance`-as-cell sweep: five more grid views

The tmux fix above named a hazard rather than an isolated bug: `digit_advance`
returns a digit's advance **in the proportional UI face**, which is a cell only
digits fit. Every caller using it to size a character grid had the same defect
latent, and `grep` found five more. All are now on `text::cell_advance` and
draw inside a `PushFont { Mono }` scope.

| Where | What it laid out on the wrong cell |
|---|---|
| `gui/toolkit/src/textview.rs` — `SimpleTextView` | Log/terminal output. Spans overran their own selection bands and search highlights; every column after the first drifted. |
| `apps/hexeditor` | The **ASCII column** — the earlier doc argued the grid was all hex digits and overlooked the column beside it, which draws whatever the bytes spell. `hit_test`'s `(ascii_x / char_w)` is this arithmetic run backwards, so a click resolved to the wrong byte, further wrong the further right it fell. |
| `apps/filediff` | The inline view's character-level highlight is placed at `columns(span) * char_width()`, so it slid off the very change it was drawn to mark. |
| `apps/markdowneditor` | The source pane's caret (`col_x`), selection band and find highlights drifted left of their characters, further with every wide glyph on the line. |
| `apps/snippets` | The token pen advances `columns(token) * char_width()`, so consecutive tokens on a line overlapped and indentation stopped lining up between rows. |

Each now carries two postcondition tests — every glyph of a sample set fits the
cell, in regular *and* bold (bold marks keywords, changed spans and headings on
the same grid) — plus a scope-balance test that walks the command list and
asserts the depth returns to zero, the scope was opened exactly one deep, and
glyphs were actually drawn **inside** it. That last clause is what stops the
test passing vacuously on an empty view.

**One caller was deliberately left proportional.** `RichTextView`'s
`char_width` looked like the same bug but is not: the widget was already
migrated to measure spans with `text::measure` and draw them proportionally,
and `char_width` survives only as the width of a gutter digit and the quantum a
list indents by. Both are UI-face quantities, so it now calls a separate
`default_indent_unit`, and the misleading "(monospace)" doc on the config field
is corrected. A test pins it to the UI face so the sweep cannot later "fix" it
into a regression.

**Remaining debt (not a bug, an enhancement).** `RichTextView` renders
`RichBlock::CodeBlock` in the proportional UI face like the prose around it.
That is self-consistent — the spans are measured in the face they are drawn in
— so nothing misaligns, but a code block *should* be mono now that the toolkit
can express it. Doing it properly means threading a family through
`span_width`, `x_of_col`, `col_at_x` and `wrap_spans` so the wrap is computed
in the same face the block is drawn in. The widget currently has **no callers
outside its own file**, so this is queued rather than urgent.
