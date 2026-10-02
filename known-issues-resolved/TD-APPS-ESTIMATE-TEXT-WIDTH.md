## TD-APPS-ESTIMATE-TEXT-WIDTH — apps still guess at text width instead of measuring it

**Status.** **Closed for the original defect** as of 2026-08-14. `gui/**` was
converted on 2026-08-13 (`fa5135c36`, `f2d74c8a2`), the desktop shell's 18 files
and `gui/notifications` on 2026-08-14 (`210172279`, `b654d2cd0`), fourteen
applications after that (`1523f7412`, `353a41211`, `99198808c`, `77bf7bf14`,
`504c67bc0`, `0c107cce7`, `f5befec99`, `537a9eaa8`, `dba94444d`, `1e5dc7edb`,
`1b436c341`, `a3669e178`, `50c6b17a5`, `45aa280d1`), and the remaining tail in
two sweeps — seven surveyed files (`68d477601`) and the last 26 crates
(`6f4dd870e`). The last non-app site, the About dialog's licence list, went with
`7948cf8d5`.

Every survivor of `rg 'len\(\) as f32\)? \* [0-9]'` under `apps/` and `gui/` is
now either a comment, a genuine collection-length calculation, or a grid view
that legitimately counts characters against a cell derived from the face
(`textview`'s line-number gutter). Of the two follow-ups this entry used to
defer, one is done and one is tracked below:

- `apps/editor`'s own `char_width` config field is **gone** — the field was
  deleted and the doc comment in its place (`apps/editor/src/main.rs`, on
  `line_height`) now records *why* there is deliberately no horizontal
  counterpart: a nominal per-character width is only ever right for one font
  at one size, and wrong by a compounding amount along every line for all the
  others. There was never a separate `TD-` entry for it despite this sentence
  once promising one.
- The wrapping defect is `TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP`, immediately
  below.

Of ~90 apparent sites in the final survey, ~41 were real; the rest were false
positives from the naive pattern (`filediff`, `videoplayer`, `devicemanager`,
`benchmark` and `unitconverter` turned out to have none at all). Refine the
grep with an identifier-based pattern before trusting a count.

**What it is.** Roughly 322 sites across ~90 files under `apps/` and
`gui/desktop/` size and position text with a per-app fudge factor —
`text.len() as f32 * font_size * k`, where `k` is 0.5 in `apps/ebook`, 0.55 in
`apps/lockscreen` and `apps/launcher`, 0.6 in `apps/editor`, and a bare 8.0
elsewhere. Every one of these is wrong in the same two ways the toolkit's were:

1. `str::len` counts **bytes**, so any non-ASCII text is measured two to four
   times too wide. This was invisible while the compositor drew a box for every
   non-ASCII character; it is visible now that they render.
2. The constant describes a fixed 8x14 cell that the compositor no longer
   draws. `font_size` is honoured now, and the built-in face is only monospace
   until an outline face is loaded — at which point every one of these
   estimates becomes wrong even for ASCII.

The visible symptoms are the same class the toolkit had: labels overflowing
their buttons, text cursors landing between characters rather than on them, and
centred text drifting off-centre in proportion to its length.

**Where it lives.** The tail is now flat: no file has more than four sites.
Densest remaining: `apps/systemrestore`, `apps/spreadsheet`, `apps/podcast` and
`apps/dbviewer` (4 each), then `apps/unitconverter`, `apps/sysmonitor`,
`apps/settings` (three files), `apps/passwordgen`, `apps/notes`,
`apps/imageviewer` and `apps/fontmanager` (3 each), then 40 files with one or
two. `apps/editor/src/main.rs` is a special case: it declares its own
`char_width` config field, mirroring the `SimpleTextViewConfig` design that was
fixed in `guitk/textview`. `apps/spreadsheet` is likely a second grid case —
check whether its column widths are a genuine grid before converting.

**Reproduce.** `rg 'len\(\) as f32\)? \* [0-9]|CHAR_WIDTH|char_width' apps/
gui/desktop/`. Note the earlier version of this command missed two forms that
turned out to be common: a parenthesised cast (`(label.len() as f32) * 7.0`)
and a bare numeric factor with no named constant at all. Both are in the
pattern above. Files that legitimately derive a cell (`fn char_width()`) will
still match; check before converting.

**Proper fix.** Call `guitk::text` — `measure`, `width`, `fit`, `elide`,
`char_index_at`, `line_height`, `digit_advance` — which every app already has
in scope via its `guitk` dependency. There is no new API to design; the module
exists and the toolkit's own nine modules are already converted to it. Two
judgement calls recur:

- **Terminal-style views** (`apps/tmux`, `apps/hexeditor`, `apps/logviewer`)
  genuinely want a character grid. Keep the grid, but derive the cell width
  from `text::cell_advance(font_size, weight)` and draw the grid inside a
  `RenderCommand::PushFont { family: FontFamily::Mono }` scope, counting
  **characters** rather than bytes — this is what
  `guitk::textview::SimpleTextView` now does, and it is the pattern to copy.

  ⚠️ **This advice used to say `digit_advance`, and that was wrong.** See
  design-decisions.md §425. `digit_advance` returns a digit's advance *in the
  proportional UI face* — a cell only digits fit. At 13px a digit is 7.55px
  while `'W'` is 13.08px, so every non-digit glyph overhung its neighbour's
  cell, and any code inverting the arithmetic to hit-test resolved to the
  wrong cell, further wrong the further right. Five grid views were built on
  that advice before it was caught (`3477cf982`): `hexeditor`'s ASCII column
  (clicks selected the wrong byte), `filediff`'s inline highlight (slid off
  the change it marked), `markdowneditor`'s caret and selection band,
  `snippets`' overlapping code tokens, and `textview`'s own simple view. A
  cell is only a cell if the face is monospace — hence the scope, not just
  the arithmetic. `digit_advance` remains correct for its actual purpose:
  sizing something that really does hold only digits (a line-number gutter,
  `RichTextView`'s indent unit).
- **Prose and labels** should measure. Where a widget both measures and draws,
  the two must go through one call so they cannot drift — see
  `RichTextView::span_font`/`span_width` for the shape of that.

Best done a few apps at a time, each with a test that a measured label fits the
box drawn for it. It is mechanical, but it is 90 files, so it is not one commit.

**What the first six conversions turned up.** The estimates were not the whole
problem — they were a marker for four bugs that recur, and are worth looking
for deliberately rather than only fixing the multiplication:

- **Hit-test / render drift.** Where a clickable strip is laid out, the click
  handler and the renderer each computed the item width, and the two had
  already diverged independently of the fudge factor. In `jsonviewer` the click
  test sized a tab from `doc.title` while the render sized it from the title
  *plus* its dirty marker, so clicking a tab after a modified one selected its
  neighbour. The fix is one shared function, not two corrected copies.
- **Weight ignored.** An active tab or heading is drawn bold and was measured
  regular, so the one item the user is looking at is the one that overflows.
  Measure in the weight the text is actually drawn in — and for a strip of
  tabs, measure them *all* bold, or the strip reflows every time the selection
  moves.
- **Byte offset used as a column.** Text buffers store cursor and selection
  positions as byte offsets on character boundaries, which is correct; the
  renderer then multiplied that offset by a cell width. On any line holding an
  accent the caret sat two or three columns right of its character and the
  selection band stretched with it (`markdowneditor`), and match highlights
  slid off the text they mark (`regextester`). Convert with a helper that
  counts characters in the prefix, and make it survive a mid-character or
  past-the-end offset — slicing a `str` off a boundary is an abort, not a
  glitch.
- **Home-grown truncation.** Several apps carry a local `truncate_display`-style
  helper comparing `s.len()` (bytes) against a character budget derived from
  the nominal cell. These cut accented text short when it fitted *and* let wide
  text overflow. Delete them for `text::elide`, which measures the ellipsis too.
  Four found so far: `regextester`, `logviewer`, `snippets`, `diskimager`.

**And what the next four turned up.** Two more classes, both worse than a
mis-sized label:

- **Byte-sliced truncation is a crash, not a glitch.** `diskimager`'s
  `truncate_path` kept the *tail* of a path with `&path[path.len() - keep..]`.
  That index lands mid-character on any path holding a non-ASCII byte, and
  slicing a `str` off a character boundary aborts. Our paths admit every byte
  but `/` and NUL, so it was reachable. Fixing it properly needed a toolkit
  addition — `text::elide_start` and its `fit_end` primitive (`1e5dc7edb`) —
  because eliding a path from the *front* (`/home/user/proje...`) throws away
  the only part the reader wanted; from the start it reads `...deep/disk.img`.
  Reach for `elide_start` for paths, `elide` for anything read left-to-right
  (a hash, a message, a title).
- **Punctuation drawn but not measured.** A field rendered as `[source]` or
  `#tag` was repeatedly sized from the bare `source` / `tag`, so whatever came
  next overlapped the closing bracket, or the last character sat on a pill's
  rounded edge. Found in `logviewer`, `snippets` and `credmanager`. Measure the
  string that is actually drawn — ideally by building it once and using it for
  both.

`credmanager` is the model for the drift fix: `draw_badge` now *returns* the
width it drew, and the three callers that lay something out beside a badge use
that instead of each re-deriving it. Three separate estimates for one badge had
already diverged there (`len*7.0+12` drawn, `len*7.5+16` used for the tag
strip's wrap test, `len*7.0+20` before the audit list's entry name).

**And what the desktop shell turned up.** The shell is the most *localised*
surface in the system — its tab labels, month names, key names, snap-zone names
and application names are all translated or user-supplied — so it is where a
byte-based width was guaranteed to be wrong rather than merely likely to be.
Three things came out of converting it:

- **The padded-box shape belongs in the toolkit.** Buttons, tabs, chips, badges
  and pills are all "text with N px of space on each side", and 30-odd sites had
  each written that out as `label.len() * 8.0 + 16.0` — which is how the byte
  count got in. `text::padded_width(text, padding, size, weight)` names the
  shape (`519ad41e2`). Its sibling `padded_width_any_weight` covers the tab
  strip: the selected tab is drawn bold and the rest regular, and sizing each
  tab to the weight it currently has makes the whole strip shuffle sideways
  every time the selection moves. It takes the wider of the two weights rather
  than assuming bold is the wider one.
- **An estimate used for caret positioning is a correctness bug, not a cosmetic
  one.** `gui/desktop/src/run_dialog.rs` carried an `estimate_text_width` at
  0.55 em per byte and used it to place the caret *and* the selection highlight
  in the Run box. Typing anything non-ASCII put the caret somewhere other than
  where the glyphs were. Grep for estimate helpers used by anything other than a
  box width.
- **A pill holding one glyph needs a floor.** The notification centre's unread
  badge is drawn with a corner radius of half its height; measured honestly, a
  single digit is narrower than the badge is tall, so it rendered as a squashed
  oval. `max(BADGE_HEIGHT)` — the old byte estimate had been hiding this by
  being too wide.

**And what the final sweep turned up.** The tail was mostly mechanical, but it
surfaced three things worth carrying forward:

- **Three carets, not cosmetics.** `sysmonitor` and `procexplorer` both placed
  their process-filter caret from a byte count, and `pdfviewer`'s search
  highlight spread a span's document-supplied width over its *bytes* while
  indexing it by a byte offset — so on any span holding a two-byte character
  the highlight was both too narrow and displaced left by one cell per
  preceding accent. `screenshot` stored a text annotation's bounding box from
  an estimate, so the box did not contain its own text. This confirms the
  earlier finding: an estimate feeding anything other than a box width is a
  correctness bug.
- **A whole app with no tests.** `apps/procexplorer` had no `#[cfg(test)]`
  module at all. Worth a sweep for others: an app with no tests is not
  "untested here", it is untested.
- **`RenderCommand::Text` does not wrap — it truncates.** See the next entry;
  this is the successor defect and the reason this one is not simply closed.
