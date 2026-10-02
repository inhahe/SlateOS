## `B-COREUTILS-WRITE-ERROR-OMITS-THE-ERRNO-ON-A-PARTIAL-FLUSH` — an accepted divergence, not a bug to fix

*(lane B, 2026-08-24 — measured against GNU coreutils 9.4 / glibc 2.39 in WSL)*

**In short:** when a utility's output cannot be written because the disk is
full, GNU sometimes prints `wc: write error` with no explanation attached, and
we always print `wc: write error: No space left on device`. Ours says more.
The reason GNU says less is an accident inside the C library it is built on,
not something it chose, so we are not copying it. Nothing is broken either way
— this is written down so that the next person to compare the two does not
"fix" ours into being less informative.

### What is actually different

Every write failure carries a reason from the operating system (an *errno* —
the number the kernel returns, rendered as a sentence like `No space left on
device`). GNU prints it in most cases and drops it in some:

| invocation | GNU 9.4 | ours |
|---|---|---|
| `wc f >&-` | `wc: write error: Bad file descriptor` | identical |
| `head f >/dev/full` | `head: write error: No space left on device` | identical |
| `wc f >/dev/full` | `wc: write error` | `wc: write error: No space left on device` |
| `nl huge nosuch >/dev/full` | `nl: write error` | `nl: write error: No space left on device` |

The exit status is the same in all four. Only the sentence differs, and only
on `/dev/full`.

### Why GNU drops it

It is a two-step accident:

1. glibc's `new_do_write` resets the stream's write pointers whether or not
   the underlying `write(2)` succeeded. So a flush that failed still leaves the
   buffer *empty*.
2. gnulib's `close_stream` therefore finds nothing left to write, its `fclose`
   succeeds, and it sets `errno = 0` before reporting — because `ferror` was
   already set, so it knows the failure happened earlier and no longer has the
   number that described it.

Which is why the divergence tracks *whether a flush already failed*, not which
utility is running: `head f >/dev/full` (short output, one flush, at exit)
carries the errno and `head -n 200000 huge >/dev/full` (a flush mid-run) does
not. The utilities that show it most often — `wc`, `du`, `md5sum`, `cksum`,
`sum` — are simply the ones that flush per record.

On a *closed* descriptor (`>&-`) GNU prints the errno every time, because
nothing was ever written and so nothing was ever discarded. That is the case
the whole closed-descriptor sweep is about, and there we match exactly.

### Why we are not emulating it

Emulating it means deliberately throwing away the one piece of information the
message exists to convey, in exactly the situation where the user most needs it
— they are looking at a failed write and want to know whether the disk is full,
the file is on a dead NFS mount, or the descriptor was closed. Reproducing it
would also mean reproducing glibc's buffer-discard rule inside
`coreutils::stdfd::Stream`, i.e. carrying a bug-for-bug quirk of a C library
this OS does not use, in the one module every utility depends on.

### Where it is enforced

`scripts/write-error-diff.sh` accepts "ours is GNU's message with a `: <errno>`
suffix" as an expected difference **and nothing else** — a message that goes
missing, changes shape, or comes with a different exit status is still a
failure. The exemption is recognised by shape rather than kept as a list of
cases, because the same utility diverges or not depending on the invocation.
