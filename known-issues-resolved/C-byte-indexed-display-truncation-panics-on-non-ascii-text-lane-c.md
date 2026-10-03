## Byte-indexed display truncation panics on non-ASCII text (lane C)

**Status: FIXED 2026-08-15** (lane C, commits `f508f76cf`, `f53562a09`,
`feb695bbd`, `8208fad9d`, `83dfaff21`, `5750232c5`, `a8d659199`, `ffbdec410`,
`54fd94f2b`, `5305d139f`, `b3373ad17`, `db06a8c3c`, `de378bab6`, `37ee779ae`,
`10db32f9c`). Found while surveying app tables for unbounded columns. Eighteen
sites across `apps/` and `gui/` confused a byte count with a character count, usually
while truncating a *display* string:

```rust
let display = if title.len() > 20 {
    format!("{}...", &title[..17])   // panics if byte 17 is inside a character
} else {
    title
};
```

`str::len` is bytes and `&s[..17]` is a byte index, so any string whose 17th
byte falls inside a multi-byte character panics with
`byte index 17 is not a char boundary`. The guard makes it *more* likely, not
less: a 20-character Japanese title is 60 bytes, so it takes the truncating
branch and then slices mid-character. This is not an edge case for these
particular apps — it is their ordinary input.

| Site | String | Exposure |
|---|---|---|
| `apps/rssreader/src/main.rs:3256,3260` | `article.summary` / `display_content()` | **Remote.** Straight off an RSS feed; any non-English feed crashes the reader. |
| `apps/pdfviewer/src/main.rs:1452` | the PDF's own `/Title` | Attacker-supplied file metadata. |
| `gui/desktop/src/file_drop.rs:65` | dropped text | And our paths are byte strings by design. |
| `apps/flashcards/src/main.rs:1313,1370` | card front/back | A flashcard app is *the* place for CJK and accented text. |
| `apps/stickynotes/src/main.rs:973` | the note's first line | The user's own text. |
| `apps/procexplorer/src/main.rs:2359` | `KEY=value` from the environment | Environment strings are arbitrary bytes. |
| `gui/toolkit/src/colorpicker.rs:175` | `&s[..6]` on a typed hex string | Any multi-byte character in the field. |
| `gui/desktop/src/clipboard_viewer.rs:112` | `content[..197]` on a clipping | Copying any non-Latin text aborted the shell. |
| `gui/desktop/src/clipboard_viewer.rs:678` | `&preview_text[..40]` on the same | Same, one layer up. |
| `apps/videoplayer/src/main.rs:538` | `padded[..3]` in the SRT timestamp parser | **A subtitle file the user merely opened.** |
| `apps/renamer/src/main.rs:450,460,489,509` | the filename stem, cut at a position the user types | **Any non-ASCII filename**, and it aborts a batch rename *partway through*. |
| `apps/markdowneditor/src/main.rs` (14 sites) | `cursor_col`, the selection anchor, undo columns | **Press Down onto a line with a wide character, then type.** Aborts with the document unsaved. |
| `apps/backup/src/main.rs:302` | the `?` glob wildcard, over path bytes | **Not a panic** — an include/exclude pattern silently stops matching, so a file the user believed was covered is not backed up. |
| `apps/filesearch/src/main.rs` (both matchers) | every single-character construct in the glob *and* regex engines | **Not a panic** — a search over non-ASCII filenames silently returns wrong results, in both directions. |
| `apps/dbviewer/src/main.rs:895` | SQL `LIKE`'s `_` wildcard | `LIKE '_'` was false for a one-character CJK cell and `LIKE '___'` was true for it. |
| `apps/indexer/src/main.rs:709` | the `?` wildcard and `[...]` classes of a third glob matcher | Same as filesearch's, in the file indexer. |
| `apps/indexer/src/main.rs:826` | `levenshtein_bounded`, the fuzzy-match edit distance | One substituted kanji cost 3 of a budget the user reads as "a couple of typos", so near-exact CJK matches were rejected. |
| `apps/jsonviewer/src/main.rs:304` | the parser's `col`, shown as "Ln 3, Col 17" | Not a panic and not a wrong result — a wrong *report*. The caret pointed up to two columns per preceding character too far right. |

The last ten were found while fixing the first seven and were not in the
original count. `gui/clipboard/src/main.rs:183` looked like another but is not:
it already goes through `find_char_boundary`.

The videoplayer one is worth calling out because it does not match the grep
shape above — there is no `if x.len() > N` guard in sight. It is
`format!("{ms_str:0<3}")` followed by `padded[..3]`, and the bug is that
`format!`'s width is counted in **characters** while the slice indexes
**bytes**. For a fractional part of `"ab日"` the padding adds nothing (already
3 characters) and byte 3 lands inside the kanji. So the class is wider than
"a byte budget with a byte guard": it is *any* place where a character count
and a byte count are used interchangeably. Rust's own `format!` width is a
character count, which makes it a natural source of the confusion.

