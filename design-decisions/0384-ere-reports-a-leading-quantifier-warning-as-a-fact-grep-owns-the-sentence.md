## §384 — `ere` reports a leading-quantifier warning as a fact; `grep` owns the sentence

**Decided by:** Claude (autonomous)
**Date:** 2026-08-25

**In short:** `grep -E '*a'` is a pattern with a repetition sign (`*`) in front
of nothing to repeat. GNU grep runs it anyway — it ends up meaning plain `a` —
but prints `grep: warning: * at start of expression` first, because a user who
wrote that almost certainly meant something else. Ours ran it and said nothing.
Fixing that meant deciding where the message should be *made*: in the shared
regular-expression engine that noticed the problem, or in `grep`, the only one of
the engine's five users that treats it as a warning at all. It is made in `grep`;
the engine only reports that it happened.

### What was decided

The engine (`userspace/ere`) collects a list of
`Warning::QuantifierAtStart(Quantifier)` values and hands it back from a new
`Regex::new_syntax_warn`. It contains no English: the `Quantifier` names which
operator it was (`*`, `+`, `?`, or `{...}` for any interval), and nothing else.
`grep`'s `compile_patterns` turns each into GNU's sentence and `main` prints them
before searching. `Regex::new_syntax` still exists and drops the list, so no
existing caller changed.

### Why the engine does not own the wording

The engine has five callers, and four of them — `osh`, `find -regextype
posix-extended`, `sed -E` and `awk` — compile in `Syntax::POSIX_EXTENDED`, where
exactly these patterns are **errors** (`REG_BADRPT`), not warnings. There is no
sentence that is right for both groups: `find` must refuse `*a` and say so, and
`grep` must run it and merely remark. A message baked into the engine would have
to be either wrong for four callers or parameterised by caller, which is the same
decision made in a worse place.

The `Quantifier`/`Warning` pair is also the smaller commitment. It says what the
parser saw, which cannot become stale; a string would pin the engine to grep's
phrasing for every future caller, including ones that want to translate it or to
report a column number.

### Why a warning at all, rather than leaving it

This was one of eight `?` gaps in `scripts/grep-diff.sh` and the only one that
was not a matter of phrasing. The other seven are diagnostics that say the same
thing in different words. This one is a diagnostic we did not emit **at all**,
and it is the only signal a user gets that their pattern silently means something
other than what they wrote — `grep -E '*.txt'` looks like a glob and matches
every line containing `.txt`, with no complaint from us. Reproducing grep's
matching behaviour without its warning reproduces the trap and removes the sign.

### The rule, which had to be measured rather than recalled

Warn once per repetition operator *consumed* while the current branch has not yet
produced an atom. Every clause of that was measured against grep 3.11 (~50
patterns), because none of it is guessable:

| shape | warns | why |
|---|---|---|
| `*a` `+a` `?a` `{2}a` `{,3}a` | yes | nothing at all before the operator |
| `**a` `*+?a` | yes, once each | a stacked operator still has no atom under it |
| `^*` `$*` `\b*` `\<*` | yes | an assertion is not something to repeat |
| `a^*b` `a$*` `a\b*` | no | the `a` is the atom the branch needed |
| `()*` `a()*` `(a)*` | no | a group is an atom to what encloses it, even empty |
| `a\|*b` `(*a)` `(\|*a)` | yes | each branch and each group body starts afresh |
| `{}a` `{1,2,3}a` `{b}a` | no | the `{` rolled back to a literal; no operator was consumed |
| `-e '*a' -e '*a'` | once | GNU collapses duplicate patterns before compiling |

The last row is why `grep` now deduplicates its pattern list. That is invisible in
a search — two copies of a pattern select the same lines as one — so it had never
mattered; the warning is what makes it observable, and doing it for real rather
than only for the diagnostic also saves the duplicate its search.

The `{}a` row is the one that would have been got wrong by reasoning. `a{}` is a
hard error in both dialects, so `{}a` looks like it should warn; it does not,
because at the start of an expression glibc rolls the malformed brace back to a
literal and never reads an operator there. Our parser already had that rollback,
which is why the warning could be attached to "an operator was consumed" and come
out right on all three brace shapes without a special case.

### Alternatives rejected

- **Print from the engine directly.** Rejected: the engine is a library with no
  business writing to stderr, and it does not know the program name to prefix.
- **Return the warnings as strings.** Rejected for the reason above — four of five
  callers need a different sentence for the same fact.
- **Make it an error under egrep syntax too.** Rejected outright: it would refuse
  patterns GNU grep runs, which is the failure `Syntax::EGREP` exists to prevent.
- **Deduplicate only for the diagnostic, leaving the search to run both copies.**
  Rejected as the more complicated of the two: it needs a second pass whose only
  purpose is to be consistent with a pass that already exists, and it leaves a
  measurable behaviour (GNU's dedup) unimplemented for no gain.
