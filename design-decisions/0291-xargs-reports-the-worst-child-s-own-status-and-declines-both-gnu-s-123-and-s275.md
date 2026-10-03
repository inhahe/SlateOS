## §291 — `xargs` reports the worst child's own status, and declines both GNU's 123 and §275's third value

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `xargs` runs one command many times, and has exactly one number
to say how it went. It was handing that number to whichever run happened to
finish last, so a batch where an early run failed and a late one succeeded
reported success. Fixing it needs a choice about *which* number a failure
should be. GNU picks 123 — a magic value meaning "some child failed" — and this
shell now picks the failing child's own number instead. If you run
`ls | xargs grep pattern` and grep finds nothing, you get grep's answer (1),
not 123.

### The three candidates

| Option | *What changes* |
|---|---|
| GNU's 123 | any child failure reports 123, whatever the child said |
| **The worst child's own status** ✓ | `cmd \| xargs foo` reports what `foo` reported |
| A reserved value per §275 | a third number distinguishing "xargs failed" from "a child failed" |

**Why not 123.** GNU does not use 123 because 123 is meaningful; it uses it
because it needs 124–127 free for *its* other conditions — child exited 255,
child killed by a signal, command not executable, command not found. This shell
implements none of those rows: an unknown command is a flat `1` here
(`kshell.rs`, the `Unknown command: '…'` arm). Adopting one row of a table
whose other rows we cannot produce leaves a caller unable to tell which
convention it is reading — which is the same argument that kept the
`Syntax error: …` sites at 1 rather than borrowing bash's 2, recorded under the
`grep`/`cmp`/`diff` reversal in `known-issues.md`.

**Why not a reserved third value.** §275 established that a command whose "no"
can mean either *I checked and the answer is no* or *I could not check* needs a
third status, and gave the trigger question in as many words. Asked of `xargs`,
the answer is no: its non-zero status is not an assertion about anything. It
means "something in the batch went wrong", and a script writing
`xargs foo || handle` handles a bad `-n` and a failing child identically.
`grep`'s 1 is different in kind — it is a positive claim about a file's
contents, which a failed open would falsify. So §275's template does not
extend here, and this entry is a datapoint for the "candidates not yet
converted" list at the end of it: *considered and declined*.

**Why `max` is the right operator and not merely a convenient one.** Under §275
a larger status is a worse one — 2 outranks 1 in `grep`, `cmp` and `diff`,
because a failure to look outranks an empty result. Taking the maximum across
invocations therefore propagates that precedence outward from a single command
to a batch of them, for free and without `xargs` knowing what any particular
number means.

**The cost, stated plainly.** A script that checks `[ $? -eq 123 ]` because it
was written against GNU will not fire here. That script does not work today
either — we have never emitted 123 — so nothing regresses; what is given up is
the option of becoming compatible with it later without a second change. The
compensating gain is that `xargs` stops being opaque: for the
single-invocation case, which is the common one, it now costs nothing at all to
put `xargs` in front of a command.

**Not decided here:** whether empty input should run the command once. GNU does
(absent `-r`), BSD does not, we follow BSD. That is a genuine fork between two
real implementations rather than a bug, and is recorded as such under
`TD-A-XARGS-…` in `known-issues.md`.
