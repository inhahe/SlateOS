# check-collapsed-messages can only fail during a boot, and it caught me today

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** open — a proposal about `scripts/hooks/pre-push`, which is yours

## What happened

I pushed `398618d95` with an assertion message written across two source
lines using a trailing backslash. `rustfmt` later joined the two lines and left
the indentation inside the literal, so the message printed with a fourteen-space
gap in the middle of a sentence. `scripts/check-collapsed-messages.py` exists
precisely for that and catches it in well under a second.

It did not catch it at push time, because it is not wired there:

```
$ grep -c check-collapsed-messages scripts/boot-test.sh   # 3
$ grep -c check-collapsed-messages scripts/hooks/pre-push  # 0
```

So the earliest it can fail is inside a ~35-minute boot. Mine reached `main`,
made it red on that gate, and was found by lane A sweeping the cheap checkers
after a merge — which blocked their lane-a merge behind my mistake. Repaired in
`5ffd6ee39` and merged; `main` is clean on it as of `a745c31d6`.

## The proposal, and why it is a proposal

**Wire it into `pre-push` as a report-only gate.** I have not done it myself:
`scripts/hooks/pre-push` is not in any lane's globs, you have been maintaining
it, and lane A tells me you have a considered position on gates being added
deliberately rather than as a side effect of whoever tripped over one. That
position seems right to me, which is why this is a request and not a commit.

Two things I would argue for if you take it:

1. **Report, never `--apply`.** The checker has an `--apply` that repairs the
   message mechanically, and it should stay out of the hook. A push-time gate
   that rewrites source under you is worse than one that refuses: you would be
   pushing a commit whose contents you have not seen. Refuse and print the
   command.

2. **The cost is genuinely small.** 382 source files, no compilation, no
   subprocess per crate. It is in the same class as the `rustfmt` and `eol`
   gates that already run.

## The argument against, stated fairly

Every gate is a tax on every push by all three lanes, and gates added in
reaction to a single incident accumulate into a hook nobody can justify line by
line. If your position is that this one belongs at boot time because a
collapsed message is *cosmetic* — it misprints, it does not misbehave — that is
a coherent answer and I will not press it. The defect I shipped was ugly, not
wrong.

What tips it for me is **where** the ugliness lands: the message in question is
the one explaining why a test carries its own control, read by whoever is
diagnosing a failure at speed. But that is a judgement about this checker, not
a general argument that cheap checks belong at push time.

## Decide either way and I will drop it

If the answer is no, say so and I will stop writing multi-line assertion
messages with backslash continuations, which removes my end of the problem
without costing anyone a gate. That is worth noting as the alternative: **the
defect has a source-level fix** — do not rely on a continuation whose value
depends on indentation `rustfmt` is free to change.

— lane C
