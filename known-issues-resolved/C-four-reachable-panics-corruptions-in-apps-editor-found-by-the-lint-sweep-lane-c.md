## Four reachable panics/corruptions in `apps/editor`, found by the lint sweep (lane C)

**Status: FIXED 2026-08-16** (lane C). All four were found by working through
`clippy::indexing_slicing` in `apps/editor` and checking each site for a *real*
panic rather than silencing it. None was caught by any existing test; each now
has one.

**1. JS regex literal emitted a token past the end of the line.** In
`highlight.rs`, the regex-literal scanner stepped `i += 2` over a backslash
escape without checking that the second byte existed, so a line ending in
`/\` produced a token ending at `len + 1`. The renderer slices the line by
token range, so this panicked the editor — taking every unsaved buffer in
every other tab with it — on a keystroke. Proved reachable by reverting the
fix and watching the highlighter sweep test fail with `JavaScript: token 0..3
out of bounds for "/\" (len 2)`. Fixed by routing the scan through
`scan_to_delimiter`, which clamps. Covered by the sweep test, now 12 languages
× 5 prefixes × 17 line-endings × 7 entry states = 7140 cases.

**2. Find reported overlapping matches.** `FindState::find_all` resumed the
search one byte into the match it had just recorded, so `"aaaa"` reported
three occurrences of `"aa"`. Two consequences: the match count shown to the
user was wrong, and `replace_all` then rewrote overlapping ranges one after
another, shredding the line. The one-byte step also landed *inside* a
multi-byte character, where the old `search_line[start..]` panicked outright.
Fixed by resuming past the match end.

**3. Find computed offsets against a string that is not the one being
edited.** The case-insensitive path searched a `to_lowercase()` copy of the
line and used the offsets it found to index the *real* line. `to_lowercase`
is not length-preserving — Turkish `İ` (U+0130) folds to two characters,
three bytes, from two — so past the first such character every offset is
wrong. The editor selected, or replaced, the wrong bytes; a span past the end
of the line reached `String::replace_range`, which panics. Fixed with
`folded_match_end`, which folds *incrementally* while walking the real line,
so every offset it returns is an offset into the line the user is looking at,
and returns the match's real end (which can differ in length from the needle,
for the same reason).

**4. Replace trusted ranges recorded against a different document.**
Find/replace records `(line, start, end)` against whichever document was
searched, and nothing stopped a caller handing a different document to
`replace_all`, or editing the buffer between the search and the replace.
`String::replace_range` panics on an out-of-range span *and* on one that
splits a character. Fixed by making `Document::replace_in_line` validate the
line index, the ordering, the end bound and both char boundaries, returning
`false` rather than panicking.
