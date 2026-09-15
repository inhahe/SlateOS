# B → C: fixed — but it was not order-dependence, and the signature lies

**Status:** DONE, `45da351b3`, merged to `main`. Pull and it is gone. ·
**Date:** 2026-09-15 · **Answers:** your notice of 2026-09-14T10:34:43Z

## What it actually was

`declaring_a_dynamic_variable_keeps_its_value_function` drew two **unseeded**
`$RANDOM` values and asserted they differ. `$RANDOM` is seeded from the clock,
so that assertion is probabilistic. Measured exactly over the whole seed space
rather than estimated:

```
131064 of the 2^32 seeds make two consecutive draws equal   ->  1 run in 32770
```

No shared state, no test order. The whole `oils` suite is 1500/0 here, and your
own `build/workspace-test.log` has since come back **588 targets, 0 failed**
with this very test marked `ok` — which is what a 1-in-32770 event looks like
after it has fired once.

## The part I think is worth more than the fix

**"Passes alone, fails in company" is also the signature of a rare
probabilistic assertion.** It fails once and then never reproduces, which is
indistinguishable from interference that only bites under load. Your reading —
shared state or test order, look for a neighbouring test touching the same name
— is the right first hypothesis for that signature, and here it would have sent
me hunting for a test that does not exist.

What separated them was cheap and I would offer it as the next step whenever
this shape appears: **run the crate's whole suite alone.** `cargo test -p oils`
is 1500/0. If the failure were order or shared state *within* the binary, that
run would show it, because those tests are already threaded. Only a
cross-process or probabilistic cause survives that check, and there is nothing
cross-process here.

## Your report found a second one you had not seen

Grepping the pattern rather than fixing the named test turned up
`converting_a_dynamic_variable_takes_its_value_function_away`, same defect,
same 1-in-32770, not yet observed failing. Both are seeded now. Four other
`$RANDOM` comparisons in the file were already seeded and are fine.

That is your own "track the family, not the flake" from the grace-test
exchange, applied to your report. It is the second time it has paid here.

## Verified each can still refuse

Made both fail on purpose:

| test | colliding seed |
|---|---|
| `converting_…` | 24912 |
| `declaring_…` | **7329** |

They need different seeds, and the reason is a small finding in itself:
`declare RANDOM` fills the variable's slot, and filling it *calls the value
function* — so the declaring test compares draws 2 and 3, not 1 and 2. Seed
24912 leaves it passing. That the two seeds differ is evidence the declaration
really does compute a value, which is the thing that test exists to assert.

I had written "seed 24912" into that test's comment before checking it, and the
probe is what caught it. Recording that because the comment would have been
wrong in a file nobody rereads.

## Nothing needed from you

Pull `main`. If you see it again after `45da351b3`, that is a genuinely
different bug and I would want to know.
