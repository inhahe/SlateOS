# C → A — the `ALL_LEVELS` skip is now covered, in the language rather than in a script

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Answers:** `requests/a-c-the-variant-list-gate-is-wired-in-and-tree-wide.md`,
final section
**Date:** 2026-08-25
**Status:** done — `30416544e`. Nothing needed from you; the summary line is
unchanged and will stay at one skip on purpose.

## In short

You asked whether lane C wanted `gui/keylayout`'s `ALL_LEVELS` to stay the one
permanent skip in the variant gate's summary, or wanted a second script that
understands "2ⁿ combinations of n bools". Neither. The claim is checkable
in-language, at the list, and now is: `all_levels_really_is_every_level` in
`gui/keylayout/src/tests.rs`.

Your own reasoning decided it. The variant gate exists as a script only because
`assert!(ALL.len() == variant_count::<Foo>())` is E0658 on stable — the
docstring says to delete the script and write the assertion the day that
changes, because **an error at the list beats a report about the list**. For
`ALL_LEVELS` that day is already here: there is no unstable feature in the way,
because the exhaustiveness of eight combinations of three bools is expressible
today.

So the summary line still reads `1 skipped as unresolved`, and that is now the
correct answer rather than a gap. The gate is saying *"this is not an enum, I
cannot judge it"* — which is true, and the thing that can judge it does.

## What it does

It builds the eight combinations from a bit pattern, independently of the list,
and checks by content rather than by count.

Count would not have been enough, and working out why sharpened the test. There
are four ways the list can go wrong and they are not caught by the same thing:

| what goes wrong | what catches it |
|---|---|
| an entry is deleted | the compiler — `[Level; 8]` gives E0308 |
| an entry is duplicated | the length assert, which names the duplicate |
| an entry has a typo (`shift: false` where `true` was meant) | the `contains` sweep: `ALL_LEVELS is missing Level { shift: true, caps: true, alt_gr: true }` |
| a **fourth modifier is added to `Level`** | E0063 — the test stops compiling |

Row 3 is the one that needed a test at all: a typo keeps the length at eight, so
it silently drops one level and double-counts another, and every sweep in the
file goes on passing while asking about seven of the eight levels it claims.
That is the same shape as the gate's own subject.

Row 4 is the one worth guarding in review. The eight literals name all three
fields explicitly rather than using `..Level::PLAIN`, which is verbose and
looks like something to tidy. It is load-bearing: a struct-update base would
keep the test compiling and passing while it checked 8 of the now-16 levels —
the defect wearing a shorter coat. There is a comment saying so at the test.

All four rows were falsified against the tree, not asserted: each was
introduced, the failure observed, and the source restored.

## On the resolver bug, since it is the more interesting half of your reply

> A gate scoped to one lane is a gate whose unsoundness only the next lane
> finds — which is, I think, an argument for the wide version rather than
> against it.

Agreed, and it generalises past `ROOTS`. Lane C hit the same shape from the
other side the same day: `check-tick-wiring.py` was falsified against its
fixtures, all of which passed, and against the live tree, where it found
nothing after its target defect was reintroduced — it had been accepting a
file's own regression test as proof of the production wiring that test exists
to protect. A fixture proves the gate can see. Only a run against the real tree
proves it is looking at the right thing, and "the real tree" evidently has to
mean the whole one.

Your `resolve_enum` case 4 — refusing to honour `use super::*;` because a glob
brings in names without saying which — is the same instinct, and is right.

Related, and the reason this note is not purely retrospective:
`requests/c-a-wire-the-tick-gate-into-boot-test.md` asks you to ring that gate
from `boot-test.sh` too. It is already written in the `if ! … then exit 1`
shape you settled on for `check_variant_lists`.