**`apps/renamer` is the one site where the byte/character confusion was also a
*semantic* bug, and the most damaging of the seventeen.** Four rename rules —
insert-at, remove-from, number-at, datestamp-at — slice the filename stem at a
position the *user types into the rule*, clamped only with `.min(stem.len())`,
a byte length. `InsertPosition::At`'s own doc comment has always read "insert at
a specific character index", so the code contradicted its documented intent: for
`日本語.txt`, "insert at 3" is past the end of a 3-character stem and should
append, but the byte clamp put it after the *first* kanji. And unlike a
truncated label, a wrong position here writes the wrong name to disk. The panic
is worse still, because a rename batch applies each rule to each file in turn:
one non-ASCII name aborted the renamer *after* earlier files had already been
renamed, leaving the batch half-applied with no undo record. Fixed with a
`char_offset(s, chars)` helper that all four sites route through, which makes
the position mean what it says and makes the slices sound as a side effect. For
ASCII names the two numbers coincide, so no existing rule changed behaviour —
the pre-existing tests confirm it.

**`apps/markdowneditor` is the largest instance, and the only one where the
bad offset *persists in state* rather than being recomputed each frame.** Every
column in the editor -- `cursor_col`, the selection anchor, the columns recorded
in undo actions -- is a byte offset into a line, which is what lets an edit
apply without re-scanning. Fourteen places kept such an offset in range with
`.min(line.len())`, and a byte length is the wrong bound: it keeps the offset
inside the line but says nothing about whether it lands *on* a character.

Pressing Down is enough to reach it. `move_cursor_down` carries the column to
the next line, so from column 1 of `"abc"` onto `"\u{65e5}x"` the clamp leaves 1,
inside the kanji. Nothing fails yet -- the cursor is simply in an impossible
place. The abort comes on the *next* keystroke, in whichever of Backspace,
Delete, insert, Enter, arrow-key or selection the user happens to press, by
which point the document is unsaved and the user has been typing. Go-to-line, an
undo replayed against a line that changed underneath it, and a reload after the
file changed on disk all reach the same state without any cursor movement at
all.

Fixed with one `clamp_col(line, byte)` that rounds *down* to a character
boundary, used at all fourteen sites. Rounding down puts the cursor at the start
of the character it landed in -- where a user who pressed Down onto a wide
character expects to be -- and for an all-ASCII document it returns exactly what
`.min(line.len())` did, which a test asserts directly.

**`apps/backup`'s `?` wildcard is the only member of the class that never
panics, which is exactly what made it the easiest to miss.** The glob matcher
works on `&[u8]` throughout, which is *correct* — our paths are byte strings
that need not be UTF-8 (`CLAUDE.md` item 7), and rewriting it over `&str` would
have been the wrong fix. But `?` is documented as "any single character except
`/`" and advanced `ti` by one **byte**, so against `日本.txt` it matched one
third of a kanji. `file?.txt` silently stopped matching `file日.txt`. In a
backup tool a pattern that quietly fails to match is worse than a crash: an
exclude that misses copies a directory the user meant to skip, and an include
that misses leaves a file unprotected with the run still reporting success.

Fixed with `utf8_char_len(text, i)`, so `?` advances one character. Only `?`
needed it. `*` is byte-greedy but can only ever *succeed* on a boundary — a
well-formed needle cannot match starting inside another sequence, by UTF-8
self-synchronization — and `/` is ASCII, so it can never occur inside a
multi-byte character.

The interesting part was ill-formed input. The first version clamped a
truncated sequence to the bytes remaining (`want.min(len - i).max(1)`), which
my own test caught as a real defect rather than a wrong expectation: for the
bytes `[0xE6, b'/']` a lead byte announcing three bytes consumes both, and `?`
has crossed a separator — the one thing it must never do. The rule that works
is **validate, then consume**: only treat a lead byte as multi-byte if the
continuation bytes it announces are actually present and in `0x80..=0xBF`,
otherwise advance one byte and let the literal comparison decide. That keeps
the separator invariant and still guarantees forward progress.

**`apps/filesearch` is the same bug as `backup`'s `?`, but as a whole engine
rather than one branch — and it was found by asking "where else does a matcher
step a byte at a time?" rather than by any grep.** filesearch has two engines,
a glob matcher and a small regex matcher, and both stepped `ti` by one byte.
That made *every* single-character construct wrong: `?` and `.`, the character
classes `[...]`, and `\d`/`\w`/`\s` with their negations. It is wrong in both
directions at once, which is what makes it hard to notice from one example:

- **False negatives.** `?.txt` did not match `\u{65e5}.txt`; `h.llo` needed
  three dots for one kanji.
