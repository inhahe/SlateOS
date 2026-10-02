## 555. One glob matcher for the two search tools, two dialects for the desktop

> **Superseded in part on 2026-09-13 by §841.** The first half -- merging the
> search window's matcher with the indexer's -- stands. The second half, that
> `apps/backup` speaks a legitimately different dialect, does not: character
> classes turn out to be standard in the exclude-list family as well, so
> backup was the only matcher in the tree missing the feature rather than one
> implementing a different language. It now has them.

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop shipped with four separate pieces of code that each
decided, in their own way, what a search pattern like `report-*.txt` or
`[a-z]*.log` means. Two of them — the file-search window and the background
indexer — are the same feature seen twice, and they *disagreed*: measured over
730,236 patterns, the search window got 646 of them wrong. They now share a
single implementation, so the desktop can only hold one opinion about what a
pattern means. The backup tool's patterns deliberately stay separate, because
they are a genuinely different language and merging them would silently change
what people's existing exclude lists exclude.

### The four dialects that existed

| Location | `*` | `?` | bracket expressions | negation | element |
|---|---|---|---|---|---|
| `apps/indexer` | any run | one char | yes | `!` only | `char` |
| `apps/filesearch` | any run | one char | yes, but broken | `!` and `^` | `char` |
| `apps/backup` | stops at `/`, `**` spans | one char, not `/` | **none** | — | `u8` |
| `gui/toolkit` `context_ext` | any run | — | none | — | `char` |

Two of these are not four-fifths of a duplication problem. They are two
*different* problems:

- **`apps/indexer` and `apps/filesearch` are the same user-facing feature.** A
  user types a pattern into the search window and expects the indexer to have
  found the same files. Two implementations of one concept are not redundancy,
  they are a disagreement waiting to be found by a user — and neither test suite
  could ever see it, because each was tested only against itself.
- **`apps/backup` is a different language.** Its patterns are gitignore-shaped:
  they match *paths*, `*` stops at a `/`, `**` spans segments, and `[` is an
  ordinary character. That is not a worse fnmatch; it is the right dialect for
  "exclude this subtree."
- **`gui/toolkit`'s `context_ext`** matches file *extensions*, not paths, and
  its patterns come from a settings file rather than a user's keyboard.

### The decision

`apps/globmatch` is a new library crate holding one implementation, consumed by
`apps/indexer` and `apps/filesearch`. `apps/backup` keeps its own.

**The alternative — one matcher for all four — was rejected, not deferred.** It
would mean `apps/backup` gaining bracket expressions, which changes the meaning
of every existing exclude list containing a `[` from "a path with a literal
bracket in it" to "a character class". That is a user-visible change to data
users already wrote, so it is the operator's call rather than a refactor; it is
filed as `open-questions.md` → **C-Q9**.

**The opposite alternative — leave both matchers, just fix the bugs — was also
rejected.** It fixes the 646 known disagreements and leaves the mechanism that
produced them fully intact. The next bug fixed in one and not the other is the
same bug again.

### The one deliberate departure from POSIX

`globmatch` accepts `^` as a synonym for `!` at the start of a bracket
expression, so `[^0-9]` means "not a digit". POSIX and CPython's `fnmatch`
accept only `!`; bash, ksh and zsh accept both.

This is not a coin flip. `apps/filesearch` already accepted `^`, and its own
test suite asserts it (`("[^0-9]*", "hello", true)`). Dropping it would not have
been "conforming to POSIX" from a user's point of view — it would have silently
turned every negated class a user had already written into a class that matches
a literal caret, which is close to the exact inverse of what they asked for.
Adding `^` to the indexer, by contrast, can only *widen* what the indexer
understands: no pattern that worked before changes meaning, because `[^…]`
previously meant "matches a caret or one of …", which nobody types on purpose.

Cost: one construct where we differ from `fnmatch`. Measured: of 1,911,679
pattern/text pairs, 1,911,520 are identical to CPython `fnmatch`, and every one
of the 159 that differ is a `^`-negation — where we agree with bash instead. The
two disagreement sets are disjoint and both are documented in the crate's module
docs.

### The three rules that look like bugs and are not

All three are POSIX 2.13.1, all three are what CPython does, and all three are
now tested:

1. **`[]abc]` — a `]` in the first member position is a literal `]`**, not an
   empty class. A bracket expression has no escape character, so this is the
   only way to put a `]` in a class at all.
2. **`[a-]` — a `-` in final position is a literal `-`**, not a half-written
   range. The old indexer read `pattern[i+2]` after checking only that index
   `i+2` *exists*, so it built the range `a`..`]`.
3. **`[abc` — an unterminated `[` is an ordinary character.** POSIX:
   *"Otherwise, the open bracket shall be treated as an ordinary character."*
   bash disagrees here, but inconsistently with itself: it falls back to a
   literal `[` when its scan runs out at a member position and hard-fails when
   it runs out at a range-end position, so `[-` matches itself and `[a-` matches
   nothing. That is an implementation artifact, not a rule, and it is the one
   place where bash — not us — is the outlier against both POSIX and CPython.

### Where it lives

`apps/globmatch/src/lib.rs` (`glob_match`, `glob_match_chars`,
`match_char_class`), consumed by `apps/indexer/src/main.rs` and
`apps/filesearch/src/main.rs`, which re-exports it so its callers are unchanged.
The defect inventory and the measurement method are in `known-issues.md` →
`C-FILESEARCH-COMPARED-A-BRACKET-LITERALLY-BEFORE-PARSING-IT-AS-A-CLASS`.
