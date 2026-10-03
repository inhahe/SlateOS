## 1160. `regexec` reports submatches by the standard's rule, not by the first path glibc's automaton finds -- and follows the standard in the places glibc contradicts itself

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a regular expression can often match the same text in more
than one way -- `(a|ab)(c|bcd)(d*)` matches "abcd" as "a", "bcd" and "", or
as "ab", "c" and "d" -- and a program asking `regexec` for the parts in
parentheses gets one of those ways. POSIX says which: after the whole match is the
leftmost and longest, "each subpattern, from left to right, shall match the
longest possible string". glibc does not follow that rule; it takes the
first way its internal automaton happens to find, which favours the first
alternative and the greediest loop. This library's new `regcomp`/`regexec`
follows the standard, so for a pattern where the two differ, `\1`, `\2`...
come out as POSIX says rather than as glibc returns them. The whole match
(where it starts and ends) is the same in both, except in a handful of
places where glibc is simply wrong by its own answers elsewhere, and there
this library gives the answer glibc gives everywhere else.

**The rule, exactly.** "Longest subpattern first, left to right" is read as
Okui and Suzuki formalise it (CIAA 2010, and Borsotti and Trofimovich after
them): of all the ways -- parse trees -- the leftmost-longest match can be
made, the one greatest when the trees are compared position by position in
pre-order, each position by the length it matched (-1 for a subpattern that
took no part). So a concatenation's elements are made as long as they can
be in turn, the first first; of an alternation's branches that match the
same text, the first; a repetition's iterations are made as long as they can
be in turn, each past the minimum count non-empty, and one that matched
nothing reports one empty iteration rather than none ("a null string shall
be considered to be longer than no match at all", XBD 9.1, whose own
example is `\(a*\)*` against "bc"). Then, as XSH regexec says, a group
reports its last match, and a group inside another reports only within what
that one reports.

| Pattern | Subject | Here (POSIX) | glibc 2.39 | Why glibc's is wrong |
|---|---|---|---|---|
| `(a\|ab)(c\|bcd)(d*)` | abcd | (0,2) (2,3) (3,4) | (0,1) (1,4) (4,4) | the first subpattern is not the longest it can be |
| `(a\|ab)b*` | ab | `\1`=(0,2) | (0,1) | same |
| `((a)\|b)*` | ab | `\2`=-1 | `\2`=(0,1) | `\2` is inside `\1`, whose report is the last iteration, "b" -- `\2` took no part in it |
| `(a?){2,3}` | a | `\1`=(1,1) | (0,1) | the first iteration is the longest, "a"; the second, required, is empty |
| `(a*)+\1` | aaa | (0,3) | (0,2) | glibc misses the longer match altogether |
| `(a*)*\1` | aaa | `\1`=(1,2) | `\1`=(0,-1) | a half-set pair is no submatch at all |
| `$.` (no REG_NEWLINE) | "\n" | no match | (0,1) | glibc's `a$` does not match "a\nb" -- without REG_NEWLINE a newline is no line end, as POSIX says |
| `(\Ba){0,2}` | a | (0,0) | (0,1) | glibc's own `\Ba` and `(\Ba)?` find no `\B` there |
| `(^[a-c]{0,2}){0,2}.{2,}\|[^a]{0,1}` | aaa | (0,3) | no match | the second branch matches the empty string anywhere |
| `\a`, REG_ICASE | a | (0,1) | no match | glibc upper-cases the pattern but not the escaped letter |
| `[Z-a]`, REG_ICASE | | accepted | REG_ERANGE | the range is valid as written; glibc reads it as `[Z-A]` |
| `(){32767}` and four more | | an answer | crash | |

In the oracle (`posix/tools/oracle/regex_harness.py`, `regex_oracle.txt`,
some 544,000 answers) the 16,444 lines of `posix/src/regex_deviations.txt`
record, with both answers, each place the standard's is given instead of
glibc's. By the harness's count of subjects: 8,259 differ in the submatches
alone (the rule above); 1,662 have back-references (where glibc also misses
matches); 5,599 are under REG_ICASE (the case of escapes and range ends),
where 277 patterns also compile differently; 408 are where glibc's `\B`, `^`
or `$` contradict its own answers; and 5 patterns crash glibc. `posix/tools/oracle/regex_model.py` computes the standard's answer
by enumerating every parse -- independently of this library's engine, which
dissects the match with automata -- and the harness refuses to write the
files if the model and glibc disagree anywhere but in the submatches or one
of those listed reasons.

**Alternatives.**

- *glibc's answers exactly.* That would mean reproducing its engine's
  search order, which is nowhere documented, and its outright bugs (the
  crashes, the missed matches, the half-set pairs, the `\B` and `^`
  failures) -- the D-Q6 rule is glibc where POSIX leaves it open, and the
  submatch rule is not left open.
- *Fowler's "left-associative" reading of the rule*, under which
  `(a|ab)(c|bcd)(d*)` makes the first two groups together as long as they
  can be (glibc's answer happens to agree there). The text says "each
  subpattern, from left to right", which is the reading above; it is also
  Okui and Suzuki's, Kuklewicz's (regex-tdfa) and RE2C's.
- *Empty iterations allowed anywhere* (dispreferred rather than excluded).
  It only matters with back-references -- `(a*)*\1` against "a" could then
  match (0,1) through an empty second iteration -- and glibc's answer there,
  (0,1) with `\1`=(0,0), fits neither reading.

**The cost.** A program ported from Linux that relies on glibc's particular
choice of submatch -- a `sed` script using `\(a\|ab\)\(c\|bcd\)`, say -- sees
the standard's instead. Such patterns are ambiguous by construction, and a
program written from the standard, or tested against musl, BSD or AT&T's
library, expects this.

**Where:** `posix/src/regex.rs` and `posix/src/regex/`; the oracle and its
model in `posix/tools/oracle/`.
