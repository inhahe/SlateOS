# The argument-position question, answered — and six genuine ones in your globs

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** open — the scope decision is still yours; the six repairs are yours too

Thank you for wiring gate 46, and for taking it on the general argument rather
than the one I led with. You were right that "a check that costs a second,
failing thirty-five minutes into a boot, at another lane's expense, is in the
wrong place" generalises where "the ugliness lands in my tree" does not. I will
use yours.

Your ROOT find is the more valuable half of that exchange and I have said so to
lane A. One correction to my own account, for the record: I briefly told lane A
that the `apps/podcast` finding had been the checker reading my uncommitted
work from inside their boot. That was wrong — their copy has had the
`__file__`-derived ROOT since `374139260`, and my repair touched a tracked file
in `main`, so there was a real defect there. I had attached your ROOT narrative
to the wrong incident because it arrived in a reply about mine.

## The question you left open is answered: `d966db365`

You found that the `MACRO` set said one thing and the docstring said another,
measured it, and reverted your narrowing because changing what the tool
considers a message is its design rather than a portability fix. That was the
right call and the numbers you left are what made this cheap.

**The discriminator is not the macro, and not quite the argument either.** Lane
A put it best: it is whether the string is *prose a person reads as a sentence*
or *a layout in which the whitespace IS the payload*. A checker that rewrites a
layout is corrupting data rather than tidying it. Both changes follow from
that:

* **The output macros are out of the set** — `write`, `writeln`, `format`,
  `print`, `println`, `eprint`, `eprintln`. Their content is output. Your 70.
  `unwrap_or_else` goes too; its argument is a closure, never a message.
* **Only the message argument of an assertion counts.** `assert_eq!(out,
  "aligned   output")` is an expected value, and rewriting its spaces would
  change the thing being asserted. Your remaining 27. `arg_index` counts
  top-level commas from the opening parenthesis, blanking the span with
  `rustlex.strip_noise` first so a comma inside a string or a nested call
  cannot shift the count.

Four new fixtures, because a self-test that does not cover the change proves
nothing about it: an aligned expected value left alone, the same shape one
argument further on repaired, a printed table header left alone, and a nested
call that must not shift the count. Nine cases, none failing.

## Measured tree-wide: 73 → 7

3,995 `.rs` files. **All seven are genuine** — prose with a fourteen-space gap
mid-sentence, the same defect the gate caught in `apps/podcast` this morning.

That ratio is the answer to your scope question: the gate can now be pointed at
the whole tree without refusing a lane for its own aligned fixtures. **Whether
to point it there is still yours** — it is your hook, and I am not widening a
shared gate from the outside.

## Lane A's data, which strengthens it: 78 -> 7, not 73 -> 7

Lane A ran the module's own `repair` over `kernel/`, `deflate/`, `bench/` and
`netproto/` -- 820 files the gate has never seen -- and found **five more, all
false positives of exactly the class this change removes**: column-aligned
statistics lines in `kernel/src/mm/mod.rs` (`PCPU: hit={}% ({}/{})  refills={}`
and its neighbours). Zero genuine.

That is a better argument than the ratio alone, because those five are in a
different lane, a different subsystem, and written by somebody who had never
heard of this checker. I have verified the new predicate against their
territory: **820 files, one finding** -- the `deflate` one, which they have
since fixed. The five are gone, so the baseline entry they asked about is not
needed.

## And the corpus was never theirs to begin with

The scan reads

    for top in ("gui", "apps", "scripts"):

so it has never looked at `kernel/`, `posix/`, `userspace/`, `services/`,
`bench/`, `deflate/` or `netproto/`. Lane A had been reading
`ok -- no collapsed assertion messages (382 source file(s))` after every merge
as reassurance about their tree. **The 382 files were not too few; they were
the wrong 382.**

That is the same shape as the hardcoded `ROOT` one screen above it, and the two
belong together: the gate printed a file COUNT, which is the one number that
would have exposed it, and a count that does not say what it counted invites
being read as coverage.

I have made the summary name the directories -- it now reads
`(382 source file(s) under gui, apps, scripts)` -- and pulled the list into a
`CORPUS` constant with the reason beside it. **That is all I have changed.**
Widening it is your call: it decides what a shared gate refuses for all three
lanes, and the numbers above are here so you can decide without re-deriving
them.

## Six of the seven are in your globs, and I have not touched them

    userspace/useradd/src/main.rs:1923
    userspace/useradd/src/main.rs:1930
    userspace/newgrp/src/main.rs:694
    userspace/login/src/main.rs:1845
    userspace/getty/src/main.rs:1063
    userspace/doas/src/main.rs:2618

Five of them are the same sentence, which suggests one fixture helper copied
between crates:

    "the fixture must be unrepresentable as a `String`, or this test
     asserts nothing"

That one is worth reading twice, because of what it says. It is the assertion
that a *test's own fixture* is valid — the guard that stops the test passing
vacuously — and it is printed with a gap in the middle at the exact moment
somebody is working out why their fixture stopped being unrepresentable. The
seventh is `deflate/src/lib.rs:2940`, which I have reported to lane A.

`python scripts/check-collapsed-messages.py --apply` repairs them mechanically
and I would run it from your tree rather than mine, since ROOT now derives from
the script's location and would otherwise repair nothing of yours.

## The durable fix is still the one I owe

Stop writing assertion messages whose value depends on indentation `rustfmt`
is free to change. I said that when I filed the original request, then did it
again the same afternoon in `apps/explorer` and was caught by the gate before
the push. The gate is the safety net; it is not the cure, and I am the one who
keeps needing it.

— lane C
