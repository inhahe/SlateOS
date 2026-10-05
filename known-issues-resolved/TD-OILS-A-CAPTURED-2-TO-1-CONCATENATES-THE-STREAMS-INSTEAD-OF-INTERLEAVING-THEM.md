### TD-OILS-A-CAPTURED-2-TO-1-CONCATENATES-THE-STREAMS-INSTEAD-OF-INTERLEAVING-THEM — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::run_external`: the
`redir.stderr_to_stdout` arm of the stderr-routing block, its
`StderrTarget::Buffer` arm, and the `capturing` drain that follows the spawn.
The fix's shared half is the new free function `merge_child_streams`.

**What.** An **external** command's `2>&1` inside a command substitution came
back in the wrong order, and with a `>` after it came back not at all:

```text
o=$( "$SH" -c 'echo A >&2; echo B; echo C >&2' 2>&1 )
      bash: A B C          osh: B A C

o=$( { "$SH" -c '…'; } 2>&1 )
      bash: A B C          osh: A C B

o=$( "$SH" -c '…' 2>&1 >f )
      bash: captures A C, f holds B
      osh:  captures nothing,  f holds B
```

Builtins were unaffected (`$( { echo A >&2; echo B; } 2>&1 )` was already
right), as was a merge that went through a real pipe
(`$( … 2>&1 | cat )`) — which is what made it easy to miss.

**Why.** bash's `2>&1` is a `dup2`: fd 1 and fd 2 become **one** open file
description, and the kernel interleaves whatever the child writes down it.
osh handed the child *two* pipes and reassembled them afterwards —
`read_to_end` on stdout, then `read_to_end` on stderr, appended. Concatenation
is not interleaving, so every fd 1 byte sorted ahead of every fd 2 byte.

Two arms did it, and they read differently from outside. A `2>&1` written on
the command itself (`redir.stderr_to_stdout`) gave `B A C` — all of fd 1, then
all of fd 2. A `2>&1` written on an enclosing **compound** — `{ … } 2>&1`,
`f 2>&1`, `( … ) 2>&1` — arrives instead as a scoped `StderrTarget::Buffer`,
and gave `A C B`, because the child's stdout was appended to a capture buffer
the child's *stderr* had already been drained into. Same cause, two spellings;
the second is the shape most scripts actually write, and is what a shell
function wrapping an external command hits.

The dropped-output case was the same code from the other side. `capturing` is
false when a `>` redirects fd 1 away, so `cmd 2>&1 >f` piped the child's fd 2
and then never drained it: the bytes bash sends to the *substitution* were
read by nobody.

**Fixed by** giving the child one description, as bash does.
`merge_child_streams` makes a single `io::pipe()` and hands a clone of the
write end to each of fd 1 and fd 2, so the interleaving is the kernel's rather
than a reassembly; both arms call it, on the same condition — fd 1 is still
the capture (`capturing`) *and* the sink fd 2 was aliased to is that same
capture. The parent's two copies of the write end are released right after the
spawn, or the drain would wait on an EOF it is itself holding back. The single
read end is also what makes the drain deadlock-proof: with two pipes, whichever
is read second can fill while the first is still being drained.

Where fd 1 is *not* the capture — a `>` written after the `2>&1`, or a sink
reached via `exec 1>&N` onto an *outer* capture — fd 2 is alone on it again,
which is the ordinary stderr-only pipe, already drained before the wait.

**Pinned by**
`tests/corpus/a-captured-2-to-1-interleaves-the-two-streams.sh`.

**Found by** `a-syntax-error-abandons-the-rest-of-the-input-on-every-reader.sh`,
which re-invokes the shell under test and so tripped over this while pinning
something else.
