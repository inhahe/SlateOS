## 749. The unified destination-open takes what is at the name as one value, not two booleans — and pays a vtable so that `mv` never has to invent a stdout

**Date:** 2026-09-02
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `cp` and `mv` each had their own copy of the code that creates
the file being copied *to*. They were 335 lines of near-duplicate, and stage 3
of the copy-engine extraction merged them into one `copy::open_destination`.
Three small choices inside that merge had a real argument on both sides, and
this records them, because each one is the kind of thing a later reader would
otherwise "simplify" straight back to the rejected option.

> **The title above is now out of date, deliberately.** Choices 1 and 2 were
> superseded on 2026-09-03 — there is no `Clobber` and no vtable any more; see
> the "SUPERSEDED" subsection between choices 2 and 3. The heading is left as
> written because these headings are cross-referenced by number and by name,
> and because the superseding note is only legible next to the reasoning it
> replaced. Choice 3 stands unchanged.

### 1. `Dest::New` / `Dest::Exists(Clobber)`, not two booleans

The merged function needs to know two things: is something already at the
destination name, and — if the create fails — may it unlink and retry? (That
retry is `cp -f`.) The obvious signature takes two `bool`s.

It is the wrong signature, because **only three of the four combinations
exist**. `cp`'s unlink-and-retry is reachable *only* inside the branch that
already established something is there; `mv` always arrives at a name it has
already cleared, so it is always the `New` case and never has a clobber policy
at all. "Nothing is there, but unlink it if the create fails" is not a state
the callers can produce, so with two booleans the function must contain a
line deciding what to do about a case that cannot happen — and that line can
never be tested, because no caller can reach it.

Nesting the policy inside the variant that makes it meaningful deletes the
fourth combination from the type rather than from a comment:

| | two `bool`s | `Dest::Exists(Clobber)` |
|---|---|---|
| states spellable | 4 | 3 |
| unreachable arm to write and never test | yes | none |
| `mv`'s call | `open_destination(…, false, false, …)` | `open_destination(…, Dest::New, …)` |

The cost is that `Clobber` is a second public type for what a reader skimming
the signature might have taken in as a flag. That is the price, and it is
worth it: a positional `false, false` at `mv`'s call site says nothing about
*why* both are false, whereas `Dest::New` says exactly what `mv` knows.

### 2. `&mut dyn Write` for the `-f` verbose message, not a type parameter

`cp -f --verbose` prints `removed 'x'` when it unlinks, so the `Unlink` policy
has to carry somewhere to print. Everywhere else in this crate that shape is a
generic writer parameter (`Run::err`), which is faster and monomorphises away.

Here it is a trait object, and the reason is the opposite of an inconsistency.
A writer type parameter on `Clobber` propagates to `Dest`, and therefore to
**every** construction of `Dest` — including `Dest::New`, which is `mv`'s only
case. `mv` has no stdout in that function to name. It would have to name one
anyway to satisfy the parameter: `io::Sink`, or `io::Stdout` it never writes
to. **A type parameter satisfied by a fiction is worse than a vtable dispatch
taken at most once per operand and only under `-f`** — the fiction is
permanent and misleads every reader of `mv`, while the indirect call is
invisible next to the `unlink` syscall it accompanies.

Rejected alternative: keep the generic and give `Dest` a defaulted type
parameter. Defaults do not apply to a type in argument position, so `mv` would
still have had to write it.

### Choices 1 and 2 are SUPERSEDED (2026-09-03) — by an option neither of them considered

**Decided by:** Claude (autonomous)

`Clobber` is deleted. `Dest` is now two fieldless variants, and
`open_destination` takes `&mut Run<'_, E>`, reading the unlink policy, the
`-v` flag and the stdout from it.

**The reasoning above was not wrong; its premise moved.** Choice 1 asked "two
booleans, or one nested value?" and answered correctly — but both options
assume the clobber policy is an *argument the caller computes*. It never had
to be. `unlink_dest_after_failed_open` was already a field of `Opts`, and
`Opts` is already inside `Run`. The third option is "the policy is an option,
like every other option," and it is better than either:

| | two `bool`s | `Dest::Exists(Clobber)` | read from `Run` |
|---|---|---|---|
| states spellable | 4 | 3 | 2 |
| unreachable arm to write | yes | none | none |
| `mv`'s call | `…, false, false, …` | `…, Dest::New, …` | `…, Dest::New, run, …` |
| callers that can get it wrong | 2 | 2 | 0 |

The last row is the point. Under the old shape both call sites had to
*recompute* the policy from the options — `cp`'s did
`if run.opts.unlink_dest_after_failed_open { Clobber::Unlink { … } } else {
Clobber::Never }`, and `mv`'s passed `Dest::New` with a comment explaining why
that was safe. Two callers deriving the same fact from the same field is two
chances to derive it differently. Choice 1's "fourth combination" argument
survives intact and is in fact strengthened: the impossible state is not
merely unspellable, it is unspeakable, because the policy is not in the type
at all.

Choice 2 evaporates with it. There is no writer on `Dest` to make generic or
`dyn`, so there is no fiction for `mv` to invent and no vtable to justify.
(`Run::out` is independently `&mut dyn Write`, for its own reasons.)

**What actually unblocked this** was stage 4, which put the options and both
streams on one `Run`. Choice 2's whole difficulty — and the note in
`open_destination`'s header that `preserve_xattr` had to be a bare `bool` —
came from `cp`'s options and stdout living on the same `Job`, so a function
asking for both took two mutable borrows of one value. `Run` holds them as two
fields of one struct, which is exactly what dissolves that. The lesson worth
keeping: **when a signature is contorted to avoid a borrow, the fix is usually
to group the things being borrowed, not to weaken the signature.**

A bonus the collapse revealed: `cp`'s call site had been hoisted into a `let`
because `Dest` held a live borrow of `run.out` and a scrutinee's temporaries
live to the end of its `match`. With `Dest` fieldless the call inlines into the
scrutinee — verified by compiling it both ways, not by reasoning, after the
comment asserting otherwise turned out to be false.

Certified as a no-op the same way stage 3 was: `scripts/cp-diff.sh` 581/0/30
and `scripts/mv-diff.sh` 361/0/10, both unchanged.

### 3. `DestError::Dangling` carries the `EEXIST` that revealed it

GNU refuses to write through a destination symlink pointing at nothing, with
its own sentence. `cp` prints that sentence and never reads the underlying
error, so the payload looks dead — and a variant with an unused payload is
exactly what a later cleanup deletes.

It is not dead: `mv` reaches the same variant, and has no sentence of its own.
`mv` reports the underlying `File exists`, which is what its hand-written open
printed before the merge, and byte-identity in that case is the whole
certification of the stage. Synthesising an equivalent error instead would not
do, because a synthesised `io::Error` has **no `errno`**, and the message is
rendered by `strerror` from that number. Dropping the payload would have
changed `mv`'s output while every test stayed green, since the case is only
reachable when another process replaces the destination mid-operation.

The doc comment on the variant says this, so that the next reader who notices
`cp` ignoring the payload finds the reason before deleting it.

### How the whole stage was certified

Not by the test suite — by the two differential harnesses, run immediately
before the change to take a baseline and again after: `cp 581 passed, 0
differed, 30 differ on purpose` and `mv 360/0/11`, identical on both sides. A
pure move that changes those numbers has a bug in the move; that is the rule
`known-issues.md` sets for every stage of this extraction, and it is the only
check that would have caught choice 3 going the other way.
