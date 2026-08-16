# Known Issues — Resolved Archive

Entries from `known-issues.md` that are **fixed and verified**, moved here so
that file stays a list of what is *still* wrong. Nothing is deleted: an entry
keeps its full text, its `**Status: FIXED**` stamp, and its commit hashes, so
the reasoning behind a fix is still greppable from one place.

**When to move an entry here:** it is fixed, and the fix has been on `main`
through a full boot test. Until then it stays in `known-issues.md` with a
`**Status: FIXED**` stamp — a fix that has not survived a boot is a claim, not
a resolution.

**Who may move one:** the lane that owns it, into that lane's section below.
This file is lane-partitioned like the others (`roadmap.md` rule 3,
`design-decisions.md` §437), so three lanes archiving at once land at three
different offsets and the merge is automatic.

The migration is **incremental**, not a one-shot sweep. As of 2026-08-16
`known-issues.md` held 999 `###` entries plus 77 `##` ones, of which roughly
777 read as resolved — about 55,000 of its 73,000 lines. Lane C's are below;
lanes A and B have been asked to move theirs (`requests/c-a-…`,
`requests/c-b-…`). Until they do, resolved entries still live in both files,
so **grep both**.

---

# Lane A

*(none moved yet — see `requests/c-a-known-issues-archive.md`)*

---

# Lane B

*(none moved yet — see `requests/c-b-known-issues-archive.md`)*

---

# Lane C


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


## `u8 as char` reinterpreted UTF-8 as Latin-1 in four parsers (lane C)

**Status: FIXED 2026-08-15** (lane C, commits `237636350` kanban, `3b6b60e39`
backup, `18f1e9abc` rssreader). Found while sweeping for byte-at-a-time text
walkers, and it is a *different* class from the byte/character-count confusion
above — worth keeping separate, because the symptom, the detection method and
the fix all differ. Three JSON readers and one XML reader carried it.

`apps/kanban`'s `JsonImporter::parse_string` built its result one byte at a
time:

```rust
} else {
    result.push(b as char);   // b: u8
}
```

`b as char` maps a **byte value** to the Unicode scalar with that value. That is
a Latin-1 decode. There is no count involved, nothing is truncated, and nothing
panics: an imported card titled `日本語` (E6 97 A5 ...) simply comes back as
`æ\u{97}¥...`. It is `String::from_utf8_lossy`'s failure mode reached by a
different route, and it is worse than a panic in one specific way — **the damage
persists.** The mojibake becomes the card's title in memory, and the very next
save writes it to disk as the new truth. Import a board, glance away, and the
original text is gone.

Why the count-confusion sweep would not have found it: there is no `len()`, no
slice, no guard, no wildcard. The tell is the cast itself. The generalisation
worth carrying forward is that **`u8 as char` is almost always a bug on text**;
it is sound only where the byte is already known to be ASCII — which is exactly
the distinction that made `apps/jsonviewer`'s identical-looking line correct.

The fix copies unescaped runs out as whole `&str` slices instead. That is sound
precisely because the two bytes that terminate a run — `"` and `\` — are ASCII,
and an ASCII byte can never occur inside a multi-byte UTF-8 sequence, so the cut
is always on a character boundary. (This is the same self-synchronisation
property that cleared so many near-misses in the sweep above; here it is what
makes the fix work rather than what made the bug absent.) It is also faster than
pushing char by char.

Fixing the function properly turned up two further defects in it:

- **`\uXXXX` was never decoded.** It fell through to the unknown-escape arm and
  came back as a literal backslash, `u`, and four digits. This was not
  hypothetical: our own `JsonExporter::escape_json` emits exactly that form for
  every character below U+0020, so **export followed by import did not
  round-trip** for any card whose text contained a control character. Now
  decoded, including leading/trailing surrogate pairs — which is how JSON spells
  anything outside the BMP, so emoji in a board exported by any other tool were
  equally unreadable. An unpaired surrogate degrades to U+FFFD rather than
  failing the whole import.
- **The unknown-escape arm had the same cast** (`result.push(esc as char)`) on a
  single byte, so a backslash followed by a multi-byte character both corrupted
  that character and left the scan stranded mid-sequence. It now consumes a
  whole character.

Non-vacuity was checked by reinstating the byte-at-a-time parser: all four new
tests fail under it while the ASCII control test and both pre-existing parser
tests keep passing — the profile that shows the new tests discriminate *and*
that the rewrite changed nothing for ASCII.

Violates `CLAUDE.md` self-review item 7 in its strongest form: this is not an
assumption about encoding, it is an actual re-encoding.

### The same bug in `apps/backup`'s manifest reader — the worst instance

`apps/backup`'s `parse_string` had the identical cast, but on data that makes it
far more consequential: **the strings in a backup manifest are file paths.** A
backup of `写真/2024.jpg` reads back as a path naming no file at all, so restore
cannot find it and verify reports it missing. The manifest is the only record of
what was backed up; corrupting it silently invalidates the archive.

Fixing it turned up two further defects in the same function, both worse than
the first:

- **A reachable panic.** The `\u` arm did `&input[i + 1..i + 5]` with no bounds
  or boundary check. On `"\u日本"` that cuts at byte 7, inside `本`, and Rust
  panics on the non-boundary slice. Reachable from merely *reading a manifest
  off disk* — no attacker needed, just a path that happens to follow a
  backslash-u with non-ASCII text. `parse_hex4` now uses
  `input.get(start..end).ok_or("incomplete unicode escape")?`.
- **Silent data loss on astral characters.** The `\u` decode used a bare `u16`
  with no surrogate pairing and `if let Some(c)` with *no `else`* — so an
  escaped emoji or CJK-extension character in a path did not become U+FFFD, it
  simply **vanished**, shortening the path to something else entirely. Now
  paired properly, with U+FFFD for an unpaired surrogate.

Three defects, so non-vacuity was checked with three separate breaks, each
confirmed to fail only its own tests while the ASCII control kept passing. The
last test builds a real `Manifest` of non-ASCII `FileEntry` paths and
round-trips it through `serialize`/`deserialize`. 46 tests pass.

### The same bug in `apps/rssreader` — the only remotely-fed instance

`XmlParser::read_attribute_value` accumulated with `value.push(b as char)` on
bytes straight off a downloaded feed, so any non-ASCII enclosure URL, title or
author arrived as mojibake. What makes this one instructive is that it was the
**outlier in its own file**: `read_until`, `read_name` and the text-node reader
all already sliced the range out whole and used `from_utf8_lossy` — which is
exact here, since `parse_xml` takes a `&str` and every delimiter is ASCII. The
correct pattern was sitting three functions away. Fixed to slice the same way.

Fixing it exposed an unrelated robustness defect in the same path, arguably more
damaging in practice than the mojibake: `decode_entity` returned `Err` for
anything outside XML's five entities, and `read_attribute_value` propagated it
with `?`. So **one `&nbsp;` in any attribute failed the parse and threw away the
entire feed** — as did a bare `&` in a query string, which is ubiquitous in
enclosure URLs. The same entity in a *text node* merely rendered literally,
because that caller fell back to the raw string; only the attribute path was
fatal. `decode_entities` is now infallible: unrecognised entities are emitted
exactly as written, bare `&` passes through, and twenty-six common HTML entities
are decoded rather than left as source text. Two breaks, two disjoint failure
sets. 147 tests pass.

The pattern across all four: **the cast is never the only bug in the function.**
Every site that had it also had at least one other defect in the same escape or
delimiter handling — a panic, a dropped character, or a fatal error on ordinary
input. Byte-at-a-time text handling seems to correlate with not having thought
about the hard cases at all.


## The file explorer's paste and delete were a weaker duplicate of its own engine (lane C)

**Status: FIXED 2026-08-15** (lane C, commit `bcd1e2d5d`). Found while auditing
`to_string_lossy` uses in `apps/explorer` — those turned out to be fine (the
real `PathBuf` is always kept as the truth and the lossy `String` is only ever
displayed), but the surrounding code was not.

`apps/explorer/src/fileops.rs` is a complete file-operation engine: plans with a
conflict policy, a crash-recovery journal, per-file error policy, progress, an
undo stack, and a `RecycleBin` that stores each item with its original path so
it can be listed and restored. `apps/explorer/src/main.rs` used **none of it.**
`paste()` and `delete_selected()` called `fs::copy` / `fs::rename` /
`fs::remove_file` in a loop, discarding every `Result` with `let _ =`. Three
distinct silent failures followed:

- **Paste destroyed an existing file of the same name.** `fs::copy` overwrites
  its destination, so pasting `notes.txt` into a folder that already had one
  replaced it with no prompt, no rename, no undo.
- **"Move to recycle bin" produced files that could not be restored — and
  destroyed each other.** It renamed into a flat `/var/recycle` with no
  metadata, so `RecycleBin::list` never saw the item and `restore` had no
  original path to restore *to*. And because `fs::rename` overwrites, deleting a
  second `notes.txt` from a different directory silently destroyed the first
  one already sitting in the bin. The recycle bin was, in effect, a shredder
  with an unreliable name-collision hazard.
- **The status line reported unconditional success.** "Paste complete" whether
  or not any file copied; "N item(s) deleted" where N was the number
  *selected*, not the number that worked.

The fix routes both through the existing engine rather than patching the
duplicate — `copy_dir_recursive` is deleted, not repaired. Two implementations
of one operation is precisely how the weaker one ends up on the user-facing
path; `CLAUDE.md`'s "watch for band-aid accumulation" rule names this shape.

**A fourth defect surfaced only when the tests were written.** Every operation
ends by calling `load_directory()`, which calls `update_status()`, which
overwrote `status_message` with the folder/file summary. So no operation result
was *ever* visible to the user — not a paste, not a delete, not a rename, and
not the `Error: {e}` set when `read_dir` fails, which was assigned and then
discarded two lines later (an unreadable directory rendered as "0 folder(s), 0
file(s) — 0 B"). The transient result and the derived summary are now separate
fields, with `status_bar_text()` preferring the result and navigation clearing
it.

**The root cause behind all four: `apps/explorer/src/main.rs` had no
`#[cfg(test)]` module at all.** `columns.rs`, `dropzone.rs`, `fileops.rs` and
`thumbs.rs` are all well tested; the file holding `ExplorerState` — navigation,
selection, clipboard, paste, delete, rename — had zero tests. It now has 11,
with each of the three behavioural fixes verified non-vacuous by a separate
break. Worth generalising: **a well-tested support module is not evidence that
the code calling it is tested**, and in this crate the untested file was the one
users actually touch.

Two smaller defects in `fileops.rs` noticed during the same read — **also FIXED
2026-08-15**, commit `35f17dfd7`:

- `RecycleBin::recycle` moved data with a bare `fs::rename`, which fails with
  `EXDEV` across a mount point — so deleting anything from a separate data
  partition simply errored, and `restore` had the same problem in reverse. Now
  routed through a `move_path` helper that tries the rename and falls back to
  copy-then-remove. Note that `fileops::same_device` exists for exactly this
  check and was referenced only by its own test (`dropzone.rs` carries a second
  copy of the same function); it was not used here, because attempting the
  rename and reacting to its failure is both cheaper in the common case and
  correct where a first-component heuristic guesses wrong.
- `RecycleBin::recycle` wrote the original path to `meta.txt` with
  `path.display()`, which is lossy, and `read_entry` parsed it back as UTF-8 —
  so a non-UTF-8 path was restored under a *different name*, silently renaming
  the user's data during an operation advertised as reversible. Same class as
  the `u8 as char` section above, reached through `Display` instead. The path is
  now percent-encoded from `OsStr::as_encoded_bytes`, with a version marker on
  line 1 so an already-populated bin is still readable.

A third, unlogged defect fell out of writing those tests: metadata was written
before the data was moved but not removed if the move failed, so a failed
recycle left the bin listing an entry whose `data/` was not there. Ordering
kept (metadata first is the safe order — orphaned metadata is harmless, moved
data with no metadata is unrestorable), with cleanup on the failure path.


## Fixing a parser is not fixing a format: `apps/backup` corrupted paths on the *write* side (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. This is a follow-up to the
`u8 as char` section above, and the lesson is the one worth keeping.

Commit `3b6b60e39` fixed `apps/backup`'s manifest **reader** — the JSON string
parser that re-encoded UTF-8 as Latin-1. That looked like the whole bug. It was
not. `FileEntry.path` was a `String`, and every one of those strings was
produced by `relative_path`, which did
`full.to_string_lossy().replace('\\', "/")`. So a filename the filesystem
happily stored — our paths allow every byte but `/` and NUL — was flattened to
U+FFFD *before the manifest writer ever saw it*. A backup of `café.txt`
(0xE9, not UTF-8) recorded `caf<FFFD>.txt`; restore recreated the file under a
different name, and `verify` reported the original as missing. The archive was
self-consistently wrong, so nothing downstream could detect it.

**Generalization: when you find a lossy conversion in a parser, the format has
two sides — go find the writer.** A round-trip test through the parser alone
passes vacuously, because the corruption happened upstream of the data the test
constructs by hand. The reader fix and its tests were both real and both blind
to this.

Fixed by making the path a `PathBuf` end to end: `relative_path` now strips the
base and rejoins components on `/` at the byte level, and the manifest stores
paths percent-encoded from `OsStr::as_encoded_bytes` — the same escape and
version-marker scheme just adopted for the recycle bin's `meta.txt`. A manifest
with no `version` field is read as version 1 (paths verbatim), so archives taken
before this change still restore.

Three further defects fell out of the work:

- `detect_changes` contained a dead push/pop pair and two empty `if` bodies
  left over from a half-finished edit; it computed `modified` twice, and the
  first computation was discarded. Rewritten as a single pass. Behaviour is
  unchanged — the hash comparison was always the one that counted — but the
  size/mtime "quick check" it pretended to do was never wired to anything.
- The file-type breakdown in `stats` used `path.rsplit('.').next()`, which
  reports the whole filename as the extension for `README` and `.gitignore`
  alike. Now `Path::extension`.
- Both new percent-decoders (here and in `fileops.rs`) built their `OsStr` with
  `OsStr::from_encoded_bytes_unchecked` under a SAFETY comment claiming every
  byte string is valid for the platform's encoding. That is true on Unix and
  **false on Windows**, where `OsStr` is WTF-8 — and Windows is the host the
  tests run on, so the unsound branch was the only one ever executed. Replaced
  with a `#[cfg(unix)]` split: `OsString::from_vec` (safe and total) on the
  real target, which is `target-family = ["unix"]`, and a documented
  best-effort on the test host. The lossless core is now byte-level
  (`encode_bytes`/`decode_bytes`) and tested there, so the round-trip is
  asserted at the level the file is actually written at rather than at a level
  the test host cannot represent.

Related tooling gap, now closed: `rustup target add x86_64-unknown-linux-gnu`
was not installed, so **no `#[cfg(unix)]` code in this lane had ever been
compiled**, let alone checked. `cargo check --target x86_64-unknown-linux-gnu`
needs no linker and now covers those branches.


## `apps/indexer` stored index paths lossily and panicked on a short header (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. Third instance of the
lossy-path class, found by continuing the sweep. The index is a binary,
length-prefixed format, so unlike `meta.txt` and the backup manifest there was
never a readability tradeoff to weigh — it simply stored the wrong bytes:

- `serialize` wrote `entry.path.to_string_lossy().as_bytes()` and
  `deserialize` read them back with `String::from_utf8_lossy`. A file whose
  name is not UTF-8 was indexed under a name containing U+FFFD, so the search
  hit that named it could not be opened. Both sides now carry
  `OsStr::as_encoded_bytes` verbatim; `INDEX_VERSION` goes 1 → 2. No migration
  is needed — the index is a derived cache and the existing version check
  already tells the user to reindex.
- **Panic on a truncated index.** The header check was `data.len() < 28`, but
  `dirs_scanned` is read from bytes `24..32`, so a file of 28..=31 bytes
  passed the check and then indexed out of bounds. The existing
  `test_index_deserialize_too_short` used a 4-byte input and never reached it.
  Now `< INDEX_HEADER_LEN` (32), with a test that sweeps every length below it.

Two smaller things fixed in passing: the two scanners each carried a verbatim
copy of the directory-exclusion check (now one `is_excluded_dir`), and each
copy tested `dir_str.ends_with(excl) || dir_str.contains(excl)` — the same
predicate written twice, since `contains` is true whenever `ends_with` is.

The `filename` field stays a lossy `String`, now documented as a **search key
only**: a query is UTF-8 text the user typed, so matching against a lossy
rendering is a selection heuristic. It is never displayed and never used to
name a file — `path` is, and `path` is exact. Both producers of the key now go
through one `filename_key` function so they cannot drift.


## The thumbnail cache keyed on a lossy path, so one file showed another's image (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. Fourth instance of the
lossy-path class, and the first where the damage is not a lost name but a
**collision**.

`Thumbnail::source_path` was a `String` built with `to_string_lossy`, and it is
the disk cache's key: `simple_hash` FNV-hashes it with the mtime to produce the
cache filename. Every undecodable byte in a name became the same U+FFFD, so two
genuinely different files whose names differ only in such bytes hashed to one
cache entry — and the file explorer displayed one of them the other file's
thumbnail. Nothing errors; the wrong picture is simply shown. `source_path` is
now a `PathBuf` and `simple_hash` takes `&Path` and hashes
`as_os_str().as_encoded_bytes()`.

`purge_stale` had a matching problem in the other direction: it compared
directory entries by `to_string_lossy`, so a foreign file in the cache
directory whose name is not UTF-8 could be *rendered into* something matching
our `{hash:016x}.thumb` shape and deleted. Now compared as bytes.

**The lesson worth keeping is about the tests, not the bug.** The natural
regression test for this class needs a path the platform cannot decode, and on
the Windows test host `OsString` cannot hold arbitrary bytes at all — so the
obvious test has to be `#[cfg(unix)]` and never actually *runs* here. A
`cfg(unix)` test is compile-checked at best (and until this session, not even
that — see the note in the `apps/backup` entry above). The fix is to find the
host's *own* uncodable case: on Windows an unpaired surrogate is a legal
`OsString` that `to_string_lossy` maps to U+FFFD, which reproduces exactly the
same collision. Both tests now exist, and the Windows one was confirmed to fail
when the lossy hash is put back. Any future test in this class should carry a
runnable-on-the-host twin rather than a Unix-only assertion.


## TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED — `max_width` cuts mid-glyph and says nothing — ✅ **RESOLVED 2026-08-15**

**Resolution.** `RenderCommand::Text` gained a **required** `overflow:
TextOverflow` field (`Clip` | `Ellipsis`), and the compositor draws the mark.
The operator chose "required, no `Default`" from four options precisely so that
every one of the 4,517 constructions in the tree had to answer the question
`max_width` had been posing and never answering; see `design-decisions.md` §427
for the options and §429 for why the commit also had to fill in lane B's 31
sites. The second measurement the entry complains about below is gone from the
policy path: the compositor decides about the mark from the run it has already
shaped, so `text::elide` is no longer the only way to get a cut marked.

Bounded sites default to `Ellipsis` rather than to the behaviour-preserving
`Clip`, because today's behaviour *is* this entry — a sweep that faithfully
preserved it at four thousand sites would have done nothing.

Tested at all three layers: the compositor (a mark appears only when earned,
stays inside the limit, falls back to clipping when the mark itself does not
fit, and never blanks a field clipping would have filled), the toolkit (each
helper emits the right policy), and `guiremote` (both policies survive the wire,
are distinguishable on it, and an unknown byte is a `DecodeError` rather than a
guess — `PROTOCOL_VERSION` went to 2 for it).

**Status.** ~~Open, and deliberately not fixed in the pass that closed
`TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP`, because the good fix is a change to
`RenderCommand::Text` itself and wants a decision rather than a sweep.~~
The decision was asked and answered.

**What it is.** `max_width` clips: the compositor walks glyphs and stops when
the next one would cross the limit. It draws no ellipsis. So a label that does
not fit ends mid-word — and, worse, ends *plausibly*: "Gateway 192.168.1.1 res"
reads as a complete string to anyone who cannot see the field it was cut from.
A caller that wants the cut marked has to call `text::elide` first, which
measures the string a second time to answer a question the compositor is about
to answer again while drawing. That is the same two-calculations-for-one-quantity
shape as the wrap bug, one layer down.

**How widespread.** Every single-line label in the app tree that passes
`max_width: Some(..)` without eliding first — well over a hundred sites. Most
are fine in practice because the values are short and app-authored; the ones
that bite are those carrying user or network data (file names, SSIDs, error
strings, host names). `netmanager`'s diagnostics detail line was fixed by hand;
the rest were left.

**Proper fix.** Give the command an explicit overflow policy — `Clip` (today's
behaviour, correct for a progress label that must not jitter) versus `Ellipsis`
(the right default for a data-bearing label) — and let the compositor draw the
mark, since it is the only party that knows exactly where the glyphs ran out.

**Why it is not done.** Adding a field to `RenderCommand::Text` touches every
struct-literal construction of it in `gui/**` and `apps/**` — several hundred —
because Rust has no default for a struct-variant field. The alternatives are
each a compromise: a second variant (`TextClipped`) splits the match arms in
every renderer; a builder function leaves the literal form available and so does
not actually prevent the mistake; a blanket `text::elide` sweep at the call
sites fixes the symptom while keeping the double measurement. The mechanical
churn is cheap to *do* and expensive to *review* against three lanes' in-flight
work, so it should be scheduled deliberately rather than smuggled into an
unrelated fix. Recorded for the operator in `open-questions.md`.

---

### TD-FONT-NOT-ACTUALLY-NO-STD. `osfont` documents itself as `no_std` but links `std` — 2026-08-14 — OPEN

**What.** `gui/font` is written entirely in `alloc` terms (`alloc::vec::Vec`,
`alloc::string::String`, no `std::` paths, `extern crate alloc;` at the top),
and a comment in `cff.rs` asserted outright that "this crate is `no_std`". It
is not: `src/lib.rs` carries no `#![no_std]` attribute, so the crate links the
standard library like any other and the discipline is enforced by nothing but
habit.

**How it was found.** Adding `#![no_std]` to see whether the claim held. It
does not — the build fails with 47 errors, in two groups:

- **Float math (35 errors).** `f32::sqrt`, `floor`, `ceil`, `round` and
  `mul_add` are inherent methods provided by `std`, not by `core`. They are
  used throughout `raster.rs` and `scaled.rs`, which is unavoidable for a
  rasterizer.
- **Prelude items (12 errors).** `String`, `vec!` and `format!` are reached
  through the `std` prelude at a dozen sites instead of being imported from
  `alloc`.

**Why it matters.** The compositor and the toolkit both depend on this crate
and both are meant to run on SlateOS. As long as the attribute is absent, a
`std::`-only construct added here compiles cleanly on the development host and
fails only when someone finally builds for the target — at which point the
offending code is old and its author is a previous session. The false comment
made this worse than a silent omission, because it told the next reader the
invariant was already being checked.

**Proper fix.** Add `libm` to the workspace, replace the inherent float
methods with `libm::{sqrtf, floorf, ceilf, roundf, fmaf}` (or the
`num-traits`/`libm` float shim), import the prelude items from `alloc` at the
dozen sites, then add `#![no_std]` and `#[cfg(test)] extern crate std;`. The
mechanical part is small; what makes it more than mechanical is that `libm`
would be this workspace's first float-math dependency, and whether SlateOS
userspace GUI binaries get a `std` port at all is Lane B's call (`posix/**`) —
if they do, `no_std` here buys much less than it seems to. That question
should be settled before spending the churn.

**Interim.** The false comment in `cff.rs` was corrected and the crate docs in
`lib.rs` now state the real position, so nobody is misled into thinking the
invariant is enforced. Keep writing `alloc::` paths: the point of doing so is
that closing this stays a small change.

---


## FIXED: TD-START-MENU-POWER-ROW-IS-A-LABEL

**Fixed 2026-08-14.** The footer row is a real button: `power_button_rect()`
reports `Hit::PowerButton`, which toggles `power_menu_open`, and
`power_menu_rect()` / `power_menu_row_rect(row)` place a popup that
`power::render_power_menu` draws and `hit_test` reads — one accessor per
clickable part, as the `Rect` documentation requires. Its five rows are
`power_menu_entries()`, exactly the `Category::System` entries that
`start_menu_entries()` filters out, and clicking one returns the same
`ShellAction::Launch` an application row does: `/sbin/shutdown` and its
neighbours are what actually shut the machine down, not the window manager.
The popup is themed and scaled by the shell (it takes a `PowerMenuStyle`)
rather than by `power.rs`'s own palette, so it follows the light theme and the
display scaling like everything else. `close_start_menu()` is now the single
place the menu closes, which is what keeps the submenu from being stranded over
an empty desktop. Nine tests in `pointer_tests.rs`, including one that walks
every scale from 100% to 200% asserting no system action is dropped or drawn
where it cannot be clicked. No confirmation prompt: Start → Power → Shut down
is one click on every desktop that has this menu, and an extra "are you sure"
is not what makes shutdown safe.

The original report follows.

**What.** The foot of the start menu draws the word "Power" in grey. It is
text, not a control: `hit_test` reports `Hit::StartMenuPanel` there, and the
five `Category::System` entries of the app database — Shutdown, Restart,
Sleep, Lock, Logout — are consequently unreachable from the shell. They are
filtered *out* of `start_menu_entries` on purpose, so that "Shutdown" is not
one mis-click below "Screenshot"; but nothing yet offers them anywhere else.

**Why it bites.** There is no way to shut the machine down from the desktop.

**Proper fix.** A power submenu opened from that row: a small popup listing the
`Category::System` entries, which resolves to the same `ShellAction::Launch`
the application rows produce. `gui/desktop/src/power.rs` already models power
actions and confirmation prompts and should be the thing that renders it,
rather than a second list inside `render_start_menu`. Needs the same
geometry-shared-with-the-hit-test treatment as the rows above it — see the
`Rect` documentation in `main.rs`.

**Where.** `gui/desktop/src/main.rs` — `render_start_menu`'s footer,
`DesktopShell::hit_test`; `gui/desktop/src/power.rs`.


## FIXED: FLAKY-GUITK-SCALING-TESTS-SHARED-A-PROCESS-GLOBAL

**What.** Five tests in `gui/toolkit/src/scaling.rs` — `global_scale_default_is_1`,
`set_and_get_global_scale`, `global_scale_clamped`, `per_monitor_override`,
`per_monitor_clear_falls_back` — each wrote the process-wide `SCALE_TABLE` and
then asserted on it. Cargo runs tests on parallel threads, so
`per_monitor_clear_falls_back` (which sets the global scale to 1.5) failed
whenever another of the five reset it to 1.0 in between. Observed failing once
in a full `cargo test -p guitk` run and passing when run alone.

**Fix.** A `SCALE_LOCK` mutex in the test module, taken by a `ScaleGuard` whose
`Drop` restores the whole table. Restoring on drop rather than at the end of
each test body means a failing assertion — which unwinds — still leaves clean
state, so one failure cannot cascade. The lock is taken with
`unwrap_or_else(|e| e.into_inner())` because a poisoned lock carries no
information once the guard restores the state anyway.

**Residual gap.** `DesktopShell::set_appearance` now publishes the display
scaling into that same process-global table, so `guitk` widgets hosted in the
shell lay out at the scale the chrome is drawn at. That one line has no unit
test of its own: every desktop test that builds a shell writes the value an
assertion would read, and `desktop` is a binary crate so the assertion cannot
be moved to an out-of-process integration test. Rationale is recorded on the
method.

**Where.** `gui/toolkit/src/scaling.rs` (test module);
`gui/desktop/src/main.rs` — `DesktopShell::set_appearance`.


## TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES — ✅ FIXED 2026-08-14

**What.** The `GSUB`/`GPOS` walk in `gui/font/src/otl.rs` starts at the
FeatureList and takes *every* feature carrying a wanted tag, rather than
starting at the ScriptList and taking the features that the run's script and
language actually select. A face that registers the same feature tag under
several scripts therefore has all of those scripts' lookups applied to every
run, whatever the run is written in.

**Why it bites now.** This was a documented, mostly-theoretical limitation
while only `liga`/`rlig` were read: a ligature belonging to another script
almost never matches Latin glyphs, so the wrong lookups ran but did nothing.
Reading `ccmp` changes that. `ccmp` is precisely where a script puts its
normalisation rules, and those rules are meaningless — or wrong — outside it.

**Reproduce.** `cargo test -p osfont --target x86_64-pc-windows-gnu --test
host_fonts -- --ignored --nocapture installed_fonts_leave_plain_latin_alone`.
On a stock Windows host, `ebrima.ttf` and `ebrimabd.ttf` substitute the *space*
glyph in plain English prose: their `ccmp` lookup 15 is an extension-wrapped
type-1 format-2 subtable mapping glyph 3 (space) to 2220, and it belongs to one
of the African scripts Ebrima covers, not to Latin. Verified against an
independent Python parse of the table, so this is our *selection* being wrong,
not our *parsing*.

The damage is small — 2 faces of the 275 with `GSUB` on this host, and the
substituted glyph is a space variant — but it is a genuinely wrong glyph, and
the class of fault grows with every feature added.

**Proper fix.** Script and language selection, in two parts:

1. **The table walk.** Walk the ScriptList, pick the ScriptRecord for the run's
   script (falling back to `DFLT`), then its LangSys (falling back to the
   default), and intersect that LangSys's feature indices with the wanted tags.
   This is contained work in `otl.rs` and affects `kern.rs` and `mark.rs` too,
   since they share the walk.
2. **Script itemisation.** Deciding what a run's script *is* needs the Unicode
   Script property, which this crate does not have — a run must be split into
   same-script pieces before it can be shaped, which is also the prerequisite
   for bidi and for complex-script reordering. This is the larger half and is
   the reason (1) is not enough on its own.

Until both land, `installed_fonts_leave_plain_latin_alone` tolerates a small
proportion of faces changing plain Latin prose. When script selection works,
that count should drop from eight to the six Linux Libertine files, whose `Th`
ligature is correct.

