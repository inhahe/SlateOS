## B-BACKREFERENCES-ARE-REFUSED-SO-SOME-CLASSIC-ONE-LINERS-FAIL (lane B, 2026-08-16) — **FIXED 2026-08-18**

**Status:** FIXED 2026-08-18 — `userspace/ere` grew the backtracking matcher
this entry asked for, and the design entry it asked for first is
`design-decisions.md` §333. See the "Fixed" section at the end.

**In short:** a backreference is the part of a pattern that says "and here the
*same text* again" — `\(.*\)\n\1` means "some text, a newline, then that exact
text once more". Our regex engine cannot express one, so it refuses the pattern
with an error instead of matching. GNU accepts it. The visible effect is that a
few well-known `sed`/`grep` one-liners — most famously the `sed` spelling of
`uniq` — print an error and exit non-zero where on Linux they would work.

```
$ printf 'x\nx\ny\n' | sed '$!N;/^\(.*\)\n\1$/!P;D'
sed: -e expression #1, char 18: backreference \1 is not supported     # ours, rc=1
x                                                                     # GNU,  rc=0
y
```

This is the one case out of 89 in `scripts/sed-diff.sh` that does not match GNU,
and `grep '\(a\)\1'` fails the same way.

### Why it is refused rather than wrong

`userspace/ere` is a Pike VM (a Thompson NFA simulation): it advances *all*
alternatives of the pattern through the subject in one left-to-right pass, so it
costs `O(len(input) × len(pattern))` and cannot be made to backtrack into
catastrophic time — that immunity is why it was chosen (`design-decisions.md`
§322). But the same property is why a backreference is impossible: matching one
requires re-comparing against text an *earlier* alternative captured, and in a
simulation where every alternative advances together there is no single "the"
capture to compare against. So this is not an unfinished feature; it is the
shape of the engine.

Refusing by name is deliberate. Silently treating `\1` as a literal `1` would
turn a pattern that cannot be answered into one that is answered *wrongly*, and
a wrong match in `sed` edits the user's file.

### The proper fix

What glibc does: keep the Pike VM for everything, and add a **backtracking
matcher used only for patterns that contain `\1`–`\9`**. The compiler already
knows at parse time whether a pattern has one, so the choice is made once, per
pattern, not per input line — and every pattern that does not use a
backreference keeps today's linear guarantee untouched.

It is a real tradeoff and wants its own `design-decisions.md` entry before it is
built, because it reintroduces exponential blowup for exactly the patterns that
take the fallback, and `grep -f` / `sed -f` read patterns from files. The
mitigations are the usual ones — a step budget that aborts the match with an
error rather than hanging, and the existing `MAX_PROG` bound on program size.

**Until then** the behaviour is safe: it is loud, it is specific about which
construct it cannot do, and it exits non-zero. Nothing silently produces a wrong
answer. What it costs is GNU compatibility for a small, well-known family of
scripts.

### Fixed (2026-08-18)

Built as the entry above proposed, and as glibc does it: the Pike VM still runs
every pattern that has no `\1`–`\9`, and a pattern that has one is run by a new
backtracker instead. The choice is made once, at compile time, from a flag the
parser sets — `Regex::has_backref` — so no pattern that does not use the feature
pays anything for its existence. `design-decisions.md` §333 records the
alternatives and why each was rejected; the short version:

- **The step budget is real and reachable.** `1_000_000 + 1_000 × len(subject)`,
  capped at `100_000_000`. The pathological `\(a*\)\(a*\)\(a*\)\(a*\)\(a*\)\1\2\3\4\5b`
  against 300 `a`s now stops in **54 ms** with
  `grep: '…': backreference matching exceeded its step limit` and status 2.
- **"I gave up" is not spelled "no match."** This is the part that reached
  outside `ere`: every matching entry point returns `Result<_, MatchLimit>`, so
  the five callers had to be changed rather than merely recompiled. Reading an
  abandoned search as a non-match would have made `sed '/re/!d'` delete the
  lines it declined to examine and `grep -v` nominate them for `xargs rm`.
  `awk`'s `FS`/`RS` are program-chosen regexes, so the same requirement made
  record splitting — and therefore field and variable access — fallible there.
- **The backtracker uses an explicit stack**, not recursion (depth grows with
  repetitions matched, so a megabyte of `a` would otherwise overflow), and
  refuses a backward jump twice at the same input position on the same path,
  which is how an empty loop terminates.
- **A reference to a group that did not participate fails**, per POSIX; it does
  not match the empty string. A reference to a group the pattern does not have
  is a compile error, not the literal digit.

What the fix bought, measured:

| Check | Before | After |
|---|---|---|
| `printf 'x\nx\ny\n' \| sed '$!N;/^\(.*\)\n\1$/!P;D'` | error, rc=1 | `x` `y`, rc=0 |
| `scripts/sed-diff.sh` | 88/89, 1 differed | **89 passed, 0 differed** |
| `scripts/expr-diff.sh` | 1 xfail for `\1` | that case is a `run_case` now; 5 backreference cases agree with GNU |
| `scripts/nl-diff.sh` | 227 passed, 5 xfail, one of them `-bp'\(ab\)\1'` | **228 passed, 4 xfail**; the backreference case agrees |
| `scripts/awk-diff.sh` | 9 xfail | 10 — awk gains one, because `gawk --posix` reads `\1` as the octal escape `\001` and we read GNU `grep -E`'s backreference. POSIX leaves it undefined, so this is a choice between two extensions and the one that agrees with this system's other four regex users wins. |

The `nl -bp` and `sed-diff` divergences this entry named as *possibly* caused by
the gap were in fact caused by it: both disappeared with no further change.
