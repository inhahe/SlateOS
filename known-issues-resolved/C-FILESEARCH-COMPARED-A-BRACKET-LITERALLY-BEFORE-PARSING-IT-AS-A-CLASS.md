## C-FILESEARCH-COMPARED-A-BRACKET-LITERALLY-BEFORE-PARSING-IT-AS-A-CLASS (lane C, 2026-08-26) — ✅ FIXED 2026-08-26

**In short:** The desktop had two file-search tools — the background indexer and
the Search application — and each contained its own, separately written code for
interpreting search patterns like `*.txt` or `[a-z]`. They did not agree. Typing
the same pattern into the two tools could give you two different sets of files,
and in the worst cases one of them silently found nothing at all and said so as
"no results" rather than as an error. Both tools now use one shared piece of
code, checked against Python's and bash's implementations on 1.9 million
patterns.

**Where it lived.** `apps/filesearch/src/main.rs` → `glob_match_impl` and
`char_class_matches`; `apps/indexer/src/main.rs` → `glob_match_chars` and
`match_char_class`. Both are now `apps/globmatch`.

**How it was measured.** Each matcher was extracted verbatim into a scratch
binary, run over an exhaustive product of every pattern of length ≤ 4 and every
text of length ≤ 3 drawn from the metacharacter alphabet (`* ? [ ] ! - a b` for
patterns, `[ ] - a b` for texts) — 4,681 × 156 = **730,236 pairs** — and every
verdict compared against CPython's `fnmatch`, which implements POSIX 2.13.1.

| Matcher | Disagreements with `fnmatch` |
|---|---|
| `apps/indexer`, after `698afd7cd` | **0** |
| `apps/filesearch` | **646** |

**The three defects in `filesearch`.**

1. **Literal comparison ran before the class parse** (338 of the 646). The
   `if`/`else if` chain tested `pattern[pi] == text[ti]` *first*, so whenever
   the text happened to contain a `[`, the pattern's `[` matched it as an
   ordinary character and the bracket expression was never parsed. `[*]` — the
   only way to search for a literal asterisk — matched `[]`, `[a]` and `[b]`,
   and did not match `*`. The same ordering bug applies to `*`: `*a` failed
   against `*ba`, because the pattern's star was spent matching the text's.

2. **A `]` in the first member position terminated the class.** `class_end` was
   `position(|&b| b == ']')` from the `[`, which for `[]]` finds the `]` at
   offset 1 and yields an *empty* class. POSIX carves out that one position as
   a literal member precisely because a bracket expression has no escape
   character — without the rule there is no way to write a pattern matching a
   `]` at all. So the pattern that means "a bracket" matched nothing.

3. **…and therefore `[!]` was a negated empty class**, which matches every
   character. A user typing `[!]` got every file in the index.

**Why it went unseen.** Same reason as the two sibling bugs found the same day
(`C-INDEXER-A-BRACKET-EXPRESSION-ENDING-IN-A-DASH-MATCHED-NOTHING`,
`C-BACKUP-AN-EXCLUDE-PATTERN-CAN-HANG-THE-BACKUP`): every test used a
well-formed, conventional pattern — `[abc]`, `[a-z]`, `[!a-z]`, `src/*.rs` — and
none of those reaches the arms the bugs live in. A metacharacter never appeared
in a *text*, so defect 1 could not fire; no pattern contained a `]` as a member,
so defects 2 and 3 could not fire.

**The deeper finding, which is why the fix was a new crate rather than a patch.**
Lane C had **four** glob matchers with four different definitions of a glob:

| Location | `*` | `?` | classes | negation | element |
|---|---|---|---|---|---|
| `apps/indexer` | any run | one char | yes | `!` only | `char` |
| `apps/filesearch` | any run | one char | yes (buggy) | `!` and `^` | `char` |
| `apps/backup` | stops at `/`, `**` spans | one char, not `/` | **none** | — | `u8` |
| `gui/toolkit` `context_ext` | any run | — | none | — | `char` |

The first two are both user-facing search tools in the same desktop, both
document `*`, `?` and `[a-z]`, and disagreed — including on whether `[^abc]`
negates, which worked in one and was a literal `^` in the other. That is not a
duplication smell; it is a user-visible inconsistency.

**The fix.** `apps/globmatch`: POSIX `fnmatch` without `FNM_PATHNAME`, over
`&[char]`, with `^` negation carried over from `filesearch` as the single
deliberate extension. Both search tools now depend on it and their private
copies are deleted (−4,498 and −6,205 bytes).

**Evidence.** Over an alphabet extended with `^` — 7,381 patterns × 259 texts =
**1,911,679 pairs** — every verdict is adjudicated by an independent
implementation, and the two disagreement families are disjoint and explained:

| Oracle | Agrees | Differs | The differences |
|---|---|---|---|
| CPython `fnmatch` | 1,911,520 | 159 | all `^`-negation, which `fnmatch` does not implement; all 159 confirmed correct by bash |
| bash 5.2 | 730,167 of 730,236 (no-`^` alphabet) | 69 | all unterminated-`[` at a range-end position; all 69 confirmed correct by `fnmatch` |

The bash divergence is worth recording so nobody "fixes" it: bash falls back to
a literal `[` only when its scanner runs out of pattern at a *member* position,
and hard-fails when it runs out at a *range-end* position — so `[-` and `[a-b-`
match themselves but `[a-` and `[ab-` do not. POSIX 2.13.1 is explicit
("Otherwise, the open bracket shall be treated as an ordinary character") and
CPython agrees with us, not with bash.

**Lesson worth keeping.** Two implementations of the same user-facing concept
are not redundant — they are a *disagreement waiting to be discovered by a
user*, and neither one's test suite can see it, because each is tested only
against itself. The differential is what makes the disagreement visible, and it
needs a third-party oracle: had the two been compared only to each other, all
646 differences would have looked like a coin-flip over which was right.
