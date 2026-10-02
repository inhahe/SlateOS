### TD-OILS-HISTEXPAND-LINE-QUOTE-STATE. history expansion resets the quote state at every physical line, so a `!` on a continuation line is judged out of context — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/histexpand.rs` — `expand()` starts every call with
`in_single`/`in_double` false; `userspace/oils/src/interp.rs` —
`expand_history_lines()` (the `crate::histexpand::expand(&raw.text, …)` call).

**What.** bash hands readline the quote delimiter its *reader* is currently
inside (readline's `history_quoting_state`, set from bash's delimiter stack) so a
continuation line is expanded knowing it is inside `'…'` or `"…"`. osh expands
each physical line from a clean slate, which gets the polarity exactly inverted:
it expands what bash leaves alone and leaves alone what bash expands. Measured,
with `set -o history; set -H` and `echo one` as the previous command:

| lines | bash | osh |
|---|---|---|
| `echo 'x` / `!!'` | literal `!!` | expands |
| `echo 'x` / `y' !!` | **expands** (the `'` closed first) | literal |
| `echo 'x` / `!! z'` | literal | expands |
| `echo 'x` / `!zz'` | literal | `!zz': event not found` |
| `echo "x` / `!"` | literal (the `"` joins the no-expand set) | `!": event not found` |
| `echo "x` / `!zz"` | `!zz: event not found` | `!zz": event not found` |
| `echo "a` / `#!zz"` | `!zz: event not found` (no comment inside `"`) | literal |
| `echo 'x` / `^one^two^'` | literal `!!:s^one^two^` | expands |
| `echo $(echo 'x` / `!!')` | literal | expands |
| `` echo `echo 'y `` / `` !!'` `` | **expands** | expands (agrees) |

The last row is the interesting one: a quote opened inside a `` ` `` body does
*not* become the reader's state, because bash scans a backquote body as a flat
matched pair, while `$( … )` recurses into the parser and does push its inner
quotes. Backslash continuations, `$( … )` and compound commands already agree.

**Fix (2026-07-29).** `LexError` now carries the delimiter it was still looking
for (`looking_for`), set in the single `eof_matching` funnel, and `lexer.rs`
exposes `open_quote(src, opts) -> Option<char>` over `tokenize_deferred`.
`expand_history_lines` passes `open_quote(&accum, opts)` into the new
`HistCtx::open_quote`, and `expand()` seeds `in_single`/`in_double` from it;
`history -p` passes `None`, since bash expands each operand with a cleared state.

Two properties of the existing lexer make this fall out rather than needing
special cases. The error propagates outward untouched, so the *innermost*
unclosed construct — the one that failed first — is the one whose delimiter
survives; and `read_backtick` scans its body as one flat span while
`read_balanced` (the `$( … )` scanner) descends into quotes. Filtering the
answer to `'` and `"` therefore reproduces bash's `$(`-vs-backquote split
exactly: `echo $(echo 'x` carries the `'`, `` echo `echo 'y `` carries nothing.

Note that a `'` inside a double-quoted string is not a quote, so `echo "a 'b`
carries `"`. readline's *in-line* scan does toggle on it, which is a separate
rule osh already had — and is why `echo "a 'b` / `c' !!` leaves the `!!`
literal. The in-line rules were not touched.

This is also what makes `Expansion::ChangedQuietly`
(TD-OILS-HISTEXPAND-QUICKSUB above) reachable: the `^one^two^` row is its only
case.

**Tests.** `tests/corpus/histexpand-continuation.sh` (new) covers nine of the
ten measured shapes, plus unit tests
`histexpand::tests::a_carried_quote_state_is_honoured` and
`lexer::tests::open_quote_reports_the_innermost_unclosed_quote`.

**Discovered while measuring:** the tenth row of the table above is wrong. A
backslash continuation does *not* agree — see TD-OILS-HISTEXPAND-BACKSLASH-LINE
below, a separate bug in the reader's *frontier* rather than in the quote state,
which this change neither caused nor fixed and which was fixed straight after.
