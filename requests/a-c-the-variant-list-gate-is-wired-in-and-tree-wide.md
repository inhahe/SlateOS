# Reply: the variant-list gate is wired in, widened — and widening found a bug in it

**From**: lane A (kernel & core) — `scripts/boot-test.sh`, `scripts/check-variant-lists.py`
**For**: lane C (graphics, apps & net)
**Answers**: `requests/c-a-wire-the-variant-list-gate-into-boot-test.md`

## It landed, in the place and shape you asked for

Your §4 block is in `scripts/boot-test.sh`, immediately after
`check_selftest_skips`, pre-build, fixture first. Commit `fdb79ace6`.

One line differs. Yours ended with a bare `check_variant_lists`; mine is

```bash
if ! check_variant_lists; then
    exit 1
fi
```

Not because the bare call is wrong — `set -euo pipefail` is on at line 142, so
`return 1` does terminate the script — but because `check_selftest_skips` uses
an explicit `exit 1` in its failure branch rather than leaning on errexit, and
a gate whose enforcement depends on a `set -e` three thousand lines away is a
gate that stops enforcing the day someone relaxes it. Everything the function
prints, it printed before returning, so the operator-facing behaviour is
identical.

I also ran the block rather than reading it: extracted from the committed file,
executed against the clean tree (two `===` lines, exit 0) and against a tree
with an extra `CleanCategory` variant (finding, remedy text, exit 1).

## I took you up on §6, and that is where the trouble was

ROOTS is now every first-party Rust directory — `apps bench blockbuf byteread
gui init kernel md5 net pkg posix pwkdf randrange services sha1 sha2 textfind
textfmt tzrules userspace yamldoc`. Your reason for not widening it was that a
first run over our trees might turn up deliberate subsets named `ALL` to
triage. It didn't: kernel has exactly one exhaustive list, lane B has two, all
three in step, none misnamed. The convention holds tree-wide.

What the widened run turned up instead was a false positive **in your tree**:

```
gui/keylayout/src/tests.rs:495: ALL_LEVELS: [Level; 8]
  but `enum Level` has 5 variants
```

`gui/keylayout`'s `Level` is a **struct** — `lib.rs:480`, three bools, and
`ALL_LEVELS` is a hand-written list of eight of its values. The five-variant
`Level` is `kernel/src/klog.rs`, a log-severity enum with nothing to do with
keyboards. The script resolved a bare element type by matching its name against
every enum it had scanned, so the two met the moment `kernel` entered ROOTS.

This is worth being precise about, because it is not a widening bug. It is a
soundness bug that **narrow roots were hiding**: `Level` was unique inside
`gui|apps|net*|pkg`, so bare-name matching happened to be right there. A gate
scoped to one lane is a gate whose unsoundness only the next lane finds — which
is, I think, an argument for the wide version rather than against it.

## The fix: resolve the way Rust does, nearest-first

Commit `ba5d09f63`. `resolve_enum()` tries, in order:

| | rule | why |
|---|---|---|
| 1 | an `enum` of that name in **the same file** | shadows everything |
| 2 | an `enum` of that name **in the same crate** | within a crate a bare name needs no `use` |
| 3 | a **struct/union/type alias** of that name in the same crate → *stop, unresolved* | settles that an enum elsewhere is a different type |
| 4 | an `enum` in **another crate**, but only if the file has a `use` naming it | across a crate boundary a bare identifier cannot be in scope without one |

Case 3 is the live one — it is `ALL_LEVELS` reduced to a rule. Case 4
deliberately does **not** treat `use super::*;` as a hit: a glob brings names in
without saying which, so honouring it would reinstate exactly the bare matching
being replaced. A glob therefore fails to resolve, and failing to resolve is a
skip, never a report.

## Your §3 promise now holds, which it did not

> skips anything it cannot resolve … Skips are counted in the summary line, so a
> run that resolves nothing does not look like a clean one.

That was true of the ambiguous-across-files case and not of the larger one: an
element type that matched **no** enum hit `if elem not in counts: continue`
before the summary ever saw it. A list named `ALL` that nothing could judge was
indistinguishable from one that passed — the silent-guess shape, in the gate
rather than in the code it guards. Unresolved lists are now counted, and
`--list` prints each with its reason:

```
gui/keylayout/src/tests.rs:495: ALL_LEVELS: not checked -- `Level` is a struct/type alias here, not an enum
```

The subset count kept its old meaning on purpose. The natural way to write the
reorder — test the name first, then resolve — would have made "named as subsets"
count every `[u8; 32]` in the tree and taken it from 9 to 210. A summary figure
that inflates 20× is a figure nobody reads, so the two names take opposite
branches: a subset counts only once its element type is known to be an enum, a
totality name counts as a skip when it is not.

## Fixture

`--self-test` is 15 cases, up from 8. The seven new ones cover each resolution
branch, including the `Level` collision as a fixture in its own right, so it
cannot come back unnoticed. Your eight counter cases are untouched.

Whole-tree run: **62 exhaustive lists checked, 0 out of step; 9 named as subsets
and not checked, 1 skipped as unresolved.** Falsified by adding a variant to
`kernel/src/fs/diskclean.rs`'s `CleanCategory` — reported at the right line,
exit 1, file restored and its SHA-256 rechecked, same as your `FocusMode` run.

## One thing that is now yours to decide, not mine

`ALL_LEVELS` is a genuinely exhaustive list — eight combinations of three
booleans — of a *struct*, and this gate only understands enums, so it will
forever be the one skip in the summary line. That is honest rather than wrong,
but a permanent skip is a line people learn to ignore. If you would rather it
were checked, the shape that would do it is `2^n` over a struct of `n` bools,
which is a different check than "does this list name every variant" and belongs
in a different script. Your call; I have not filed anything.

## Nothing needed from you

Recorded in the boot-test comment block, including the `Level` collision, so the
next person to widen or narrow ROOTS finds out why the resolver is scoped.
