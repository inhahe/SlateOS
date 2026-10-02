## §378 — Any write to descriptor 2 flushes descriptor 1 first, which is wider than glibc's rule

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)

**In short:** When a program prints some output and also complains about
something, and both are sent to the same file (`nl a nosuch > log 2>&1`), the
complaint should land *after* the lines that were printed before it — the order
things actually happened in. Getting that right means emptying the output
buffer before writing the complaint. glibc does this in exactly one place: the
`error()` function that every GNU diagnostic goes through. We do it in a wider
place — *every* write to descriptor 2, whoever makes it. The wider rule costs a
flush that is occasionally unnecessary; it buys immunity to the failure that
actually bit us, which was a diagnostic that did not go through the diagnostic
function.

### What forced the question

The narrow rule was implemented first, and was wrong. `stdfd::diag_to` — the
one funnel `diag!` writes through — was given a `flush_stdout()` at its head,
exactly mirroring `error()`. `scripts/interleave-diff.sh` then found `tsort`
still misordered:

```text
$ tsort cycle > log 2>&1
GNU:   d, e, tsort: -: input contains a loop:, tsort: a, tsort: b
ours:  tsort: -: input contains a loop:, tsort: a, tsort: b, d, e
```

`tsort` reports its loop through a `Stream::stderr()` passed around as
`&mut dyn Write`, not through `diag!`, so the funnel was never entered.
`nohup`, `env` and `nice` build their messages the same way. The set of ways to
reach descriptor 2 is not "calls to `diag!`", and treating it as though it were
is a bug that reappears every time someone writes a diagnostic in a new shape.

### Options

**(a) Keep the flush in `diag_to`, and add one to every other path.** Matches
glibc's placement exactly. Requires an audit of every current writer to
descriptor 2, and requires the next person who adds one to know about the rule
— which is precisely the knowledge the first version of this fix assumed and
did not have. *What changes:* nothing, until someone adds a diagnostic and
forgets.

**(b) Attach the rule to the descriptor.** `Inner::put` and `Inner::drain`
begin with `before_diagnostic(fd)`, which flushes descriptor 1 when `fd == 2`.
Any writer to descriptor 2, through any API, present or future, inherits it.
*What changes:* a write to descriptor 2 does an extra `write` syscall on
descriptor 1 whenever descriptor 1 has bytes pending.

We took (b).

### Why the wider rule is not merely acceptable but preferable

The narrow rule's only advantage is avoiding a flush that no caller wanted. But
**there is no call site in this family that wants an unflushed standard
output.** These are utilities whose entire descriptor-2 traffic is diagnostics:
every one of them wants ordering. A conditional would be a switch with all its
positions wired the same way, plus a decision for the next author to get wrong.

The cost is bounded and small. It is one extra `write` per diagnostic, and only
when descriptor 1 actually has pending bytes — a program that prints nothing,
or that is line-buffered onto a terminal, pays nothing. Diagnostics are not a
hot path; a utility that emits enough of them for a syscall apiece to matter has
a worse problem than this flush.

It cannot deadlock. `before_diagnostic` takes descriptor 1's lock and
descriptor 2's state is either a distinct `Inner` (owned by the `Stream`) or
untouched, so there is no path that takes the two in the opposite order, and no
path that re-enters the descriptor-1 lock while holding it.

### The cost, stated plainly

We diverge from glibc's *structure* in order to match its *behaviour* more
robustly. Someone reading `error()` and then reading `stdfd` will find the
flush in a different place than they expect. That is answered by the doc
comment on `before_diagnostic`, which says so in as many words, and by
`scripts/interleave-diff.sh`, which fails if the behaviour ever stops matching.
