## `A-KSHELL-FIND-SIZE-DEFAULT-UNIT-IS-BYTES-NOT-BLOCKS` (lane A, 2026-08-25) — **open**, and **queued for the operator as `open-questions.md` A-Q1** (2026-08-29)

> **Not a bug for whoever reads this next to go and fix.** It is a user-visible
> compatibility policy with three defensible answers, so it was promoted to the
> operator's decision queue rather than settled inside a lane. The options and
> the recommendation now live in `open-questions.md` → **A-Q1**. What follows is
> the background, kept here because it is what the *implementation* will need
> once the answer arrives.

**In short:** `find . -size 100` means "100 bytes" here and "100 512-byte
blocks" (i.e. up to 51 200 bytes) in GNU `find`. A command line copied from
anywhere else therefore selects a completely different set of files, and says
nothing about it. Nobody has been bitten by this yet because `-size` is young
here, which is exactly why it is worth settling before scripts depend on it.

**Where.** `kernel/src/kshell.rs` — `parse_size_predicate`, the final `else`
arm: `(rest, 1i64) // default: bytes`.

**What GNU does.** A bare number is 512-byte blocks, **rounded up**; `c` is
bytes; `k`, `M`, `G` are the obvious binary multiples; `b` is 512-byte blocks
explicitly; `w` is two-byte words. We implement `c`, `k`/`K`, `M`/`m`, `G`/`g`
and treat a bare number as `c`. We have no `b` and no `w`.

**Why it is not simply a bug.** The GNU default is a genuine historical wart —
it surprises everyone once — and "bytes" is the reading a person actually
expects. But a `find` that quietly disagrees with every other `find` about what
a number means is worse than one that is merely surprising, because the
disagreement is invisible: both produce a plausible list of files.

**Three ways out, and the tradeoff.**

| | *What changes* |
|---|---|
| Match GNU | `find . -size 100` selects files of ~50 KiB rather than 100 bytes. Portable; surprising; silently changes what existing local scripts select. |
| Keep bytes, add `b` and `w` | Nothing changes today; `-size 100b` becomes available for people who want blocks. Still disagrees with GNU on the bare number. |
| Keep bytes, **require** a suffix | `find . -size 100` becomes an error telling you to write `100c` or `100b`. Nobody is ever silently wrong; every existing bare-number use must be edited. |

**Recommendation.** The third. This is the same principle the sweep above
applied everywhere else: where two readings are both plausible and the
difference is invisible in the output, refuse rather than pick. `-size` is new
enough that the cost of requiring a suffix is close to zero.

**If never answered.** Current behaviour is safe in itself — it is documented
in the function's doc comment — and the risk grows only as scripts accumulate
bare-number `-size` uses.