**Where.** `gui/font/src/otl.rs` — `lookup_indices` (the FeatureList walk, and
the module doc's "What is not here"); `gui/font/src/gsub.rs` — the feature tag
list in `Substitutions::parse`; `gui/font/tests/host_fonts.rs` —
`installed_fonts_leave_plain_latin_alone`.

**Fixed 2026-08-14** (commit `6e0746636`), both parts, as designed above and
recorded in design-decisions.md §411.

1. `ByScript` in `otl.rs` walks the ScriptList, resolves every script the face
   registers once at parse time, and shares the decoded lookups keyed by
   LookupList index. `Substitutions::apply` takes the run's script and binary
   searches for it, falling back `dev2`→`deva`→`DFLT`→`dflt`.
2. `gui/font/src/script.rs` carries the Unicode Script property (generated
   into `script_tables.rs` from `fontTools.unicodedata`) and `script::runs`
   splits a piece list into maximal same-script stretches. The split happens
   in `ScaledFont::shape` *before* substitution, while glyphs are still one
   per piece — after anything ligates, a boundary counted in pieces is no
   longer a boundary counted in glyphs.

Ebrima no longer substitutes the space, and the same change fixed
`B-FONT-CALIBRI-SHAPES-A-FRACTION-SLASH-DIFFERENTLY-FROM-HARFBUZZ`, whose
cause turned out to be identical.

**The prediction in this entry was wrong, and the correction is the
interesting part.** It said the plain-Latin count "should drop from eight to
the six Linux Libertine files". It is *nine*, and all three non-Libertine
faces are correct: `segoesc`/`segoescb` have genuine Latin `calt`, and
`SansSerifCollection` maps `space` through its Latin `locl` — a feature this
crate had been skipping, and which was only safe to add once features were
script-scoped. All nine now agree with HarfBuzz glyph for glyph. The test's
bound is a proportion, not a list, which is why it kept working; a hard-coded
expected count would have had to be relaxed for a change that made the shaper
*more* correct.

**Successors.** Four narrower gaps remain and are filed separately:
`TD-FONT-IGNORES-LANGSYS-OVERRIDES`,
`TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`,
`TD-FONT-SCRIPT-RUNS-IGNORE-SCRIPT-EXTENSIONS` and
`TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER`.


## TD-FONT-IGNORES-LANGSYS-OVERRIDES — a font's per-language rules were unreachable — ✅ **RESOLVED 2026-08-15**

**Resolution.** Exactly the fix sketched below, and the required-feature gap
beside it. `ScaledFont::shape_lang(text, Option<Lang>)` and
`SystemFont::shape_lang` take a language; `shape(text)` is `shape_lang(text,
None)`, so the change is purely additive and no caller that names no language
can shape differently than it did. `otl::ByScript::parse` now precomputes a
lookup selection per **(script, language)** rather than per script, preferring
the named LangSysRecord over the DefaultLangSys, and `feature_indices` finally
reads `requiredFeatureIndex` — the one feature a language system states outside
its index list, which the walk had been dropping for the default language too.

`lang.rs` does the BCP 47 → OpenType mapping, following HarfBuzz's
`hb_ot_tags_from_language` rule for rule: complex rules first (`ro-MD` →
`MOL `, `zh-Hant` → `ZHT `), then extended-language-subtag substitution, then
the 2- and 3-letter registries, then the blocked list, else uppercase. It is
allocation-free and puts no bound on the tag's length.
`tools/gen_lang_tables.py` generates its four tables from HarfBuzz's source, so
a registry update is a regeneration rather than an edit.

Four things worth keeping in mind about the shape of the fix:

- **A LangSysRecord replaces the default's feature list; it does not add to
  it.** So naming a language can take a feature *away* — which is exactly what
  `TRK ` does to `liga`. Callers should pass `None` rather than a guess: a
  wrong language is worse than no language.
- **One BCP 47 tag resolves to a *list* of up to three OpenType tags, not to
  one.** `ro-MD` is `MOL ` and then `ROM `; `ml` is Malayalam Traditional and
  then Reformed; `ga` is `IRI ` and then `IRT `. They are candidates and not
  synonyms: a face is asked for each in turn and the first it **registers**
  wins. The cap of three is HarfBuzz's `HB_OT_MAX_TAGS_PER_LANGUAGE`, and
  truncating where HarfBuzz truncates is what keeps the two engines answering
  alike. See "What the oracle caught" below — the first version of this fix
  kept only the head of each list and was wrong on 66 of the host's 556 faces.
- **Language selection deliberately does not fall back the way script
  selection does.** A script that does not register the language takes its own
  default, never another script's — HarfBuzz's split between
  `hb_ot_layout_table_select_script` and
  `hb_ot_layout_script_select_language`. `gsub::tests::language_selection_does_not_fall_back_to_another_script`
  pins it.
- **A script's default entry is stored even when it selects nothing**, because
  that entry is what says the script exists and stops the fallback chain.
  Language entries identical to their script's default are *not* stored; two
  thirds of the host's are. This is why `ByScript` keeps a second list of every
  (script, language) the face *registers*: "which candidate wins" is decided by
  what the font registered, never by what happened to be worth storing, or a
  `MOL ` that says nothing would hand Moldavian to `ROM `'s overrides on the
  strength of an optimisation.

**Scale.** `tools/langsys_survey.py` measured the host before the fix: of 581
installed faces, 290 register at least one LangSysRecord, 3031 (script,
language) records in all, 1203 of which differ from their default and **996 of
which differ in a feature tag this crate asks for, across 230 faces**. Moved
tags: `locl` 856, `ccmp` 90, `liga` 67, `calt` 28, `mark` 25. The survey's
feature list is pinned equal to the shaper's by
`otl::tests::the_survey_matches_the_shapers_feature_list`, so a number it
reports cannot quietly come to mean something else.

**Tested** by seven new unit tests over hand-built ScriptLists that
`fixture::script_list` cannot express (a script with no DefaultLangSys, a
script with named languages, a `requiredFeatureIndex`, a face registering only
a language's second candidate, and both orders of a face registering two of
them), by 20 tests over the BCP 47 mapping, and by `tools/harfbuzz_sweep.py`,
which grew a language field: each new corpus entry is a string already in the
corpus plus a tag, so a difference between the two halves is the language and
nothing else, and both halves map the tag with the same rules. The sweep's
buffer language is set *after* `guess_segment_properties`, and explicitly to
`""` for the language-less entries, because the guess otherwise fills it in
from the machine's locale and the run would pass or fail by where it was made.

**What the oracle caught.** The first version of this fix passed 521 unit
tests, was clippy-clean, and was wrong. The sweep found it in one run: `ro-MD`
disagreed with HarfBuzz on **345** faces where plain `ro` disagreed on 279, and
the 66-face gap was the bug. HarfBuzz's `hb_ot_tags_from_language` returns an
ordered list of up to `HB_OT_MAX_TAGS_PER_LANGUAGE = 3` candidate tags and asks
the face for each in turn; `gen_lang_tables.py` had deliberately kept only the
first of each list, on the reasoning that a language has one tag. Those 66
faces — `Candara.ttf` among them — register `('latn', 'ROM ')` and no `MOL `,
so HarfBuzz reached Romanian's comma-below `locl` for Moldavian through the
second candidate and we did not. After the generator was reworked to keep all
of them, the `ro-MD` bucket is 279: exactly `ro`'s, exactly the language-less
twin's, and entirely the pre-existing NFC divergence recorded in
`design-decisions.md` §410. Final sweep: 556 faces × 35 strings, 18235 agree,
reordered 0, misplaced 0.

This is the third bug the HarfBuzz oracle has found that a green unit-test
suite could not, and for the same reason every time: "this face has no glyph
/ no language system for that" is a *legal* answer, so no self-consistency
check can tell it apart from the truth. Only a second implementation can.

The original entry follows.

---

**What.** `otl::select` reads each ScriptRecord's DefaultLangSys and ignores
its LangSysRecords entirely. The per-language overrides — Turkish dotless `i`
under `TRK `, Serbian Cyrillic italic letterforms under `SRB `, Moldovan
comma-below under `MOL ` — are never reached, and a face whose *only* route to
a feature is a language system contributes nothing at all.

**Why it bites.** It is invisible until it is not. A Turkish reader gets the
wrong dot on `i`/`ı`; a Serbian reader gets Russian italics for бгпт. Both are
the kind of wrongness a native reader notices immediately and nobody else ever
does.

**Why it is filed rather than fixed.** There is nothing to select *with*.
Language is a property of the text's provenance, not of its characters — the
same Cyrillic codepoints are Serbian or Russian depending on who typed them —
so it cannot be derived the way script is. It needs a language carried on the
text down to `ScaledFont::shape`, which means an API change reaching the
toolkit and the locale system, neither of which has a language to hand yet.

**Proper fix.** Add an optional BCP 47 language to the shaping call, map it to
an OpenType language system tag (the registry is a fixed table, `tr` → `TRK `,
`sr` → `SRB `), and have `select` prefer that LangSysRecord over the
DefaultLangSys. Default stays "no language", which is what every shaper does
when not told and what this crate does now — so the change is additive and
cannot regress text that names no language.

**Reproduce.** `gsub::tests::a_feature_only_a_language_system_reaches_is_not_applied`
pins the current behaviour: a `locl` reachable only through `TRK ` yields no
`Substitutions` at all.

**Where.** `gui/font/src/otl.rs` — `select`, `LangSys`, and the module doc's
"What is not here".


## TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES — RESOLVED 2026-08-15

**Resolved in two commits, because it was two bugs wearing one name.**

*Half one — erasing them* (`88ee69ca7`): `norm::ignorable` classifies the
character, `SubGlyph::ignorable` carries the answer and is cleared wherever a
`GSUB` lookup rewrites the glyph, and `ScaledFont::shape` replaces what is left
with the space glyph, or drops it where the face has none.

*Half two — stepping over them* (this commit): erasing an ignorable at the end
is not enough, because the lookups in between still saw it as a wall. `f ZWJ i`
did not ligate; a contextual alternate did not match across a soft hyphen. The
matcher now answers three ways rather than two, as HarfBuzz's does — hide,
*step over*, or consider — with the kind of ignorable and the kind of lookup
deciding which. See `design-decisions.md` §434 for the shape of that, and
`gui/font/src/skip.rs`'s `Joiners` for the table.

**Measured.** Host sweep, 556 faces × 60 strings: `differ` on `f\u200di` went
from 76 faces to 0, and `misplaced` from 331 to 170. Khmer probe: 45/45 before
and after, which is the point — the Indic-family features read the joiners
themselves and had to come through unchanged.

**The 170 that remain are a deliberate divergence, not a residue.** They are
every corpus string containing an ignorable, and in all of them the glyphs and
every *visible* glyph's position agree; what differs is the x of the erased,
zero-advance glyph itself. HarfBuzz spends a legacy `kern` on the right-hand
glyph's offset, so its erased glyph sits at the *unkerned* pen — 13 units
inside the following letter's image, for `a◌͏b` in Arial Rounded. We charge the
kern to the pair's left glyph, so ours sits exactly where the next glyph is
drawn. A caret asked to land on the ignorable's cluster wants ours. Recorded in
`design-decisions.md` §434; do not "fix" it without reading that first.

---

*The original entry follows, as filed.*

**What.** A handful of characters exist to instruct the shaper and are never
meant to be drawn: the zero-width joiner and non-joiner, the soft hyphen, the
bidi controls, the variation selectors, the byte-order mark. Once shaping is
over, HarfBuzz erases them — `hb_ot_hide_default_ignorables`, in
`hb_ot_substitute_post`, replaces each one's glyph with the face's `space`
glyph, or **deletes the glyph entirely** if the face has no space — and
`hb_ot_zero_width_default_ignorables`, during positioning, zeroes their
advances and x-offsets first. We do neither: `ScaledFont::shape` maps the
character through `cmap` like any other and returns whatever glyph came back.

**Symptom, measured.** The two strings the Khmer probe font disagrees on
(`gui/font/tools/khmer-corpus.txt`, the `\u17d2\u200d\u1781` and
`\u17d2\u200c\u1781` lines) are exactly this: HarfBuzz emits the space glyph
where we emit ZWJ's and ZWNJ's own glyphs. It is invisible in the host sweep
only because the built-in corpus has no string containing an ignorable that
the face also maps.

**Why it matters beyond the joiners.** This is crate-wide and
script-independent, and the joiner case is the *benign* one — a face that maps
ZWJ usually maps it to something blank anyway. The soft hyphen U+00AD is the
one that bites: fonts routinely map it to a real hyphen glyph, so a word
carrying a discretionary break renders with a hyphen sitting in the middle of
it whether or not the line broke there. The bidi controls and variation
selectors are the same shape of bug.

**One subtlety that is easy to get wrong.** HarfBuzz's predicate is
`(unicode_props() & UPROPS_MASK_IGNORABLE) && !_hb_glyph_info_substituted()` —
a character stops counting as ignorable the moment a GSUB lookup rewrites it,
because at that point the glyph is whatever the font asked for and is no
longer the control character. So the flag has to be *cleared on substitution*,
not merely tested at the end. And the set is HarfBuzz's own hard-coded list
(U+00AD, U+034F, U+061C, U+17B4–17B5, U+180B–180E, U+200B–200F, U+202A–202E,
U+2060–206F, U+FE00–FE0F, U+FEFF, U+FFF0–FFF8, U+1BCA0–1BCA3, U+1D173–1D17A,
U+E0000–E0FFF), *not* Unicode's `Default_Ignorable_Code_Point` property; using
the Unicode set would make the sweep disagree in the other direction.

**Proper fix.** A flag on `SubGlyph`, set in `scaled.rs`'s per-piece build loop
from the character, cleared at the three sites in `gsub.rs` that assign a
glyph id — `apply_single`, `apply_alternate`, the ligature path — and by
`apply_multiple`'s splice. Then in the loop that builds `out: Vec<ShapedGlyph>`
at the end of `shape`, zero the advance and offsets and substitute the space
glyph, or drop the glyph if the face maps no space. Corpus strings containing
a soft hyphen and the joiners go into `harfbuzz_sweep.py`'s built-in `CORPUS`
in the same change, so the fix is measured on all 556 host faces rather than
on the one probe font that happened to expose it.

**Where.** `gui/font/src/scaled.rs` — the per-piece loop that derives
`tab`/`klass`/`mark`/`indic` from each character, and the `out`-building loop
after it; `gui/font/src/gsub.rs` — `apply_single`, `apply_multiple`,
`apply_alternate` and the ligature path; `gui/font/tools/harfbuzz_sweep.py` —
`CORPUS`.


## TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS — ✅ FIXED 2026-08-15

**What.** `gui/font/src/hangul.rs` is a complete, tested transcription of
HarfBuzz's `preprocess_text_hangul` — 673 lines, 19 tests, all passing — that is
**not declared in `lib.rs`** and therefore compiles nowhere and changes no
output. It was parked mid-task on an explicit halt, at the point where it worked
in isolation but was not yet connected.

**Why it is parked rather than either finished or deleted.** The connection is
all-or-nothing, and the half of it that was written first is a regression on its
own. Wiring the shaper means telling `norm::pieces` to stop normalizing Hangul —
HarfBuzz's Hangul shaper sets `HB_OT_SHAPE_NORMALIZATION_MODE_NONE` precisely
because composing first destroys the distinction the shaper reads. But `pieces`
composing `<L,V,T>` to a syllable is currently the *only* thing that makes
Korean render at all on the ordinary Korean text font, which ships the 11,172
precomposed syllables and no jamo. Exempt Hangul from normalization without the
shaper in place and that font draws three missing-glyph boxes where it used to
draw one correct syllable. So the `norm.rs` half was reverted and the module
kept: a tested, inert file loses nothing, whereas a half-wired one is worse than
neither.

**The four edits that connect it**, in the order they have to happen:

1. `norm.rs` — thread a private `enum Hangul { Normalize, LeaveAlone }` through
   `decompose_once`, `compose_pair`, `decompose_into` and `compose`; split `nfc`
   into `nfc` (which passes `Normalize`, because NFC is NFC and a question about
   *text* must get that answer) and a private `normalize(text, hangul)`. `pieces`
   then calls `normalize(text, Hangul::LeaveAlone)`, and `split_undrawable` calls
   `decompose_once(ch, Hangul::LeaveAlone)` — the latter because a syllable
   `hangul::preprocess` declined to split has been declined on grounds
   `split_undrawable` cannot see, namely that the face has no jamo either. Three
   call sites in `norm.rs`'s own tests need the new argument.
2. `gsub.rs` — add `b"ljmo"`, `b"vjmo"`, `b"tjmo"` to `FEATURES` with `LJMO`,
   `VJMO`, `TJMO` bit constants, and a `SubGlyph::jamo(gid, cluster,
   Option<Jamo>)` constructor that ORs the one jamo bit and **clears `CALT`**.
   Clearing `calt` is not an optimization: Noto Sans CJK and Source Han Sans file
   all of their jamo lookups under `calt`, and HarfBuzz's `setup_masks_hangul`
   turns it off for every L/V/T so those lookups cannot fire twice.
   `the_masks_match_the_feature_list` has to keep passing.
3. `scaled.rs::shape` — call `hangul::preprocess` immediately after
   `norm::pieces`, with `has_glyph = |ch| self.face.glyph_index(ch).is_some()`
   and `zero_width = ` has-glyph *and* zero horizontal advance; then choose
   between `SubGlyph::cursive` and `SubGlyph::jamo` in the piece loop on
   `hangul::is_jamo(ch)`. Guard the whole thing with `hangul::present` so a run
   with no Korean in it pays nothing.
4. `fallback.rs` — add `*b"hang"` to `NO_ZERO_WIDTH_MARKS` (the Hangul shaper's
   `zero_width_marks` is `NONE`) and **not** to `COMPLEX_SCRIPTS` (its
   `fallback_position` is `true`). Both lists are `is_sorted`-asserted.

**What it should buy.** 553 of the sweep's 892 remaining `differ` cases are the
single string `\u1100\u1161\u11a8` — jamo we compose to `각` and HarfBuzz leaves
as three glyphs. Expect `differ` 892 -> ~339. The residue after that is composed
Latin diacritics, which is a *different* and unsettled question: HarfBuzz
decomposes and recomposes against font coverage, which reverses the layering
`norm.rs`'s module doc deliberately chose (`nfc` pure Unicode, `fit_to_face` pure
font). That one is an operator question, not a bug.

**Where.** `gui/font/src/hangul.rs` (parked), `gui/font/src/lib.rs` (the missing
`mod hangul;`), and the three files named above. The reference is HarfBuzz's
`src/hb-ot-shaper-hangul.cc`.

**Resolution — 2026-08-15.** All four edits landed together with the missing
`mod hangul;`, and the prediction above held to the case. The HarfBuzz
differential sweep (556 host faces × 23 strings, 12,739 comparisons):

| bucket | before | after |
|---|---|---|
| `agree` | 11,847 | **12,400** |
| `differ` | 892 | **339** |
| `reordered` | 0 | 0 |
| `misplaced` | 0 | 0 |

`\u1100\u1161\u11a8` — all 553 of its cases — left the disagreement list
entirely, and nothing regressed into `reordered`/`misplaced`. `osfont` goes
from 482 to **501 passing tests**: the module's own 19 tests had never run
before, because a module that is not declared does not compile and therefore
does not test either. That is the sharper lesson here — "19 tests, all
passing" was a true statement about a file `cargo test` had never once
looked at.

Two notes on how the edits differ from the plan above. `gsub.rs`'s three new
feature tags are **appended** to `FEATURES` rather than inserted in tag order,
so that no existing bit constant shifts; the bits are `1 << 34/35/36`.
And `norm::nfc` lost its last production caller in the split, so it now
carries `#[cfg_attr(not(test), allow(dead_code, …))]` — it is kept deliberately
as the text-question half of the split (NFC is NFC), not as dead weight, and
the reason is written at its definition.

The residual 339 are exactly the composed-Latin-diacritics cases this entry
predicted (`\u1e09` 255, `\u212b` 57, `été` 10, …). They are **not** tracked
here as a bug; they are the layering question in `norm.rs`'s module doc, and
belong to the operator. See `open-questions.md`.
### [B] D-POSIX-SOCKET-META-WAS-NOT-SCOPED-TO-ITS-FD-TABLE — ✅ FIXED 2026-08-14

**Found while running the eighth audit pass**, not by looking for it:
`socket::tests::test_phase201_bind_port443_no_cap_eacces` failed once with
`ENOTSOCK` where `EACCES` was expected, then passed three runs in a row.

`SOCKET_META` (posix/src/socket.rs) is indexed by fd number, so it must have
exactly the same scope as the fd table it is keyed by. `fdtable` made its
storage **per-thread** on host builds (design-decisions.md §110) precisely
because libtest runs tests on parallel threads. `SOCKET_META` stayed a
process-global `static mut`, and the mismatch was reachable: two tests on
different threads each create a socket and, drawing from *separate* per-thread
fd tables, both get the same fd number `N` — near-certain, not unlikely, since
each thread's table starts empty. They then shared one `SOCKET_META[N]`, and
the first to `close()` wiped the entry the other was still using, whose next
call saw a live fd with no metadata and reported `ENOTSOCK` for a good socket.

Fixed by giving `SOCKET_META` the same `cfg`-split storage as
`fdtable::fd_store`. Six consecutive full runs clean afterwards.

Two things worth keeping from this. First, the `// SAFETY: Single-threaded
access.` comments on these accesses were **true on the target and false under
`cargo test`** — a safety comment that silently changes truth value with
`cfg` is worse than none, and `fdtable` had already learned this lesson
without the fix being propagated to the table keyed by its own indices.
Second, an intermittent failure at roughly one run in four is easy to
dismiss as noise when it appears in a test unrelated to what you are
changing; it was worth the ten minutes to chase.

### [B] D-POSIX-TIMED-WAITS-DID-NOT-VALIDATE-TV-NSEC — ✅ FIXED 2026-08-14

`pthread_cond_timedwait`, `pthread_mutex_timedlock` and `sem_timedwait`
accepted any `timespec` whatsoever. A `tv_nsec` of `1_000_000_000` or `-1` —
the classic result of adding a nanosecond offset without carrying into
`tv_sec` — should be `EINVAL` (glibc `valid_nanoseconds`, `include/time.h:517`);
instead it fell through to the deadline comparison, where a too-large
`tv_nsec` silently extended the wait by up to a second and a negative one made
the call return `ETIMEDOUT` immediately. Both are wrong in the direction that
hides the caller's bug. Separately, `mqueue::deadline_from_timespec` checked
`tv_nsec` but not `tv_sec < 0`, which the kernel's `timespec64_valid` rejects.

