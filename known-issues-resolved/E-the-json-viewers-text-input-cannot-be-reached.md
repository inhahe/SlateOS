### [E] The JSON viewer's text input cannot be reached -- 2026-09-25
**Status:** FIXED 2026-09-26 -- `apps/jsonviewer/src/main.rs` (`SourceEdit`,
`App::open_source`, `source_key`, `source_click`, `render_source`). The raw
view edits the document's own text: Enter there, Enter in a tree that is empty
or does not parse, or a press in the raw view opens it -- verbatim, in the
fixed-pitch face, with a caret, a selection, Ctrl+A/C/X/V, Tab as a step of the
text's own indent, the caret kept on screen down the text and along a long
line (a minified document is one line, and only the part on screen is drawn).
Every change is written back to `input` and parsed again at once, so the tree,
the statistics and Ctrl+S see it; the line that fails is marked in the gutter
and the parse's verdict is at the foot. Enter on a document that does not parse
puts the caret where the parse failed, and the error banner now says so.
Escape, or leaving the raw view, stops; the find bar and the text each close
the other. The text cannot grow past `MAX_OPEN_BYTES`, what opening it again
would read whole -- the status line says so when a key is refused. `guitk` still
has no multi-line editor, but `apps/textarea` is one, and it serves here. The
dead `input_focused`/`cursor_pos`/`handle_input_key` are gone. Found beside it
and fixed: the wheel moved every view three *pixels* a notch (its `dy` is in
notches -- `guitk::wheel::pixels` now), and opening a file read all of it before
applying the 8 MiB cap (`safeio::read_to_string_capped` now). 18 tests;
`apps/jsonviewer/mutate.py` has 40 rows for it.

**In short:** a new JSON viewer tab says "Enter JSON in the input area or
paste a document" (now "press Ctrl+O to open a JSON file", which is true), and
there is no input area. `handle_input_key` is a complete little text editor --
insert, Backspace, Delete, arrows, Home, End, Enter -- guarded by
`input_focused`, and nothing ever sets `input_focused` to `true`; only Escape
sets it, to `false`. So a new document can never be given any content, and an
opened file with a parse error -- the commonest reason to open JSON in an
editor -- can be looked at but not repaired.

**Where.** `apps/jsonviewer/src/main.rs`: `App::input_focused`,
`App::handle_input_key`, and the raw view (`render_raw_view`), which draws the
*formatted* text rather than `input` and has no caret, so it cannot simply be
made focusable.

**The proper fix** is a source-editing mode for the raw view: a click (or
Enter) in it edits `input` itself -- drawn verbatim, with a caret, Up/Down by
line, the caret kept on screen, the parse re-run as the text changes -- and
Escape returns to the formatted view. `guitk` has no multi-line editor to lend
(`textedit` is single-line), so the editing stays in this crate.