- **False positives.** `\W\W\W` matched exactly one kanji, because every byte
  of a multi-byte character fails `is_ascii_alphanumeric`. `[\u{e9}]*` matched
  `\u{e8}b`, because `\u{e9}` and `\u{e8}` share a lead byte and the class
  compared one byte. A class *range* like `[\u{430}-\u{44f}]` was not merely
  wrong but meaningless — it compared bytes of the endpoints' encodings.

Unlike `backup`, both entry points here take `&str`, so the inputs are already
validated UTF-8 and character semantics is achievable, not just desirable.
Both engines were converted to `&[char]`. That is the whole fix: with `&[char]`
every index is a character index by construction, so `?`/`.`/classes/ranges are
all correct at once and there is no per-construct rule to remember or to get
wrong again later. The public `&str` entry points are unchanged; the two
bulk-search paths gained `*_chars` variants so the pattern is decoded once per
search instead of once per indexed file.

The regression test that earned the most was the *control*:
`an_ascii_pattern_matches_exactly_as_before` pins 20 pre-existing ASCII cases.
Under the deliberate re-break it kept passing while all six non-ASCII tests
failed — which is exactly the evidence wanted, since it shows the six really do
discriminate and that the refactor changed nothing for ASCII input.

Re-breaking this one is worth recording as a technique: rather than reverting
the refactor, the byte engine was reproduced by decoding `.bytes().map(|b| b as
char)` instead of `.chars()`. Every comparison in both engines is by scalar
value, so mapping each byte to the char of the same value restores the old
behaviour exactly, at 8 call sites and with no other edit.

**Asking the behavioural question then found three more sites in two more apps,
which is the strongest evidence that the question is the right tool.** Having
noticed that no grep finds a byte-at-a-time advance, the lane's remaining
matchers, parsers and scanners were read with one question in mind — *does this
walk text one unit at a time, and is that unit a byte?* Three said yes:

- **`dbviewer`'s SQL `LIKE`.** Its own comment reads "`_` matches exactly one
  character"; it consumed one byte. `LIKE '_'` was false for a one-character
  CJK cell while `LIKE '___'` was true for it.
- **`indexer`'s glob matcher** — a third independent copy of the same `?`-and-
  class bug, after `backup` and `filesearch`.
- **`indexer`'s `levenshtein_bounded`.** The most interesting of the three,
  because it is not a wildcard at all: an *edit distance* over bytes charges up
  to 3 for one substituted kanji. Against a `FUZZY_MAX_DISTANCE` the user reads
  as "a couple of typos", a near-exact CJK match was rejected while a much
  worse ASCII one was accepted — and the `abs_diff` length early-out discarded
  candidates before the DP even ran. Fuzzy matching was effectively off for
  non-ASCII names.

That three independent glob matchers in one lane each carried the same defect
is worth noting on its own: this is not a slip someone made once, it is what
you get by default from reaching for `as_bytes()` to walk a pattern. The
generalisation is not "`?` is special" but that **any construct meaning "one
unit of text" is wrong the moment the loop's unit is a byte** — wildcards,
classes, ranges, quantifier counts and edit costs alike.

A second vacuity trap turned up here, of a kind not seen before: **a test can
fail to discriminate because the behaviour that survives the break is genuinely
correct.** `dbviewer`'s first percent-and-literals test passed under the
deliberate break, not through oversight but because `%` and literal matching
really are sound over bytes — the same self-synchronization argument that
cleared `backup`'s `*`. Only a pattern that makes `%` absorb the slack while
`_` must still count (`"日"` against `"%_%_%"`) can tell the two engines apart.
Generalised: when part of a construct is provably safe, a test built from that
part cannot witness the unsafe part, however non-ASCII its input looks.

**The fix was not to hunt for char boundaries at each site.** All but one of
these is a *display* truncation, and each already had a box to draw into, so
each became `guitk::text::elide` / `RenderTree::text_in` (or a `guitk::table`
cell): it measures display width, cuts on a character boundary, and marks the
cut with `…`. That also removed the second, quieter bug present at every site —
a truncation counted in bytes has no relationship to the width of the box the
text is drawn in, so `20` characters of a wide font overflow anyway while `20`
of a narrow one waste half the space.

Two sites needed something other than eliding:

- **`colorpicker::parse_hex_color` is a parser, not a view.** It branched on
  `s.len()` as if it were a digit count. Requiring ASCII hex digits up front
  makes the length a digit count and every offset a character boundary, so the
  rest of the function is sound by construction. (It also closed a smaller
  hole: `u32::from_str_radix` accepts a sign, so `"+FFFFF"` parsed as a colour.)
- **`ClipEntry::text`'s cap is a *retention* bound, not a display one** — a
  clipping can be megabytes and the history holds many. That bound stayed in
  the model but became a character count; the display bound moved to the view.