Fixed by adding `time::valid_nanoseconds` (glibc's predicate, verbatim) and
calling it from each site **at the position its own upstream uses** — eagerly
in `pthread_cond_timedwait` and `sem_timedwait`, lazily (contended branch
only) in `pthread_mutex_timedlock` — plus the missing `tv_sec` half in
`mqueue`. See the ninth-pass write-up under
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT` for why the three
placements differ and why the mqueue predicate is not the same predicate.

Seven tests pin the distinctions, including the two that would silently pass
under a naive "check it at the top of every function" fix:
`test_pthread_mutex_timedlock_uncontended_ignores_a_bad_deadline` and
`test_sem_timedwait_checks_the_deadline_before_the_fast_path`.

**Not fixed, because we do not have them:** `pthread_cond_clockwait`,
`sem_clockwait` and the `pthread_rwlock_{timed,clock}{rd,wr}lock` family are
unimplemented. When they are added they need the same predicate plus
`futex_abstimed_supported_clockid`, and the rwlocks check **eagerly** — see
the comment at `pthread_rwlock_common.c:286-291`.

---

### [B] TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-PERFORMED. bash runs `${z:-<(echo hi)}` and substitutes `/dev/fd/63`; osh yielded the nine characters `<(echo hi)` — 2026-08-14 — ✅ FIXED 2026-08-14

**Where it was:** `userspace/oils/src/lexer.rs`, [`Lexer::read_word_verbatim`],
which reads the operand, the pattern and the replacement of a `${ … }` and had
no `<`/`>` arm at all.

bash splits this construct across two files and osh had only one half of it.
**Part (A) — the parse** — is `parse_matched_pair` naming `<(`, `>(` and `$(` in
one breath (parse.y:5028) and sending all three through `parse_comsub`
(parse.y:5042), so a `${ … }` body's scan parses a process substitution where it
meets it, its syntax error is the enclosing unit's, and what survives is the
parse *re-printed*; see
`userspace/oils/tests/corpus/a-process-substitution-in-a-brace-body-is-parsed-where-it-is-met.sh`
and [`parser::procsub_reprints`]. **Part (B) — the performance** — is
`expand_word_internal` *running* it, and was this entry.

**The rule** is bash's quoting flag, not the position. `expand_word_internal`
reads a process substitution only when `if (string[++sindex] != LPAREN ||
(quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) || (word->flags & W_NOPROCSUB))`
lets it (subst.c:11079), so an **operand** runs one when the expansion is bare
and keeps the characters when it is double-quoted, a **pattern** and a
**replacement** run one either way (both are re-entered without
`Q_DOUBLE_QUOTES`), and a **subscript** or a **substring bound** never does
(`Q_DOUBLE_QUOTES|Q_ARITH`), so its arithmetic error names the characters.

**The fix.** [`Verbatim`] gained an `Arith` mode beside `Bare`, `Replacement`
and `Dquote` — identical to `Bare` in every other respect — and
[`Lexer::read_word_verbatim`] gained a `<`/`>` arm live in `Bare` and
`Replacement` only. On the parser side [`parser::verbatim_word_at`] picks the
lexer entry from a new `Frag` (`Word` or `Arith`), which is what a subscript and
the `' … '` runs inside it now pass. The body the arm reads is already the
*re-print* part (A) spliced in, which is what bash performs too: the token
buffer a `${ … }` scan leaves behind holds the re-print and nothing else.

No new expansion machinery was needed. The double-quoted operand was already
right — the splice puts the re-print into the text and its nested `$( … )` then
expands normally, so `"${z:-<(echo $(echo q))}"` is `<(echo q)` in both shells —
so the whole of part (B) was one liveness decision taken at lex time, which is
where osh decides quoting.

**The pre-existing inconsistency this closed.** The substring bound
(`${z:<(echo hi)}`, via [`parser::parse_slice_bounds`]) *did* perform the procsub
while the subscript beside it did not, so osh's two arithmetic contexts — which
bash expands identically — disagreed. The bound is tokenized rather than read
verbatim, so it has no `Verbatim` mode to set; [`parser::word_from_source`], its
only reader, now turns a `Seg::ProcSub` back into the characters it was read
from. Both contexts are on the same side now.

**Verified:** `a-process-substitution-in-a-brace-body-is-performed-unless-the-expansion-is-quoted.sh`,
27 cases across the five contexts. None of them prints a substitution's path —
bash names it `/dev/fd/N` and osh a temporary file — so each asks a question the
path does not answer: whether the text still begins `<(`, whether it names
something that exists, or what a `cat` of it reads.

**How it was found:** implementing part (A) — the eager parse and re-print of a
process substitution met by a `${ … }` body scan.

### [B] TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN. bash's `brace_gobbler` and its `${x@P}` re-read each meet a `<(` osh's do not — 2026-08-14 — ✅ FIXED 2026-08-14 (both halves, and the arithmetic-fragment residue)

Two residues of TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-PERFORMED
(above), left after both halves of it were done. Each is a *second* scan of the
same text — one that is not `parse_matched_pair` and not `expand_word_internal` —
which has a `<(` row of its own that osh's counterpart lacks. The `$(` spelling
of each already matches bash byte for byte, so in both the machinery is there
and only the row is missing.

**Where:** `userspace/oils/src/interp.rs`, [`Shell::gobbled_subs`]; and the
`${x@P}` re-read, `userspace/oils/src/parser.rs`, [`dquote_word_from_source`].

* **✅ FIXED 2026-08-14.** `echo "${z:-"<(fi)"}"` — bash reports
  `command substitution: line N+1: syntax error near unexpected token 'fi'`
  plus the tail of the physical line, where osh prints `<(fi)`. The agent is
  **`brace_gobbler`**, whose command-substitution row names all three spellings
  (`(c == '$' || c == '<' || c == '>') && text[i+1] == '('`, braces.c:675) and
  reaches `extract_command_subst` → `xparse_dolparen`, which *parses* the body
  and throws the result away. Two facts pin it down. The gobbler's `quoted`
  state does not nest and `${` opens none of its own (it is treated like `\{`),
  so the **inner** `"` is `c == quoted` and clears the state — which is why the
  row fires here and not in the plain `"${z:-<(fi)}"`, where parse.y has
  already answered. And it fires only where brace expansion runs: an argument
  or command word errors (`: "${z:-"<(fi)"}"`, `f "${z:-"<(fi)"}"`,
  `echo "${a["<(fi)"]}"`), while an assignment RHS — which is not brace-expanded
  — does not (`x="${z:-"<(fi)"}"` is silent). bash only ever *parses* it: with a
  body that does parse, `echo "${z:-"<(echo hi)"}"` prints `<(echo hi)` in both
  shells, so this is a diagnostic and not a missing expansion.

  What was missing was something to hang the row on. [`Shell::gobbled_subs`]
  walks the *parse* structurally, and here the tree is right to hold characters
  — the `<(` sits in a `" … "` run inside a double-quoted operand, where neither
  bash's expander nor osh's reads one — so no part was ever going to appear for
  it. The fix is therefore not another lexer mode but a text-level pass beside
  the structural walk, as `gobbled_backtick_subs` already is for a backquote:

  * `wordscan::gobbler_procsubs(s, dquoted)` — the same flat-state loop as
    `gobbler_readable`, reporting the index of each `<(`/`>(` met while `quoted`
    is 0. (`gobbler_readable` could not answer this: it reports the stretches the
    **`$(`** row fires in, which is `quoted == 0` *and* `quoted == '"'`, and the
    `<(` row is the first of those alone.) A `$( … )` is skipped whole rather
    than reported — that is the one spelling a part already stands for.
  * `Shell::gobbled_procsubs` — for each index, lex `$(` + the rest of the word
    with `parser::dquote_word_from_source` and take the resulting
    `CmdSubBody::Unread`. The two spellings reach the same
    `extract_command_subst`, so the swap is exact, and one lex settles the body,
    the remainder and whether there was a `)` at all. It is a *lex*, not the
    paren count `gobbler_readable` skips with, because `xparse_dolparen` is a
    real parse: a `(` inside a quoted run of the body is not a nesting level to
    it, and a count would carve `echo <(echo "(")` into a body that fails.
  * The two are merged by **remainder length**: every tail the gobbler's word
    carries is measured against the whole word (`unparse::gobbler_word`), so a
    longer one is an earlier meeting. That is what keeps the interleaving right
    where a word holds both — measured, `echo "${z:-'$(fi)'"<(for)"}"` reports
    the `$(fi)` and `echo "${z:-"<(fi)"'$(for)'}"` the `<(fi)`.
  * `Shell::has_gobbled_sub` — the cheap pre-test — gained a `WordPart::Literal`
    row, answering wide (any `<(`/`>(` in a literal under quotes) so the word
    reaches the scan that settles it.

  **Verified:** `userspace/oils/tests/corpus/a-process-substitution-a-brace-scan-meets-is-read-where-the-quoting-is-clear.sh`,
  29 rows, all matching bash 5.2.37 — including the parity (`"${z:-"a"<(fi)"b"}"`
  is a *parse* error, `"${z:-"${y:-"<(fi)"}"}"` is silent), the `set +B` gate, the
  words brace expansion does not reach (assignment RHS, `case` word, here-doc
  body), the read happening before expansion (`z=Z`, `${z:+…}`), and the `declare
  -f` re-print.
* **✅ FIXED 2026-08-14 for the double-quoted operand** (`${z:-…}`, `${z:+…}`,
  `${z:=…}`, `${z:?…}` and the plain `${z-…}` family) — which is the position
  the report named, and the only one a `${x@P}`/`PS4` re-read reaches with the
  quoting bash's own expansion declines a process substitution under. The
  remaining positions are a residue of their own, logged at the end of this
  bullet. Original report: `x='${z:-<(fi)}'; echo "${x@P}"` — bash's `extract_dollar_brace_string`
  (subst.c:1881-1950) has a `<(` row of its own and recurses into it with a real
  parse, so the `@P` re-read is a `bad substitution` and the text is printed
  unchanged; osh splices the re-print and prints `<(fi)`.

  **Measured against bash 5.2.37 (2026-08-14).** The row behaves as the `$(`
  row beside it in every respect: `A${z:-<(fi)}TAIL` and `A${z:-$(fi)}TAIL`
  give byte-identical output, down to the quoted remainder `` `fi)}TAIL' ``
  and the `line 2` numbering `xparse_dolparen` gives an unread body. It is the
  scan's row and not the string's — `x='a<(fi)b'` is silent — and it is reached
  only where the scan's own quoting allows: `"<(fi)"` (double-quoted),
  `'<(fi)'` (single-quoted, `skip_single_quoted`) and `\<(fi)` are all silent
  and print their text. A body that parses is silent too and is *not*
  performed: `A${z:-<(echo A >&2)}B` prints `A<(echo A >&2)B` and no `A` on
  stderr.

  osh already matched on six of those shapes. What it got wrong:

  | written (as `x`, then `echo "${x@P}"`) | bash | osh (before) |
  |---|---|---|
  | `A${z:-<(fi)}TAIL` | reports, `bad substitution`, text | `A<(fi)TAIL` |
  | `A${z:-${y:-<(fi)}}B` | reports (nested body too) | `A<(fi)B` |
  | `A${z:-p<(fi)q$(for)r}B` | reports the **`<(fi)`** | reports the `$(for)` |
  | `A${z:-<(fi}B` | `unexpected EOF`, `bad substitution`, text | runs `fi}` — `command not found` |

  All but the last now match. The last is a *different* defect that the `$(`
  spelling has identically — see
  TD-OILS-AN-UNCLOSED-SUBSTITUTION-IN-AN-UNREAD-BRACE-BODY-IS-RUN-INSTEAD-OF-REFUSED
  below — so it was left alone here rather than fixed twice.

  **Why it was not a two-line change.** The `<(` span *is* already collected —
  `Lexer::read_dollar_brace` has the row (lexer.rs:7069) and records a
  `CmdSubSpan` with `SubOpen::Proc`, its `src`, its `range` and
  `SubBody::Unread`. What is missing is a [`WordPart`] for
  [`Shell::brace_scanned_subs`] to walk to: `procsub_reprints`
  (parser.rs:6288) splices a re-print only for a `SubBody::Eager` span, and the
  re-lex that carves the operand out of the body (`read_word_verbatim`) leaves
  a `<(` as characters on purpose. So for an *unread* body the process
  substitution survives only as text in a `WordPart::Literal`.
  `arith_unread_subs` is the shape of the answer for the arithmetic scan, and
  it excludes this spelling deliberately (parser.rs:6233-6240).

  Two things make the obvious fixes wrong, both measured above:

  * **The remainder runs past the `}`.** `` `fi)}TAIL' `` and
    `` `fi)}B${y:-<(for)}C' `` are the rest of the *whole re-read string*, not
    of the `${ … }`. So a text scan confined to the brace's own source (the
    only text [`Shell::brace_extent_scan`] is handed) cannot build the part's
    `tail`, and the `$( … )` spelling gets its own from
    `unparse::attach_comsub_tails`, which runs over the assembled word in the
    parser.
  * **It must interleave with the `$(` spelling**, in the order the one scan
    meets them — hence the `p<(fi)q$(for)r` row above.

  Reusing [`CmdSubBody::Unread`] for the synthesized part is safe for the
  *read* (the diagnostic quotes the body's remnant, never the delimiter, so a
  `<(` and a `$(` in this position are byte-identical) but not for anything
  that re-prints or *runs* one — `interp.rs:34302` performs an unread body, and
  a process substitution here is never performed. So either the part carries
  its spelling (a new field on `CmdSubBody::Unread`, two construction sites and
  one printer, plus the run site taught to refuse) or it is synthesized late
  enough that it can never escape into a print or a run — which is what
  `Shell::gobbled_procsubs` does for the `brace_gobbler` half above, and the
  reason that one could be done without touching the AST.

  **What was done.** The first of the two: the part carries its spelling, which
  makes both blockers vanish rather than needing to be worked around.

  * `ast::SubDelim { Dollar, ProcIn, ProcOut }`, with `bytes()` (the delimiter
    as written) and `is_performed()` (true only for `Dollar`). Recorded on
    `CmdSubBody::Unread` and on the lexer's `SubBody::Unread`. Only the unread
    form needs it: a body a parser *read* is a `CmdSubBody::Parsed` for `$(`
    and a `WordPart::ProcSub` for the other two, so those two shapes already
    tell the spellings apart.
  * `Lexer::read_word_verbatim` gained a `<(`/`>(` row for `Verbatim::Dquote`
    **when the text is unread** (`self.here_text`), emitting
    `Seg::CmdSub(body, close, SubBody::Unread { delim })`. The existing
    `Verbatim::Bare | Verbatim::Replacement` row above it is untouched — those
    fragments really do *perform* the substitution, measured:
    `x='A${z/p/<(echo hi)}B'; echo "${x@P}"` prints a `/dev/fd/N` in bash.
  * `unparse.rs` prints the body back in `delim.bytes()`, and
    `Shell::command_sub_body` returns that text instead of running anything
    when `!delim.is_performed()`.
  * The backslash arm of the same loop takes a `\<(`/`\>(` into the literal
    run, because the *scan* that produced this text honours a backslash
    whatever follows it (`extract_dollar_brace_string`'s `case '\\'`,
    subst.c:1899) while the operand's own dquote read does not. `A${z:-\<(fi)}B`
    prints `A\<(fi)B` and reports nothing.

  Both blockers then answer themselves: the `tail` is filled by
  `unparse::attach_comsub_tails` over the whole assembled word (so it runs past
  the `}`, giving `` `fi)}TAIL' ``), and the interleaving is
  `Shell::brace_scanned_subs`'s existing left-to-right walk.

  **Verified:** `userspace/oils/tests/corpus/a-process-substitution-a-brace-re-read-meets-is-read-like-the-dollar-spelling.sh`,
  22 rows, all matching bash 5.2.37 — the byte-identity with the `$(` spelling,
  both interleavings, the nested body, the not-performed rows (including
  `>(cat)` and a body writing to stderr, quoted and unquoted), the read
  happening before the operand is chosen (`z=Z`, `${z:+…}`), the four shields
  (unbraced text, `" … "`, `' … '`, backslash), the stepped-over subscript, and
  the `PS4` spelling of the same re-read.

* **✅ FIXED 2026-08-14** (every position but the arithmetic one; that one
  closed later the same day, at the end of this bullet)**.** The row
  was wired for the double-quoted **operand** only, and bash's scan reads the
  whole `${ … }` body — it walks characters and knows nothing of the `#`, `/`
  or `^^` it has already passed — so every other fragment wanted the same row:

  | written (as `x`, then `echo "${x@P}"`) | bash | osh before |
  |---|---|---|
  | `A${z#<(fi)}B` (pattern) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z/p/<(fi)}B` (replacement) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z^^<(fi)}B` (case pattern) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z:0:<(fi)}B` (offset) | reports ×2, `bad substitution`, text | `AB` |

  The `$( … )` spelling was right in all four (measured), so again only the row
  was missing. It was harder than the operand's, because in these positions the
  substitution is *both* read for its extent **and** performed — a replacement
  really does expand to `/dev/fd/N`, measured — so the part could not simply be
  the non-performed `CmdSubBody::Unread` the operand's is.

  **What was done.** The split `CmdSubBody` already makes between a body a
  parser read and one only a scan read is now made for the process-substitution
  part too, so one part answers for both halves:

  * `ast::ProcSubBody` — `Parsed(Program)` or `Unread { src, tail, closed }` —
    replaces the bare `Program` in `WordPart::ProcSub`.
  * `lexer::ProcRead` (`Eager` / `Unread { closed }`) rides on `Seg::ProcSub`;
    the `Verbatim::Bare | Verbatim::Replacement` arm of
    `Lexer::read_word_verbatim` picks it from `self.here_text`, and now
    tolerates a missing `)` exactly as the `$(` spelling does.
  * `parser::seg_to_part` parses only an eager body. An unread one is carried
    as text, because its read belongs to the scan and happens later, from
    where a failure is `bad substitution` rather than a script syntax error.
  * `unparse`: an unread body prints back as written, and joins
    `attach_comsub_tails` so it gets the same remainder the `$(` spelling does.
  * `interp`: `Shell::brace_scanned_subs_slice` collects it,
    `Shell::extent_read_of_subs` reads it through the same
    `comsub_reparse_read`, and the new `Shell::proc_sub_body` parses-then-
    performs at expansion — only reachable if that read succeeded.

  **Verified:** corpus case
  `a-process-substitution-a-brace-re-read-meets-is-read-wherever-in-the-braces-it-sits.sh`,
  21 rows, IDENTICAL against bash 5.2.37.

  **✅ The arithmetic fragment, 2026-08-14.** Deferred at first, because osh
  diverged over `<` in a bound before any process substitution was written at
  all (`${z:1<(2)}` is `bcdef` in bash and was an `operand expected` in osh);
  that was fixed as
  TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND,
  and this row followed.

  It was **not** simply `Verbatim::Arith`'s row, as the deferral assumed. A
  subscript shares that mode and must *not* get it: bash's scan steps over a
  subscript whole (`skip_matched_pair` from the `[`), so `${z[<(fi)]}` never
  offers its body to `extract_command_subst` and is an `operand expected` —
  which osh already matched. A bound is walked in the open. So the mode split
  in two: `Verbatim::Bound` / `Frag::Bound`, reached by `lex_bound_verbatim`
  and `parser::word_bound_from_source_at`, identical to `Arith` in every
  respect but that it takes `Dquote`'s unread-`<(` arm. That is the whole
  change — the arm was already written for the operand, and the read/perform
  split it produces (`SubBody::Unread`) is exactly a bound's: read for its
  extent by the scan, never performed, because `Q_DOUBLE_QUOTES|Q_ARITH` is
  what stops `expand_word_internal` (subst.c:11079).

  No interp-side work was needed: `unparse::nested_parts` already classifies
  `ParamSubstr`/`ArraySlice` bounds as `Nested::Operand`, so
  `Shell::brace_scanned_subs_slice` was already descending into them.

  **Verified:** 14 further rows in the same corpus case (the bound in offset
  and length position, `${a[@]:…}` and `${@:…}`, the `@P` and `PS4` spellings,
  the three quotings that shield it, and the well-formed `${z:<(echo 1)}` that
  reaches the evaluator as characters), IDENTICAL against bash 5.2.37.

**How it was found:** implementing the entry above.

### [B] TD-OILS-AN-UNCLOSED-SUBSTITUTION-IN-AN-UNREAD-BRACE-BODY-IS-RUN-INSTEAD-OF-REFUSED. `x='A${z:-$(fi}B'; echo "${x@P}"` runs `fi}` where bash reports `bad substitution` — 2026-08-14 — ⚠️ OPEN

A `$( … ` with no `)` inside a `${ … }` written in text no parser read — a
`${x@P}` re-read, a `PS4`, a here-document body. bash reads the extent with
`xparse_dolparen`, which fails at end of input; `si` is left past the end of the
string, so the brace never closes, so `parameter_brace_expand` reports
`bad substitution` naming the whole text and prints the text unchanged. Nothing
is run. osh gets the *first* diagnostic right and then runs the body anyway:

```sh
x='A${z:-$(fi}B'; echo "${x@P}"
# bash: command substitution: line 3: unexpected EOF while looking for matching `)'
#       line 1: A${z:-$(fi}B: bad substitution
#       A${z:-$(fi}B
# osh:  command substitution: line 3: unexpected EOF while looking for matching `)'
#       line 1: fi}: command not found
#       A

x='A${z:-$(echo hi}B'; echo "${x@P}"
# bash: … unexpected EOF …; … bad substitution; A${z:-$(echo hi}B
# osh:  … unexpected EOF …; Ahi}
```

Both spellings are affected identically — `<(fi}` behaves exactly as `$(fi}`,
which is the point: the delimiter is not what is wrong here.

**Where:** `userspace/oils/src/interp.rs`, [`Shell::extent_read_of_subs`]
(~29622) and [`Shell::run_abandoned_extent`]. The scan classifies the failed
read as `ExtentRead::Abandoned { body, rest }` and hands the body on to be run.
That classification is *right* for an abandoned extent bash really does run on
— it is `extract_command_subst`'s no-`)` path with the `jump_to_top_level`
suppressed — but wrong when the caller is the brace scan, because there the
unclosed read is also what stops the `}` from ever being found, and the
`bad substitution` that follows pre-empts the run.

**Proper fix:** distinguish the two callers. `extent_read_of_subs` should
report the abandonment to the brace scan (so `brace_extent_scan` fails the
whole `${ … }` and takes the `bad substitution` path with the source text)
rather than letting the body reach `run_abandoned_extent`. The `closed: false`
flag on `CmdSubBody::Unread` already names exactly this shape, so the test is
to hand.

**How it was found:** measuring the `<(` row of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN
against its `$(` twin, which turned out to be wrong the same way.

### [B] TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND

**Status:** ✅ FIXED 2026-08-14. Found 2026-08-14, measured against bash 5.2.37.
The cause turned out to be wider than the title: the two bounds were
**tokenized as a command** rather than read as arithmetic, so `<` was only the
most visible of the operators being lost. See "The fix" at the end.

A `<` in the offset or length of `${z:o:l}` swallows everything to its left.
The same expression inside a plain `$(( ... ))` is fine, so this is the brace
fragment's own reading of the text, not the arithmetic evaluator's:

| written | bash | osh |
|---|---|---|
| `z=abcdef; echo "${z:1<(2)}"` | `bcdef` | `z: <(2): syntax error: operand expected` |
| `z=abcdef; echo "${z:0:1<(2)}"` | `a` | same error |
| `echo $(( 1<(2) ))` | `1` | `1` |

bash reads `1<(2)` as `1 < (2)`, which is `1`, so the offset is 1. osh
evaluates `<(2)` alone -- the `1` is gone by the time the evaluator sees the
expression, which is what the quoted error token shows.

**Where:** `userspace/oils/src/lexer.rs`, the `Verbatim::Arith` path of
[`Lexer::read_word_verbatim`], and whatever splits a `${z:o:l}` body into its
two fragments in `userspace/oils/src/parser.rs`. The `<` is being taken for
something other than a comparison operator -- most likely a fragment boundary.

**Proper fix:** treat `<` in an arithmetic fragment as the comparison operator
it is, so the whole fragment reaches the evaluator. A `<(` there is *not* a
process substitution to be performed either -- measured, `${z:0:<(echo 1)}` is
an `operand expected` in bash with the characters `<(echo 1)` standing as the
error token, which osh already matches.

**Blocked, and then unblocked (same day):** the arithmetic-fragment row of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN.
bash's `${ ... }` scan reads a `<( ... )` in an arithmetic fragment exactly as it
reads one anywhere else in the body -- `x='A${z:0:<(fi)}B'; echo "${x@P}"`
reports the parse twice and then `bad substitution`, where osh printed `AB` --
but a corpus row for it would have been measuring this bug instead, so the
corpus case
`a-process-substitution-a-brace-re-read-meets-is-read-wherever-in-the-braces-it-sits.sh`
left that position out and said so. The fix below removed the obstacle, and the
rows went in the same day: that case now measures a bound in seven further
positions.

**How it was found:** measuring where bash's brace scan reads a `<( ... )`,
while checking whether the `Verbatim::Arith` fragments needed the same row as
the pattern and replacement ones.

**The fix (2026-08-14).** `parse_slice_bounds`
(`userspace/oils/src/parser.rs`) read each bound with `word_from_source`, which
called `tokenize(...)` — a *command* tokenizer — and then joined the surviving
`Tok::Word`s with a literal space. So every operator character was claimed by
the tokenizer instead of reaching the evaluator, and whatever it could not make
a word of was silently dropped. `<` was merely the case that produced an IO
number and a redirect. The rest, all measured against bash 5.2.37 with
`z=abcdef`:

| written | bash | osh, tokenized |
|---|---|---|
| `${z:1<2}` | `bcdef` | `cdef` — `1<` taken for a redirect |
| `${z:1>2}` | `abcdef` | `cdef` — likewise |
| `${z:1<=2}` | `bcdef` | `=2: operand expected` |
| `${z:1 < (2)}` | `bcdef` | `1 2: syntax error` |
| `${z:1;2}` | `;2: invalid arithmetic operator` | `1 2: syntax error` |
| `${z:1&2}` | `abcdef` | `1 2: syntax error` |
| `${z:3|2}` | `def` | `3 2: syntax error` |
| `${z:1&&2}` | `bcdef` | `1 2: syntax error` |
| `${z:1)}` | `1): syntax error in expression` | silently `abcdef` |

Both bounds now go through `word_subscript_from_source_at` — the very reader an
array subscript uses, which is `verbatim_word_at(..., Frag::Arith)` plus
`attach_subscript_reads`. The two arithmetic fragments therefore no longer
disagree with each other, which is what `attach_subscript_reads`'s own doc
comment had been asking for.

Two further defects of the same splitter were found while measuring it, and are
fixed in the same change:

* **Which colon cuts.** bash does not `strchr` for the `:`; `skiparith`
  (subst.c) skips one `:` for every `?` seen, and counts nothing at all inside
  a `( … )`. `${z:1?2:3}` is `cdef` (the whole text is the offset) while
  `${z:1?2:3:1}` is `c`; `${z:1?1?2:3:4}` is `cdef`, two `?` swallowing both
  colons; `${z:(1?2:3):1}` is `c`. osh split on the first `:` unconditionally
  and so reported `` `:' expected for conditional expression `` for all of
  these. Now `slice_split_colon` implements the rule.
* **An empty bounds text.** `${z:}` is `${z:}: bad substitution` in bash, and
  uniformly so — `${@:}`, `${*:}`, `${a[@]:}`, `${a[1]:}` and an unset
  parameter all report it. osh printed the whole value. It is the *text* that
  must be non-empty, not what it expands to: `${z:$e}` with `e=` is `abcdef`.
  `parse_slice_bounds` now returns `None` for an empty text and each of its
  three call sites turns that into `WordPart::BadSubst`.

Verified by the corpus case
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`
(75 rows, IDENTICAL), the lib suite and a full sweep.

**Unblocked, and then done (same day):** the arithmetic-fragment row named
under "Blocks" above was the only thing left of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN,
and it is now closed there. It was a separate row from this entry's — after
this fix `${z:1<(2)}` evaluated correctly but `x='A${z:0:<(fi)}B'; echo
"${x@P}"` still printed `AB`, where bash reads the body for its extent and
reports `bad substitution`. It turned out **not** to be the `Verbatim::Arith`
row this entry's title suggested, because the *subscript* shares that mode and
must not get it: bash's `${ … }` scan steps over a subscript whole
(`skip_matched_pair`), so `${z[<(fi)]}` never offers its body to the scan and
is an `operand expected` in bash — which osh already matched. Only a bound is
walked in the open, so `Frag::Arith` split in two and the new `Frag::Bound`
took the row. See that entry for the change.

### [B] TD-OILS-AN-UNBALANCED-PAREN-IN-A-SLICES-BOUNDS-IS-AN-ARITHMETIC-ERROR-NOT-A-BAD-SUBSTITUTION

**Status:** ✅ FIXED 2026-08-14. Found 2026-08-14, measured against bash 5.2.37.
The fix turned up a second rule of the same walk, fixed with it — see "The fix"
at the end.

`skiparith` (subst.c) balances parens while looking for the colon that cuts
`${x:off:len}` in two, and an unbalanced `(` makes it run off the end. bash
then reports that as a **bad substitution** naming the whole bounds text, before
either bound is evaluated. osh implements the balancing (that is what makes
`${z:(1?2:3):1}` cut in the right place) but not the complaint, so the text
reaches the evaluator and produces an arithmetic diagnostic instead:

| written | bash | osh |
|---|---|---|
| `${z:(1}` | ``bad substitution: no closing `)' in (1`` | ``z: (1: missing `)' (error token is "1")`` |
| `${z:(1:2}` | ``… no closing `)' in (1:2`` | ``z: (1: missing `)'`` — and it cut at the colon |
| `${z:((1:2}` | ``… no closing `)' in ((1:2`` | likewise |
| `${z:1+(2}` | ``… no closing `)' in 1+(2`` | ``z: 1+(2: missing `)'`` |
| `${a[@]:(1}` | ``… no closing `)' in (1`` | arithmetic error |
| `${@:(1}` | ``… no closing `)' in (1`` | arithmetic error |

Both are rc=1, so only the message differs — but the message differs in class,
not just wording: bash's is the DISCARD-class `bad substitution` family, raised
by the cut, and it names the bounds text rather than the parameter.

Three things scope it precisely, all measured:

* It is the **whole bounds text** that is checked, once, before the cut — the
  message quotes `(1:2` entire, the colon never having split it.
* It is only the text the *cut* walks. Once a colon has been found with the
  depth back at zero, an unbalanced `(` in the length is an ordinary arithmetic
  error: `${z:0:(1}` is ``z: (1: missing `)'`` in bash too, and osh matches.
* A stray `)` at depth zero is not an error at all: `${z:)1}` is
  `)1: syntax error: operand expected` in both.

**Where:** `userspace/oils/src/parser.rs`, `slice_split_colon` — which already
tracks the depth and would only need to report a non-zero one at the end — and
its three call sites in `parse_braced_param_in`, which currently turn the
`None` that means "empty bounds" into `WordPart::BadSubst(raw)`.

**Proper fix:** `slice_split_colon` reports the unbalanced case distinctly from
the empty one, and the call sites raise ``bad substitution: no closing `)' in
<bounds text>``. That message shape already exists in
`userspace/oils/src/interp.rs` (`b"bad substitution: no closing `)' in "`,
~35600) but it names the whole *word*, whereas this one names the bounds text
only, so it needs its own carrier on the word part rather than a reuse of
`BadSubst`, whose printer names `${…}` entire.

**Blocked:** one row of the corpus case
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`,
which said so in its header and left the shape out. Now measured there.

**How it was found:** measuring bash's slice bounds exhaustively while fixing
TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND. It
was the last of four divergences that measurement turned up, and the only one
not fixed there.

**The fix (2026-08-14).** Two things, because measuring the first turned up the
second.

**(1) The complaint.** `slice_split_colon` now returns the depth it ended at
beside the split index, `parse_slice_bounds` carries a non-zero one as
`SliceBounds::unclosed`, and both `WordPart::ParamSubstr` and
`WordPart::ArraySlice` gained an `unclosed: Option<Str>` field for it. It is a
field on the operator rather than a `WordPart::BadSubst`, because *where* it is
raised is the whole of what distinguishes the two: `${z:}` is a bad
substitution even for an unset parameter, while `${u:(1}` with `u` unset is
silently empty. So the check sits exactly where the offset would have been
evaluated — `Shell::slice_bounds_unclosed`, called from `scalar_slice`,
`assoc_slice` and the indexed path of `slice_elements_resolved`, each after its
own "nothing to measure" exit. Every ordering measured lines up: an empty
array, an empty `$@`, `set -u`, and a set-but-empty scalar (which *does* report,
having one position).

`no_longjmp_on_fatal_error` — `Shell::prompt_expanding` — **suppresses** the
complaint rather than rewording it, so under `${x@P}` or `PS4` the characters go
on to the evaluator and the arithmetic error is what comes out. That is the
`if (no_longjmp_on_fatal_error == 0)` guard the report sits behind, and it is
why osh's *old* answer was right in those two contexts and only those two.

**(2) The walk is quote-aware.** Measuring (1) showed the walk steps over a
`' … '` run, a `" … "` run and a backslash-escape whole — all three counters
included, not just the paren one. `${z:"1:2"}` does not split (the evaluator
meets `1:2` as one bound and says so), `${z:1"?"2:3}` does split (the quoted `?`
buys no colon), and `${z:0"("}`, `${z:0'('}`, `${z:0\(}` and `${z:(1"("2)}` are
all balanced. osh's walk saw none of that, so before this fix it both cut in the
wrong place and complained where bash did not. Note this is about the *walk*
only: the quote characters stay in the bound, and the arithmetic reading each
half is given removes them (or does not — a `' … '` keeps its second reading).

The walk is over the text **as written**, which the same measurement pins down
from the other side: `p="("; ${z:$p 1}` and `${z:$(echo "(1")}` are ordinary
arithmetic errors, each being balanced as written however unbalanced its value.

**Verified:** 37 further rows in
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`, the
lib suite and a full sweep.

### [B] TD-OILS-THE-WAIT-NO-OPERANDS-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP. The job holding `$!` is not spared, once per many sweeps — 2026-08-14 — OPEN

**Where:** `userspace/oils/tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`,
the group "only the last one backgrounded is spared", against
`Shell::builtin_wait`'s operand-less arm and `Shell::drain_jobs`
(`userspace/oils/src/interp.rs`).

**What — and this time the whole row was captured.** One full
`scripts/osh-bash-diff.py` sweep came back `654 matched, 0 waived, 1 failed`
with **one line** of the case different, everything else in it identical:

```sh
( exit 3 ) & ( exit 4 ) & sleep 0.4; wait; echo "  noargs=$?"
VAR=stale; wait -n -p VAR; echo "  n=$? $(pvar)"
```

| | bash 5.2.37 | osh (this sweep) |
|---|---|---|
| `noargs=` | 0 | 0 (agreed) |
| `n=` | `4 a pid` | **`127 unset`** |

So osh had nothing left to report where bash still had the last-backgrounded
job. Re-run on its own immediately after: `1 matched, 0 waived, 0 failed`.
Saved report:
`target/dvscratch/corpus-failures/20260814-145703/wait-with-no-operands-and-a-job-that-just-ended.txt`.

**What a 127 requires, read out of the code rather than guessed.** The spare is
`builtin_wait`'s operand-less arm: after `drain_jobs`, every job with a status
is marked `notified` *except* the one whose pid is `last_bg_pid`, and
`cleanup_dead_jobs` then drops exactly the notified ones. But `drain_jobs`
itself marks `notified` for every job it *waited for*, and it waits for any job
not already in its `known` snapshot — `known` being the jobs whose `exit_seen`
was set **before** the wait was reached. So the spare survives only when the
`$!` job's `exit_seen` was already set, which the unit-boundary
`cleanup_dead_jobs` does for a job that is both finished and older than
`JOB_EXIT_NOTICE_GRACE` (20 ms). A 127 means that did not happen for the `$!`
job specifically: had it been the *other* job that was late, `drain_jobs` would
have waited that one and the spare would still stand.

**The margin is not thin, which is what makes this odd.** Both shells were
measured at four margins (`build/pgS.sh`), and they agree exactly:

| `sleep` before the `wait` | bash | osh |
|---|---|---|
| none | `127 unset` | `127 unset` |
| 0.01 | `4 a pid` | `4 a pid` |
| 0.05 | `4 a pid` | `4 a pid` |
| 0.4 | `4 a pid` | `4 a pid` |

The flip is between 0 and 0.01, so the case's `sleep 0.4` is a ~40x margin — not
the ~1x margin that
TD-OILS-THE-COMPGEN-JOB-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP turned out to
be. **Do not assume the same diagnosis and just widen the sleep.**

**Loads that do NOT reproduce it — do not spend the time again.** The job is
thread-backed, not a process (`( exit 4 ) & echo $!` prints the synthetic
`900000`, where `sleep 0.4 &` prints a real pid), so both of the obvious
starvation stories were tried and neither bit:

- 20 serial runs of the group alone: clean.
- 119 runs of the group at 8-way concurrency: clean.
- 64 runs of the *whole case* at 8-way concurrency: clean.
- 36 runs under a process-spawn storm (6 loops spawning `osh -c :` and
  `bash -c :` back to back, this host's documented ~200-290 ms spike source):
  clean.
- 30 runs under CPU saturation (24 busy-loop processes on 12 cores): clean.

Probes are `build/repro-wait.sh` (the group), `build/repro-wait2.sh` (the whole
case), `build/spawnstorm.sh`, `build/cpuburn.py` — all in the gitignored
`build/`, so re-create them from this entry if they are gone.

**Proper fix.** Unknown, and deliberately not guessed at. The next sighting
should establish which of the two conditions failed — whether the `$!` job's
body was genuinely unfinished at the unit-boundary poll, or whether the poll did
not run — by instrumenting `poll_jobs` to record, per job, `is_finished` and
`born_at.elapsed()` at each call, and dumping that when `wait -n` answers 127.
That distinguishes "the thread really was 400 ms late" from a bookkeeping fault,
and only the first is a case-margin problem.

**Impact.** An intermittently red sweep, which is the gate on every commit —
and the sweep takes ~19 minutes, so a re-run to disambiguate is expensive.

**Sighting 2026-08-14, in the *unit* suite, and fixed there.**
`interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported` failed
once under `cargo test -p oils --lib` (`wait -n` answered 127 where 3 was due,
i.e. the operand-less `wait` had *not* spared the job) and passed when re-run
alone. Same shape as this entry, but with a cause the test owned: it backgrounded
`( exit 3 ) &` and then slept a constant `0.2` to make the job finish first, and
no constant is long enough to promise that on a loaded machine. Fixed properly
rather than by lengthening the sleep — a new `settle_jobs` test helper (the
whole-table form of the existing `settle_job`) polls `poll_jobs` until every job
has a status, after the same `JOB_EXIT_NOTICE_GRACE`. That removes this test from
the flaky family; the *corpus* case above is untouched and stays open.

---

### TD-OILS-AN-ARITHMETIC-SCAN-REPORTS-NONE-OF-THE-READS-IT-MAKES. `$(( … ))` swallows the diagnostics its nested `$( … )` should raise, and loses the text after a read that stopped early — 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — `Shell::arith_extent_expand` /
`arith_extent_frame` and the `$((` route out of `Shell::arith_extent_route`.

**What is wrong.** `param_expand` reaches a `$((` through
`extract_command_subst` with `SX_COMMAND` (subst.c:10575), so the paren count
*does* recurse into a nested `$( … )` — a real parse, reported where it is met.
osh runs the count but never reports, and in one shape stops in the wrong place.
Measured against bash 5.2.37 (`build/pgX.sh` rows a/c, `build/pgY.sh` d4/d5):

| word (inside `v='…'`, via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$((1+$(echo hi⏎q` | reports EOF, `[A]` | reports EOF, `[Ahi]` |
| `A$((1+$(for⏎q))B` | reports **twice** (`for`, then `` `(1+$(for' ``), `[AB]` | silent, `[A]` |
| `A$((1+$(for⏎xB` | reports `for`, `[A]` | reports `for`, **runs `fo`**, `[A⏎xB]` |

Rows 1 and 3 report because the read runs from `Shell::arith_nested_read`,
which does call `Shell::comsub_reparse_read`; what those two get wrong is the
*value*, both by performing the abandoned extent the way the string level does
and the brace level does not. Row 2 is the substantive one: the read stopped
part way, so bash's count resumed after the `for`'s line and found the `))`,
leaving `B` to the word. osh consumes to the end and loses it — and so never
reaches the read at all, which is why it is the one row that is also silent.

**What the proper fix looks like.** The `$((` count needs the same two-outcome
treatment `${ … }` got on 2026-08-14: `Shell::comsub_reparse_read` for the
report (which also decides jump vs. no-jump), and
`Shell::failed_extent_split`'s resume point for where the count carries on.
`Lexer::unread_comsub_stop` already puts the lexer in the right place; what is
missing is the interp half — an `arith`-side counterpart of
`Shell::unclosed_brace_reads`.

**Impact.** Diagnostics only for two of the three rows; a wrong value for the
third. Needs `@P`/`PS4`/here-doc text to be reachable at all.

---

### TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN. A `<(`/`>(` in it is never read, though the brace scan names it — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — `Shell::extent_read_of_rest` and
`Shell::unclosed_brace_reads`, both of which lex their text with
`crate::parser::dquote_word_from_source` → `crate::lexer::lex_dquote_body`.

**What is wrong.** `extract_dollar_brace_string` names `$(`, `<(` and `>(`
together and hands each to the same `extract_command_subst` (subst.c:1881-1950),
**whatever the quoting** — that is why `x='A${z#<(fi)}B'` reports the parse
twice. A double-quoted *run*, by contrast, has no process substitution in it at
all: at string level bash and osh agree that `v='A<(echo hi⏎q'` is literal
text. So `lex_dquote_body` is the right lexer for a string-level remainder and
the wrong one for text the **brace scan** is walking.

Measured (`build/pgY.sh` d6), `A${z:-P1<(echo hi⏎S1}B` under `${…@P}`:

| | bash 5.2.37 | osh |
|---|---|---|
| reports | `` unexpected EOF while looking for matching `)' `` **then** `…: bad substitution` | the `bad substitution` only |
| value | undecoded word | same |

The `$(` spelling of the same row (`build/pgW.sh` row 5) is byte-exact, so this
is precisely the two openers `lex_dquote_body` cannot see. The dollar spelling
of d7 — where the read stops early and the brace closes — is also exact,
because that path re-lexes through `parse_braced_param_in` in
`Quoting::Unread`, which *does* read them.

**It is not only the two openers — the whole quote model is wrong** (measured
2026-08-14, `build/pq1.sh` and `build/pq2.sh`). `extract_dollar_brace_string`
**skips** a quoted run rather than walking it, and the two quotes skip
differently:

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | what it shows |
|---|---|---|
| `A${z:-P1<(echo hi⏎S1}B` | reports EOF, `bad substitution` | the bare `<(` row **is** read |
| `A${z:-P1"<(echo hi⏎S1"}B` | silent, `[AZZB]` | a `<(` inside `" … "` is **not** |
| `A${z:-P1"$(echo hi⏎S1"}B` | reports EOF, `bad substitution` | a `$(` inside `" … "` **is** |
| `A${z:-P1'$(echo hi⏎S1'}B` | silent, `[AZZB]` | a `$(` inside `' … '` is **not** |
| `A${z:-P1'<(echo hi⏎S1'}B` | silent, `[AZZB]` | …nor a `<(` |
| `A${z:-P1"<(echo hi⏎S1}B` | `bad substitution`, **no** read report | a lone `"` swallows to end of string |
| `A${z:-P1'<(echo hi⏎S1}B` | `bad substitution`, **no** read report | …and so does a lone `'` |
| `A${z:-"x"<(echo hi⏎S1}B` | reports EOF, `bad substitution` | a *closed* run does not suppress what follows |
| `A${z:-P1\<(echo hi⏎S1}B` | silent, `[AZZB]` | a backslash escapes the opener |

So the brace scan delegates a `" … "` run to a double-quote skipper that has
the `$(` row and **not** the `<(`/`>(` row — bash's ordinary rule that there is
no process substitution inside double quotes — and skips a `' … '` run whole,
offering its interior to nothing.

`lex_dquote_body` models neither — it treats both quote characters as ordinary
literals, which is correct for `Q_DOUBLE_QUOTES`, where the string *is* already
the quoted run. Measured (`build/pq1.sh`, `build/pq2.sh`, `build/pq3.sh`), osh
nevertheless agrees with bash on every *quoted* row above, by a different
mechanism in each case: where the run closes, the brace closes too and the word
goes through `parse_braced_param_in`, which does model quotes; where the run does
not close, `lex_dquote_body`'s missing `<(` row happens to suppress the same read
bash's skip suppresses. Two rows were left where the mechanisms did not coincide;
the first of them is now fixed:

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | osh |
|---|---|---|
| `A${z:-P1"$(echo hi⏎S1}B` | reports EOF, `bad substitution`, undecoded | ✅ same since 2026-08-14 |
| `A${z:-'p$(echo hi'q$(fi⏎S1}B` | reports `fi`, `[AZZB]` | silent, `[AZZB]` |

Row 1 was the serious one — a **spurious command execution**: osh reported the
EOF, then ran `S1}` and produced `[Ahi]`. A lone `"` opens a run that swallows to
end of string, leaving the brace nothing to close on, so bash condemns the word;
osh instead let the failed read out of `read_opaque_span`'s `"`-run `$(` sub-arm,
where [`Lexer::unclosed_seg`] degraded the whole word into a *string-level*
`$( … )` and then performed it. Fixed 2026-08-14 by giving that sub-arm
(`userspace/oils/src/lexer.rs`, `read_opaque_span`'s `'"'` arm) the same
`Err(e) if self.unread_comsub(&e)` recovery the two `read_dollar_brace_body`
arms already had: re-emit the `$(` into the raw text, take back what the reader
consumed with `Lexer::unread_comsub_stop`, and `continue` the quoted-run loop.
The read is still reported — it happened — and the run then swallows the rest,
so the brace never closes and the word is condemned, exactly as in bash. The bug
was **pre-existing**, not a regression: measured identical on the commit before
the earlier 2026-08-14 brace-scan fix.

Row 2 is a lost diagnostic only; the same row before the brace-scan fix had the
wrong value *and* ran `f`, so it is much improved.

**A second mechanism loses the same report where the brace *does* close**
(measured 2026-08-14, `build/pr1.sh` r3). `A${z:-'i"t'<(fi⏎S1}B` reports `fi`
in bash and expands to `[AZZB]`; osh now gets the value right (it was the
undecoded word until the unmated-`"` fix of the same day) but still says
nothing. That path never goes near `extent_read_of_rest`: the brace closed, so
the reads are replayed off the *parsed operand*, and the operand lexer is
`read_word_verbatim` in [`Verbatim::Dquote`] — which has a perfectly good `<(`
row, but never reaches it, because the `"` inside the `' … '` run opens a
quoted run that swallows `t'<(fi⏎S1` whole.

Both scans are right about their own text and wrong about each other's, which
is the shape of the whole issue: bash runs **two** passes over these bytes with
**different quote rules** — `extract_dollar_brace_string`, where a `'` run is
skipped and a `"` is a quote, and `expand_word_internal`, where a `'` is an
ordinary character and a `"` is a quote. osh derives the reads from the
expansion's lex in one path and from a string-level lex in the other, and
neither is the scan's.

**What the proper fix looks like.** A real lex entry for "text a brace scan is
walking" — not `lex_dquote_body` with a row bolted on, and not the operand lex
either. It needs, at its own level: the `<(`/`>(` openers beside `$(`; a `'`
that consumes to the next `'` or to end of string, offering nothing inside it;
and a `"` that consumes to the next `"` or to end of string, offering only `$(`
(and `` ` ``) inside it. A backslash hides the byte after it. Then
`extent_read_of_rest`, `unclosed_brace_reads` **and `brace_extent_scan`** all
take their reads from that one pass, `lex_dquote_body` keeps its current
string-level callers unchanged — the p1/p2 probe above confirms those answers
are right as they stand — and the operand lex stops being asked a question it
was never answering.

These rows are the acceptance test the table above does not already cover — the
ones that pin *which* quote wins when the two are interleaved (measured
2026-08-14 against bash 5.2.37, `build/pr1.sh`):

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | osh today |
|---|---|---|
| `A${z:-"it's"$(fi⏎S1}B` | reports `fi`, `[AZZB]` | same |
| `A${z:-"it's"<(fi⏎S1}B` | reports `fi`, `[AZZB]` | same |
| `A${z:-'i"t'<(fi⏎S1}B` | reports `fi`, `[AZZB]` | ✅ same since 2026-08-14 |
| `A${z:-P1\'<(echo hi⏎S1}B` | reports EOF, `bad substitution` | ✅ same since 2026-08-14 |
| `A${z:->(echo hi⏎S1}B` | reports EOF, `bad substitution` | ✅ same since 2026-08-14 |
| `A${z:-${y:-<(fi⏎S1}B` | reports `fi`, `bad substitution` | same |

So a `'` inside a closed `" … "` run opens nothing (rows 1-2) and a `"` inside a
closed `' … '` run opens nothing (row 3) — each quote is invisible inside the
other's run — and a backslash spends itself on the quote it precedes, leaving
the `<(` after it live (row 4).

An attempt that added only the `<(`/`>(` row to `lex_dquote_body` was written
and reverted on 2026-08-14, before being compiled, because these measurements
showed it would have regressed the three suppressed rows above (they are silent
in bash today and in osh today, and would have started reporting).

**Fixed 2026-08-14**, along the lines above. Three pieces:

- `Lexer::brace_scan` (`userspace/oils/src/lexer.rs`) — a flag saying "this
  scan stands in for `extract_dollar_brace_string`, not for the expansion after
  it". With it set, `read_double_quote_until` grows the scan's other two
  openers: a `<(`/`>(` becomes a `SubBody::Unread` segment carrying its own
  `SubDelim`, which the expansion prints straight back
  (`SubDelim::is_performed` is false for both), so the word's **value** is
  untouched and only the extent walk gains a construct to read. The new entry
  `lexer::lex_brace_scan_body` → `parser::brace_scan_word_from_source` is what
  `Shell::extent_read_of_rest` now lexes its remainder with, which is the
  unclosed-brace half (rows 4-5 of the interleaving table above).
- The closed-brace half (row 3) is the same flag turned on from
  `read_word_verbatim`'s `"` arm, and **only** when that run opened inside a
  `' … '` one — `in_run && self.here_text`. That is exactly the case where the
  scan never saw a quote at all, because it stepped over the single quotes
  whole. Outside a run the `"` is the scan's own, and there `skip_double_quoted`
  reads the `$(` spelling alone, which is what the reader already did.
- The quote state itself moved out of the lexer and into the walk, as
  `ScanQuote` (`interp.rs`): two independent flags, because
  `skip_single_quoted` hunts for a `'` and `skip_double_quoted` for a `"` and
  neither knows the other character — so each quote is an ordinary byte inside
  the other's run. `Shell::brace_scanned_subs_slice` tracks both over the
  literal runs (a `\` still hides the byte after it) and suppresses the two
  process-substitution spellings inside a `" … "` while letting `$(` through;
  `brace_scanned_subs_in` no longer resets the state on entering a
  `WordPart::DoubleQuoted` whose `"` the scan never saw.

Corpus case:
`userspace/oils/tests/corpus/the-brace-scan-reads-a-process-substitution-and-the-expansion-after-it-does-not.sh`
— 14 shapes plus a here-document body, byte-identical to bash 5.2.37 including
stderr.

**Impact while it stood.** Diagnostics only — the values already agreed. A
`<(`/`>(` at brace level lost its read report. The worst shape — a lone `"`
before a `$( … )` making osh run a command bash does not, and yield the wrong
value — was fixed earlier the same day (see row 1 of the two-row table above).
Reachable only through `@P`/`PS4`/here-doc text holding a malformed `${ … }`.

**Not fixed by this, and tracked separately:** row 2 of the two-row table,
`A${z:-'p$(echo hi'q$(fi⏎S1}B`. That one is not about the openers but about
where a construct *ends*; see
`TD-OILS-A-SQUOTE-RUN-DOES-NOT-CUT-A-SUBSTITUTION-SHORT-FOR-THE-BRACE-SCAN`.

---

### TD-OILS-AN-UNMATED-DOUBLE-QUOTE-GROWS-A-MATE-WHEN-THE-WORD-IS-PRINTED-BACK — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/unparse.rs` — `part_src`'s
`WordPart::DoubleQuoted` arm, which writes a `"` on both ends unconditionally;
the run that has no closing `"` is built by `userspace/oils/src/lexer.rs`,
`Lexer::read_word_verbatim`'s `'"'` arm under `ParseOpts::tolerant`.

**Repro** (bash 5.2.37, `build/pr11.sh` t1):

```sh
z=ZZ
v='A${z:-'"'"'i"t'"'"'$(fi)}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| remainder quoted by the read | `` `fi)}B' `` | `` `fi)"}B' `` |
| word named by `bad substitution` | `A${z:-'i"t'$(fi)}B` | `A${z:-'i"t'$(fi)"}B` |
| value | `[A${z:-'i"t'$(fi)}B]` | same |

**What is wrong.** In text no parser read, a `"` with no mate is not an error:
`string_extract_double_quoted` is handed a *finished word* and its walk ends at
the end of the string as readily as at a quote (that is
`ParseOpts::tolerant`, and the corpus case
`a-double-quote-with-no-mate-in-an-operand-runs-to-the-end-of-the-operand.sh`
pins the expansion of it). The resulting `WordPart::DoubleQuoted` therefore
covers a run whose closing quote **was never in the source** — but the part
does not record that, and `part_src` prints the pair back. Every consumer of
`crate::unparse::word_src` then sees one byte that was not in the word.

The value is unaffected, because quote removal drops the `"` either way. What is
affected is everything derived from the *text*: `Shell::bad_sub_word` (the word
`bad substitution` names), the tail `extract_command_subst` quotes back in its
own diagnostic, and — in principle, though no divergence has been measured for
it yet — `crate::wordscan::word_fault`, which re-scans `word_src` for the
unclosed `${`/`` ` `` verdicts and could be pushed either way by a stray quote.

The single-quote analogue exists in the same shape:
`Lexer::read_single_quote` has a `None if self.opts.tolerant => return Ok(s)`
arm, and `part_src`'s `WordPart::SingleQuoted` likewise writes both `'`s. No
divergence has been measured for it, because the paths that produce an unmated
`'` do not currently reach a diagnostic that prints the word back — but the
defect is the same one and a fix should cover both.

**What the proper fix looks like.** Record the missing mate on the part rather
than guessing at print time: `Seg::Dq(Vec<Seg>)` → `Seg::Dq(Vec<Seg>, bool)`
and `WordPart::DoubleQuoted(Vec<WordPart>)` → a `closed` field, exactly as
`Seg::Sq(Str, bool)` already carries its own flag, with `part_src` writing the
trailing quote only when it was there. About 27 sites mention `DoubleQuoted`
across `ast.rs`, `parser.rs`, `interp.rs` and `unparse.rs`; most are matches
that need only a `..`. The single-quote half is the same edit on
`WordPart::SingleQuoted`.

Not worth reaching for a cheaper trick: an unmated run always extends to the
end of its text, so "omit the quote when the part is last" would be *nearly*
right, and nearly-right quoting is how a word stops re-parsing.

**Fixed 2026-08-14**, along the lines above. `read_double_quote_until` now
reports whether a `"` really ended the run — it has exactly two `Ok` returns,
one per case, so the flag falls straight out of the existing control flow — and
that rides on `Seg::Dq(Vec<Seg>, bool)` into
`WordPart::DoubleQuoted { parts, closed }`. `part_src` writes the trailing quote
only when `closed`. The single-quote half is the same edit:
`read_single_quote`'s tolerant arm answers `false`, `Seg::Sq` became a struct
variant `{ text, escaped, closed }` rather than grow a second unnamed `bool`,
and an unmated run prints as `'` + text instead of going through
`sh_single_quote`, whose whole job is to supply the mate.

Two returns needed thought rather than transcription: the pair inside
`read_double_quote_until` that end the run on an *unclosed construct* absorbed
into a `Seg::Unclosed` answer `false`, since the run ended on the construct and
not on a quote; and the backslash spelling of `Seg::Sq` is unconditionally
`closed: true`, having no quotes to match.

Corpus case:
`userspace/oils/tests/corpus/a-double-quote-with-no-mate-does-not-grow-one-when-the-word-is-printed-back.sh`
— 8 shapes including `PS4` and a here-document body, byte-identical to bash
5.2.37 including stderr.

**Impact while it stood.** Diagnostics only — one spurious `"` in the two lines
bash prints for a malformed `${ … }` whose operand holds a `"` opened inside a
`' … '` run. Reachable only through `@P`/`PS4`/here-doc text.

---

### TD-OILS-AN-UNMATED-SQUOTE-IN-A-SUBSCRIPT-LOSES-ITS-QUOTE-BYTES-FROM-THE-WORD-PRINTED-BACK — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `attach_subscript_reads` (`userspace/oils/src/parser.rs`), which gives
each top-level `' … '` of an arithmetic fragment its interior parse.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
declare -A m=([k]=V)
echo "[${arr['x${m:-']}]"
```

bash names `` 'x${m:-' `` — the whole fragment, quotes included. osh named
`x${m:-` — the interior of the run alone.

**Cause, which was not the one first written here.** The first note guessed the
text came from `crate::unparse::word_src` by way of `crate::wordscan::word_fault`.
It does not: `word_fault` returns `None` for these words, and the word source osh
builds is byte-correct. The diagnostic comes from `Shell::expand_unclosed` on an
`Unclosed::BadSubst` whose `text` the *interior's own lexer* filled in with
`Lexer::whole_text` — the interior being a string of osh's making. bash has no
such string: an arithmetic fragment is expanded with `Q_DOUBLE_QUOTES` set, which
switches the single quote off, so `expand_word_internal` walks straight through
the pair and the string it was handed is the fragment. Both "no closing"
reporters echo that string (`report_error (…, string)`, subst.c:1498 for
`$[ … ]`, subst.c:1972 for `${ … }`).

That also explains the shape the note found puzzling — a name that begins one
byte late and ends two bytes early is exactly the interior of a `' … '` run.
There were not two faults there, but there is a second one beside it; see
`TD-OILS-A-BRACE-WHOSE-NAME-SCAN-RUNS-OFF-A-FRAGMENT-TAKES-THE-OTHER-DIAGNOSTIC`.

**Fix.** `attach_subscript_reads` already re-measures the fragment after parsing
an interior — that is what `crate::unparse::attach_comsub_tails` does for a
`$( … )`'s echoed remainder. It now also re-*names*: a new
`name_unclosed_after_the_fragment` walks the interiors it just attached and gives
every top-level `WordPart::Unclosed(Unclosed::BadSubst { text, .. })` the
fragment's source for its `text`. Only the run's own level is renamed; a `" … "`
inside the interior is carved out by `string_extract_double_quoted` as its own
string and keeps naming itself, as one written a character to the left of the `'`
would. `src` is left alone — it is the construct's spelling for a re-print, not a
diagnostic's `%s`.

Corpus:
`a-construct-left-open-in-a-quoted-subscript-names-the-fragment-around-it.sh`
(seven rows: a `${ … }` body scan running off, the same with text after the run,
a `$[ … ]`, both substring bounds, and a run that closes nothing early).

---

### TD-OILS-A-BRACE-WHOSE-NAME-SCAN-RUNS-OFF-A-FRAGMENT-TAKES-THE-OTHER-DIAGNOSTIC — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `Shell::expand_unclosed` (`userspace/oils/src/interp.rs`) and the
`Unclosed::BadSubst` the lexer raises for it (`userspace/oils/src/lexer.rs`).

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
declare -A m=([k]=V)
echo "[${arr['x${m']}]"
```

| | |
|---|---|
| bash | `` 'x${m': bad substitution `` |
| osh | ``bad substitution: no closing `}' in 'x${m'`` |

The same string is named — that much was fixed the same day — but it is the
wrong one of bash's two messages.

**Why bash has two.** A `${ … }` in a string is read in two steps, and only the
second one is `extract_dollar_brace_string`. First `parameter_brace_expand`
extracts the *name* with `string_extract (string, &t_index, "#%^,~:-=?+/@}",
SX_VARNAME)` (subst.c:9550), which stops at one of those operator characters or
at the end of the string — `SX_VARNAME` stepping over a whole `[ … ]` subscript
on the way. If it stopped at the end, `c` is `NUL` and the `switch (c)` falls to
`default: case '\0': bad_substitution:` (subst.c:10018-10024), which is
`report_error (_("%s: bad substitution"), string)` and no longjmp. Only if it
stopped at an *operator* does the body go to `extract_dollar_brace_string`, whose
own running-out is the "no closing" one that longjmps (subst.c:1972).

So the two messages divide on whether the unclosed brace got as far as an
operator, and the division is visible:

| fragment | bash |
|---|---|
| `'x${m'` | `` 'x${m': bad substitution `` |
| `'x${#m'` | `` 'x${#m': bad substitution `` |
| `'x${m[0]'` | `` 'x${m[0]': bad substitution `` |
| `'x${m['` | `` 'x${m[': bad substitution `` |
| `'x${m:-'` | ``no closing `}' in 'x${m:-'`` |

**Two things the entry got wrong, found while fixing it.**

*It is not only a fragment.* A here-document body takes the same two messages,
and osh had the same one answer for both — `cat <<E`/`a${m b`/`E` is
`a${m b⏎: bad substitution` in bash. The `${x@P}` case really does collapse
(`no_longjmp_on_fatal_error` makes `extract_dollar_brace_string` return `NULL`
quietly and its caller fall to the same label), which is why the divergence
looked narrower than it was.

*The name scan is not the whole story.* Two checks between it and
`extract_dollar_brace_string` also reach `bad_substitution:` with an operator
already found — `valid_brace_expansion_word` on the name (subst.c:9803) and the
length branch's `string[sindex-1] != RBRACE` (subst.c:9687). So `'x${m[a:b'`
(the `:` *is* reached, but `m[a` is no name) and `'x${#q:-'` are both plain bad
substitutions. A third check, `parameter_brace_expand_indir` (subst.c:9807),
runs there too and reports in the missing brace's place: `a${!nosuch:-b` is
`nosuch: invalid indirect expansion`, and a pointer holding `not a name` is
`not a name: invalid variable name`.

**The fix.** None of this needed new state on `Unclosed::BadSubst`. osh already
had the whole decision procedure — `Shell::unterminated_brace_kind`, written for
the arithmetic-string scanner, which answers `BadSub` / `NoClosing` /
`Indir(name)` from the body text alone and has `Shell::arith_indir_resolves`
beside it for the third. `Shell::expand_unclosed` now asks it, for `close ==
'}'`, before anything else it does, and a new `Shell::unclosed_bad_substitution`
reports the `BadSub` answer naming `text` (bash's `string`) with the
`ErrexitOrPosix` class the `bad_substitution:` label carries.

Asking it *first* matters, and is bash's own order: a `$( … )` written inside
the name is walked over by `string_extract` without being parsed, so
`a${m$(fi) b` names the bad substitution and never mentions the `fi` — where osh
used to run `Shell::unclosed_brace_reads` first and report the `fi`.

**Fixed by:** `Shell::expand_unclosed` + `Shell::unclosed_bad_substitution`
(`userspace/oils/src/interp.rs`). Corpus:
`a-brace-whose-name-scan-runs-off-the-text-is-a-bad-substitution-not-a-missing-brace.sh`
— sixteen shapes covering the fragment, the here-document, the command
substitution in each half, all three indirection outcomes and the prompt
collapse, byte-identical to bash 5.2.37 including stderr.

---

### TD-OILS-AN-UNCLOSED-ARITH-SUBSTITUTION-IN-A-QUOTED-SUBSCRIPT-IS-NOT-CAUGHT-BEFORE-EXPANSION — 2026-08-14

**Where:** `crate::wordscan` (`userspace/oils/src/wordscan.rs`), the word-extent
pass `Shell::begin_word` runs before a word is expanded.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
echo "$(touch RAN)[${arr['x$(( 1+ ']}]"
```

bash prints ``bad substitution: no closing `)' in "$(touch RAN)[${arr['x$(( 1+ ']}]"``
— the **whole word** — and `RAN` is never created. osh prints
``bad substitution: no closing `)' in 'x$(( 1+ '`` — the fragment — and the
`touch` runs.

The side effect is the real defect; the name follows from it. bash reaches this
one on the *extent* pass, before any part of the word expands, so it names the
string that pass was walking. osh reaches it only when the subscript is expanded,
by which time the substitution ahead of it has already run.

**Not the same as the two entries above.** Those are about which string a fault
found *during* the fragment's expansion names. This one is about a fault bash
finds before expansion starts and osh does not find at all until later.

**What the proper fix looks like.** `wordscan::scan` has rows for `${`,
`` ` ``, `$(`, `$[` and `<(`/`>(`, and its faults are `WordFault::Brace` and
`WordFault::Backquote`. An unclosed `$((` inside a `' … '` in a subscript is a
third: `extract_delimited_string`'s (subst.c:1498), which names the scanned
string and closes it with `)`. Adding it means teaching `word_fault` a fault that
carries its own closing delimiter, and teaching the subscript skip that a `'` in
there does not hide a `$((` from the enclosing scan.

**Impact.** A command substitution written before such a subscript runs when bash
would not have run it. Narrow, but it is a side effect and not just text.

---

### TD-OILS-A-BACKQUOTE-IN-A-QUOTED-SUBSCRIPT-IS-A-PARSE-ERROR-WHERE-BASH-EXPANDS — 2026-08-14 — ✅ FIXED 2026-08-14 (in-scope half; see the scope note at the end)

**Where:** the `' … '` interior parse of an arithmetic fragment —
`attach_subscript_reads` (`userspace/oils/src/parser.rs`) and the lexer path
behind it.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
echo "[${arr['x`fi']}]"
echo TAIL
```

| | |
|---|---|
| bash | ``bad substitution: no closing "`" in `fi'`` at line 2, then `TAIL` |
| osh, before | ``unexpected EOF while looking for matching `` ` ``'`` at line 4 — the script never runs |
| osh, now | identical to bash |

osh turned a runtime diagnostic into a *parse* error, so the whole script was
rejected. bash's parser stops at the `'` and resumes at its mate, so the
backquote inside is text as far as any parse is concerned; it is met only by
`param_expand`'s own `string_extract (…, SX_REQMATCH)` at expansion time
(subst.c:11269), which names `string + t_index` — the text from the backquote on.

**The fix.** Three parts:

- `Lexer::read_word_verbatim`'s `` ` `` arm used a bare `?`, which let the
  `LexError` escape as a parse error. It now converts to an `Unclosed::Backquote`
  segment via `unclosed_seg`, exactly as the `$` arm does for an unmatched `${`.
  This is the part that stopped the script being rejected.
- `Unclosed::Backquote` gained a `text` field, because its `%s` is
  `string + t_index` and not `string`: the report runs from the backquote to the
  end of the **fragment**, whereas `src` is also what `part_src`/`parts_src`
  re-print and so cannot be widened in place.
- `name_unclosed_after_the_fragment` (`parser.rs`) widens that `text` with the
  fragment tail past the run's interior — the run's own closing quote and
  whatever follows it — mirroring what it already did for `BadSubst`.

**Verified.** `userspace/oils/tests/corpus/an-unmated-backquote-in-a-quoted-subscript-is-met-at-expansion-and-not-by-a-parse.sh`
is byte-identical to bash 5.2.37, as are probes `build/pr28.sh` and
`build/pr29.sh`. Full sweep green.

**SCOPE: one residue is out of frozen scope (§305) and is deliberately left
unfixed.** Where the unmated backquote sits inside a *nested double quote* within
the run — `build/pr30.sh` d2, `echo "[${arr['x"`fi"']}]"` — bash reports
``no closing "`" in `fi"'`` and osh reports ``no closing "`" in `fi"``: osh is one
trailing `'` short. Everything else matches, including the script surviving, the
exit status and all other output. The cause is known:
`name_unclosed_after_the_fragment` visits only the run's own top level and does
not descend into a nested `DoubleQuoted` part (`crate::unparse::nested_parts_mut`
would give the descent; note its `SingleQuoted { .. }` arm returns `Vec::new()`,
so it can only supplement the outer loop, not replace it).

This is **the exact substring an error message echoes**, which design-decisions
§305 names as out of scope: nothing SlateOS runs will ever depend on it. Fix it only
if it turns up as part of something that does. The in-scope half of this
entry — a whole script being rejected where bash runs it — is closed.

**Fixed by:** the corpus case named above, plus `lexer.rs` (`Unclosed::Backquote`
`text` field, `read_word_verbatim`'s `` ` `` arm), `interp.rs`
(`Unclosed::Backquote` report) and `parser.rs`
(`name_unclosed_after_the_fragment`).

### TD-OILS-A-SQUOTE-RUN-DOES-NOT-CUT-A-SUBSTITUTION-SHORT-FOR-THE-BRACE-SCAN. A `$( … )` opened inside one swallows the read that should have followed it — 2026-08-14

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_word_verbatim`'s `$`
arm in [`Verbatim::Dquote`], reached through
`Shell::brace_extent_scan` → `Shell::brace_scanned_subs`.

**Repro** (bash 5.2.37, `build/pr12.sh`):

```sh
z=ZZ
v='A${z:-'"'"'p$(echo hi'"'"'q$(fi
S1}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| reports | ``syntax error near unexpected token `fi' `` | **nothing** |
| value | `[AZZB]` | same |

**What is wrong.** The two passes bash makes over this word carve it into
*different constructs*, not merely read the same constructs differently.

- `extract_dollar_brace_string` meets the `'` and hands the run to
  `skip_single_quoted`, which stops at the **mate**. So `'p$(echo hi'` is one
  skipped run, the `$(` inside it is never seen at all, and the scan resumes at
  `q` — where it meets `$(fi⏎S1}B`, reads it, and reports `fi`.
- `expand_word_internal` has no `'` left to speak of, so its
  `string_extract_double_quoted` meets the **first** `$(`, hands the rest of the
  word to `extract_command_subst`, and — there being no `)` anywhere — takes
  everything. One substitution, not two.

osh derives the brace scan's reads from the expansion's lex, so it gets the
second carving and the second `$(` is inside the first's body, where the walk
never reaches it. `Shell::brace_scanned_subs_slice`'s single-quote bookkeeping
then correctly suppresses the one construct it *can* see (it is inside the run),
and the result is silence.

This is the residue of
`TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN`, which
fixed the part of the same disagreement that was only about *which openers*
count. Rows where the two passes agree on the extents but not on the openers are
now handled by `Lexer::brace_scan`; this row is one where they disagree on the
extents, and no flag on the expansion's lex can express it.

**What the proper fix looks like.** `Shell::brace_extent_scan` has to run over
the brace's **text**, with the scan's own carve, rather than over the parsed
part. Concretely: keep the undecoded source of an unread `${ … }` on the part
(or reach it through `crate::unparse`), and lex it once in
`Lexer::brace_scan` mode with the single-quote rule the scan really has — a `'`
consumes to its mate and offers nothing inside, so a `$(` in there can neither
be read nor run past the mate. `read_word_verbatim` already computes that mate
(`sq_close`); what it does not do is let it bound a substitution, because for
the *expansion* it must not.

Note that `Lexer::brace_scan` as it stands is deliberately the narrow version:
it adds openers and leaves extents alone. Widening it to bound a `$( … )` at
`sq_close` would be wrong for the same lexer's expansion duty, so the widening
has to come with the second pass, not instead of it.

**Impact.** Diagnostics only — the value is already right. Reachable only
through `@P`/`PS4`/here-doc text holding a `${ … }` whose operand has both an
unterminated `$( … )` inside a `' … '` run and a failing one after it.

---

### TD-OILS-A-DOLLAR-BRACKET-BOUND-DOES-NOT-PERFORM-ITS-COMMAND-SUBSTITUTION. `$[ 1+$(… ]` reads the `$( … )` as an arithmetic operand token — 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — the evaluation of a
`WordPart::ArithSub { bracket: true, … }` whose expression text holds an
unclosed `$( … )`.

**What is wrong.** `extract_arithmetic_subst` is
`extract_delimited_string (string, sindex, "$[", "[", "]", 0)` (subst.c:1299) —
flags `0`, so **no** `SX_COMMAND` and no nested read. The `$[` therefore closes
at its `]` by plain delimiter counting, and the unclosed `$( … )` inside is met
later, by the *arithmetic expansion* of the bounds text, which performs it under
`Q_DOUBLE_QUOTES|Q_ARITH`: it reports, runs the abandoned extent, and yields
nothing. osh instead hands the raw characters to its arithmetic tokenizer, which
calls them a bad operand.

Measured (`build/pgY.sh` d1/d2):

| word (inside `v='…'`, via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$[1+$(for⏎x]B` | reports `for`, runs `fo`, `[A1B]` | silent, `[AA$[1+$(for⏎x]B]` |
| `A$[1+$(echo hi⏎x]B` | reports EOF, `[A1B]` | silent, `[AA$[1+$(echo hi⏎x]B]` |

Row d3 — the same body with no `]` at all — is byte-exact in both shells
(silent, undecoded text), because there the `$[` genuinely never closes.

**What the proper fix looks like.** Two things, in order. (1) `$[`'s lex must
close at its `]` by plain delimiter counting, without the nested read — which
means `Lexer::read_opaque_span` needs to know its enclosing close character, so
that the `$((` spelling (SX_COMMAND) and the `$[` one (flags `0`) can part
company. Routing that arm through `Lexer::unread_comsub_stop` was tried on
2026-08-14 and reverted: it made the `$[` bounds text match bash on d1/d2, but
it *regressed* the `$((` spelling in the corpus case
`an-unterminated-construct-in-text-no-parser-read-is-a-runtime-failure`, whose
`$((1+$(echo` row must report the read and stop rather than condemn the `$((`.
A passing case outranks a documented divergence, so that arm keeps its `?`.
(2) The arithmetic evaluator must perform a `$( … )` in its expression text
with the unread-text rule rather than tokenizing it — which is what makes both
rows' values follow.

**Impact.** Wrong value and wrong diagnostic for a deprecated spelling of
arithmetic expansion, in malformed input, reachable only through `@P`/`PS4`/
here-doc text.
---

### [A] B-SMP-FAST-CPU-INDEX-PANICS-BEFORE-APIC-INIT. `smp::fast_cpu_index()` reads the APIC before it is mapped — `debug_assert` panic in debug, wild read in release — FIXED 2026-08-14

**Where:** `kernel/src/smp.rs` — the tier-3 fallback in `fast_cpu_index()`;
`kernel/src/apic.rs:~214` — `apic_read()`'s `debug_assert!(base != 0, "APIC not
initialized")`.

**What.** `fast_cpu_index()` has three tiers: RDPID, then `rdtscp`, then an APIC
MMIO read. On a CPU where neither RDPID nor `rdtscp` is advertised — which is
exactly the boot-test configuration, `qemu64,+smep,+smap,+umip` under TCG —
every call lands in tier 3 and does `crate::apic::read_id()`. Before
`apic::init` has run, `APIC_BASE_VIRT` is still 0, so:

- **debug builds:** `debug_assert!` fires → `KERNEL PANIC: APIC not initialized`.
- **release builds:** *worse* — the assert is compiled out and `apic_read`
  dereferences `(0 + offset) as *const u32`, a wild read of low memory. Silent
  garbage, or a fault, depending on what is mapped there.

**How it surfaced.** Wiring `frame_owner` ownership tagging into the frame
allocator (TD-FRAME-OWNER-1GIB) made `current_owner()` — and therefore
`fast_cpu_index()` — run on *every* frame allocation, including the allocator's
own boot-time self-test. That self-test runs long before `apic::init`, so the
kernel panicked at `[mm] Running frame allocator self-test...`:

```
!!! KERNEL PANIC !!!
panicked at kernel\src\apic.rs:214:5:
APIC not initialized
  Task: 0 (""), priority 0, cpu 0
```

**Why it was latent.** The pre-existing tier-3 callers were all gated behind
flags that only go true well after APIC init — the frame allocator's own
per-CPU cache checks `PCPU_ENABLED` first, for instance. Nothing called
`fast_cpu_index()` early, so the landmine was never stepped on. It was a real
bug regardless: the function's contract claims tier 3 "always works", and any
future early-boot caller would have hit it, in release builds silently.

**Fix.** Added `apic::is_ready()` (`APIC_BASE_VIRT != 0`) and made tier 3 check
it, returning CPU 0 when the APIC is not yet mapped. That is not a fudge: before
`apic::init` the system is strictly uniprocessor (BSP only), so 0 is the
*correct* index, not a fallback guess. Cost is one relaxed atomic load on the
already-slowest tier; tiers 1 and 2 are untouched, so real hardware pays
nothing.

**Lesson.** A "this can't happen yet" precondition that is enforced only by the
accident of who happens to call the function is not enforced at all. When the
cheap tiers of a tiered fast path are unavailable, the "always works" fallback
is the one that runs — so it is the one that has to actually always work.

### [A] B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION. The run-over-run diff named six regressions in code that had not changed — FIXED 2026-08-14

**Where:** `scripts/bench-history.py`, `diff()` / `report()`.

**Symptom.** The first post-merge `--bench` run (commit `17dbde179`, host
`Logoplex3`, BOOT_OK, exit 0) reported:

```
  REGRESSED (>25% slower):
    firewall_check: 270ns -> 482ns (+79%)
    shm_create_close: 58556ns -> 84996ns (+45%)
    ipc_semaphore: 11676ns -> 16112ns (+38%)
    net_veth_roundtrip: 47097ns -> 60102ns (+28%)
    net_veth_send: 23240ns -> 29552ns (+27%)
    io_ring_nop: 1948ns -> 2460ns (+26%)
```

**Why it was wrong.** `git diff bf26aabdb 17dbde179` over the perf-critical
paths is **two files, +54/-8**: `kernel/src/syscall/number.rs` (doc comments)
and `kernel/src/syscall/handlers.rs` (`sys_thread_join` moving its exit value
to an out-pointer). Nothing under firewall, veth, shm, semaphore or io_uring
changed at all, so not one of the six flagged benchmarks executes a line that
differs between the two commits.

The actual distribution over all 63 benchmarks: **median +6.1%, mean +9.4%,
48 slower vs. 15 faster** — and the sorted tail is a smooth continuum,
`24.4, 24.5, 24.6, 24.9, 26.3, 27.2, 27.6`. There is no gap anywhere near the
threshold. A real regression is a few outliers standing clear of a ~0% median;
what this was is a fixed 25% line drawn through the middle of a shifted
distribution.

**Root cause.** The module docstring claims run-over-run comparison "cancels
the emulation constant". That holds across *hosts*, not across *runs on one
host*: TCG is pure emulation and therefore CPU-bound, so whatever else the
machine was doing scales the whole suite by a common factor. Shift a
distribution whose own per-benchmark wobble already reaches ~20% by a further
6% and its tail crosses 25%. The `diff()` docstring even anticipated the noise
("a 10-20% wobble carries no information") but chose the wrong remedy — a
coarser *absolute* threshold cannot subtract a *global* shift, it can only
trade false positives for false negatives.

**Fix.** Added `global_drift()`: the **median** of every benchmark's
run-over-run ratio, used to normalise each ratio before thresholding, so the
threshold applies to how a benchmark moved *relative to its peers on the same
run*. The median (not the mean) is the estimator precisely because it is
unaffected by a genuine regression in a minority of benchmarks — the signal
that must not be subtracted away. Skipped below `MIN_SAMPLES_FOR_DRIFT = 8`,
where the median means nothing and a handful of benchmarks can legitimately
all move together. The report now prints the drift itself (information in its
own right — it says the machine was busy), shouts if it exceeds 15%, and shows
both numbers per entry (`+68% vs suite, +79% raw`) so no one has to trust the
correction blindly.

Replayed against the real data, the four pure-drift entries drop out and the
report goes from six regressions to three.

**Why this mattered enough to fix immediately.** It is the same class of defect
as the bug that produced this harness in the first place
(`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`): a report you cannot
act on. A silent skip trains you not to notice; a comparator that cries wolf on
every run trains you to skim past the one time it is right. Six false
regressions on the *very first* run it was used in anger would have retired the
feature within a week.

**Related precedent:** `TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT`
burned five boots on "ownership tagging costs 8500 cycles" that was also the
emulator rather than the code. Same underlying trap, one level up.

### [A] W-BENCH-THREE-BENCHMARKS-ABOVE-SUITE-DRIFT-WITH-NO-MATCHING-CODE-CHANGE. firewall_check / shm_create_close / ipc_semaphore — ✅ RESOLVED 2026-08-14: all three were noise

**RESOLUTION (2026-08-14, third run `a18ea83a9`).** All three were noise, and
the third run says so about as loudly as data can. They came back not merely
to the suite median but to *below* their first-run values — and they are the
**top three entries in the IMPROVED list**, in the same order they had
occupied in REGRESSED:

| benchmark | run 1 `bf26aabdb` | run 2 `17dbde179` | run 3 `a18ea83a9` | verdict |
|---|---|---|---|---|
| `firewall_check` | 270 ns | 482 ns | **228 ns** | run 2 is the outlier |
| `shm_create_close` | 58 556 ns | 84 996 ns | **56 734 ns** | run 2 is the outlier |
| `ipc_semaphore` | 11 676 ns | 16 112 ns | **11 219 ns** | run 2 is the outlier |

Runs 1 and 3 agree to within 3–16 % in every case; run 2 stands alone. A real
regression does not un-regress with no code change, so the correct reading is
that run 2 was the anomaly, not run 3 — i.e. these were never regressions at
all, and the flat 25 % threshold flagged them purely because their intrinsic
spread exceeds it. The prediction recorded below — that `firewall_check` at
270 ns would prove the noisiest by construction — held: its spread is 111 %,
the second-widest in the suite.

**Measured per-benchmark spread (max/min across the three runs), which is the
number the comparator has been missing all along:**

* median across all 63 benchmarks: **13 %**
* but the tail is long: `crypto_ed25519_verify` 416 %, `firewall_check` 111 %,
  `tcp_checksum_v6` 56 %, `shm_create_close` 50 %, `sched_pick_next` 49 %,
  `syscall_dispatch` 44 %, `ipc_semaphore` 44 %.

So a flat 25 % threshold is below the natural spread of at least seven
benchmarks and far above that of the median one — it is simultaneously too
tight and too loose, which is exactly the failure mode observed. **This
promotes the "proper fix" named below from a suggestion to the next task:
give the comparator a per-benchmark variance estimate.** Logged as
TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE below.

**Caveat recorded honestly: run 3 is partially contaminated, by me.** I ran
greps, `git`, and `python` in the same window as the benchmark suite, having
explicitly noted beforehand that the machine should be idle. Median drift
correction removes a *uniform* slowdown; it cannot remove contention that
lands on whichever benchmark happens to be running at the time. That is the
most likely explanation for run 3's own new outliers —
`crypto_ed25519_verify` (30.7M → 31.4M → **158.6M**, i.e. two tight samples
then 5.1×) is the longest-running benchmark in the suite and therefore the
most exposed to a contention window. Do **not** treat that as a regression on
this evidence; see TD-BENCH-RUNS-ARE-CONTAMINATED-BY-THE-AGENTS-OWN-COMMANDS
below.

The original WATCH text follows unchanged.

---

### [A] W-BENCH-THREE-BENCHMARKS-ABOVE-SUITE-DRIFT-WITH-NO-MATCHING-CODE-CHANGE (original entry). firewall_check / shm_create_close / ipc_semaphore — WATCH, needs a third data point

**Where:** benchmarks `firewall_check`, `shm_create_close`, `ipc_semaphore`;
history in `bench/history.jsonl` (host `Logoplex3`).

**What.** After the drift correction above, three benchmarks still sit clear of
the suite: `firewall_check +68%` (270→482ns), `shm_create_close +37%`
(58556→84996ns), `ipc_semaphore +30%` (11676→16112ns). As established above,
none of their source changed between `bf26aabdb` and `17dbde179`.

**Why it is a WATCH and not a bug (yet).** `bench/history.jsonl` holds exactly
**two** runs on this host, so there is no per-benchmark variance estimate — the
drift correction removes the *common* factor but says nothing about how noisy
an individual benchmark is around it. `firewall_check` at 270ns is the prime
suspect for being intrinsically noisy: it is the shortest benchmark in the
suite, and at TCG timer granularity a couple of hundred nanoseconds is very
few ticks, so its relative variance should be the largest by construction.

**How to resolve.** Take a third `--bench` run on an otherwise-idle machine and
compare. If these three land back at the suite median they were noise, and the
proper fix is to give the comparator a per-benchmark variance estimate (flag on
deviation from a benchmark's own historical spread, not a flat percentage)
rather than to keep hand-adjudicating. If they stay high, they are real, and
the next question is whether the `handlers.rs` change shifted code layout
(icache/alignment) — cheap to test by benchmarking `bf26aabdb` again.

**Do not** act on either theory from the current two runs; that is exactly the
inference-from-insufficient-samples mistake the entry above documents.

### [A] TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE. A flat 25% threshold is below the natural spread of seven benchmarks and far above the median one's — 2026-08-14 — OPEN

**Where:** `scripts/bench-history.py`, `diff()` / `THRESHOLD_PCT`.

**What.** The comparator flags a benchmark when its drift-corrected change
exceeds a fixed ±25 %. Three runs of history now show that a single flat
threshold cannot work, because the suite's per-benchmark spread (max/min
across runs, *with no code change explaining it*) ranges over an order of
magnitude:

* median benchmark: 13 % spread → 25 % is far too loose; a genuine 20 %
  regression here would pass unnoticed.
* `crypto_ed25519_verify` 416 %, `firewall_check` 111 %, `tcp_checksum_v6`
  56 %, `shm_create_close` 50 %, `sched_pick_next` 49 %, `syscall_dispatch`
  44 %, `ipc_semaphore` 44 % → 25 % is far too tight; these produce false
  positives every single run.

Two investigation cycles have now been spent hand-adjudicating false
positives thrown by this threshold (see the RESOLVED entry above and
B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION). That is the
signal to fix the estimator rather than keep adjudicating its output.

**Proper fix.** Give each benchmark its own noise band derived from its own
history, and flag only moves outside it. Concretely: keep the existing
whole-suite median drift correction (it removes the common factor correctly
and is not in question), then compare the drift-corrected change against a
robust per-benchmark dispersion — median absolute deviation of the log-ratios
across the recorded runs — rather than a constant. Retain a flat *floor* so
that a benchmark with an implausibly tight history cannot be flagged on a
sub-noise move, and require a minimum number of runs (the existing
`MIN_SAMPLES_FOR_DRIFT` precedent) before the per-benchmark band is trusted,
falling back to the flat threshold until then.

**Test it the same way the drift fix was tested:** replay the estimator
against the recorded `bench/history.jsonl` and confirm it drops the three
now-known-noise entries while still flagging a deliberately injected
regression. Do not ship it on reasoning alone — that is the mistake this
whole thread of entries keeps documenting.

**Update 2026-08-14 — the fix above is DATA-BLOCKED; the unblocking step has
landed.** Attempting the implementation established that it cannot be built
*or* validated yet, which is worth recording so the next attempt does not
rediscover it:

* The MAD-of-log-ratios estimator needs the spread of each benchmark across
  runs. `bench/history.jsonl` holds **3** records, all from one host — which
  yields **2** consecutive run-over-run residuals per benchmark. A median
  absolute deviation over 2 points is not an estimate of anything; with
  residuals of `{+2 %, +406 %}` it returns ~204 %, a band so wide it would
  flag nothing, and one more run could as easily make it 2 %, a band so tight
  it flags everything. A minimum-runs gate (the fix's own proposal) would
  simply keep it disabled.
* The test requirement above is therefore *also* unsatisfiable today: with 3
  records there is no held-out data to replay against. Shipping it anyway
  would be exactly the "on reasoning alone" failure the entry warns about, so
  it was not shipped.

**What landed instead** (commit alongside this update): the harness now emits
and records a per-benchmark **dispersion** figure, which supplies the missing
noise scale *from a single run* rather than requiring history to accumulate.
`kernel/src/bench.rs::print_scorecard` extends the machine-readable line to

```text
[bench] SCORE <name> <min_ns> <target_ns> <PASS|OVER> <mean_ns> <iters>
```

and `bench-history.py` stores `mean_ns` / `iterations` as sibling maps in each
record. The trailing pair is optional in the parser, so the 3 existing records
still load — `scripts/test-bench-history.py` pins that down against the real
history file, because those records are ~9-minute boots on commits that are
now in the past and cannot be regenerated.

`mean/min` is a genuine per-benchmark noise scale and not a proxy for one: the
scorecard reports `min` because it is the least-contaminated estimate, but a
benchmark whose mean sits at 1.05× its min took a clean measurement on nearly
every iteration, while `dashboard_api_status` at 6.6× (160.4 ms mean vs 24.4 ms
min) was interrupted on most of them — so its reported min is whichever
iteration happened to dodge the interference, and is correspondingly fragile
run-to-run. That is precisely the property the band needs to size itself by,
and the two entries plainly should not share one threshold.

**Remaining work, in order.** (1) Accumulate ≥6 same-host records — this is a
by-product of ordinary benchmarked boots, not a task. (2) Validate the
`mean/min` → run-over-run-sigma mapping *empirically* against those records
before using it; the causal story above is plausible but the coefficient is
not known, and inventing one would just build a new false-positive generator
with more decimal places. (3) Then implement the band, preferring the
historical MAD where enough runs exist and falling back to the dispersion
prior where they do not. Do **not** skip step (2).

### [A] TD-BENCH-RUNS-ARE-CONTAMINATED-BY-THE-AGENTS-OWN-COMMANDS. I ran greps and git during a benchmark suite after noting the machine had to be idle — 2026-08-14 — OPEN

**What.** The benchmark suite runs under QEMU TCG, which is pure emulation and
entirely CPU-bound, so any other load on the host scales the measurements.
During run 3 (`a18ea83a9`) I ran roughly a dozen `grep`, `git`, `python` and
file-read commands in the same window, despite having stated at the start of
the run that the machine needed to stay idle for the numbers to mean anything.

**Why the existing drift correction does not save it.** The median-ratio
correction removes a *uniform* whole-suite factor — a machine that is
consistently 6 % slower for the whole run. Contention from a handful of short
commands is not uniform: it lands on whichever benchmark is executing at that
moment and leaves the rest untouched. It therefore shows up as exactly what a
real regression looks like — one or two benchmarks clear of an unchanged
median. `crypto_ed25519_verify` is the canary: 30.7M → 31.4M → 158.6M ns,
i.e. two runs agreeing within 2 % and then a 5.1× jump, on a benchmark whose
source did not change and which is the longest-running in the suite (so the
most likely to overlap a command).

**Proper fix — structural, not a discipline reminder.** "Remember to stay
idle" is not a fix; it already failed once, the same day it was written down.
Make contamination *detectable* instead: have the bench harness re-run one
cheap, low-variance reference benchmark at the start and again at the end of
the suite, and record both. If the two disagree by more than a few percent,
the host load changed mid-run and the whole run should be marked contaminated
in `history.jsonl` and excluded from comparison (or at minimum reported as
such). This turns "the operator/agent must behave" into a property the data
itself can verify — the same principle as the stall detectors: a check that
cannot fire is indistinguishable from a check that passes.

**Interim mitigation until that exists:** when a `--bench` run is in flight,
do read-only work only if it is genuinely necessary, and prefer to simply
wait. Treat any single-benchmark outlier in a run that overlapped agent
activity as unproven.

**[A] ✅ FIXED 2026-08-14 — and the first version of the fix was itself blind
to the case it was built for.** Worth reading for the second half.

*Stage 1 (commit `be167dd90`).* The reference memory-access cost that already
calibrates every budget in `bench.rs` is now measured a second time at the end
of the suite, emitted as `[bench] CANARY <start> <end> <pct>`, recorded by
`bench-history.py` as a sibling key with a `contaminated` flag, and covered by
11 checks in `test-bench-history.py`. The measurement was factored into one
parameterless function used by both ends, because the comparison means nothing
unless both ends measure precisely the same thing.

*What the first real run showed (commit `be167dd90`, host Logoplex3).* Two
things, one confirming the entry and one refuting the fix.

Confirming: `crypto_ed25519_verify` came back at **30.0M ns**, against 30.7M
and 31.4M in the two runs before the spike and 158.6M during it. Three runs
now agree within 4% and the spike stands alone, so run `a18ea83a9` **was**
contaminated, exactly as this entry argued. Whole-suite drift for the new run
was −0.1%.

Refuting: the canary reported the host stable to within **3%** (283 → 275
cycles) — while in that same run `shm_rw_64bytes` (298 → 771), `tcp_checksum_v4`
(20714 → 35410), `net_ipv4_parse` (933 → 1645) and `net_ethernet_parse`
(873 → 1216) all sat 40–160% above their established values. So the run was
contaminated and the canary passed it.

*Why.* Endpoint sampling detects a **sustained** load change. The
contamination described at the top of this entry is a **transient burst** that
"lands on whichever benchmark is executing at that moment and leaves the rest
untouched" — which by construction is invisible to a check that only looks at
the two ends. The first fix was therefore a check that could not fire on its
own motivating case: the failure mode this project keeps rediscovering, arrived
at from the opposite direction.

*Stage 2 (this commit).* The reference is now sampled **throughout** the suite
— every 8th scored benchmark, giving ~8 samples across the 63 — and the verdict
uses the min-to-max spread rather than the endpoint ratio. Sampling is hooked
into `score()`, the one function every benchmark already calls, so it spreads
automatically and stays correct as benchmarks are added or reordered; a
hand-placed list of sample points in `run_all` would rot. The line gains four
append-only fields, `[bench] CANARY <start> <end> <pct> <min> <max> <spread>
<samples>`, so the single record written by stage 1 still reads back and is
still judged by the endpoint rule it was written under.

*Tolerance status.* Still 25%, still a placeholder. One clean-endpoint
observation (3%) is not a distribution, and the mid-suite spread has now to be
observed over several runs before the bound is tightened — the same discipline
applied to `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`. The raw min/max
are recorded on every run precisely so the bound can be retuned later against
real data instead of being invented; a stored verdict alone could never be
re-judged.

*Consequence for the four elevated benchmarks above:* unproven, not regressions.
They are diffed against `a18ea83a9`, a run this entry now shows was itself
contaminated, so the comparison is contaminated at both ends. They need a clean
run-over-clean-run comparison before anyone reads them as real.

**[A] Update 2026-08-14 — stage 2 verified, and all four elevated benchmarks
were indeed contamination.** Run `5a2002bac` reported `spread 2%` over **10**
mid-suite samples (267–275 cycles), so the sampling works end to end. Against
that clean run every one of the four returned to its established value:
`shm_rw_64bytes` 771 → **414**, `tcp_checksum_v4` 35410 → **20182**,
`net_ipv4_parse` 1645 → **952**, `net_ethernet_parse` 1216 → **829**. None was
a regression, which is what the refusal to report them was protecting.

**Honest limitation — the production check has not yet been observed firing.**
The unit tests prove the *logic* fires (a 173% mid-suite spread with quiet
endpoints reads as contaminated), and both real runs so far were clean, so the
mid-suite path has only ever been seen returning "OK". Host contamination
cannot be summoned on demand, so this is a check believed-good rather than
demonstrated-good in production — the precise distinction this entry exists to
insist on. It should not be described as proven until a real run trips it.
Whole-suite drift for `5a2002bac` was +3.1%.

**RECURRENCE 2026-08-14, run `fcd066231` — I did it again, and this time it
landed on a number I then acted on.** During the ~58 s QEMU bench run I ran
`grep` over the 60 000-line `known-issues.md`, `git log`, `git show`, and
several `Read`s. The dispersion report for that run flagged five benchmarks at
≥5x `mean/min`, and **`vfs_stat_root` was one of them at 12x**. I then took
that run's `vfs_stat_root` = 5920 ns, called it "8.5x over its 700 ns target",
committed that claim, and opened an investigation into the VFS dcache on the
strength of it.

The number may well still be broadly right — `score()` records `min_ns`, and a
burst inflates the mean far more than the min. But "broadly right" is not the
standard, and the specific escape hatch does not close here: this benchmark is
**500 iterations at ~6 µs ≈ 3 ms of wall time**. A host load episode lasting
longer than 3 ms — which is to say, essentially any of them — covers the
*entire* benchmark and inflates min and mean together, leaving `mean/min`
looking normal while every sample is uniformly slow. The 12x ratio says a
burst happened *inside* those 3 ms; it says nothing about whether a slower,
broader episode also raised the floor. So the honest status of 5920 ns is
**unverified**, not "confirmed 8.5x over".

Two things follow, and both were done rather than noted:

1. The re-measurement (the `vfs_stat_breakdown` run) is executed with **no
   agent commands issued while QEMU is running** — the read-only work is done
   before the run starts or after it finishes, never during.
2. The dcache finding is not being justified by the 5920 ns figure at all. It
   rests on reading the code: `VfsDcache::lookup` is a linear scan over 1024
   slots with a full `PathBuf` compare per slot, which is a design defect
   under CLAUDE.md's "linear scans … must be O(1) or O(log n)" rule
   independently of what any timer says. A contaminated benchmark can motivate
   a code review; it must not be the evidence.

**The pattern, stated plainly, because this is the second occurrence.** The
first time, the contamination hit numbers I merely recorded. This time it hit
a number I *reasoned from* within minutes of producing it. The entry above
correctly predicted the mechanism and even built the detector that caught it —
and the detector working did not stop me, because I read the dispersion list
*after* I had already drawn the conclusion. A check that fires after the
decision is documentation, not a gate. The ordering is the fix: read the
dispersion report **before** quoting any number from a run, not after.

### [A] B-BENCH-WATCHLIST-WATCHED-LESS-THAN-HALF-THE-SUITE-IT-GUARDS. `BENCH_CRITICAL_PATHS` omitted idt.rs, fs/, net/ and crypto.rs — FIXED 2026-08-14

**Where:** `scripts/boot-test.sh`, `BENCH_CRITICAL_PATHS` (feeds
`report_bench_absence`).

**What.** The list added earlier the same day to close
`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE` held five entries —
`kernel/src/{mm,sched,ipc,syscall,smp.rs}` — because it was derived from
CLAUDE.md's perf-critical *table*, read as directory names. The suite it is
supposed to guard measures far more than that. Against the 63 recorded
benchmark names:

- `isr_latency`, `page_fault` → **`kernel/src/idt.rs`**. CLAUDE.md's table
  names both "interrupt dispatch" and "page fault handling", but the handlers
  live in `idt.rs`, not under `mm/` — so the two benchmarks that measure them
  were unwatched.
- 8 × `vfs_*` (`read_256`, `write_256`, `readdir`, `stat_{root,3comp,deep}`,
  `throughput_16k_{read,write}`) → **`kernel/src/fs`**. CLAUDE.md lists "VFS
  path lookup" and "filesystem read/write" as critical.
- ~20 × `net_*`, `tcp_checksum_*`, `dns_build_query`, `firewall_check`,
  `http_*`, `dashboard_api_*` → **`kernel/src/net`** (`http.rs`,
  `dashboard.rs` live under it).
- 9 × `crypto_*` → **`kernel/src/crypto.rs`**.

So **30+ of 63 benchmarks measured code the watch list did not watch**, and a
change to any of them printed "No perf-critical changes since the last
benchmarked commit, so skipping the suite is reasonable here." Confidently,
and wrongly.

**How it surfaced.** The `W-KERNEL-COW-WRITE` diagnostic commit edits
`kernel/src/idt.rs`. The following boot reported no perf-critical changes —
while the suite contains `isr_latency` and `page_fault`, both measured by code
in that exact file. (No real regression: that diagnostic sits on the fatal
path, which is not hot. The harness had no way to know that, and did not
reason about it — it simply never looked.)

**Fix.** Widened the list to the four missing paths and annotated **every**
entry with the benchmarks it guards, so the mapping is auditable instead of
implicit. Verified: `git diff --name-only 17dbde179 HEAD` over the new list
now returns `kernel/src/idt.rs`, which the old list missed.

**Lesson (the recurring one this week).** This is the third instance in a row
of the same shape: `TD-BENCHMARKS-...` (the suite silently never ran),
`B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION` (the diff
confidently named innocent benchmarks), and now a watch list that confidently
reported "nothing to see" about a file it had never been told to look at. A
check that cannot fire is indistinguishable from a check that passes — and
every one of these was *my own* freshly-written tooling, reporting success.
When adding a guard, the first test should be "does it fire on a case I know
is positive?", not "does it run cleanly?".

### [A] B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x. A hot loop that straddles a 4 KiB guest page costs ~1.7x under TCG, deterministically per build — ROOT-CAUSED 2026-08-14, fix pending

**Where:** `kernel/src/bench.rs`, `bench_net_tcp_checksum_v4` (3281) /
`bench_net_tcp_checksum_v6` (3340) and their bench-local kernels
`tcp_checksum_bench` (3309) / `tcp_checksum_v6_bench` (3366).

**What.** In 3 of the 5 recorded runs on host `Logoplex3`, one member of the
pair sits near ~35000 ns while the other sits near ~20000 ns; in the other 2
runs both sit in the 20000–26000 band. Which member is the elevated one
varies:

| commit | `tcp_checksum_v4` | `tcp_checksum_v6` |
|---|---|---|
| `bf26aabdb` | 20667 | 23021 |
| `17dbde179` | 25279 | 25751 |
| `a18ea83a9` | 20714 | **35899** |
| `be167dd90` | **35410** | 20953 |
| `5a2002bac` | 20182 | **35039** |

The two kernels are near-identical byte-at-a-time fold loops over the same
1460-byte segment; v6 does 36 more pseudo-header bytes than v4, i.e. ~2.5%
more work. A 1.7x gap between them — in either direction — is not explicable
by the work they do.

**What the dispersion data does and does not show.** The recorded figure is
`result.min_ns`, the **minimum** over 2000 iterations, and since the
append-only `mean_ns` extension landed we also record the mean, so `mean/min`
is available as a within-run dispersion measure:

| commit | benchmark | min | mean/min |
|---|---|---|---|
| `be167dd90` | `tcp_checksum_v4` (elevated) | 35410 | 1.16 |
| `be167dd90` | `tcp_checksum_v6` | 20953 | 1.20 |
| `5a2002bac` | `tcp_checksum_v4` | 20182 | 1.21 |
| `5a2002bac` | `tcp_checksum_v6` (elevated) | 35039 | 1.33 |

In both runs the elevated member's dispersion is indistinguishable from the
other member's. Compare the visibly burst-hit numbers in the same records:
`net_ethernet_parse` at 2.86 and `context_switch` at 10.62 in `be167dd90`.
So the elevated member is uniformly ~1.7x slower across all 2000 iterations
with normal spread.

**This rules out a sub-benchmark burst, and nothing more — do not read it as
"not contamination".** A first draft of this entry concluded that a normal
`mean/min` proved the slowdown was a steady-state property of the build. That
does not follow. 2000 iterations at ~20 µs is only ~40 ms of wall time, and a
host load episode that spans the *entire* 40 ms window inflates the min and
the mean by the same factor, leaving `mean/min` untouched. Such an episode is
entirely ordinary on a desktop. So the dispersion data distinguishes "a spike
during part of the benchmark" from "uniformly slower", and is silent on
*why* it was uniformly slower. Both a build property and a benchmark-length
contamination episode predict exactly what is in the table above.

**Two live hypotheses, and the test that separates them.**

1. *Code-layout sensitivity under QEMU TCG.* The two loops are compiled
   separately (deliberately duplicated "to avoid depending on tcp module
   internals"), so their alignment and translation-block boundaries shift with
   every unrelated code change; whichever lands unluckily pays a fixed
   per-iteration penalty.
2. *A contamination episode long enough to cover one whole benchmark.* The
   mid-suite canary samples every 8 scored benchmarks, so an episode lasting
   one benchmark can slip between two samples and be reported as a quiet run —
   which is what `5a2002bac` reported.

**These are separated by re-running the bench on the *same commit*.** Hypothesis
1 is a property of the binary and must reproduce: same member elevated, same
factor. Hypothesis 2 re-rolls: the elevated member moves, or neither is
elevated. This needs no new code, just a second `--bench` boot on an unchanged
tree.

**RESOLVED 2026-08-14 — hypothesis 1, decisively.** That run was done on a
byte-identical binary (only markdown had changed since `5a2002bac`):

| | `5a2002bac` | re-run, same binary | agreement |
|---|---|---|---|
| `tcp_checksum_v4` | 20182 | 20687 | 2.5% |
| `tcp_checksum_v6` | **35039** | **35048** | **0.03%** |

The same member is elevated, at the same value — and the host was *noisier*
this run, not quieter (canary spread 16% over 10 samples, against 2% before),
which rules out the contamination reading rather than merely failing to
support it. It is a deterministic property of the binary.

**Mechanism: the elevated member's hot loop straddles a 4 KiB guest page.**
Disassembling the staged binary and locating the backward branch in each fold
loop:

| | fold loop | span | pages |
|---|---|---|---|
| `tcp_checksum_bench` (v4, fast) | `ffffffff805d7202` → `ffffffff805d73f7` | 501 B | `…805d7` → `…805d7`, **one page** |
| `tcp_checksum_v6_bench` (v6, elevated) | `ffffffff805d9ea9` → `ffffffff805da086` | 477 B | `…805d9` → `…805da`, **straddles** |

Under TCG a translation block is bounded by the guest page — a loop that
crosses a page boundary cannot stay a single directly-chained TB, so every
iteration pays a dispatcher round-trip instead of a direct jump. That predicts
exactly what is observed: a *uniform* per-iteration penalty (so `mean/min` is
untouched), perfectly reproducible on the same binary, and re-rolled whenever
unrelated code shifts the function's address — which is why runs 1 and 2 show
neither member elevated (in those builds neither loop straddled).

**Falsifiable prediction, to be checked on the next bench run:** disassemble
first, and whichever of the two fold loops straddles a page is the one that
will come back elevated — with neither elevated if neither straddles. This
entry should be treated as provisional until that prediction has been made
*before* a run and held.

**This generalises to the whole suite, and that is the real finding.** Nothing
about the mechanism is specific to `tcp_checksum`. Any benchmark whose hot loop
happens to straddle a 4 KiB page pays the same penalty, and which benchmarks do
re-rolls at every build. So commit-to-commit comparison under TCG carries an
irreducible per-benchmark noise floor of up to ~1.7x that is *deterministic
within a run* — meaning neither the canary nor `mean/min` can ever detect it,
because both look for variation and there is none. Every noise-suppression
mechanism built for this suite so far is structurally blind to it.

**It is also mostly the same bug as
`B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL`, and mostly the same
fix.** The straddle probability scales with the byte length of the hot loop. At
`opt-level = 0` this fold loop is 117 instructions / ~500 bytes, giving it
roughly a 1-in-8 chance of crossing any given page; optimised it would be a
few dozen bytes, closer to 1-in-100. Building the bench kernel `--release`
therefore shrinks this noise source by about an order of magnitude as a side
effect. Do that first and re-measure before considering anything more invasive
(forced function alignment via `-Z align-functions` costs padding across the
whole kernel and would only paper over the loop-length problem).

**Why it matters.** Both are on the `over_target` list (targets 2000/2200 ns,
measured 20000–35000), so the absolute numbers are already known-bad under TCG
and nobody is being misled about pass/fail. The damage is to the *comparator*:
a 1.7x swing that re-rolls every build is pure noise in any commit-to-commit
diff, and `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE` will size its
band from exactly this history. If the band is fitted without knowing this
pair is bimodal, it will either be stretched wide enough to hide real
regressions everywhere else, or it will keep flagging these two forever.

**Remaining plan.** Steps 1 and 2 are done (above). What is left:

1. Build the bench kernel `--release` (see
   `B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL`) and re-measure.
2. Make the straddle prediction *before* that run and record it, so the
   mechanism is confirmed prospectively rather than fitted after the fact.
3. If page straddling still moves benchmarks materially at `opt-level = 3`,
   teach the comparator about it: the check is mechanical (disassemble, locate
   the backward branch, compare `addr >> 12` at both ends) and could be emitted
   alongside each score, which would turn an invisible deterministic bias into
   a recorded per-benchmark flag.

**Reproducing the disassembly.** `llvm-nm` / `llvm-objdump` ship with the
rustup toolchain — no binutils install needed:
`~/.rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/`.
The two kernels are `_ZN6kernel5bench18tcp_checksum_bench…` at
`ffffffff805d7130` and `_ZN6kernel5bench21tcp_checksum_v6_bench…` at
`ffffffff805d9df0`. Note the symbol hash differs per build, so match on the
demangled prefix rather than pasting a mangled name.

**Incidental finding from the disassembly: the benchmarked kernel is built
without optimisation.** `tcp_checksum_bench` spills every intermediate to the
stack (`movl %eax, -0x64(%rbp)` after each add). That is consistent with the
whole suite sitting ~10x over targets that were set from optimised reference
implementations, and it means the absolute numbers measure debug codegen under
TCG, not the code that would ship. Worth confirming against the boot-test
build flags and recording separately — it is a much larger effect than the
1.7x this entry is about, and it is not this entry's subject.

**Related observation — `mean/min` sees contamination the canary missed.** The
canary called run `5a2002bac` clean (spread 2% over 10 samples). In that same
run `crypto_ed25519_verify` had mean 323487129 against min 31875588, a
**10.15x** ratio; `context_switch` had 10.62x in the run before it. The canary
samples the host *between* benchmarks; `mean/min` measures dispersion *inside*
the benchmark that was running, so it catches a burst that fell between two
canary samples. The data is already recorded per benchmark and needs no
cross-record history to interpret, so a per-benchmark "this number is suspect"
flag is implementable now.

Neither measure dominates the other, and the reason is exactly the failure
above: `mean/min` is blind to any slowdown that covers a whole benchmark
uniformly — including a sustained load change, which is what the canary
endpoints exist to catch — while the canary is blind to bursts shorter than
its sampling interval. The comparator should consult both, and should treat
"canary quiet **and** `mean/min` normal" as the only combination that
licenses reading a number as real.

**PROSPECTIVE PREDICTION, recorded 2026-08-14 BEFORE the first release-profile
bench run.** This entry says above that the page-straddle mechanism "should be
treated as provisional until that prediction has been made *before* a run and
held." This section is that prediction. It was written from the disassembly of
`target/x86_64-unknown-none/release/kernel` (built clean, 0 warnings, 9m25s)
with **no release-profile measurement in existence yet** — the first such run
has not been performed. Whatever the numbers turn out to be, this text is not
to be edited afterwards; the result goes in a separate section below it.

Structural facts read out of the release binary:

| | v4 | v6 |
|---|---|---|
| closure inlined into the timed loop? | **yes** | **no** — `callq`+`ret` per iteration |
| hot fold loop | `ffffffff80985cc2`–`…985cf7` | `ffffffff80976ba0`–`…976bc7` |
| straddles a 4 KiB page? | **no** (all in `…985000`) | **no** (all in `…976000`) |
| timed outer loop | `…985caa`–`…985d51` (page `…985`) | `…9864a5`–`…9864fd` (page `…986`) |
| per-iteration indirect branch | none | one `ret` |
| bytes consumed per loop iteration | 4 (2x unrolled) | 4 (2x unrolled) |

So in the release build the *specific* mechanism this entry root-caused — a
hot loop split across a guest page boundary — is **not active for either
benchmark**. Both fold loops are comfortably interior to a page. If the 1.7x
bimodal swing were caused by anything else, it should survive the profile
change; if it was the straddle, it should vanish.

Predictions, in falsifiable form:

1. **The 1.7x v6/v4 gap collapses.** Predicted release ratio **1.00–1.20**.
   A ratio still ≥1.5 falsifies the straddle explanation outright.
2. **A residual v6 penalty is still expected, but small.** v6 pays one
   out-of-line call and — the part that actually costs under TCG — one `ret`,
   which is an *indirect* branch and cannot be direct-chained between
   translation blocks; it takes a jump-cache lookup every iteration. But that
   is one dispatch amortised over ~365 fold-loop iterations of real work, so
   it should be a low-single-digit percentage, not a multiple. v6 also has the
   genuinely larger 40-byte pseudo-header (the straight-line preamble at
   `…976aa3`–`…976b8e`), which is real work and legitimately makes v6 slower.
3. **Both numbers drop by roughly an order of magnitude** from the debug
   figures (v4 ~20200–20700 ns, v6 ~35000 ns). The debug loop spilled every
   intermediate to the stack and consumed 2 bytes per iteration; the release
   loop is 10 instructions, register-only, 4 bytes per iteration. Predicted
   release: **v4 ~2000–3000 ns, v6 ~2200–3500 ns** — i.e. at or near the
   2000/2200 ns targets, which were set from optimised reference
   implementations and have been failed by ~10x for the whole life of the
   suite for exactly that reason.
4. **The run is scored against no baseline.** `bench-history.py --profile
   release` should report that no same-profile record exists and decline to
   diff against the five debug records, rather than reporting a fabricated
   ~10x "improvement". This is the profile-isolation change under test.

If (1) holds and (3) holds, the mechanism is confirmed prospectively and the
entry can be closed. If (1) fails while (3) holds, the optimisation level was
a confound and the straddle explanation is wrong — in that case the same-binary
re-run table above (v6 35048 vs 35039, 0.03%) still stands as proof the effect
is deterministic per build, and a different per-build mechanism must be found.

**RESULT of the prediction above — run `fcd066231`, release profile,
2026-08-14T15:57:59.** Scored against the four predictions as written, with no
edits to them:

| | Predicted | Measured | Verdict |
|---|---|---|---|
| 1. v6/v4 ratio | 1.00–1.20 (≥1.5 falsifies) | **0.93** | central claim **holds**, band missed |
| 2. v6 slightly slower than v4 | yes, low single-digit % | v6 **6.6% faster** | **WRONG** |
| 3. both drop ~10x | v4 2000–3000 ns, v6 2200–3500 ns | v4 **1716**, v6 **1602** | order right, **both beat the band** |
| 4. no cross-profile baseline diff | refuses to compare | refused, verbatim | **holds exactly** |

Raw: `v4 min 1716 ns (6366 cyc), mean 1772` and `v6 min 1602 ns (5946 cyc),
mean 1663`. Dispersion 1.03 and 1.04 — both clean, so neither number is a
contaminated read. Against the debug records (v4 20182–35410, v6 20953–35899)
that is **11.8x and 21.9x faster**, and both now pass their 2000/2200 ns
targets — the first time either has, ever.

The bimodality is gone outright. Across the six debug records the ~35000 band
was occupied by v6, v6, v4, v6, v6 and neither (a middle run at 25279/25751);
in release both members sit in a 1602–1716 band with no elevated member. So
the entry's *central* claim is confirmed: **the 1.7x swing was an artefact of
the build, not a property of the checksum code.**

**But this run does not isolate the page-straddle mechanism, and it would be
dishonest to close the entry as if it had.** Going from `opt-level = 0` to `3`
rewrote the code completely — new instruction sequences, 2x unrolling, new
addresses, new inlining decisions. The straddle hypothesis predicted the gap
would vanish and it vanished; but so would *any* hypothesis of the form "this
is a build artefact", which is a much weaker and much easier claim. I changed
two variables at once and can only credit the one they share. The experiment
confirms the **class**, not the **mechanism**.

**Prediction 2 failing matters more than prediction 1 succeeding.** v6 does
strictly more work than v4 — a 40-byte pseudo-header instead of 12 — *and* in
this build pays an out-of-line `callq` plus a `ret` (an indirect branch, not
direct-chainable between TCG translation blocks) on every one of its 2000
iterations. It came out faster anyway. That is the same fine-grained "what
costs what under TCG" reasoning the straddle story rests on, applied to a case
where the answer was checkable, and it got the *sign* wrong. Confidence in the
straddle attribution should be downgraded accordingly, not raised by
prediction 1.

**The experiment that would actually isolate it** (not yet done): stay within
one profile and move a function's address deliberately — insert padding, or a
`#[repr(align)]`/`.balign` on the hot loop — so that a loop which currently
sits interior to a page is pushed across a boundary, with nothing else
changed. Same optimisation level, same instructions, same trip count, one
variable. Until that is run, "TCG translation blocks are page-bounded" remains
a plausible and well-documented QEMU property that *fits* the data rather than
a mechanism this project has demonstrated.

**Much larger incidental result: the profile switch moved the whole suite.**
`over_target` went **58–59 of 63 on every debug record to 15 of 63 on
release** — scorecard `48/63 within hardware target`. The suite had been
reporting a near-total failure that was overwhelmingly an artefact of
measuring unoptimised codegen, exactly as
`B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL` predicted. The 15
remaining over-target entries (`syscall_dispatch` 661 ns vs 200,
`futex_wake_empty` 944 vs 500, `futex_wait_mismatch` 1507 vs 500,
`vfs_stat_root` 5920 vs 700, `vfs_stat_deep_2comp` 31046 vs 1400,
`isr_latency` 164652 cyc vs 37000, …) are now the first *credible* performance
findings this suite has produced, because they are the first measured on the
code that would ship. They should be triaged on their own merits — `vfs_stat`
at 22x and 8x target is the standout — and are not this entry's subject.

**Caveat on those 15, added after the fact.** This run's dispersion report
flagged five benchmarks at ≥5x `mean/min`, and `vfs_stat_root` — the one
singled out as "the standout" above — was among them at **12x**. I ran greps
and git commands during the QEMU boot, which is exactly the mistake recorded
in `TD-BENCH-RUNS-ARE-CONTAMINATED-BY-THE-AGENTS-OWN-COMMANDS`. The
over-target *set* is unlikely to be an artefact (a 22x miss does not come from
host noise), but the individual magnitudes from this run should be treated as
provisional until re-measured on an idle host. See the RECURRENCE note in that
entry for why `min_ns` does not fully rescue a 3 ms benchmark.

### [A] B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL. Every recorded benchmark ran at `opt-level = 0` and was scored against optimised-reference targets — **FIXED 2026-08-14, confirmed by measurement**

> **Resolution.** `scripts/boot-test.sh` now builds `--release` and stages from
> `target/x86_64-unknown-none/release/kernel` when `--bench` is passed, and
> `bench-history.py` records/compares a `profile` field so release and debug
> records are never diffed against each other. Confirmed end-to-end by run
> `fcd066231`: the release kernel built clean (0 warnings, 9m25s), booted, and
> **`over_target` fell from 58–59 of 63 on every debug record to 15 of 63** —
> scorecard `48/63 within hardware target`. The comparator correctly refused
> to diff against the six debug records. Quantified per-benchmark evidence is
> in the RESULT section of
> `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` above (e.g. `tcp_checksum_v4`
> 20667 → 1716 ns, `v6` 35048 → 1602 ns).
>
> Two things this did **not** settle, both tracked elsewhere and neither a
> reason to keep this entry open: (a) whether the *non-bench* boot test should
> also build release — that is **Q46**, still with the operator, and the
> default deliberately stays debug meanwhile; (b) the 15 benchmarks still over
> target, which are now genuine findings rather than codegen artefacts and
> need triage on their own merits.

**Where:** `scripts/boot-test.sh:602` (`"$CARGO" build`) and `:218`
(`KERNEL_BIN=".../target/x86_64-unknown-none/debug/kernel"`); `Cargo.toml`
`[profile.dev]` (357–365) vs `[profile.release.package.kernel]` (370–373).

**What.** The boot test builds with a bare `cargo build` — no `--release` — and
stages the artefact out of `target/x86_64-unknown-none/**debug**/kernel`. The
benchmark suite is compiled into the kernel unconditionally; `--bench` only
changes which serial marker the script waits for (`BENCH_OK` instead of
`BOOT_OK`), it does not change the build. `[profile.dev]` sets only
`panic = "abort"`, and there is no `[profile.dev.package.kernel]`, so the
kernel is built at **`opt-level = 0`**.

So every number in `bench/history.jsonl` — all 5 records, all 63 benchmarks —
measures unoptimised codegen, and every one of them is scored against
`baselines.toml` targets taken from *optimised* Linux / Fuchsia / L4 / jemalloc
implementations.

**Evidence.** Disassembling the staged binary shows textbook `opt-level = 0`
output. `tcp_checksum_bench` reloads and re-spills the accumulator to the stack
around every single add:

```
805d7181:  addl  %ecx, %eax
805d7183:  movl  %eax, -0x64(%rbp)
805d7186:  movl  -0x64(%rbp), %eax     # reload of the value just stored
```

That is one store + one load per accumulation in a loop whose entire body is
one accumulation. (`llvm-objdump` ships with the rustup toolchain — see the
path in `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` — so this needs no binutils
install.)

**The irony.** `[profile.release.package.kernel]` already exists and is
deliberately tuned for exactly this — `opt-level = 3`, `codegen-units = 1`,
`strip = "none"` — with a comment explaining the per-package override. The
benchmark path has simply never used it.

**Why it matters.** This invalidates the *absolute* verdicts wholesale, and
they are the ones CLAUDE.md's benchmarking protocol is built on:

- The `over_target` list is not a list of subsystems that are too slow. It is
  mostly a list of subsystems compiled without optimisation. `tcp_checksum_v4`
  at 20000 ns against a 2000 ns target is a 10x miss that says nothing about
  the shipped code.
- "If a change regresses a benchmark by more than 10%, investigate before
  merging" cannot be applied to numbers whose baseline is debug codegen.
- The scale is wrong in the direction that matters: `opt-level = 0` → `3` on
  byte-loop code of this shape is routinely 5–20x. That dwarfs both the ~1.7x
  swing in `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` and the 25% canary
  tolerance, which means the noise work done so far has been tuning the
  measurement of the wrong binary.

*Relative* commit-to-commit comparisons are not destroyed — both sides are
debug — but they are still measuring optimisation-sensitive code paths whose
debug/release ratio is not uniform, so a debug-visible change need not be a
release-visible one.

**Same family as the three before it.** `TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN`
(the suite never ran), `B-BENCH-WATCHLIST-...` (the watch list never looked),
`B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION` (the diff named
innocents) — and now a suite that ran, reported, and was compared against
targets, while measuring a binary nobody intends to ship. A check that measures
the wrong thing is indistinguishable from a check that passes.

**Proposed fix.** Build the kernel `--release` for `--bench` runs, staging from
`target/x86_64-unknown-none/release/kernel`, and add an append-only `profile`
field to each `bench/history.jsonl` record so the comparator only ever compares
like with like. The 5 existing records must keep their meaning: absent
`profile` reads as `"debug"`, and a release record must never be diffed against
a debug one. This is a real cost — a second full kernel build, and a bench
history that restarts from zero same-profile records, which also resets the
≥6-record threshold that `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`
is waiting on. It is still the only honest option: a benchmark that does not
measure the shipped build is not a benchmark.

**Open sub-question:** whether the *non*-bench boot test should stay debug.
Keeping it debug preserves fast iteration and readable panics, at the cost of
two kernel builds in the tree and the risk that release-only miscompiles or
UB-dependent behaviour are only ever exercised on bench runs. Leaning toward
keeping the default boot test debug and making release the `--bench` path, but
this is worth the operator's view — see `open-questions.md`.

### [A] AUDIT 2026-08-14 — the softirq × hard-IRQ shared-lock class is clean. No action needed; recorded so it is not re-audited

**Why it was worth checking.** `softirq::process_pending` re-enables interrupts
(`kernel/src/softirq.rs`, module docs 51–56), so any lock held by a softirq
handler can be observed by a hard-IRQ handler that preempts it. That is
structurally the same failure mode as the rtl8139 deadlock and as
`B-COMPLETION-TIMER-IRQ-DEADLOCK`: the hard IRQ spins on a lock whose holder
cannot run until the IRQ returns. The intersection was believed empty only
because rtl8139 was the tree's single hard-IRQ lock acquisition — "empty by
accident" is not a property that stays true, so it needed enumerating rather
than assuming.

**What was audited.** Every callee reachable from the three softirq handlers
(`handle_timer` 355, `handle_sched` 434, `handle_irq_poll` 445):

| Callee | Lock discipline | Verdict |
|---|---|---|
| `sched::process_sleep_wakeups` (sched/mod.rs 5248) | atomic scan of `SLEEP_QUEUE`, no lock | clean |
| `sched::process_deferred_wakes` (sched/mod.rs 4897) | non-blocking wake path | clean |
| `ipc::timer::process_timer_expirations` (ipc/timer.rs 211) | explicitly non-blocking on `CP_TABLE`/`SCHED`, leaves the timer un-advanced on contention so the next tick retries | clean, and documented against `B-COMPLETION-TIMER-IRQ-DEADLOCK` |
| `ktimer::process_expirations` (ktimer.rs 323) | atomic scan of `TIMERS` | clean |
| `fs::cache::try_flush_expired` (fs/cache.rs 906) | `try_lock`, result deliberately discarded — retries in ~5 s | clean |
| `watchdog`, `kstat`, `loadavg`, `irq_storm`, `irqbalance`, `cpufreq`, `thermal` | zero `.lock()` calls; atomics only | clean |
| `rcu::tick` → `process_callbacks` (rcu.rs 483) | all three `CALLBACKS.lock()` sites (403, 486, 547) wrapped in `cpu::without_interrupts`, popping one callback per critical section and invoking it with the lock released | clean, and the comment at 393–401 records the observed 2/10 boot hang that motivated it |

`rcu` is the only softirq callee that takes a blocking lock at all, and it is
the one already hardened — the fix predates this audit and cites the boot hang
it was found by.

**Result: the intersection is empty, and empty by construction rather than by
luck.** Each site either uses atomics, uses `try_lock`, or masks interrupts for
the lock-hold window. No change was made.

**What would break it.** Adding a `.lock()` (not `try_lock`, not
`without_interrupts`-wrapped) to any callee of `handle_timer` — which is a wide
and growing list: it already fans out to 12 subsystems — while that same lock is
reachable from a hard-IRQ handler. The `handle_timer` fan-out is the risk
surface to re-check when a subsystem is added to it, not the whole kernel.

### [A] B-BENCH-CANARY-CERTIFIES-CLEAN-RUNS-THAT-CONTAIN-MULTI-X-STALLS. All three runs it passed had 5–8 benchmarks stalled ≥5x — MITIGATED 2026-08-14

**Where:** `kernel/src/bench.rs` (`maybe_canary_sample`, `CANARY_SAMPLE_EVERY = 8`)
and `scripts/bench-history.py` (the `Canary OK` verdict).

**What.** The mid-suite canary has never once fired. That was read as "the host
has been quiet"; it is not what the data says. Cross-checking each run's canary
verdict against the per-benchmark `mean/min` recorded in the same run:

| run | canary verdict | benchmarks with `mean/min` ≥ 5x |
|---|---|---|
| `be167dd90` | clean (endpoints 97%) | **8** — `ipc_channel` 23x, `page_alloc_free` 19x, `syscall_dispatch` 16x, `pick_next` 16x, `context_switch` 11x, `crypto_ed25519_sign` 8x, `dashboard_api_status` 8x, `ipc_channel_sync` 6x |
| `5a2002bac` | clean (spread 2%) | **5** — `page_alloc_free` 24x, `vfs_stat_deep` 15x, `vfs_stat_3comp` 12x, `crypto_ed25519_verify` 10x, `vfs_throughput_16k_write` 5x |
| `f74f97b6d` | clean (spread 16%) | **6** — `context_switch` 21x, `vfs_stat_deep` 16x, `vfs_stat_3comp` 14x, `vfs_throughput_16k_write` 8x, `dashboard_api_health` 7x, `crypto_ed25519_verify` 7x |

The run reported as the *cleanest* of the three — `5a2002bac`, spread 2%, the
one used to certify that four earlier benchmarks had merely been contaminated —
contained a benchmark whose mean was **24x its own minimum**.

**Why the canary cannot see this.** It samples the host *between* benchmarks,
once per 8 scored entries — 10 samples across 63 benchmarks. A stall confined
to one benchmark falls between two samples and leaves no trace in it. The
canary measures the gaps; the stalls are in the benchmarks.

**Why `mean/min` can.** It is computed from the benchmark's own iterations, so
it sees precisely the interval the canary skips. And the data was already being
recorded — the append-only `mean_ns` extension landed for a different reason
(the variance band) and turns out to answer this too.

**These are not intrinsically noisy benchmarks.** That was the obvious
alternative reading, and it is wrong. Across the three runs only
`ipc_channel_sync` is *persistently* elevated (6.0 / 3.9 / 4.6). Every other
high reading is spiky — `pick_next` 15.8 then 1.1 then 1.2; `syscall_dispatch`
16.1 then 1.2 then 1.2; `page_alloc_free` 19.3, 24.4, then 1.3. A benchmark
that is 16x dispersed in one run and 1.2x in the next is being disturbed, not
behaving that way.

**Nor is it one cold first iteration.** `vfs_stat_3comp` in `f74f97b6d`: min
1334082, mean 18349532, max 758926475 over 500 iterations. The single worst
iteration accounts for only ~8% of the total time, so the elevation is broad —
many slow iterations, not one outlier. Same shape for `crypto_ed25519_verify`
(max is ~7% of total over 50 iterations).

**Mitigation applied.** `scripts/bench-history.py` now reports per-benchmark
dispersion (`suspect_dispersion` / `report_dispersion`,
`DISPERSION_SUSPECT_RATIO = 5.0`) and the canary's verdict line no longer
claims "host load stable" — it now says only that the reference access cost was
steady *between* benchmarks, and points at the dispersion line. 6 new tests
(48 total, all passing), including the real `page_alloc_free` 24x shape from the
run the canary called clean.

**The threshold is deliberately unfitted.** Measured across the three records:
median benchmark 1.26–1.59, the large majority under 2, excursions at 5–25x,
and little in between. 5.0 sits in that empty band. It wants retuning once
release-profile records exist, since optimised benchmarks run for less wall
time and so present a smaller target to a burst.

**Not yet done — this reports, it does not correct.** A flagged benchmark's
recorded figure is still its *minimum*, which may well be sound; the flag says
"do not read movement here as signal", not "this number is wrong". Deciding
which is which needs a per-benchmark dispersion *baseline*, i.e.
`TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`, whose record count has just
been reset to zero by the debug→release profile switch.

**Lesson, the fourth of this shape.** After `TD-BENCHMARKS-...` (the suite never
ran), `B-BENCH-WATCHLIST-...` (the watch list never looked), and
`B-BENCH-COMPARATOR-...` (the diff named innocents): a canary that never fired,
read as evidence of quiet. Its own motivating case had already refuted the
first version of it, and the second version was written specifically to catch
per-benchmark bursts — yet it was still reporting "host load stable" over runs
containing 24x stalls. "It has never fired" is a claim about the check, never
about the world.

---

### B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY — 2026-08-14 — OPEN (`kernel/src/fs/vfs.rs`, `kernel/src/ipc/namespace.rs`)

`vfs_stat_root` — `Vfs::stat("/")`, the single cheapest path operation the VFS
can perform — costs **6151 ns** on the release-profile run (`min` of 500
iterations, and *not* flagged by the dispersion check in that run, so the number
is clean). The CLAUDE.md target for a cached lookup is 200–500 ns per component.
For a zero-component path that is roughly **12–30x over**.

**The hypothesis I started with was wrong, and measurement is what killed it.**
`VfsDcache::lookup` (`kernel/src/fs/vfs.rs:1189`) is an O(n) linear scan over
`VFS_DCACHE_SIZE = 1024` slots, and CLAUDE.md explicitly forbids linear scans in
VFS path lookup. It was the obvious culprit and I was one step from rewriting it
as a hash table. Instrumenting first (`bench_vfs_stat_breakdown`, this commit)
showed:

```
vfs_stat_breakdown: dcache 25 valid entries (of 1024), +550 hits +0 misses
```

**25 live entries, filled from index 0, 100% hit rate.** A hit-scan terminates
in ~25 iterations, not 1024 — the cost of a linear scan is a function of
*occupancy*, not capacity. The scan cannot account for microseconds. The
1024-slot scan remains a latent defect (it degrades as occupancy grows, and it
is the *miss* path that walks all 1024) and is tracked as such below — but it is
**not** this bug's cause. Had I "fixed" it I would have burned a refactor and
moved the number by nothing.

**Where the time actually goes.** Splitting `Vfs::stat` at its own seam —
`resolve_follow(path)` then `stat_resolved(&path)`:

```
vfs_stat_breakdown_full:      6191 ns
vfs_stat_breakdown_resolved:  2442 ns
  => resolve_follow ~3749 ns (61%) + stat_resolved 2442 ns (39%)
```

So path *resolution* is the larger half, and both halves are individually over
target.

**Prime suspect for the 3749 ns, not yet confirmed.** `resolve_follow`
(`vfs.rs:1553`) calls `namespace::resolve_path` (`ipc/namespace.rs:721`), which
via `resolve_path_for` (`:735`) takes **`PROCESS_NS.lock()`**, then
**`PROCESS_ROOT.lock()`**, then conditionally **`PROCESS_MOUNTS.lock()`** — three
global spinlocks — and performs `path.to_path_buf()`, a heap allocation, *even
in the trivial `ROOT_NAMESPACE` pass-through case where the answer is the input
unchanged*. That is a fixed per-resolution cost paid by every single VFS
operation in the system. `validate_path`, `normalize_path` (another alloc), the
`VFS_DCACHE.lock()`, and `entry.resolved.clone()` (another alloc) are the other
candidates in that 3749 ns.

**Explicitly not yet attributed.** The above is a reading of the code, not a
measurement, and the last time I reasoned this way about a hot path
(`B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`, prediction 2) I got the *sign*
wrong. The next step is to split `resolve_follow` the same way this commit split
`stat` — `namespace::resolve_path` vs `validate_path`+`normalize_path` vs the
dcache lock+clone — and let the numbers pick the target. Do not optimise any of
the four candidates before that split exists.

**Related, same shape, worse:** `vfs_stat_deep_2comp` = 33573 ns, ~16786 ns per
component against a 200–500 ns/component target. If the fixed per-resolution
prologue is the cause of `vfs_stat_root`, it does not explain this one — 2
components cost 5.4x one component, so there is a *per-component* cost here too.
Both need the same treatment.

#### PROSPECTIVE PREDICTION (written and committed before the stage-split run)

Same protocol as `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`: the prediction is
committed before the measurement exists, so it can be graded rather than
rationalised. Last time this protocol caught me getting a *sign* wrong; the
point is to let it do that again.

**Primitive costs from the same release run** (`bench/history.jsonl`, commit
`040049442`) — these are the anchors, not guesses:

| primitive | measured | what it bounds |
|---|---|---|
| `heap_alloc_free_64` | 184 ns | one alloc+free pair ⇒ a single alloc ≲ 180 ns |
| `sched_pick_next` | 40 ns | takes the run-queue lock ⇒ an uncontended spinlock is *cheap*, ≲ 20 ns |
| `context_switch` | 1275 ns | nothing here should approach this |

**What each stage actually does** (from the code, and this is the weak part —
inspection is exactly what was wrong about the dcache):

* `ns_translate` = `current_task_id()` + `owner_process()` (a `THREAD_OWNERS.lock()` + `BTreeMap::get`) + `PROCESS_NS.lock()` + get + `path.to_path_buf()` (**1 alloc**, of a 1-byte path) + `PROCESS_ROOT.lock()` + get → `None`. So **3 spinlocks + 3 map lookups + 1 alloc**.
* `validate_normalize` = a byte scan of `"/"` + `normalize_path` (**1 alloc**).
* `dcache_hit` = `VFS_DCACHE.lock()` + ~25 path compares + `entry.resolved.clone()` (**1 alloc**).

**Predictions, falsifiable:**

1. `ns_translate` < 400 ns.
2. `validate_normalize` < 400 ns.
3. `dcache_hit` < 500 ns.
4. **Therefore the three stages sum to well under the 3749 ns that subtraction
   attributed to `resolve_follow` — I predict the sum is < 1500 ns.** Three
   allocations at ≤180 ns and six-ish uncontended spinlocks at ≤20 ns simply
   do not reach 3.7 µs.
5. **If (4) holds, the subtraction is what was wrong.** The specific mechanism I
   expect: `Vfs::stat` feeds `stat_resolved` the *resolved* path, while the
   isolated `vfs_stat_breakdown_resolved` benchmark feeds it the literal `"/"`.
   If `resolve_path("/")` returns something longer than `"/"`, then the
   `stat_resolved` inside `stat` is doing strictly more work than the isolated
   measurement of it, and subtraction charges that surplus to `resolve_follow`.
   **In that case the real culprit is `stat_resolved` — `resolve_mount`'s
   `VFS.lock()` + linear mount scan + `to_path_buf()` + `Arc::clone`, then
   `fs.lock().stat()` — and I will have misattributed the cost twice in a row
   on this one benchmark.**

This run therefore carries a direct measurement of `resolve_follow`
(`Vfs::resolve_path` is a public alias for it) *alongside* the subtraction, plus
a print of what `resolve_path("/")` actually returns. Prediction 5 is decided by
those two lines and needs no further argument.

**Standing caution, restated:** predictions 1–3 lean on the same
fine-grained cost reasoning that got the tcp_checksum sign wrong. Treat a hit as
weak confirmation and a miss as strong disconfirmation.

#### RESULT — 2026-08-14, release profile, commit `f9807f73a` (`build/stage-split.log`)

```
vfs_stat_breakdown: full 6423ns = resolve_follow ~3843ns + stat_resolved 2580ns
vfs_stat_breakdown: resolve_follow measured directly 3504ns (vs 3843ns by subtraction)
vfs_stat_breakdown: resolve_follow 3504ns = ns_translate 1948ns + validate_normalize 318ns + dcache_hit ~1238ns
vfs_stat_breakdown: resolve_path("/") -> "/" (1 bytes)
vfs_stat_breakdown: dcache 25 valid entries (of 1024), +1100 hits +0 misses over the run
```

| # | prediction | actual | verdict |
|---|---|---|---|
| 1 | `ns_translate` < 400 ns | **1948 ns** | **MISS, 4.9x** |
| 2 | `validate_normalize` < 400 ns | 318 ns | hit |
| 3 | `dcache_hit` < 500 ns | **~1238 ns** | **MISS, 2.5x** |
| 4 | three stages sum < 1500 ns | **3504 ns** | **MISS, 2.3x** |
| 5 | "the subtraction is what was wrong" | subtraction was **right** | **disconfirmed** |

**Prediction 5 was wrong in the way that matters most: it was an escape
hatch.** It said that if the stages came out cheap, the *subtraction* must be
the error and the real culprit would be `stat_resolved`. Both halves are
refuted outright by the two lines this run was built to print:
`resolve_path("/")` returns `"/"` unchanged (1 byte), so the different-inputs
hazard that would have made subtraction unsound never existed on this path; and
the direct measurement (3504 ns) agrees with the subtraction (3843 ns) to within
9.7%. `resolve_follow` really is ~55% of the whole stat, exactly where
subtraction put it. I did **not** misattribute the cost twice — I misattributed
it once, to the dcache, and then predicted I had misattributed it again in the
opposite direction. The second guess was as wrong as the first.

**Why 1 and 3 missed: a bad anchor, and it was bad by misreading the code.**
The prediction leaned on "`sched_pick_next` = 40 ns, and it takes the run-queue
lock, therefore an uncontended spinlock is ≲ 20 ns." That premise is simply
false about the benchmark. `bench_sched_pick_next` builds a **local**
`PriorityRoundRobin::new()` on the stack and calls `rq.pick_next()` directly —
it never touches `SCHED.lock()`. **It takes no lock at all.** So the one number
in the anchor table that was supposed to bound lock cost was measuring a
lock-free path, and the 20 ns figure was manufactured from nothing. This is the
same failure as the dcache: a claim about what the code does, asserted from
reading rather than from measuring, load-bearing for the conclusion.

**The cost model the measurement actually supports.** Solving the three stages
against their contents (3 locks + 3 map lookups + 1 alloc = 1948; 1 lock + ~25
path compares + 1 alloc = 1238; a byte scan + 1 alloc = 318) gives a consistent
fit at roughly:

| primitive | implied cost under QEMU-TCG |
|---|---|
| uncontended **global spinlock** acquire+release | **~500 ns** |
| heap alloc (small) | ~180 ns (matches `heap_alloc_free_64`) |
| one dcache path compare | ~21 ns |

A lock is ~3x an allocation here, and the whole path is **lock-dominated**: 4
global spinlocks across `resolve_follow` alone, ~2000 ns of its 3504. Every
optimisation instinct I had was aimed at allocations and at scan length, and
both are minor terms.

**But that model is derived, not measured, and deriving is what just failed
twice.** So the next run adds `bench_spinlock_uncontended` to measure the
primitive directly. The suite has anchors for allocation, context switch and
syscall dispatch but none for the single most common operation in the kernel,
which is precisely why a fabricated 20 ns figure went unchallenged.

**Consequences (tracked as `B-NAMESPACE-RESOLVE-TAKES-3-GLOBAL-LOCKS-TO-RETURN-ITS-INPUT` below).**
`ns_translate` is 1948 ns — 56% of `resolve_follow`, 30% of the entire stat —
and for a process in the root namespace with no chroot and no volume mounts
(i.e. every process on a normal desktop) it does all of that work to **return
its input unchanged**.

---

### B-NAMESPACE-RESOLVE-TAKES-3-GLOBAL-LOCKS-TO-RETURN-ITS-INPUT — 2026-08-14 (`kernel/src/ipc/namespace.rs`)

**Measured, not inferred:** `ns_translate` = **1948 ns**, which is 56% of
`resolve_follow` and **30% of an entire `stat("/")`**. See the RESULT section of
`B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY` above.

`namespace::resolve_path` is called before **every** path operation in the VFS —
read, write, stat, open, mkdir, unlink, all of it. For a process in the root
namespace with no chroot and no volume mounts — which is every process on a
normal desktop, and every process in this kernel today — the entire function
body is:

1. `current_task_id()` — cheap, an atomic load.
2. `owner_process(task_id)` → **`THREAD_OWNERS.lock()`** + map get.
3. **`PROCESS_NS.lock()`** + map get → `ROOT_NAMESPACE`.
4. `path.to_path_buf()` — a heap allocation.
5. **`PROCESS_ROOT.lock()`** + map get → `None`.
6. Return the path, byte-for-byte identical to the input.

**Three global spinlock acquisitions and one heap allocation, to return the
argument unchanged.** At the measured ~500 ns per uncontended global spinlock
under TCG, the locks alone are ~1500 of the 1948 ns.

This is not a micro-optimisation target, it is a missing fast path. The
structure charges every path operation in the system for a feature (containers)
that is not in use, and the charge is paid in the most expensive primitive
available.

**The fix** — a global "namespace features are in use" flag, checked with one
relaxed atomic load before any lock is taken:

* An `AtomicBool` (`NS_FEATURES_ACTIVE`) set with `Release` ordering at the
  three sites that can make namespace state non-trivial: inserting into
  `PROCESS_NS`, into `PROCESS_ROOT`, and into `PROCESS_MOUNTS`.
* `resolve_path_for` loads it with `Acquire`; if clear, it returns immediately.
* **The flag is never cleared.** Clearing it on the last teardown would
  introduce a race with a resolve already in flight, and the cost of staying on
  the slow path after containers have been used once is exactly the cost we have
  today. Monotonic is the sound choice and it is deliberate, not an oversight.

This is the standard rarely-used-feature pattern (Linux's static keys). It does
not change behaviour for any process: with the flag clear, no process has a
namespace, a root, or a volume, so every branch the slow path could take is the
identity branch — which is what makes the fast path a refactor rather than a
semantic change.

**The allocation in step 4 survives this fix** and is the correct next target:
`resolve_path` returns `PathBuf`, so the pass-through allocates a copy that
`resolve_prologue` immediately re-allocates in `normalize_path`. Returning
`Cow<'_, Path>` would remove one of the two. Deferred until the lock fix is
measured, because at ~180 ns it is a third of a single lock and chasing it first
would have been another instance of optimising the minor term.

#### PROSPECTIVE PREDICTION (recorded before the fix is built)

Same protocol, and this time with a directly measured anchor rather than a
fabricated one — the next run also adds `bench_spinlock_uncontended`.

1. `bench_spinlock_uncontended` comes out in **300–700 ns**. This is the load-
   bearing one: the whole cost model above stands or falls on it. If it lands
   below ~150 ns, the lock attribution is wrong and something else in
   `ns_translate` is the real cost.
2. `ns_translate` drops from 1948 ns to **< 150 ns** (one atomic load, one
   allocation removed only if the `Cow` change lands too — so expect ~180 ns if
   the allocation stays; I predict the allocation is skipped entirely on the
   fast path, hence < 150).
3. `resolve_follow` drops from 3504 ns to **1700–2000 ns**, now dominated by
   `dcache_hit`.
4. Full `vfs_stat_root` drops from ~6151 ns to **~4400–4700 ns**, a ~28%
   improvement on a benchmark I twice tried to fix by looking at the wrong
   subsystem.

**If (1) holds but (2) does not**, the fast path is not being taken — most
likely because some process really did set one of the three maps during boot,
which would itself be worth knowing and is why the benchmark prints the flag.

#### RESULT — 2026-08-14, two post-fix release boots ✅ FIXED

The first post-fix boot reported `namespace fast path DISABLED
(NS_FEATURES_ACTIVE=true)` — the pre-registered fallback clause above, firing
verbatim. The cause was not "some process set one of the maps during boot" but
something better: **the namespace self-tests themselves**.
`test_process_attach_detach`, `test_process_root` and `test_volume_mounts` call
`attach`/`set_root`/`add_volume`, which arm the monotonic flag, and nothing
disarmed it. So the self-tests were permanently degrading the VFS of the kernel
they had just finished validating — every path operation for the rest of the
boot paid three global spinlocks to exercise a feature that no longer had a
user. Fixed by asserting `reset_ns_features_if_trivial()` at the end of
`self_test()`, which doubles as a leak check: it can only succeed if every
namespace test cleaned up its process state.

Two boots after that fix (the first aborted on an unrelated flake — see
`B-FASTPY-SLEEP-SELF-TEST-IS-FLAKY` — so both are reported):

| # | prediction | pre-fix | run A | run B | grade |
|---|---|---|---|---|---|
| 1 | uncontended tracked lock **300–700 ns** | 628 | 448 | 632 | **HIT** (all three in band) |
| 2 | `ns_translate` **< 150 ns** | 1670 | 347 | 264 | **MISS** (1.8x over) |
| 3 | `resolve_follow` **1700–2000 ns** | 3138 | 2488 | 1627 | **UNPROVEN** (band narrower than the noise) |
| 4 | `vfs_stat_root` **4400–4700 ns** | 5930 | 2971 | 4394 | **HIT** (run B lands 0.14% under the band) |

**(1) HIT, and it was the load-bearing one.** The previous prediction on this
benchmark failed because its lock cost came from a *fabricated* anchor; this one
was measured first, and everything built on it held.

**(2) MISS, and the miss was avoidable by reading a type signature.** The
prediction said "I predict the allocation is skipped entirely on the fast path,
hence < 150". It cannot be: `resolve_path` returns `PathBuf`, so *every* return
allocates, fast path or not. The residual ~264 ns is one atomic load plus that
allocation. This is not a measurement surprise — it is a claim contradicted by
the function's own declaration, which was there to be read. It also promotes the
deferred `Cow<'_, Path>` change from "the correct next target" to "the only
remaining term".

**(3) UNPROVEN, and that is the more useful result.** 1627 and 2488 straddle the
band. The two runs differ by 1.53x while the band spans 1.18x — the prediction
was finer-grained than the instrument meant to grade it. Predicting to a
precision the measurement cannot resolve yields a verdict that is noise wearing
a grade's clothes, which is worse than no verdict. See
`TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK` below, where the same two runs
disagree by 1.67x on two byte-identical benchmarks.

**(4) HIT.** Predicted "~28% improvement"; measured −26% (5930 → 4394). This is
the benchmark twice attacked in the wrong subsystem (first the dcache, then the
subtraction). The third attempt — measure the anchor, then follow the
measurement — worked on the first try.

---

### B-LOCKDEP-CLASS-LOOKUP-IS-A-LINEAR-SCAN-ON-EVERY-LOCK — 2026-08-14 (`kernel/src/lockdep.rs`)

**Measured, and it is the largest single overhead found this session.** The lock
microbenchmark added to grade the namespace fix answered a question nobody had
asked it:

```
lock acquire+release: raw 30ns, tracked 632ns, no-lockdep 232ns, no-stats 656ns
lock overhead: total +602ns = lockdep 400ns + preempt 29ns + rdtsc 57ns + unexplained 116ns
```

`raw` is `spin::Mutex`; `tracked` is `crate::sync::Mutex`, the type every global
in the kernel uses. **The tracked mutex costs 21x the raw one, and two thirds of
the difference is lockdep.** Confirmed across both post-fix boots: 400/602 ns
(66%) and 281/430 ns (65%).

The cause is not that validation is expensive. It is that the *lookup* is
`O(classes)`:

```rust
fn find_or_register_class(lock_addr: usize, name: &[u8]) -> Option<u16> {
    let count = CLASS_COUNT.load(Ordering::Relaxed) as usize;
    for i in 0..count.min(MAX_CLASSES) {          // <-- up to 128 iterations
        if unsafe { CLASSES[i].id } == lock_addr { return Some(i as u16); }
    }
    ...
```

and `find_class` — called from `lock_release` — is the same scan again. So every
lock operation in the kernel walks the class table **twice**, and `MAX_CLASSES`
is 128. This is exactly the "linear scan on a hot path" CLAUDE.md's performance
section forbids, hiding inside the *debugging* infrastructure rather than the
code being debugged, which is why no amount of reading the subsystem under
investigation would ever have found it.

Two further consequences worth stating because they distort the whole benchmark
suite:

* **The cost is positional.** A lock class registered early is found in a few
  iterations; one registered late pays the full scan. So the same lockdep call
  is cheap or expensive depending on *boot order*, and a benchmark's own lock —
  registered last, at benchmark time — pays the worst case. The 400 ns figure is
  therefore an upper bound on the average, not the average.
* **Every benchmark in this suite that takes a lock is partly measuring this.**
  `syscall_dispatch` (653–699 ns), `futex_wake_empty` (953 ns) and the VFS
  numbers all include it.

**The fix** (implemented in the same change as this entry): an open-addressed
hash index from lock address to class slot, Fibonacci-hashed and linearly
probed, 512 buckets for 128 classes so the load factor stays at 25%. This is
what Linux does (`classhash_table`, `kernel/locking/lockdep.c`). Entries are
append-only, so a probe run is contiguous and stopping at the first empty bucket
is correct.

**This fix is what makes the tempting question go away.** The obvious reaction to
"lockdep costs 400 ns per lock" is to gate it to debug builds, as Linux does with
`CONFIG_PROVE_LOCKING` — trading deadlock detection in production for lock speed.
That would have been a real architectural fork worth escalating. It is moot: the
validator was never inherently expensive, its index was. Keep both.

**The optimisation is guarded by a test that can actually fail.** A hash that
silently *misses* a registered class is the dangerous failure: `find_or_register_class`
would then register a second class for the same lock, that lock's dependency
edges would split across two graph nodes, no cycle would ever be found through
it, and lockdep would go quiet — looking exactly as healthy as a kernel with no
deadlocks. So the linear scan is not deleted, it is demoted to an oracle:
`test_class_hash_index()` asserts the hash and the scan agree on every registered
class, agree on absence, that double registration yields one class, and — using
a colliding address it *searches for* rather than hopes for — that the probe
sequence survives a bucket collision.

#### PROSPECTIVE PREDICTION (recorded before the fix is booted)

1. `lock_tracked` drops from ~632 ns to **250–330 ns**, i.e. close to the
   measured `no-lockdep` figure (232 ns) plus a hash lookup and probe (~2 memory
   references, call it 20–80 ns under TCG). If it lands *below* 232 ns something
   is wrong — the index cannot be cheaper than not running at all.
2. `lockdep` in the overhead split drops from ~400 ns to **< 100 ns**.
3. The knock-on: `syscall_dispatch` (653–699 ns across four boots, target 200)
   improves by **at least 15%**, because it takes tracked locks. This is the
   riskiest of the three — if syscall dispatch does *not* move, then either it
   takes no tracked lock or the lock is registered early enough to have been
   cheap already, and the "every benchmark is partly measuring lockdep" claim
   above is overstated and must be narrowed.
4. `lockdep classes registered` (newly printed) comes out **> 40**. If it is in
   single digits, the scan was never long and the 400 ns has some *other* cause
   inside `lock_acquire` — most likely `smp::current_cpu_index()` or the
   re-entrancy guard — and this whole diagnosis is wrong.

#### RESULT — 2026-08-14, release boot ✅ FIXED

```
[lockdep]   class hash: OK (3 classes verified vs scan, bucket collision handled)
[bench]   lock acquire+release: raw 25ns, tracked 274ns, no-lockdep 223ns, no-stats 301ns
[bench]   lock context: 43 lockdep classes registered
[bench]   lock overhead: total +249ns = lockdep 51ns + preempt 29ns + rdtsc 56ns + unexplained 113ns
[bench] SCORE lock_uncontended 274 500 PASS
```

| # | prediction | before | after | grade |
|---|---|---|---|---|
| 1 | `lock_tracked` **250–330 ns** | 632 | **274** | **HIT** |
| 2 | lockdep's share **< 100 ns** | 400 | **51** | **HIT** (7.8x) |
| 3 | `syscall_dispatch` improves **≥ 15%** | 653–699 | **699** | **MISS** (0%) |
| 4 | **> 40** classes registered | — | **43** | **HIT** |

**The tracked mutex went from 21x the raw spinlock to 11x, and
`lock_uncontended` moved from OVER to PASS** (274 vs the 500 ns target). Knock-on
in the same boot: `vfs_stat_root` 4394 → **3344 ns**, so with the namespace fast
path the total on that benchmark is **5930 → 3344, −44%**.

**(3) MISS, and the pre-registered consequence is honoured rather than
explained away.** The prediction said: *"if syscall dispatch does not move, then
the 'every benchmark is partly measuring lockdep' claim is overstated and must be
narrowed."* It did not move — 699 ns, identical to the best of the four pre-fix
boots. **Narrowing it: the claim was overstated.** Lockdep taxed benchmarks that
take `crate::sync::Mutex` *specifically*, which is the VFS/namespace path, not
"every benchmark that takes a lock". `syscall_dispatch` evidently takes none, or
takes a different lock type (`PreemptSpinMutex`, which is a distinct type with
distinct overhead — a distinction this session already had to write a comment
about in `bench.rs`). `syscall_dispatch` at 3.5x its 200 ns target is therefore
still unexplained and remains open.

**The coherence gates from `TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK` shipped
in the same boot and reported a clean run:** drift 3331 → 3353 ns (0%),
parts/whole 96%. That is the *quiet* outcome, so it proves only that the gates do
not fire spuriously — **it does not prove they fire.** They have not yet been
observed rejecting a run, and until they have, they carry exactly the weakness
this file keeps documenting. The two incoherent runs that motivated them are
recorded above, so the next drifting boot is the test.

**Weak spot in the new test, recorded rather than glossed:** `test_class_hash_index()`
runs from `lockdep::self_test()`, which executes early in boot when only **3**
classes are registered — but the pathology it guards against (a probe run
walking into a collision) needs a *populated* table, and by benchmark time there
are 43. The synthetic collision case is what carries the test today; the
verify-every-registered-class part is checking 3 of the eventual 43. It should be
re-run late in boot as well. Tracked as
`TD-LOCKDEP-HASH-TEST-RUNS-BEFORE-THE-TABLE-IS-POPULATED`.

---

### TD-LOCKDEP-HASH-TEST-RUNS-BEFORE-THE-TABLE-IS-POPULATED — 2026-08-14 — ✅ FIXED 2026-08-14 (`kernel/src/lockdep.rs`, `kernel/src/main.rs`)

`test_class_hash_index()` verifies the O(1) class index against a linear-scan
oracle, but it is called from `lockdep::self_test()` during early boot, when the
class table holds **3** entries. By the time the kernel is doing real work it
holds **43**. So the "every registered class is found at the index the scan
reports" assertion — the one that would catch a probe-sequence bug — is
exercised at 7% of the table size it needs to defend.

The synthetic part of the test (register a fresh address, then register a
deliberately colliding one and check both resolve) does not depend on table size
and is doing the real work today. That is why this is tech debt and not a hole:
the collision path *is* covered, just not at realistic occupancy.

**Proper fix:** expose it as `pub fn verify_class_index()` and call it a second
time late in boot — after driver/subsystem init, when the table is full — so the
oracle comparison runs against all 43 classes. It must run on every boot, not
only `--bench` boots, or it inherits the "check that only runs when you're
already looking" problem.

> **Resolution.** Done as described; the call takes a `when` label so the two
> runs are distinguishable in the log and the vacuous early pass cannot be
> misread as the meaningful one:
>
> ```
> [lockdep]   class hash (early): OK (3 classes verified vs scan, bucket collision handled)
> [lockdep]   class hash (populated): OK (31 classes verified vs scan, bucket collision handled)
> ```
>
> **The placement was itself the interesting part, and got it wrong on the first
> attempt.** The late call went in next to the deferred-benchmark spawn, which
> reads as "late in boot" — but that sits *after* `BOOT_OK`, and
> `boot-test.sh` kills QEMU at `BOOT_OK` unless `--bench` is given. So the first
> version printed nothing on a normal boot test: a check that would have run only
> on benchmark boots, i.e. only when someone was already looking, which is the
> precise failure mode it was added to prevent. Moved above the `BOOT_OK` marker,
> with a comment at the site saying why it must stay there. Verified by the
> absence-then-presence of the line across two boots, not by reading the code.

**Residual, not worth a separate entry:** 31 classes at `BOOT_OK` versus 43 by
benchmark time — the last dozen register during post-boot activity. Coverage is
now 72% of the eventual table rather than 7%, and the synthetic collision case
covers the probe path independently of occupancy.

---

### TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK — 2026-08-14 (`kernel/src/bench.rs`)

Two byte-identical benchmarks, in the same boot, disagreed by 1.67x:

```
SCORE vfs_stat_root 2971 ...
[bench] vfs_stat_breakdown_full: min=25808 cycles (4976ns) ...
```

Both are `run(..., 500, || black_box(Vfs::stat("/")))`. Nothing distinguishes
them but *when in the boot they ran*. In the next boot the same pair came out
4394 and 4306 — coherent. So the harness's min-of-500 is sometimes accurate and
sometimes 1.7x off, and **nothing in the output says which kind of run you are
reading.**

The consequences are not hypothetical; they are the two runs above:

* Run A attributed `stat_resolved` 2531 → 4109 ns, a 62% "regression" caused by
  a change that cannot touch it.
* Run B printed `full 4306ns = resolve_follow ~0ns + stat_resolved 5762ns` — the
  subtraction saturated at zero because a *part* measured larger than the
  *whole*. That is arithmetically impossible and it was printed without comment.
* Run A's parts summed to 133% of its whole. Also printed without comment.

This is the project's recurring defect class in its purest form: the check was
*there* — the code deliberately measures `resolve_follow` both directly and by
subtraction, with a comment explaining that a disagreement would indict the
subtraction — and then prints both numbers side by side and says nothing when
they disagree by 2.9x. **A check whose failure is not distinguishable from its
success is not a check, it is a decoration.**

> **Resolution (same change).** Two gates, both of which say the word WARNING:
>
> * **Drift gate.** The first measurement (`vfs_stat_breakdown_full`) is repeated
>   verbatim at the *end* of the block as `..._full2`. The two are the same code
>   over the same input, so any difference is pure measurement drift across the
>   width of the block, and it bounds how much of every stage difference is real.
>   Over 25% and the run is declared not internally coherent and unusable for
>   attribution.
> * **Parts/whole gate.** `resolve_direct + stat_resolved` must land within
>   75–125% of `full`, or the stage attribution is declared "not arithmetic, it
>   is noise".
>
> The same discipline is applied to the new lock benchmark, which prints
> `unexplained` as an explicit residual and warns when the components exceed the
> total they were subtracted from.

**Not fixed:** the harness still reports a single `min` with no confidence
interval, so a *single* benchmark with no in-block replicate (i.e. all the
others) remains ungraded for coherence. The proper fix is for `run()` itself to
take two interleaved sample sets and report their disagreement, making every
benchmark self-checking rather than just this one. Tracked here; not blocking.

---

### B-FASTPY-SLEEP-SELF-TEST-IS-FLAKY — 2026-08-14 (`kernel/src/proc/spawn.rs:15508`)

`self_test_fastpy_slateos_sleep()` failed one release boot and passed the next
with no relevant code change in between:

```
[spawn]   FAIL: fastpy-sleep (ring 3) — reached Zombie but exit code was Some(3),
          expected 0 (3 = a clock read was 0 or the observed sleep delta was < 40000000 ns)
```

The tool printed its measured delta: **36 818 000 ns for a `time.sleep(0.05)`**,
i.e. the sleep returned **26% early**, against a 40 ms lower bound. The kernel's
own `[sched] sleep_ns` test in the same boot passed (`slept 20.459ms for 20ms
request`), so whatever is short is not the scheduler's `sleep_ns` at a 20 ms
scale.

Two candidate causes, not yet separated:

1. **The sleep genuinely returns early at 50 ms** — a wakeup-deadline rounding or
   timer-phase bug that a 20 ms request happens not to expose.
2. **`clock_realtime()` advances more slowly than real time** during the sleep,
   so a correct 50 ms sleep *reads* as 36.8 ms. Ratio 50/36.818 = 1.358, which is
   suspiciously close to nothing in particular, but the two clocks the test
   compares (the scheduler's timer and `clock_realtime`) are different sources
   and their agreement is exactly what the test implicitly assumes and never
   checks.

Distinguishing them is cheap and should be done before touching anything: have
the harness log its own `clock_realtime()` delta across the child's lifetime
next to the child's measured delta. It already reads both — guard #2 in the
doc comment — but only compares each against the bound, never against each
other. If the kernel-side delta is ~50 ms while the child's is ~37 ms, it is
cause (2) and the bug is in the userspace clock path, not the sleep.

Impact today: an intermittently red boot test, which is corrosive — a suite that
cries wolf gets its failures ignored, and this is the only ring-3 test of the
blocking-sleep path. Not lane-A-exclusive (the tool is fastpy/userspace), but
the timekeeping and `SYS_SLEEP` sides are, and the harness is in
`kernel/src/proc/spawn.rs`.

#### CORRECTION 2026-08-14 — the proposed discriminator cannot discriminate

The plan above ("have the harness log its own `clock_realtime()` delta next to
the child's") **would not have separated the two causes**, because the two
numbers it compares come from *the same clock*. `SYS_CLOCK_REALTIME` returns
`timekeeping::clock_realtime()`; the harness calls
`timekeeping::clock_realtime()`. Under cause (2) — that clock running slow —
both readings compress by the same factor and the comparison shows nothing.
The test would have been "instrumented" and still blind: one more instance of
this file's recurring defect, *a check that cannot fire is indistinguishable
from a check that passes.*

The real discriminator was already in the tree, unread. Reading the call chain:

| stage | clock |
|---|---|
| `sleep_ns` computes and enforces its deadline | `hrtimer::now_ns()` → **HPET** (`kernel/src/hrtimer.rs:147`) |
| the child, and the harness, measure the elapsed time | `timekeeping::clock_realtime()` → **TSC**, via `clock_monotonic()` (`kernel/src/timekeeping.rs:154`) |

So the sleep is *enforced* against one oscillator and *measured* against
another, and the test silently assumes the two agree. That assumption is the
untested one, and it is the whole bug surface:

- If HPET and TSC agree across the window, the sleep really did return early —
  **cause (1)**, a deadline/timer-phase bug in `sleep_ns`.
- If HPET says ~50 ms while TSC says ~37 ms, the sleep was correct and the
  **TSC calibration** (`bench::tsc_freq()`) is off by that ratio — cause (2),
  and then it is not a userspace clock bug at all but a kernel calibration one,
  which would also skew every `clock_realtime()` consumer and every
  wall-clock-derived figure in the tree.

Note the observed ratio: 50 / 36.818 = **1.358**. The entry above called that
"suspiciously close to nothing in particular" — but as a *TSC calibration*
error it needs no numerological explanation; a mis-measured `tsc_freq` can land
anywhere, and under TCG the calibration loop is exactly the kind of thing a
busy host perturbs. That reading also explains the flakiness the entry opens
with: a calibration performed once per boot, on a host whose load varies, gives
a different scale factor on each boot — so the same correct sleep reads 50 ms
on a quiet boot and 37 ms on a busy one. **The intermittency is evidence for
cause (2), and the original framing had no account of it at all.**

The instrument therefore is: sample **both** `hrtimer::now_ns()` and
`timekeeping::clock_realtime()` either side of the child's lifetime, print both
deltas and their ratio, and print them on the *failure* path too — today
`kernel_elapsed` is computed at `spawn.rs:15623`, *after* the guard-#1 early
return at 15611, so on the exact runs that fail, the one number that would
explain the failure is never printed.

**Prediction P16** (registered before the measurement exists): on a boot where
the child reports < 40 ms, the HPET delta will exceed the TSC delta by >= 1.2x
— cause (2). MISS if the two agree within 5%, which puts it back on `sleep_ns`.

---

### TD-BASELINES-TOML-IS-INVALID-TOML-AND-NOTHING-READS-IT — 2026-08-14 — ✅ FIXED 2026-08-14 (`bench/baselines.toml`, `scripts/test-bench-history.py`)

`bench/baselines.toml` — the file CLAUDE.md names as the place performance
baselines live, and which ~30 comments across `kernel/src/bench.rs` cite as
their source — **did not parse as TOML.** It carried two `[compositor_frame_4k]`
tables, at lines 296 and 389, which is a hard error in every conforming parser:

```
tomllib.TOMLDecodeError: Cannot declare ('compositor_frame_4k',) twice
                         (at line 389, column 21)
```

The two disagreed about the **unit**: `target_ns = 2000000` in one,
`target_ms = 2.0` in the other. Only one carried the measured figure and the
optimisation history (48.6 ms → 21.4 → 15.8 → 11.9 → 10.6 ms). So the file had
been carrying two contradictory records of the same benchmark, and a parser
that tolerated duplicates would have silently taken whichever came last.

**Why it survived: nothing reads the file.** Every reference to it in the tree
is a *comment*. `kernel/src/bench.rs` hard-codes each target as a literal with
`// Target from baselines.toml: < 200 ns` beside it; `scripts/bench-history.py`
never opens the file. So the file *looked* like the authority while the real
authority was ~60 scattered literals in Rust, and no parser was ever pointed at
the thing that was supposed to be the source of truth.

**This is the fifth instance of the same defect class**, after
`TD-BENCHMARKS-...` (the suite never ran), `B-BENCH-WATCHLIST-...` (the watch
list never looked), `B-BENCH-COMPARATOR-...` (the diff named innocents) and
`TD-BENCH-CANARY-...` (the canary never fired). The invariant keeps holding: *a
check that cannot fire is indistinguishable from a check that passes.* Here it
went one step further — the artefact could not even be **loaded**, and that too
was indistinguishable from health, because loading was never attempted.

> **Resolution.** The duplicate table is merged (the poorer one removed, with a
> comment at the site recording why). `scripts/test-bench-history.py` gained
> `test_baselines_is_valid_toml()`, which `tomllib.load`s the real file — so the
> file is now machine-read for the first time and a duplicate or syntax error
> fails the suite. The test also asserts every table names a target in some
> unit, matched by `target*` **prefix** rather than an enumerated list (the
> units are open-ended by design: `target_accesses_over_nop` and
> `target_accesses_delta` exist because TCG harness overhead swamps the
> absolute number, and an enumerated list would silently under-report the day
> it wasn't extended). Calibration constants and host metadata opt out via a
> declarative `not_a_target = true` in the data rather than a name list in the
> test. Writing that assertion immediately found four more tables to classify.
> 16 checks pass.

**Not fixed — the duplication itself.** Targets still live in two places: this
file and the literals in `bench.rs`, with nothing keeping them in sync, so they
can drift silently and at least one (`vfs_stat_root`: 700 ns in the file) should
be re-derived anyway. The proper fix is for the kernel's scorecard to be checked
against the parsed file by `bench-history.py`, so the file becomes the authority
it already claims to be. Blocked on nothing but effort; tracked here.

#### FOLLOW-UP 2026-08-14: with the file finally parseable, the drift is measurable — and it is near-total

Making `baselines.toml` load was worth doing for its own sake, but the first
thing a working parser bought was a number for the damage. Matching the 63
benchmark names the kernel prints against the 57 baseline tables:

| | count |
|---|---|
| benchmarks measured by the kernel | 63 |
| baseline tables in the file | 57 |
| **matched by name** | **30** |
| measured with no baseline at all | 33 |
| baselines naming a benchmark never measured | 27 |

**Less than half of what runs has a baseline it can be compared to.** And the
two lists are not describing different work — they are largely the *same*
benchmarks under two names, drifted apart because nothing ever had to reconcile
them:

| kernel prints | baselines.toml calls it |
|---|---|
| `syscall_dispatch` | `syscall_trivial` |
| `page_fault` | `page_fault_anon` |
| `tcp_checksum_v4` | `net_tcp_checksum_v4_1460b` |
| `tcp_checksum_v6` | `net_tcp_checksum_v6_1460b` |
| `vfs_stat_deep` | `vfs_stat_deep_2comp` |
| `vfs_throughput_16k_read`/`_write` | `vfs_throughput_16k` |
| `heap_alloc_free_64` | `heap_alloc_small` |
| `ipc_channel` | `ipc_channel_roundtrip` |
| `ipc_pipe` | `ipc_pipe_roundtrip` |
| `ipc_eventfd` | `eventfd_signal_read` |
| `ipc_semaphore` | `semaphore_signal_wait` |
| `firewall_check` | `net_firewall_inbound_check` |
| `dns_build_query` | `net_dns_build_a_query` |
| `io_ring_nop` | `iouring_sqe_submit` |
| `isr_latency` | `interrupt_dispatch` |
| `service_connect` | `service_connect_accept` |
| `cp_notify_wait_rt` | `cp_notify_wait_roundtrip` |
| `net_tcp_conn_lookup` | `net_tcp_conn_table_scan` |

That is 18 of the 33 unmatched accounted for as pure renames. The remainder
split into benchmarks genuinely lacking a baseline (`vfs_stat_root`,
`vfs_read_256`, `vfs_write_256`, `vfs_readdir`, `vfs_stat_3comp`,
`http_gzip_*`, `ipc_channel_sync`, `net_arp_lookup`, `net_checksum`,
`net_ethernet_parse`, `net_ipv4_parse`, `pick_next`, `sched_pick_next`) and
baselines for work that is not benchmarked at all (`futex_uncontended`,
`futex_contended_wake`, `futex_wait_mismatch`, `compositor_frame_4k` — the last
is Lane C's and is measured by a host-side `cargo test`, not by this suite).

**Note what this does to the headline number.** The `over_target` count the
kernel reports (15 of 63 on the release run) is computed from the literals in
`bench.rs`, not from this file — so it is not wrong, but it is also not
*checkable* against the stated baselines for the 33 unmatched. Ranking the
release run against the parsed file yields only 7 over-target entries, and that
smaller number is an artefact of the missing half, not good news. Notably
`vfs_stat_root` — the benchmark currently under investigation at 8.5x over — has
**no** table here at all; its 700 ns target exists only as a comment in
`bench.rs` citing a file that does not mention it.

**Proper fix, unchanged but now specified.** `bench-history.py` should parse
this file and check each recorded entry against it, reporting unmatched names
in both directions as a failure rather than silence. That requires first
reconciling the names — one canonical name per benchmark, used by both the
`run()` call in `bench.rs` and the table here. The rename table above is the
work list. Until then the parse test added today guarantees only that the file
is *loadable*, not that it is *true*.


#### FOLLOW-UP 2026-08-14 (2): the file is now *checked*, and 11 targets disagree

`bench-history.py` gained `load_baselines()` + `report_baselines()`, which
compare the target the kernel prints on each `SCORE` line — the literal in
`bench.rs` — against the target this file states. The very first run of that
check, against `build/serial-test.txt` (63 benchmarks):

```
Baselines: 11 disagree, 15 unbaselined, 7 unused
  context_switch:      kernel says   5000ns, file says  10000ns
  crypto_aead_1KiB:    kernel says 100000ns, file says  70000ns
  crypto_sha256_1KiB:  kernel says  50000ns, file says  40000ns
  dns_build_query:     kernel says  40000ns, file says   2000ns   (20x)
  firewall_check:      kernel says   2000ns, file says   1000ns
  heap_alloc_free_64:  kernel says    400ns, file says    200ns
  http_mime_type:      kernel says   2000ns, file says    500ns   (4x)
  io_ring_nop:         kernel says    200ns, file says    300ns
  ipc_channel:         kernel says   2000ns, file says   3000ns
  page_fault:          kernel says  10000ns, file says   8000ns
  syscall_dispatch:    kernel says    200ns, file says   1200ns   (6x)
```

**Every PASS/OVER verdict for those 11 has been graded against a number its own
documentation contradicts.** The direction matters case by case: `syscall_dispatch`
measured 653 ns is *OVER* against the kernel's 200 ns and would *PASS* against
the file's 1200 ns. Which is correct is not obvious — 200 ns is the CLAUDE.md
hardware figure (Linux getpid ~100 ns, "within 2x"), while 1200 ns looks like a
TCG-adjusted budget. That is exactly why the check **reports and does not
reconcile**: picking a side automatically is how the two drifted apart.

The check distinguishes three failure modes deliberately, because they are
different problems: *disagree* (one side edited without the other), *unbaselined*
(the Rust literal is the only record of the target — 15 benchmarks, including
`vfs_stat_root`), and *unused* (the file claims coverage that does not exist — 7).
It also refuses to conflate an unparseable file with an agreeing one, printing
`UNVERIFIED`; that distinction is the entire lesson of this entry and is pinned
by a test.

Table renames brought name-matching from 30/63 to 48/63 (the tables moved, not
the benchmarks — `history.jsonl` is append-only and its names cannot change
without orphaning every historical record). 23 checks pass, up from 13.

**Still open:** the 11 disagreements need adjudicating one at a time, and the 15
unbaselined benchmarks need tables with real provenance. Both are now *visible on
every bench run* rather than invisible, which is the change that matters.

#### FOLLOW-UP 2026-08-14 (3): the 11 disagreements were mostly ONE bug — two kinds of target merged into one number

Adjudicating the 11 turned up a structural cause rather than eleven clerical
errors. `bench.rs` says it plainly in its own comments:

```rust
// OpenSSL SHA-256 1KiB: ~1500ns.  QEMU target: 50000ns.
score("crypto_sha256_1KiB", &result, 50000);

// DNS query build includes a heap allocation (Vec::with_capacity) which
// is expensive under QEMU (~35us).  Target set to 40us to track regressions
// without false-failing on the allocation overhead.
score("dns_build_query", &result, 40000);
```

**Those are TCG budgets, not hardware references** — and `baselines.toml` was
storing the hardware reference under the same key. Comparing them reported a
20x "disagreement" where in truth the two files were each right about a
different quantity. Two more (`heap_alloc_free_64`, `http_mime_type`) were the
same shape one level down: a *scope* difference, where the benchmark measures a
fixed multiple of the per-operation target (alloc+free is 2x an alloc; the MIME
benchmark does 4 lookups).

Worse, `bench-history.py` printed this on every run:

> *(The 'target' column in the scorecard above is a **hardware** reference and
> cannot be met under TCG — see bench/baselines.toml.)*

which is **false for at least six benchmarks**, whose targets are explicit QEMU
budgets. The line explaining the number misdescribed it, and so did the
scorecard headline: "48/63 within hardware target" counts passes that were
scored against TCG budgets.

**Fix: make the two kinds separate keys.** `target_ns` stays the hardware
reference; `tcg_target_ns` is the budget the suite is graded against under
emulation, and the cross-check prefers it when present. The explanatory line now
says the column is a mix and points at which key records which.

**Three were real disagreements.** Two are settled by CLAUDE.md's performance
table, which outranks the file:

* `context_switch`: file said 10 µs, spec says *"Target: < 5 µs"* → file corrected.
* `page_fault`: file said 8 µs, spec says *"Target: < 10 µs"* → file corrected.
* `ipc_channel`: file said 3 µs, spec says *"Target: < 2 µs round-trip"* → file corrected.
* `syscall_dispatch`: file said 1200 ns, derived by doubling a **638 ns WSL2
  measurement of a full syscall including spectre mitigations** — not the same
  quantity as dispatch. Spec says *"Linux: ~100 ns for getpid. Target: within 2x"*
  → 200 ns. **This one changes a verdict:** the measured 653 ns is OVER at
  200 ns and would have PASSed at 1200 ns. The 638 ns figure is kept as context,
  not as a derivation.
* `io_ring_nop`: file said 300 ns (2x a 150 ns measurement), spec says
  *"~100-200 ns per SQE; same order"* → 200 ns.

Result: **11 disagreements → 1.**

**The last one is instructive and is deliberately still open.** `firewall_check`
carries the comment `// Target from baselines.toml: 2000ns` in `bench.rs` while
the file says 1000 ns — a citation that is simply false, and the direction
(2x looser) means the kernel silently relaxed its own target at some point.
Both pass comfortably (measured 55 ns), so nothing is hidden by it; it is left
for the next `bench.rs` change rather than fixed now, because a kernel edit
during an in-flight release build would produce a binary that does not
correspond to any commit. Recorded here so it is not lost.

---


## FIXED (2026-08-15, lane C) — three workspace test failures from real-glyph measurement, two of them real bugs

`text::measure`/`text::wrap` now measure actual glyph advances instead of
estimating from byte counts. Three lane-C tests failed as a result. Only one
was a stale test; the other two were genuine rendering bugs the old estimate
had been hiding.

**1. `weather::an_alert_card_grows_to_hold_its_description` — stale test.**
`card_h = (ALERT_BODY_TOP + body_height + 12.0).max(90.0)`, i.e. `52 + 18N`
floored at 90, so growth is only observable at N≥3 lines. `LONG_ALERT` used to
wrap to 4 lines and now wraps to 2 at `text_width = 828` (app width 900 minus
padding), so the test compared 90 against 90. Fixed by building the input by
construction — `"Secure loose objects outdoors. ".repeat(40)` — and asserting
first that it actually wraps past the floor (`drawn > 2`) so the growth check
can never again silently compare the floor to itself.

**2. `wordsearch` — real bug: the strikethrough rule and checkmark overran the
word they annotate.** A word in the list is drawn with
`max_width: Some(140.0)`, but the rule's extent and the checkmark's x were
placed from the *unclipped* `text::measure`. A word longer than the column got
a rule running out past the clip into the grid beside it. Fixed by naming the
clamp (`WORD_LIST_MAX_WIDTH`, `WORD_LIST_FONT_SIZE`) and applying it to the
measurement that positions the marks:
`text::measure(...).min(WORD_LIST_MAX_WIDTH)`. The old test asserted
`bold < word.len() as f32 * 8.0 + 1.0` — a byte-count literal, which is both
fragile and wrong for non-ASCII; replaced with three postcondition tests
(rule matches the word drawn beneath it; ÉLÉPHANT measures within 10% of
ELEPHANT, i.e. by character not by byte; a 45-char word's rule never leaves
the column).

**3. `tmux` — real bug: a terminal grid sized from a proportional face.**
`char_width()` was `text::digit_advance(...)`, the advance of `'0'` in the UI
face: 7.55px at 13px, while `'W'` in the same face is 13.08px. Glyphs overhung
their neighbours' cell backgrounds and the block cursor sat beside the
character it marks. The root cause was that **the toolkit had no way to ask
for a monospace face at all.** Fixed by building that dimension end to end —
`osfont::system::Family { Ui, Mono }` on the cache key, `text::measure_in` /
`cell_advance` / `line_height_in` / `ascent_in`, `RenderCommand::PushFont` /
`PopFont`, `guiremote` tags `0x0B`/`0x0C`, a `font_stack` in the compositor —
and pointing tmux at it. See `design-decisions.md` §413 for why the family is
scoped render state rather than a field on all 4570 `Text` construction sites.

**The pattern all three share**, and the rule that would have prevented them:
a threshold test whose threshold is a *literal* and whose input is *measured
by the environment* degrades silently long before it fails loudly. Assert a
postcondition of the function (`w <= box_w`; "the rule matches the word drawn
beneath it") or build the input by construction (`.repeat(40)`) — never encode
a fact about the host's installed fonts.

**Latent hazard this leaves.** `text::digit_advance` still exists and is still
the wrong call for any terminal-shaped view; its doc now says so and points at
`cell_advance`. Any other app that lays out a character grid should be checked
for it.

---


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


## `apps/installer` wrote unescaped strings into a GRUB config that runs at boot (lane C) — FIXED

`grub.rs`'s `generate_entry` interpolated every field of a `GrubEntry` —
`title`, `kernel_path`, `root_partition`, `uuid`, `initrd_path` and each of
`kernel_params` — straight into a `menuentry` block with no quoting and no
validation:

```rust
out.push_str(&format!("menuentry \"{}\" {{\n", entry.title));
...
out.push_str(&format!("    chainloader {}\n", entry.kernel_path));
```

That block is written to `/etc/grub.d/40_slateos` (mode 0755) and folded into
`grub.cfg` by `update-grub`. **GRUB executes `grub.cfg` at boot with full
firmware privilege — before any OS, and therefore before any OS-level security
boundary exists.** A title containing a `"` closes the string and everything
after it is parsed as fresh GRUB script; a title containing a newline does not
even need the quote. `$` expanded as a GRUB variable.

The reachability is the part worth remembering: this looked like a field the
user types into our own installer, so "who would attack themselves?". But
`os-prober` — the whole reason this module exists — *scrapes* menu titles out
of **other partitions'** `/etc/os-release`. On a dual-boot machine that is a
file the other OS controls, so the title is attacker-influenced input arriving
through a path that never looks like input.

**Fixed** by emitting every interpolated value inside `"…"` through a new
`grub_quote`, which escapes exactly the three bytes GRUB's lexer treats
specially inside a double-quoted string — `\`, `"`, `$` — mirroring
`grub_quote()` in GRUB's own `util/grub-mkconfig_lib.in`. Control characters
cannot be escaped that way, so `GrubEntry::validate` rejects them and
`generate_entry`/`generate_custom_script` now return
`Result<String, GrubError>`; `install`/`update` validate *before* touching the
filesystem, so a rejected entry leaves no file behind.

A second, non-security bug fell out of the same rewrite: `kernel_params` were
`join(" ")`ed into the line, so a parameter containing a space silently became
two parameters. Each is now quoted individually.

**Lesson, and it generalises past this file: "config file" is not a safe
output format.** The lossy-path sweep that led here trained the question *is
this value preserved byte-for-byte?* — but preservation is only half of it.
The other half is *can this value change the meaning of the document it is
written into?* A path can round-trip perfectly and still be an injection. Any
place we `format!` a value into a file that something else later *parses* —
GRUB config, shell script, YAML, JSON, a desktop entry — needs an escaping
function chosen for that grammar, not just faithful bytes. Worth auditing the
other generators in `apps/` on the same question.

Five separate defences, verified non-vacuous by breaking each one alone and
confirming it failed only its own test: escaping `$`, escaping `"`, escaping
`\`, the control-character rejection, and the per-parameter quoting.


## `gui/toolkit/src/svg.rs` named a character the author never wrote (lane C) — FIXED

`u8_from_hex_char`'s error did `c as char` on the offending byte. `c` is a
*byte* of the colour string and the bytes reaching that arm are exactly the
non-hex ones, which includes the continuation bytes of a multi-byte character:
`#ÿÿÿ` reported `bad hex char: Ã`, blaming a character absent from the input
and sending the author hunting for it. Now reports the byte (`bad hex byte:
0xc3`) for anything outside printable ASCII, and the character itself for
ASCII.

The other four `c as char` sites in this file were checked and are **correct**:
each sits in a match arm that has already matched `c` against ASCII byte
literals (or, for `cmd_char`, behind an `is_ascii_alphabetic()` guard), so the
cast is provably lossless there. Recorded so the next sweep does not re-open
them.


## Five copies of two escapers, at three levels of correctness (lane C) — FIXED

Following the GRUB finding above, the same question — *can this value change
the meaning of the document it is written into?* — was put to every generator
in `apps/`. It found five near-copies of a JSON escaper and two of an XML one,
which had drifted apart:

| Copy | JSON escaper | Verdict |
|---|---|---|
| `apps/jsonviewer` | `"` `\` `\n` `\r` `\t` `\b` `\f`, `\u00XX` fallback | correct |
| `apps/kanban` | as above (fixed in an earlier sweep) | correct |
| `apps/snippets` | `\u00XX` fallback present | correct |
| `apps/diagram` | five cases only, **no fallback** | emits invalid JSON |
| `apps/reminders` | five cases only, via `str::replace` | emits invalid JSON **and** corrupts on read |

**`apps/reminders` was the serious one.** Its `unescape_json` was a chain of
`str::replace` calls in the wrong order — `\n` decoded before `\\`:

```rust
s.replace("\n", "\n").replace("\r", "\r").replace("\t", "\t")
 .replace("\\\"", "\"").replace("\\\\", "\\")
```

So the two-character text `\n` (a literal backslash, then the letter n) was
escaped to `\n` on save and read back as a **newline**. A Windows path in a
note, `C:\temp`, came back as `C:\<TAB>emp`. The damage was then re-saved, so
the note decayed a little further every time the app was opened. The existing
test `test_json_escape_special_chars` covered this function and passed,
because its sample text — `"Hello \"world\"\nnew line"` — contains a real
newline and real quotes but not one literal backslash, the single input that
tells a correct decoder from a broken one.

**`apps/whiteboard` had an unescaped XML export**: `page.name`, `layer.name`
and both `TextLabel` and `StickyNote` content went straight into the markup, so
a sticky note reading `</sticky><rect/>` closed its own element and injected a
sibling, and any `&` made the export unparseable. Same class as the GRUB bug,
found by the audit that bug prompted.

**Fixed** by adding `gui/toolkit/src/escape.rs` (`guitk::escape`) with one
correct implementation of each — `xml`, `json_string`, and a
`unescape_json_string` that is a single left-to-right pass and so structurally
cannot make the replace-chain mistake — and routing `reminders`, `whiteboard`,
`diagram`, `snippets` and `markdowneditor` through it. Non-vacuity verified by
breaking each of the five defences alone; each failed only its own tests.

**Not converged, deliberately:** `apps/kanban` and `apps/jsonviewer` decode
inside full tokenising JSON parsers (`parse_string(data, start) -> (String,
usize)`), a different shape from a standalone `unescape`. Both are already
correct, so rewriting them onto the shared helper would risk regressing working
code for no correctness gain. If a third parser of that shape appears, extract
a shared *parser* rather than bending these two into the wrong signature.

**The generalisation, now twice-confirmed:** a value can be preserved
byte-for-byte and still be a bug. The lossy-path sweep asked *is this
preserved?*; this one asks *can this re-punctuate its document?* Every
`format!` into a file that something else later parses needs an escaper chosen
for that grammar. Remaining unaudited generators of this kind: the YAML and
`.desktop`-style writers, if any, and `pkg/`'s manifest output.


## Data exporters: CSV/JSON/SQL injection in `netscan`, `credmanager`, `dbviewer` (FIXED)

Third pass of the "a config file is not a safe output format" audit, covering
the tabular exporters. Four distinct defects, all the same shape:

**`apps/netscan` did no CSV escaping at all.** This is the worst of the four
because the inputs are not the user's: a `hostname` comes from reverse DNS and
a `service`/`banner` from banner grabbing, so both are chosen by the *scanned*
host — on a scan, precisely the party with no reason to be trusted. A comma in
a hostname added a column and a newline added a whole row, letting a hostile
host forge result rows for machines that were never scanned. The hand-written
`"{}"` around the port/service columns was not a defence either: it never
doubled an internal quote, so a `"` in a service name closed the field early.
Its JSON export had the same holes plus a banner escaper handling `"`, CR and
LF but *not* the backslash — a banner ending in `\` produced `"...\"`, an
unterminated string that truncates the document.

**`apps/credmanager` left `tags` and `folder` raw** in the CSV (the only two
of nine columns not escaped), its `escape_csv` omitted `\r` from the trigger
set (RFC 4180 records are CRLF-terminated, so a bare CR splits the record for
most readers), and `serialize_backup` escaped *nothing* — vault name, entry
name, tag and folder names all interpolated bare. For a credential vault that
is the worst possible failure: a `"` in any name yields a backup file that no
reader can load, i.e. a silently unrestorable backup.

**`apps/dbviewer` escaped every value in all three exporters and no column
name in any of them.** The corollary this pass added to the audit question:
*audit the field names, not just the field values.* Column names are not
privileged data — `import_csv` takes them straight from the header line of a
file the user opened. Also `export_json`'s `s.replace('"', "\\\"")` (escaping
the quote but not the backslash, worse than useless for a value ending in `\`)
and `export_sql_inserts` interpolating table/column names as bare SQL
identifiers.

**`apps/dbviewer`'s importer could not read its own exporter's output.**
Found while fixing the above. `import_csv` split the header with a naive
`split(',')` and iterated `csv_data.lines()`, so a quoted field containing a
comma (header) or a newline (any record) was torn apart — even though
`parse_csv_line` underneath it was correctly RFC 4180-aware for data rows.
Fixed properly by replacing both with one record-level `split_csv_records`
that never splits on a line boundary before it knows whether it is inside
quotes. It also now trims only *unquoted* fields: quoting is how a writer says
the surrounding whitespace is data. Locked in by a round-trip test.

**Fixed** by adding `guitk::escape::csv_field` (RFC 4180, trigger set
`, " \n \r`) to the shared module, a local `sql_ident` in `dbviewer` (standard
double-quote identifier quoting), and routing all of the above through them.
Non-vacuity verified by breaking each of the nine defences alone; each failed
only its own tests.

**A testing note worth keeping.** Three of the new tests failed on first run
*because the tests were wrong, not the code* — each had counted a naive
substring. Correctly escaped output legitimately *contains* the payload:
`\", \"admin` contains `"admin`, a quoted CSV field contains a comma and a
newline, and a quoted SQL identifier contains a `;`. A test for an injection
defence therefore cannot use `contains`/`split`/`lines` — it has to decode the
way a conforming reader does. The fix in each case was a small escape-aware
scanner (`parse_csv`, `json_string_token_count`, `sql_statement_count`) living
beside the tests. This is the same trap as the GRUB `menuentry ` substring
count from the first pass; it has now appeared in all three passes, so treat
"count the tokens a parser would see" as the default shape for these tests.


## `guitk::csv`: a format's writer and reader belong in one module (FIXED)

`apps/spreadsheet` turned out to have the *identical pair* of defects
`apps/dbviewer` had: an `export_csv` whose quoting trigger set omitted `\r`,
and an `import_csv` that split records with `csv.lines()` before handing each
line to a perfectly correct, quote-aware field parser. Both apps could
therefore produce an export they could not themselves read back — a quoted
cell containing a newline was torn in half and the rest of its row dropped.

Two independent apps making the same two mistakes is the signal to stop
patching and restructure, so the CSV format now lives in one module,
`gui/toolkit/src/csv.rs`, holding **both** directions: `csv::field` (write)
and `csv::parse_records` (read). Keeping them adjacent is the point — the
whole bug class is a writer and a reader drifting apart, and it is much harder
to write a line-splitting reader thirty lines below an escaper that
deliberately emits newlines inside fields.

`csv_field` moved out of `guitk::escape` in the process. Escaping a CSV field
is not a standalone escaping problem the way XML or JSON escaping is; it is
half of a codec, and filing it under "escape" is what made it natural to write
the other half somewhere else. `escape` keeps a comment pointing at `csv`.

`Field { text, quoted }` reports whether the source spelled a field in quotes,
because the two apps disagreed on trimming and both were right: `dbviewer`
wants the lenient "trim a bare field" import convention, `spreadsheet` wants
cells verbatim. Quoting is exactly the writer's statement that the surrounding
whitespace is data, so `Field::trimmed_if_bare` lets a caller be lenient
without corrupting a deliberately-padded value. Locked in by a round-trip test
in each app plus `anything_written_can_be_read_back` in the module itself.

Both apps' local parsers were deleted rather than left in place; a weaker
second parser sitting in the file is the thing that gets reached for next
time.


## `apps/musicplayer`: ID3 tags could forge M3U playlist entries (FIXED)

`export_m3u` interpolated `track.artist` and `track.title` straight into the
`#EXTINF:` line. Those two fields are not the user's: `Track::update_from_data`
sets them verbatim from the file's own ID3v2 tags, so for any downloaded file
they are chosen by whoever produced it. `load_m3u` reads every non-`#` line as
a **file path**, so a title containing a newline injected arbitrary entries
into the user's playlist.

M3U is where this audit's usual answer runs out: the format is bare
line-oriented text with no quoting and no escape syntax, so a line break
cannot be escaped — only removed or refused. The fix splits on which of those
is honest for each field:

- `#EXTINF` metadata is advisory display text, so CR/LF become a space
  (`m3u_field`). Losing a newline out of a song title costs nothing.
- A **path** containing CR/LF is legal on this OS (all bytes but `/` and NUL)
  and has no M3U representation at all. Writing it anyway would silently point
  the entry at a different file, so the track is omitted — and *reported*:
  `export_m3u` now returns `M3uExport { text, skipped }` instead of a bare
  `String`, so a caller can tell the user rather than handing them a playlist
  quietly shorter than the one they exported.

The general point, third variant of it now: when a format cannot represent a
value, the choice is reject or sanitise, and it must never be "write it
anyway." GRUB got reject (control characters), M3U metadata gets sanitise, M3U
paths get reject-and-report.


## `apps/contacts`: a chained-`replace` decoder corrupted every note containing a backslash (FIXED)

**Status: FIXED 2026-08-15** (lane C). Found while auditing the vCard/iCalendar
family during the output-escaping sweep. This is the same defect previously
fixed in `apps/reminders`, in its third instance, and this time the *correct*
implementation was already sitting in the neighbouring app.

`vcard_unescape` decoded with a chain of `str::replace`:

```rust
s.replace("\n", "\n")     // <-- runs first
 .replace("\,", ",")
 .replace("\;", ";")
 .replace("\\\\", "\\")    // <-- too late
```

`vcard_escape` correctly writes the two-character text `\n` (a backslash
followed by the letter n) as `\n`. The decoder then scans that for the
sequence backslash-n, finds it at offset 1, and emits a real newline. So
`C:\new` came back as `C:\`, a line break, and `ew`.

The trigger is ordinary content, not a crafted one: a Windows path, a regex, a
LaTeX fragment, a `\server\share` UNC name — anything a person might
reasonably paste into a contact's NOTE field.

**The corruption happens once, on the first load, and is then a fixed point** —
re-saving does not degrade it further. That is worth stating precisely because
it makes the bug *quieter* rather than milder: the damaged value is what gets
written back, so after a single load-and-save cycle the original text is gone,
and there is no accumulating drift to make the loss noticeable. A test that
looked only for unbounded growth would have passed.

Fixed with a single left-to-right pass that consumes the backslash and the
character after it together. Such a pass structurally cannot make this mistake,
because it never re-examines output it has already produced — the ordering
question that a `.replace()` chain has to answer correctly simply does not
arise.

Two things came out of the cross-check that are worth recording:

- **`apps/calendar::ics_unescape` was already correct**, single-pass, and
  carried a comment naming this exact anti-pattern. The same format family held
  one correct and one broken implementation of the same rules, a few hundred
  lines apart in a sibling crate — which is the duplication problem the
  `guitk::csv` extraction was about, showing up in a format that has not been
  extracted yet.
- **`vcard_escape` also passed a bare CR through untouched.** vCard has no
  escape for CR and its lines are CRLF-terminated, so a CR in a value ended the
  property line early and the remainder was parsed as a new property — a note
  could forge a `TEL:` line. Fourth instance of "the format cannot represent
  this value, so reject or sanitise": here it sanitises, because a CR in a text
  field means a line break, and a CRLF pair now yields one break rather than
  two.


## `apps/email`: every outgoing header was interpolated raw — header injection (FIXED)

**Status: FIXED 2026-08-15** (lane C). The most serious defect the output-escaping
audit has turned up, and the one whose consequence is least visible to the user.

`EmailDraft::build_message` wrote every header value straight into the message:

```rust
msg.push_str(&format!("Subject: {}\r\n", self.subject));
msg.push_str(&format!("To: {}\r\n", self.to.join(", ")));
msg.push_str(&format!("In-Reply-To: <{reply_to}>\r\n"));
msg.push_str(&format!("Content-Type: {}; name=\"{}\"\r\n", att.mime_type, att.filename));
```

RFC 5322 gives a header field no way to contain a line break. The field *ends*
at CRLF; folding — a CRLF followed by whitespace — is a continuation the
serialiser chooses, not something a value can request. So a CR or LF in a value
is not escaped, it **terminates the header**, and the receiving MTA reads what
follows as a header of its own.

A subject of `Lunch?\r\nBcc: mallory@evil.test` therefore adds a recipient. The
reason this is worse than an ordinary injection: **the forged Bcc appears
nowhere the sender can see it** — not in the compose window, which shows the
subject field as typed, and not in the Sent copy, which is rendered from the
same draft object. The mail silently goes somewhere the user cannot discover it
went.

### What was and was not reachable

Worth stating precisely, because the inbound side turned out to be sound and
that is a design worth not regressing.

- **Not reachable: anything parsed off the wire.** `Headers::parse` unfolds
  continuation lines into spaces, so no value read from a received message can
  carry a CR or LF. That closes what would otherwise be the nastiest path:
  `EmailDraft::reply` copies the original's `Message-ID` into `In-Reply-To`, so
  a hostile `Message-ID` would have been injected into the victim's reply with
  no interaction beyond pressing Reply. The unfolding is what prevents it, not
  anything at the serialiser, which is why the serialiser now sanitises anyway.
- **Reachable: everything composed locally** — the subject and recipients the
  user types or pastes, and attachment filenames. The filenames matter more
  here than on other systems: `design.txt` allows every byte except `/` and NUL
  in a path, so **a newline in a filename is legal on SlateOS**. A downloaded
  file can carry one, and attaching it forged headers.

### Also fixed: the boundary was a constant

The multipart delimiter was the literal `----=_Part_Boundary_001`. RFC 2046
requires the boundary to appear nowhere inside an encapsulated part, and a fixed
string cannot promise that. A body containing it — which a user produces just by
quoting a previous multipart mail — ends the part there, and **every attachment
below that point silently disappears from the sent message**. The boundary is
now derived from the body, lengthening on collision; this terminates because a
finite string contains no arbitrarily long substring, and the first candidate is
the old constant, so ordinary mail is byte-identical.

### The shape of the fix

Five helpers, chosen per field by what the grammar can express and by whether
the field is advisory or load-bearing — the reject-or-sanitise rule this audit
keeps arriving at, now on its fifth format:

| Field | Grammar offers | Treatment |
|---|---|---|
| `Subject`, display names | nothing | sanitise: control characters → space |
| `To`/`Cc`/`Bcc` | nothing | **reject and report** — a recipient decides where the mail goes, so a bad one must not be quietly rewritten into a different address |
| `Message-ID`, `Content-ID` | nothing | sanitise: drop controls, `<`, `>`, whitespace |
| attachment `filename` | `\"` and `\` inside a quoted-string | escape quote and backslash; drop controls |
| attachment `Content-Type` | nothing (it is a token) | **fall back** to `application/octet-stream` — a mangled media type is not a media type, so there is nothing to sanitise it *into* |

`build_message` now returns `BuiltMessage { text, rejected_recipients }` rather
than a bare `String`, for the same reason `export_m3u` returns skipped paths: a
dropped recipient is exactly what the sender must be told about, and a function
returning a `String` has nowhere to say it.

### Two lessons from the break-testing, not the fix

Breaking each defence in turn to check the tests notice found that **two of the
new tests could never have failed**, which is worth recording because both
mistakes are easy to repeat:

1. The header-scanning helper stopped at the first blank line — correct for the
   top-level block, but a **MIME part has its own headers after that blank
   line**, so the test for a forged header in an attachment filename was
   inspecting a region the payload never reached.
2. It split only on `\r\n`. Real receivers are lenient and many end a line at a
   bare LF, so a test that only recognises CRLF is *stricter than the attacker*
   and passes on genuinely vulnerable output.

Both fixed by scanning every line terminator across the whole message and
counting lines that *begin* with the header name. Counting line starts rather
than substrings is what keeps it honest in the other direction, and is the same
point the CSV and SQL tests reached: correctly quoted output legitimately
contains the payload text, so `contains` cannot be the assertion.

The display-name test still fails under no single break, because the value is
covered by two independent defences; breaking both together does fail it, which
is how it was confirmed to be defence in depth rather than a vacuous test.


## slides: one HTML export field skipped the escaper (fixed 2026-08-15, lane C)

`apps/slides`'s `export_html` escapes the presentation title, text-box bodies
and bullet items, and does so correctly. It did not escape the placeholder
label of an `Image` element, which is user-typed and is written straight into
the exported document. A label of `<script>…</script>` — or, more cheaply, a
`"` closing the `style` attribute early — is therefore reproduced as markup by
any browser opening the export.

### Why this one and not the other three

The three fields that were escaped each sit in a statement of their own:

```rust
push_html_escaped(&mut html, &slide.title);
```

The one that was not was a `{}` inside a larger `format!`, in the company of
five geometry values that genuinely cannot need escaping:

```rust
html.push_str(&format!(
    "  <div class=\"img-placeholder\" style=\"left:{x}px;top:{y}px;\
     width:{width}px;height:{height}px;\">{placeholder_label}</div>\n",
));
```

Reading that line, the eye is doing arithmetic, not taxonomy. Every other name
in the interpolation is an `f32`, and `placeholder_label` inherits their
apparent harmlessness by proximity. This is the recurring shape of the whole
audit: the dangerous interpolation is rarely the one on a line by itself — it
is the one *embedded among values that are obviously safe*, where the reader's
attention has already been spent. A grep for `format!` finds it; a reading of
the function does not.

The fix splits the statement so the label goes through `push_html_escaped` like
its three siblings, which also makes the asymmetry impossible to reintroduce
without deleting a call.

### Test

`no_text_field_can_inject_a_tag_into_the_export` drives *one* payload through
all four text fields at once — title, text box, image label, bullet item — and
counts tags rather than substring-matching, since escaped output legitimately
contains the payload text. Driving every field from a single payload is what
makes the test grow with the exporter: a fifth text field added later either
routes through the escaper or fails this test. A second test checks the
attribute case specifically, since a bare `"` escapes the `style` value without
needing a `<` at all.


## clipmanager, flashcards, mindmap: three exporters that could not read themselves (fixed 2026-08-15, lane C)

The same audit, three more apps. All three wrote user text raw into a
line-oriented format whose structure is made of characters that text can
contain. Two of them have importers, so both could produce an export they
themselves misread; the third has no importer, which changes who the victim is
but not whether the bug is real.

### clipmanager — the worst of the three, because of what the field holds

`export_text` wrote the clip content raw after a bare `content:` line, and
`import_text` recovered records with `data.split("---ENTRY---")` — a *substring*
split, not even a line match. So a clip containing that marker split its own
record in two, and the second half's lines were then parsed as **headers**,
letting copied text set its own `source:` and `pinned:` and add tags.

What makes this the severe one is not the mechanism but the field. A clipboard
entry is arbitrary copied text — the one value in the whole desktop guaranteed
to hold whatever the user last selected in a browser. Every other app in this
audit needed the user to type the payload into a name or a note; here they only
have to copy it.

Escaping the body would have worked and would have been wrong. The point of
this format is that you can open it and see what you copied; an escaped
multi-line body is unreadable. The fix is a **length prefix**:

```
content:<byte length>
<exactly that many bytes>
```

Bytes inside the body are then never examined, so no sequence in them means
anything — a stronger guarantee than escaping, and a cheaper one to verify.
The parser became a single left-to-right pass, necessarily: the body length is
only known once its header has been read, which `split` could not have
consulted. Header values (`source:`, tags) are sanitised so they stay on their
own line, and tags get a line each instead of a comma-joined list. The format
now needs no escaping anywhere.

A round-trip defect surfaced from the new tests, unrelated to injection:
export wrote newest-first while import replays through `add`, which prepends,
so **importing your own export reversed your clipboard history**. The file is a
log, so it is now written oldest-first. Worth noting that the existing
round-trip test did not catch this — it checked the count, not the order.

### flashcards — the failure mode is pedagogical, not technical

Every structural signal in the deck format is a character card text can
contain: the `Q:`/`A:`/`T:` prefixes, the blank line that ends a card, the
comma between tags, the line break itself. A question written the obvious way —

```
What is 2+2?
A: 5
```

— exported and re-imported as *two* cards, one of them with an answer its
author never wrote. This is the entry in this audit whose consequence is
strangest: nothing crashes, nothing is exfiltrated, and the user revises from
the deck and learns the wrong thing.

Fixed with the backslash escaper and matched single-pass decoder from the vCard
work. Two decisions differ from that one, both because this format is ours
rather than a published spec:

- **Commas are escaped in tags only.** Flashcard questions are full of commas;
  turning `What is 2, 3, and 4?` into `What is 2\, 3\, and 4?` would wreck a
  format that is meant to be hand-editable for no gain, since a `Q:` line has
  no comma-separated structure to protect.
- **CR gets its own escape** rather than being folded into `\n` with the LF
  beside it. vCard *has* to normalise — its spec says a line break is spelled
  `\n` and nothing else. Here nothing forces that, so escaping CR separately
  makes the round trip exact rather than faithful-in-spirit, and leaves no
  lossy corner to document.

Two further round-trip losses fell out: the importer trimmed each line before
matching the prefix, so leading and trailing spaces in a value were lost, and
an empty value (`Q: ` with nothing after it) failed the `strip_prefix("Q: ")`
and **dropped the card entirely**. It now matches the raw line and falls back
to the trimmed one, which keeps the leniency for hand-written decks while
making the app's own output exact.

### mindmap — no importer, so the reader is a person

`export_node_text` wrote node labels raw into an indented outline, where
structure *is* whitespace: a newline starts a sibling and the leading spaces
choose its depth. A label containing a line break therefore draws branches in
the exported map that do not exist in the real one.

There is no importer, which is worth stating precisely rather than using as a
reason to skip it: the absence of a parser does not make the output correct, it
only changes who is misled — a human reading the outline, or whatever other
outliner they open it in. Labels are short prose with nothing to escape *with*,
so this one is a sanitise: control characters fold to single spaces and runs
collapse, keeping the label a readable phrase.

### On the break-testing, again

Every defence added here was broken individually to confirm its tests notice —
twelve breaks across the three apps. Two findings worth carrying forward:

1. **A test can be vacuous by being one character short of the real attack.**
   The flashcards deck-name test passed against *unescaped* output on its first
   version, because the payload `"Name\nQ: forged\nA: forged"` has no trailing
   blank line — and a card is only committed by the blank line that ends it, so
   the forged pair was silently overwritten by the next one. The defence was
   real; the test was not exercising it. Only breaking the fix on purpose
   revealed the difference.
2. **A defence can be genuinely redundant, and that is fine as long as it is
   labelled.** In clipmanager, matching the record marker as a whole line
   rather than a substring is unreachable *inside* a record once the body is
   length-prefixed. Rather than delete it or write a test that cannot fail, the
   case that does reach it was found — the scan for the *first* record runs over
   whatever preamble the file has, such as a covering note that mentions
   `---ENTRY---` in a sentence — and the test drives that.


## indexer and fileassoc: config files whose values could re-punctuate them (fixed 2026-08-15, lane C)

The same audit again, on the two remaining `key = value` config formats. Both
bugs are silent-wrong-result rather than crash-or-corruption, and one of them
defeats a security control.

### indexer — a comma in a path defeated an exclusion

`/etc/indexer.conf` stored `index_paths`, `exclude_paths`,
`include_extensions` and `exclude_extensions` as **comma-joined** lists. On
this system a path may contain any byte but `/` and NUL, so a comma is an
ordinary filename character. Excluding `/home/u/Private, Ltd` wrote one line
that read back as *two* entries — `/home/u/Private` and `Ltd` — neither of
which named the directory the user meant. The directory was therefore not
excluded, and with `index_contents` on, its contents were read into a
searchable index.

That is the part worth stating plainly: `exclude_paths` is not a preference,
it is the mechanism by which a user keeps a directory out of a system-wide
search index. A format that cannot represent the user's answer is a format
that silently overrides it.

Fixed by giving each entry **its own line** — `index_path = …` repeated —
rather than escaping the comma. Escaping a comma works; a separator that never
appears is better than one that is escaped correctly, and it keeps an ordinary
config readable. The plural keys still parse for hand-written files, and the
first repeated key clears the built-in defaults so a config can shrink the list
and not only grow it. A related loss fell out of the tests: an explicitly empty
list used to reappear as the built-in defaults on the next read.

### fileassoc — the exporter and the importer disagreed, and nothing said so

`from_config_line` trimmed both halves; `export_config` wrote the raw strings.
An extension registered as `"txt "` is registerable — `register_file_type`
validates nothing and `set_default_app` only lowercases — so it exported as
`txt =myapp` and read back as `txt`, **silently reassigning a different
extension's default application**. No error is reported on any path: the line
parses, the extension exists, the app exists and supports it, so every
validation the importer performs passes.

`#` had the same shape in the other direction. A comment line is skipped
entirely, so an extension of `#txt` exported to a line the importer discards,
losing the association without a word.

Both are fixed by escaping through `textfmt::kv` with `=` and `#` named as the
grammar's structure characters, and by having `export_config` call
`Association::to_config_line` instead of keeping a second copy of it inline.
That second copy is the real lesson here: the writer and the reader were
*already* a matched pair on `Association`, and the drift happened because
`export_config` bypassed the writer and open-coded the format a third time. A
format with two writers has no invariant, only a coincidence.

### The band-aid, and where the escaper now lives

By fileassoc this was the fourth app in a row needing the same line-value
escaper, and the third place it had been written inline. Per CLAUDE.md's rule
about band-aid accumulation, it was extracted rather than copied again.

The extraction was not to `guitk`, where `csv` and `escape` already lived. The
components with the strongest need for these primitives turn out to be exactly
the ones that must not depend on a widget library: `apps/backup`,
`apps/indexer` and `apps/installer` are headless, and are three of the four
`apps/` crates with no `guitk` dependency. Unable to reach the shared
escapers, each had grown its own — which is the whole mechanism by which the
duplication happened. So the modules moved to `textfmt`, a dependency-free
`no_std` crate alongside `yamldoc` and `tzrules`, and `guitk` re-exports them
under their original paths so the 137 applications that say `guitk::csv` did
not have to change.

Two invariants are now documented in one place instead of being rediscovered:

1. **Decode in a single left-to-right pass, never a chain of `str::replace`.**
   Undoing `\n` before `\\` turns the two-character text `\n` — a legal
   directory name here — into a real newline. A single pass structurally cannot
   make that mistake, because it never re-examines what it has produced.
2. **An escape must not end in whitespace.** These parsers trim the value,
   which is the right leniency for a hand-edited file, but it means writing a
   trailing space as `\ ` leaves the file ending `...\`, which decodes to a
   stray backslash. Hence `\s`.

### Break-testing

Five breaks on fileassoc, each caught by a named test: removing the escape on
write, the unescape on read, the escape-aware split, `#` from the meta
character set, and routing `export_config` around `to_config_line`. That last
one is the break that reproduces the original bug exactly, and it is worth
keeping precisely because it will fail again the moment someone re-inlines the
format for convenience.


## devicemanager: a USB device could forge a section of the hardware report (fixed 2026-08-15, lane C)

`export_report` interpolated eight device-supplied strings raw — name, vendor,
type, hardware ID, location, and the driver's name, version and provider — into
a report whose structure is line breaks, `--- Section ---` headers and
two-space indentation.

What makes this one worth its own entry is where the strings come from. They
are not typed by the user; they are read off the hardware. A USB device chooses
its own product and manufacturer descriptors, and nothing in the descriptor
format constrains their content or forbids a line break. So a device that calls
itself

```
Mouse
--- Storage ---
  Fake Disk [OK] (ACME)
```

writes a whole forged section into the hardware report of any machine it is
plugged into — a report whose entire purpose is to be trusted when someone is
diagnosing that machine, and which is typically pasted into a bug report or
handed to whoever is helping.

There is no importer, which is worth stating precisely rather than using as a
reason to skip it: the absence of a parser does not make the output correct, it
only changes who is misled — here a person, or whatever they paste the report
into.

Fixed with a fold, not an escape. The choice follows from the reader: there is
nothing to undo an escape, so a literal `\n` in the output would be noise to a
human where a real newline is a forgery. Every control character becomes at
most one space, runs collapse, and edge space is dropped so a name padded with
spaces cannot appear to sit at a different depth in the report's indentation.

### flashcards' last lossy corner is closed

Migrating flashcards onto the shared `guitk::kv` was meant to be deduplication
— the fourth inline copy of the same escaper — but it also closed the deck
format's one documented limitation. `split_tags` trims each tag, which is the
right leniency for a hand-written `T: math, algebra`, but the trim reached the
*value*, so a tag of `" spaced "` came back as `"spaced"`. `kv` writes an edge
space as `\s`, which is not a space: the trim cannot find it, and the decode
happens afterwards. The trim still does its job — absorbing the layout of a
hand-written list — without being able to reach the data.

Worth noting as a general point: three of the four apps migrated onto the
shared escaper gained a fix they were not migrated for. Consolidating on one
correct implementation is not only less code; it retro-actively repairs every
corner each local copy had quietly given up on.

### A substring count is a `contains` in disguise

The first version of both devicemanager tests failed against the *fixed* code,
and the tests were what was wrong. They asserted
`report.matches("--- ").count() == baseline` and
`!report.contains("--- Forged ---")`.

But a correctly folded name still carries every character of its payload —
`--- Storage ---` is right there in the output, now harmlessly mid-sentence.
This is the same lesson already recorded for the escaping work ("count records,
never `contains`, because correctly escaped output legitimately *contains* the
payload") arriving in a disguise that got past it: a substring *count* looks
quantitative and structural, and is neither.

The guarantee a fold actually provides is positional, so the assertion has to
be too. Every interpolated field is preceded on its line by the report's own
indentation, therefore no field can begin a line, therefore none can *be* a
header. The tests now count lines that satisfy `starts_with("--- ") &&
ends_with(" ---")`, plus the report's total line count. Both survive breaking
each of the eight fold sites individually.


## sysinfo: an environment variable could write a heading of the system report

`apps/sysinfo/src/main.rs`. Fixed in `dab9fab26`. Two bugs of one cause, and
the cause is the interesting part.

`export_text` writes a report whose grammar puts headings at column 0 and data
indented by two spaces. It chose between them like this:

```rust
} else if prop.value.is_empty() {
    out.push_str(&format!("{}\n", prop.name));   // column 0
} else {
    out.push_str(&format!("  {}: {}\n", ...));   // indented
}
```

The empty-value branch exists so the file can emit its own sub-headings —
`Property::new("--- CPU Features ---", "")`. But `props_env_vars` builds a
`Property` directly from each environment pair, and `FOO=` is a legal and
ordinary environment variable. So a variable named `--- Display Outputs ---`
with an empty value printed itself at column 0, byte-identical to the heading
the report writes for the display section.

**This one needed no control characters at all.** Every other finding in this
audit required the payload to smuggle in a newline; folding was therefore a
complete fix for them. Folding does nothing here — there is nothing in the
string to fold. That is worth remembering as a class: *a value can forge
structure without containing any structural character, if the format infers
structure from something other than the value's text.* Here the inference was
from the value's **emptiness**.

The detail-pane renderer had the same bug in its own dialect:

```rust
let is_section = prop.name.starts_with("---");
```

so a variable named `---x` was drawn bold and in the accent colour.

### The fix, and why it is not "escape the name"

Two consumers were each re-deriving *is this row structure?* from the strings.
The strings are environment variables, PCI vendor names and process names —
the one place the answer cannot live. Escaping or folding the name only
narrows the set of strings that happen to fool the inference; it leaves the
inference.

So the distinction is now recorded at construction by the code that knows it:
`PropertyKind::{Heading, Blank, Field}`, with `Property::heading` for the three
sub-headings this file writes, `Property::blank` for the ten separators, and
`Property::new` for data. `Field` rows are always indented — including when
their value is empty, which now means nothing beyond an empty value.

This is the same shape as the fileassoc finding recorded above ("a format with
two writers has no invariant, only a coincidence"), reflected: there, one
format had two *writers* that drifted; here, one format had two *readers* both
inventing an invariant that was never written down.

`Property::new` folding both halves is a second, independent benefit: it closes
the ordinary newline vector for all fourteen `props_*` functions at once —
PCI descriptions, driver paths, process names — rather than at each call site.

### Multiplicity is the new position

sysinfo had no unit tests; there are now seven. The headline one did not catch
its own bug on the first draft, and the reason is the same lesson as
"a substring count is a `contains` in disguise" wearing yet another disguise.

It forged headings that duplicate ones the clean report already contains —
deliberately, because a forgery *identical* to a real heading is the strongest
form of the attack. It then asked, of each column-0 line in the hostile report,
"is this a line the clean report also produced?" The answer was yes, and it
passed.

The assertion has to compare column-0 lines as a **multiset**. Set membership
discards multiplicity exactly as `contains` discards position. Running the
break — reinstating the emptiness guess — now fails
`an_empty_environment_variable_is_not_a_section_heading`, which is the test
named after the bug; before the fix only a bystander test caught it.

Three breaks were run against the final code (reinstate the emptiness guess;
stop folding in `Property::new`; make `Property::new` return `Heading`). All
three are caught, each by at least two named tests.
