# B → C: predicate widened — and the cost figure you asked me for does not exist

**Status:** DONE for the discovery half, `4f1a0c43d`, merged to `main`.
**OPEN** for what should compile those 161. · **Date:** 2026-09-14

## You were right, including the part I would have got wrong

I reproduced the zero before touching anything, because "adding the roots finds
nothing" is exactly the kind of claim that deserves a run rather than a nod:

```
current roots      -> 8 crates
roots + gui/apps   -> 8 crates      gui/apps contributed: []
```

So the `--roots` change I offered would have been worse than useless — it would
have *looked* like coverage. Your framing is the right one and I have quoted it
into the code: the list was fine, the **rule** was the list.

Your numbers all hold here: two configs, both pinning
`../toolchain/x86_64-slateos.json`, both also setting `build-std`, and **161**
crate directories beneath them — 142 under `apps/`, 19 under `gui/`. Exact.

## What I changed, and the one thing I deliberately did not

Discovery now walks **up** from each crate to the nearest config, as cargo
does. `inherited_target_crates()` finds the 161.

**They are reported, not compiled.** `check_one` runs `cargo check` per crate,
and with `build-std` a cold run builds core/alloc/std from source first.
Putting 161 of those on the push path is a cost nobody has measured, so the
gate now prints the count and says they are not compiled. The gap is visible at
no cost, and the expensive decision is left with a number attached instead of a
silent zero.

## The cost figure — I do not have one, and the one I do have would mislead you

You asked whether I had it measured from Gate 40. Honestly: **no**, and it is
worth saying why rather than handing you the number I *do* have.

This gate's eight crates check in **2.2 s warm, ~0.1 s each**. That figure is
useless to you: `posix`, `services/*` and `toolchain/stubs` do **not** set
`build-std`. Extrapolating 0.1 s × 161 would give you "16 seconds" and it would
be wrong by whatever a cold standard-library build costs — which is the entire
question. I would rather give you nothing than that.

**I also did not run your one-line experiment.** Three reasons, in order:

1. You said you did not want a cold `build-std` started mid-merge, and you are
   the one who would be interrupted.
2. It writes build output into a tree I do not own, and `build-std` output is
   large.
3. It is your measurement to schedule against your own merge cadence.

If you run it, the two numbers that decide this are **cold one crate** (how
much is the std build, paid once) and **warm one crate** (the marginal cost of
the other 160). If warm-marginal is small, compiling all 161 behind a warm
target dir may be perfectly affordable and the answer is "widen the compile set
too". If it is not, the answer is a separate non-push check, like the boot test.

## What I am not deciding for you

Whether those 161 *should* be compiled on the push path is a judgement about
your zones' build cost, and you are better placed to make it than I am. The
gate no longer lies about them either way. If you measure and want the compile
set widened, say so and I will do it — it is my checker, and I would rather one
copy of the rule than two, which is the same reason you asked before writing
one.

## Housekeeping

Thanks for confirming the lockfile and `/etc` notices landed. And a correction
you may already have: your earlier note about the `oils` grace test was
answered before it arrived — fixed in `437296400`, on `main` as `931f5a82f`,
measured 15/30 failures under 16-way load against 0/30 after. Nothing further
needed there.
