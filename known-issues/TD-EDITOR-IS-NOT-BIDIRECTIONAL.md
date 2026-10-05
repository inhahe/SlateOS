## TD-EDITOR-IS-NOT-BIDIRECTIONAL

**Status: OPEN — all four items done. Only the per-line shaped cache (step (b),
a cost concern) and the arrow keys (C-Q2, the operator's call) remain.**

**Update 2026-09-03 — items 1 and 2 are fixed, and `measure_prefix` is gone.**

`caret_offset_px` was `measure(line[..col])` — `width_upto` under another name,
the quantity `TD-FONT-CARETS-ARE-NOT-BIDIRECTIONAL` was renamed away from
precisely because it is not a caret position. It is now `text::caret_x`, which
walks the shaped run. Hit-testing was a loop comparing prefix widths at every
character boundary; it is now `text::cursor_at`, which walks the caret stops in
*screen* order. The selection band was one rectangle between two measured
prefixes; it is now `text::selection_boxes`, a list, because a range contiguous
in the string need not be contiguous on screen — the single span painted the
gap between the pieces, telling the user they had selected text they had not.

`Document` gained `cursor_affinity` beside `cursor_col`, and
`Document::set_cursor` is the one place a hit-test's answer becomes the caret,
so dropping the affinity is something someone has to *do* rather than something
that happens by writing `cursor_col = col` and forgetting the other field. It is
a companion field rather than a `TextCursor` replacing `cursor_col` because
every edit in the file is arithmetic on a byte offset — 99 sites — and none of
them has an opinion about direction; only drawing and hit-testing do.

**The click path was already live, and this entry said otherwise.** The earlier
note that `apps/editor` "has no live input loop" is true of `main()` and false
of `input.rs`, which has `handle_mouse` with press, double-click and drag. All
three went through `caret_position_at`; all three now go through
`caret_cursor_at` and `set_cursor`, so a real click's affinity reaches the caret
it draws. Worth recording as a correction: the belief that nothing called the
hit test nearly led to it being left alone.

**`measure_prefix` is deleted, and `-D dead_code` is what proved the job was
complete.** It had exactly two callers, the caret and the hit test, and when
both were converted the lint reported it unused — a stronger statement than "I
looked and found no others": the prefix-width measurement is gone from the
editor rather than merely unused by the two sites that were examined.

**Seven tests**, in `caret_tests`, on the string `guitk::pathbar` uses — two
Latin letters, two Hebrew, two Latin — the smallest text where a prefix width
and a caret position are different numbers. The bidirectional caret is asserted
to *differ* from the prefix width, because that disagreement is the entire
content of the bug and asserting a specific x would pin the font's metrics
instead. A left-to-right regression test pins the ordinary case, which is every
line of every source file anyone will actually open. A click sweep asserts the
caret is drawn back within one character of where it was clicked, at points
across the whole run including inside the right-to-left stretch — a click that
does not land where the caret appears is the most immediately visible bug an
editor can have. And two affinities at one boundary are asserted to be two
screen positions, which is what makes the new field load-bearing rather than
noise.

Mutation-checked: restoring the prefix measurement fails exactly the three
bidirectional tests and leaves the left-to-right one green.

**What is left.** Step (b), the per-line shaped cache, which is a *cost*
concern and not a correctness one — `caret_x`, `cursor_at` and
`selection_boxes` each shape internally, so they were never waiting on it.
Worth doing when something measures the editor as slow, with a clock rather
than a profiler. And the arrow keys still step logically, which is C-Q2 and
remains the operator's call; this entry was never blocked on it, since where a
caret is *drawn* is wrong under either answer.

The original status line follows.

**Status: OPEN — items 3 and 4 (steps (c) and (d)) done 2026-08-17; 1 and 2
remain.**

**Update 2026-08-17 — item 4 is fixed.** `draw_tokens` no longer emits a
command per token. The toolkit gained a `RenderCommand::RichText`: one string,
one shaping, plus a list of byte-ranged colour spans, and the renderer — which
is the party doing the shaping, and the only party that knows the resolved
font — gives each glyph the colour of the span containing the byte it came
from. See `design-decisions.md` §455 for why the spans cross the wire as byte
ranges rather than as positioned glyphs, and why span ends are cumulative. The
editor now emits exactly one `rich_text_clipped` per visible line;
`a_highlighted_line_is_drawn_as_one_shaped_run` is the regression guard, and
`rich_text_lays_out_exactly_as_plain_text_does` in the compositor asserts the
pixels are unchanged for ordinary text. The kerning across token boundaries
that the old comment accepted losing comes back for free, and the 2.3x cost of
the decomposition goes with it.

**Update 2026-08-17 — item 3 is fixed too.** `Document::scroll_col`, a byte
offset, is now `Document::scroll_px`, a pixel offset. The line is shaped once,
whole, and *translated* left by `scroll_px` under a clip rectangle, instead of
being sliced at a byte and the tail re-shaped. That is the difference between a
window onto the correctly-ordered line and a separate shaping of part of it:
the bidi algorithm resolves visual order from the whole paragraph, so a suffix
can come out ordered differently from the way those same characters sit in the
complete line — scrolling would have rearranged the text rather than slid it.
`a_scroll_changes_only_where_the_line_is_drawn_not_what` is the regression
guard; it asserts the drawn string and every span are *identical* at rest and
scrolled, and only the `x` differs.

Three things fell out of it:

- **The "scrolled into the middle of a character" bug class is gone, not
  guarded.** There is no slice left to land inside a character, so `rebase` lost
  its scroll argument and became `span_end`, and the test that drove
  `scroll_col` through every byte of `"café au lait"` was replaced by one
  asserting a multi-byte line is drawn *whole* at every scroll position.
- **The caret is measured over the whole prefix**, from byte 0, then shifted —
  not measured from the scroll position. It therefore keeps the kerning that
  used to be lost at the scroll boundary, and sits in the same coordinate
  system as the glyphs it is placed among.
- **`max_width` is measured from the shifted x**, since the renderer stops at
  `x + max_width`. Getting that wrong is invisible until someone scrolls and
  then truncates the line early by exactly the scroll distance;
  `the_drawn_run_is_bounded_by_the_viewport` now asserts the stop lands on the
  viewport edge at a non-zero scroll.

Also added `EditorState::ensure_caret_visible_horizontally`, the companion to
the existing `Document::ensure_cursor_visible` and the reason `scroll_px` is
ever non-zero. **Note that neither is called by anything yet** — `apps/editor`
has no live input loop; its `main()` is a demo harness, and the editor is a
model plus a renderer driven by tests. Both are ready for the loop when it
lands. (`ensure_cursor_visible` having sat uncalled since it was written is
worth knowing: vertical auto-scroll is equally unwired, so a caret moved below
the viewport vanishes today for the same reason a horizontal one used to.)

**What that leaves.** Items 1 and 2 — the caret's x and the selection
rectangles are still prefix-*width* sums, and under a right-to-left run the
caret between two characters is not at the summed width of the bytes before
it, nor is a selection one rectangle. Both need the shaped run's cluster
positions rather than a measured width, which is step (e), and step (b)'s
per-line shaped cache is what makes asking for those positions cheap. **(b) is
now the head of the queue**, because (e) wants somewhere to ask for cluster
positions and (b) is what provides it.

**What.** `apps/editor` was left out of `TD-GUI-ARROW-KEYS-MOVE-IN-LOGICAL-ORDER`
above, and not because its arrow keys are fine — they have the same defect. It
is out because motion is the *smallest* of three problems there, and fixing
only motion would move the caret to positions the editor does not draw it at,
which is worse than the present state where at least the two agree with each
other about being wrong.

**This entry is not blocked on C-Q2**, and should not wait for it. C-Q2 asks
only whether the arrow keys should step by the screen or by the string; items 1
and 2 below are wrong under *either* answer, because they concern where the
caret is drawn and what a selection looks like, not which way a key moves. Item
3 is a design question of the editor's own. Whoever picks this up can fix all
three and leave the arrow keys stepping logically, exactly as the toolkit
widgets do today.

**Three problems, which have to be fixed together.**

1. **The caret is placed at the width of the prefix, not at the caret's own
   position.** `render_editor` (around `main.rs:1637`) computes
   `text_x() + text::measure(before_cursor, …)`. That is `width_upto` under
   another name — the quantity `TD-FONT-CARETS-ARE-NOT-BIDIRECTIONAL` renamed
   precisely because it is not a caret position. On any line with a
   right-to-left run the caret is drawn at the distance *into* the text rather
   than the position *across* the line. The fix is `text::caret_x`, which
   needs `cursor_col` to carry an affinity.
2. **Hit-testing and selection have the same shape.** A click resolves to a
   byte column by measuring prefixes, and a selection is painted as a span
   between two measured prefixes — but a logically-contiguous selection that
   crosses a direction change is two or more *disjoint* rectangles.
   `text::cursor_at` and `text::selection_boxes` are the replacements, exactly
   as in `pathbar`.
3. **Horizontal scrolling slices the line at a byte offset** — `scroll_col` is
   a byte index, and the renderer draws `line[scroll_col..]`. This one is not a
   substitution; it is a model problem. *The visible portion of a bidirectional
   line is not the shaping of a substring of it.* Reordering is decided over
   the whole paragraph, so shaping a suffix can yield a different visual order
   than the same characters occupy in the full line. Scrolling has to become
   pixel-based over the whole shaped line, with clipping, rather than
   byte-based over a re-shaped tail.
4. **~~Syntax highlighting draws one run per token~~ — FIXED 2026-08-17, see
   the update at the top of this entry.** Kept below as written, because it is
   the argument for the shape of the fix and (d) rests on the same reasoning.
   **Syntax highlighting draws one run per token, each positioned at the sum of
   the previous runs' widths** — `draw_tokens` (`main.rs:1513`), which walks the
   `StyledToken` list emitting a `tree.text(x, …)` per token and advancing
   `x += text::measure(piece, …)`. Found 2026-08-17 while scoping the fix; it is
   *not* a variant of 1 and it is the hardest of the four. A token is a range of
   *bytes*, and bidi reordering does not respect byte ranges: on
   `let s = "<HEBREW> x";` the string literal's glyphs are drawn interleaved in
   screen order with the punctuation around them, so there is no `x` at which
   the token can be drawn as one left-to-right run. Splitting the line by token
   and laying the pieces out end to end **is** the assumption that screen order
   equals byte order, applied once per token.

   This means colour cannot stay a property of a *substring to draw*; it has to
   become an attribute carried on the shaped glyphs. The line is shaped once as
   a whole, and each glyph takes the colour of the token containing the byte it
   came from (`ShapedRun` already keeps each glyph's source byte offset, which
   is what makes this possible). The comment above `draw_tokens` explicitly
   accepts losing kerning across token boundaries as a small cost; under bidi
   the same decomposition stops being a small cost and becomes wrong output.

**Why 3 and 4 make this its own task.** Between them they are a real design
change with a genuine tradeoff — shape-whole-line-and-clip costs work
proportional to line length on every frame, where the present code shapes only
what is visible — and the editor's long-line performance is the reason the
byte-slicing exists. **That measurement has since been taken** — see
`C-FONT-SHAPING-IS-1400X-SLOWER-THAN-IT-SHOULD-BE` below, "Note on the editor
question it was gathered for" — and it says shaping a 1,000-character line
whole costs 0.75 ms against 0.15 ms for the visible window, so the per-frame
cost is real and **the shaped line must be cached per document revision**. The
tradeoff is therefore settled: shape whole, cache, clip. Note that 4 pushes
toward shape-whole-line anyway: once colour is a
per-glyph attribute, the shaping the renderer needs is of the whole line, so the
per-frame cost 3 worries about is incurred either way and the cache stops being
optional.

**Item 4 is not a cost at all — it is a saving.** The same instrument measured
what `draw_tokens` does today against shaping the line whole, and the
decomposition *loses*: a 40-token line of 80 characters costs **2.3x** what
shaping it whole costs, an 80-character line of 10 tokens 1.5x, a
1,000-character line of 40 tokens 1.26x. Each shaping carries a fixed cost of
~3.6 us on top of ~0.75 us per character, and cutting a line into *n* pieces
pays that fixed cost *n* times. So converting `draw_tokens` to colour glyphs
rather than slice strings makes the output correct under bidi **and** makes the
common case faster; there is no tradeoff to weigh on that one.

**Where.** `apps/editor/src/main.rs`: `Document::move_left`/`move_right`
(~822), `cursor_col`/`scroll_col` (~54, ~67), the caret and text drawing in
`render_editor` (~1620–1640), `draw_tokens` (~1513), and the click handler.

**Suggested order for whoever takes this**, since the four are entangled and
doing them in the wrong order means writing code twice. Shape-the-whole-line is
the foundation and 3 and 4 both sit on it, so: (a) ~~measure the shaping cost on
a pathological line and decide the caching question~~ — **done 2026-08-17; the
answer is "cache it", and the numbers are in the shaping entry below**;
~~(c) convert `draw_tokens` to colour glyphs rather than slice strings (4)~~ —
**done 2026-08-17, see the update at the top**;
~~(d) convert scrolling to pixels over the shaped run with clipping (3)~~ —
**done 2026-08-17, see the second update at the top**; **(b) is now the head of
the queue:** build the per-line shaped
cache keyed on document revision; (e) 1 and 2 then fall out as
one-line substitutions to `text::caret_x` / `cursor_at` / `selection_boxes`,
because by then the shaped line they need is already in hand. Motion (the arrow
keys) is deliberately *last* and is the least of it — and note it is **not**
gated on C-Q2 either way, since the editor is out of that question's scope.

**(b) moved after (c) and (d)** on 2026-08-17, once the piece-vs-whole
measurement came in. Two reasons, and either alone would be enough. First, (c)
does not need it: `draw_tokens` already shapes each line once *per token*, so
shaping it once whole is strictly less work than what runs today — the cache is
an improvement on the new code, not a prerequisite for it. Second, nothing yet
pays a per-frame cost to save: `apps/editor`'s `main()` is a demo harness that
renders once and exits, and `Document` has no revision counter to key a cache
on. Building the cache first would mean inventing the key, the invalidation and
the eviction policy against a load that does not exist, and then very likely
rebuilding them when it does. Do the correctness work, then add the cache when
there is a frame loop to justify its shape.

**Severity.** Low in practice today — a code editor's content is
overwhelmingly one-directional, and on such text every one of these is
correct. It bites a user editing a document with a quoted Arabic or Hebrew
phrase in it, which markdown and comment text make entirely plausible.
