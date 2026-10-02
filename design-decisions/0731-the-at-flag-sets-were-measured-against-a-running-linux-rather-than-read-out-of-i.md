## 731. The `*at` flag sets were measured against a running Linux rather than read out of its source, and the two surprises that produced are kept rather than tidied

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous) · **Lane:** B

**In short:** three of our nine `*at` system calls — `unlinkat`, `fstatat`
and `statx` — accepted any bit pattern at all in their `flags` argument and
quietly ignored the bits they did not recognise, so a program passing a flag
we have not implemented got the *unflagged* behaviour instead of an error
saying so. They now reject unknown bits with `EINVAL`, like the other six
already did. Which bits count as "known" was determined by *running* the
calls on Linux 6.6 and recording what it did, not by reading Linux's source
— and that turned up two behaviours that a source reading would probably
have smoothed over, both of which we deliberately copy.

### The problem

Six of our `*at` calls validated their flags. `unlinkat`, `fstatat` and
`statx` did not: they tested the bits they cared about and dropped the rest
on the floor. That is the worst of the three possible behaviours. Rejecting
an unimplemented flag tells the caller. Implementing it serves the caller.
*Ignoring* it does neither, and does so invisibly: a program that asks for
`AT_SYMLINK_NOFOLLOW` and is silently given follow-the-link semantics gets a
wrong answer that looks like a right one, and the bug surfaces somewhere
else entirely.

### Why measurement rather than reading

The flag sets are small enough that reading `fs/stat.c` would have been
quicker. Two purpose-written C programs were used instead, because the
question is not "what does the kernel source say" but "what will a program
ported to us have observed on Linux", and those differ wherever the source
is subtle. Both surprises below are exactly that case.

**Surprise 1: `unlinkat` accepts only `AT_REMOVEDIR` — not even
`AT_SYMLINK_NOFOLLOW`.** The natural guess is that `unlinkat` accepts the
no-follow flag, since `unlink` must not follow the final symlink and
`AT_SYMLINK_NOFOLLOW` is how every other call in the family says so. It does
not: `unlink` implies no-follow *unconditionally*, so the flag would be
redundant, and Linux rejects it with `EINVAL` rather than accepting a
no-op. Passing it is a sign the caller has confused `unlinkat` with
`fstatat`, and Linux says so.

**Surprise 2: `newfstatat` accepts `AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC`
together; `statx` refuses that same pair.** "Force a sync" and "do not sync"
are contradictory, and `statx` returns `EINVAL` for the combination. The
older `newfstatat`, which reaches the same code, does not check — it ignores
both bits entirely, so there is nothing to contradict. We reproduce the
asymmetry rather than making the two consistent.

This is the entry's real purpose: the asymmetry looks exactly like an
oversight in our code, and a future reader tidying it in either direction
would break the compatibility it exists to provide. It is measured, it is
intentional, and there are tests pinning both halves.

### Ordering was measured too

Which error wins when a call has several things wrong with it is itself
observable, and a program that tests for `EFAULT` will not see it if we
return `EINVAL` first. For all three calls the flag gate outranks `EFAULT`,
`ENOENT` and `EBADF`; within `statx`, the reserved-mask check outranks the
NULL-buffer check. The tests assert the ordering, not merely that some error
occurs.

### The alternative that was rejected

Accepting unknown bits and ignoring them is what we did before, and it has
one genuine argument for it: it is forward-compatible, in that a program
compiled against a newer header passing a newer flag keeps working rather
than failing outright. That argument is wrong here for the same reason it is
wrong on Linux. The program passing the new flag is asking for behaviour we
do not have; "keeps working" means "silently gets different semantics than
it asked for". `EINVAL` is the diagnosis, and it is what a program that
checks for it — which is how portable code probes for a flag's availability
— is written to expect.

**Where this lives:** `posix/src/file.rs` — the flag gates at the top of
`unlinkat`, `fstatat` and `statx`, the `AT_NO_AUTOMOUNT` /
`AT_STATX_FORCE_SYNC` / `AT_STATX_DONT_SYNC` / `AT_STATX_SYNC_TYPE` and
`STATX_RESERVED` constants, and the `at_flag_validation` test module with its
three `const` tables of accepted and rejected bits.