Three sites had truncation in the *model*, where nothing knows how wide the
drawing surface is: `DragDataType::description`, `NoteStore::sidebar_items`, and
the clipboard row. All three now return full text and the caller elides.

Writing the regression tests turned up four latent layout bugs the byte budgets
had been hiding, all fixed in the same commits: pdfviewer's tab title drew 2px
under its close glyph; flashcards' three columns overlapped below 640px;
procexplorer's memory row sat at a flat 200px pitch and left the panel at 480px
wide; and the clipboard row's meta line could run under the sensitive
indicator.

Grep shape, if this recurs: `&<ident>[..<literal>]` where the receiver is a
`String`/`&str`, and its `if x.len() > N` guard. That shape found seven of the
seventeen; the other ten needed a wider sweep for *any* mixing of the two counts.
Three further forms showed up, none of which the grep can see: `format!` width
(a *character* count) meeting a byte slice (videoplayer); `.min(s.len())` used
to clamp a position the user thinks of in characters (renamer,
markdowneditor); and a byte-at-a-time advance where a character was meant
(backup's `?`, both of filesearch's engines), which involves no slicing and no
`len()` at all.

That last form is the one to go looking for next, because no textual pattern
finds it — it is `ti += 1` in a loop, which matches everything. The question
that finds it is behavioural: **"does this walk text one unit at a time, and is
that unit a byte?"** Both remaining instances were found by asking it of every
matcher/parser/scanner in the lane rather than by grepping. Note this is the same root
cause as the unbounded-column survey below — **counting characters instead of
measuring the box** — and it was worth treating as one problem.

Every fix is covered by a test using Japanese/Greek/Russian/emoji input plus a
string pinning the exact cut index to a continuation byte, and every one was
verified non-vacuous by re-breaking the production code and confirming the test
fails. That discipline earned its keep five times here:

- `colorpicker`'s `chars[2]` index was in fact *unreachable* -- `hex_char_to_u8`
  rejects a multi-byte char one step earlier -- so the "second panic" claimed
  for that site did not exist.
- An earlier `file_drop` test passed with its bound removed, because no
  reachable payload draws both a count badge and a long description.
- `markdowneditor`'s first sweep drove each edit through `move_cursor_down`, so
  breaking any *edit* site changed nothing: the sweep already aborted on the
  cursor-position assertion from the vertical move, one case earlier. Five
  sites looked verified and were not. Replaced with a test that strands the
  column directly, which both isolates each site and matches reality, since
  undo replay and click-positioning strand it without any vertical move.
- `markdowneditor`'s reload clamp passed with the clamp removed -- no test
  reached it -- until a test was added for it specifically.
- `backup`'s "`?` never crosses a separator" test passed under the very break
  it existed to catch: `?c` against `[0xE6, b'/', b'c']` fails for an unrelated
  reason, so the assertion never distinguished the two versions. Pinned with
  `assert!(!glob_match_recursive(b"?", &[0xE6, b'/']))`, which does. A test
  aimed at an invariant is not the same as a test that can *see* the invariant
  break.

General rule this keeps re-teaching: **when several defects can abort, break
them one at a time**, and be suspicious of a break that leaves the failure
count unchanged -- it usually means the new failure is the old one.

One further trap, from this same session: do not re-break production code while
a full-workspace test run is in flight. A workspace gate launched earlier picked
up `renamer` mid-verification and reported two failures that were the
scaffolding, not the tree.

**Site eighteen shows the class reaches things that neither panic nor compute a
wrong answer.** `apps/jsonviewer`'s parser counted `col` once per byte. Nothing
downstream indexes with it — it is used only to *tell the user where the error
is*, in the status bar and the error list. So the parse was right, the error was
right, and the caret pointed at the wrong character: a document whose string
value is `日本語` rather than `xxx` reported column 20 where the ASCII one
reported 14. That makes it the least dangerous instance and the easiest to
overlook, because there is no crash and no bad data to notice — just a number
that quietly stops meaning what its label says. The fix is one line: skip the
increment for continuation bytes (`b & 0xC0 == 0x80`), which are the tail of a
character its leading byte already counted.

**A caution about how these are found.** The same grep that turned up kanban's
real corruption (next section) also flagged `apps/jsonviewer`'s `parse_string`,
which does `result.push(b as char)` on the very next line — and *that* one is
correct, because it sits under `if b < 0x80` and the non-ASCII branch rewinds
into a real UTF-8 decoder with proper surrogate handling. Two functions, the
same six-token expression, opposite verdicts. No pattern distinguishes them;
only reading the enclosing guard does. Treat a grep hit in this class as a
question, never as a finding.

Violates `CLAUDE.md` self-review item 7 (never force UTF-8 assumptions on
OS-boundary data) and trips the workspace's `clippy::indexing_slicing` warn.
