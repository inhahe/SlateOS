## `C-INDEXER-A-BRACKET-EXPRESSION-ENDING-IN-A-DASH-MATCHED-NOTHING` (lane C, 2026-08-26) — ✅ **FIXED** 2026-08-26

**In short:** `indexer search` lets you use shell-style wildcards, including
`[abc]` to mean "any one of these characters". If you ended that bracket with a
dash — `[a-]`, meaning "an `a` or a dash" — it silently matched no files at
all, rather than the files it should have. You would see an empty result list
and no error, and conclude the files were not indexed. Fixed; the pattern now
behaves the way it does in every shell.

**Where it lived.** `apps/indexer/src/main.rs` → `match_char_class`.

**The defect.** The range arm was guarded by:

```rust
if i + 2 < pattern.len() && pattern[i + 1] == '-' {
```

That asks whether a third character *exists*, not whether it is a range end.
For `[a-]` the third character is the closing `]`. So the parser took the range
arm, read `]` as the upper bound of the range `a..=]` (empty, since `]` is 0x5D
and `a` is 0x61), advanced past it, and then ran to the end of the pattern
looking for a closing bracket it had already consumed. Falling out of the loop
with no `]` in hand, it returned `None` — "malformed class" — and a malformed
class never matches. `[a-]`, `[b-]` and `[--]` therefore matched nothing.

**Why it went unseen.** The five existing glob tests all used well-formed,
conventional patterns: `[abc]`, `[a-z]`, `[!abc]`. Every one of those has a
character after the `-`, or no `-` at all, so none of them enters the arm the
bug lives in. This is the same reason `apps/backup`'s `**` hang went unseen —
see `C-BACKUP-AN-EXCLUDE-PATTERN-CAN-HANG-THE-BACKUP`, found the same day. A
test suite written from the documentation exercises the shapes the
documentation describes, which are exactly the shapes that work.

**The oracle.** POSIX and `fnmatch(3)` make a `-` in final position of a bracket
expression an ordinary character, because there is nothing after it for it to
be a range to. Confirmed against bash directly rather than from memory:

```
case a in [a-]) ;; esac   # matches
case - in [a-]) ;; esac   # matches
case - in [--]) ;; esac   # matches
case - in [a-z]) ;; esac  # does NOT match
```

**The fix.** The range arm now requires the character after the `-` to exist
*and not be the closing bracket*, and the whole class parser reads through
`get` rather than indexing, so each access carries its own bound instead of
borrowing one from a comparison two lines up.

**Evidence the rewrite changed nothing else.** The old and new implementations
were run against each other over an exhaustive product of 4,681 patterns × 85
texts = 397,885 pairs, from an alphabet chosen to reach every arm (`*`, `?`,
`[`, `]`, `!`, `-`, `a`, `b`):

| Result | Count |
|---|---|
| agree | 397,877 |
| differ | 8 |

All eight differences are the trailing-dash case (`[a-]`, `[b-]`, `[--]`,
`[*-]`, `[?-]`, `[[-]`), and in all eight bash agrees with the new behaviour.
Pinned by `a_trailing_dash_in_a_class_is_a_literal_dash`.

**Lesson worth keeping.** A bounds check that asks "does an element exist here?"
is not the same question as "is the element here the one I need?", and the two
diverge exactly at the end of the input. `i + 2 < len` was true; `pattern[i+2]`
was still the wrong character. Reading through `get` does not by itself fix
that — but it forces the second question to be written down, because there is
no index to silently answer the first one instead.
