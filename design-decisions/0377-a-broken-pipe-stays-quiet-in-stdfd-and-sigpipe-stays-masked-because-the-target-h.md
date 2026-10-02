## §377 — A broken pipe stays quiet in `stdfd`, and `SIGPIPE` stays masked, because the target has no signal to restore

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)

**In short:** When you run `seq 1 100000 | head -1`, `head` stops reading after
one line and the pipe it was reading from breaks. On Linux, GNU's `seq` is
then killed outright by a signal called `SIGPIPE`, prints nothing, and the
shell records exit status 141. SlateOS deliberately has no signals at all, so
that death has no equivalent here — our `seq` instead gets an ordinary "broken
pipe" error back from its write. The choice was whether to switch the signal
back on for the Linux builds so we match GNU exactly, or to keep answering the
error quietly. We kept it quiet, and moved the rule into one shared place
(`coreutils::stdfd::reader_gone`) so that the ~80 utilities being converted
right now all inherit it instead of each re-deriving it.

### What forced the question now

`coreutils::stdfd::write_error` prints gnulib's line verbatim:

```text
seq: write error: Bad file descriptor
```

That is right for a closed descriptor and right for a full disk. It is *wrong*
for a broken pipe, where GNU prints nothing at all. Measured against the real
binaries inside WSL:

```text
seq | head (stderr)                []
cat->fifo-closed status = 141
status=141 stderr=[]
```

Every binary converted in the `stdfd` sweep routes its final flush through
`write_error`, so without a carve-out the sweep would install
`prog: write error: Broken pipe` on ~80 utilities that are silent today.

### Options

| | approach | *What changes* |
|---|---|---|
| **A** | Restore `SIGPIPE` to `SIG_DFL` in `stdfd::restore()` | `seq \| head -1` exits 141 and prints nothing, exactly as GNU does |
| **B** *(chosen)* | Recognise `EPIPE` and stay quiet, keeping the run's earned status | `seq \| head -1` exits 0 and prints nothing |
| **C** | Print the diagnostic and let it differ | `seq \| head -1` prints `seq: write error: Broken pipe` |

### Why B

C is not a real option; it is the bug this entry exists to prevent.

A is tempting because it is *upstream's own arrangement* and it deletes code:
Rust masks `SIGPIPE` before `main`, unmasking it is three lines, and the
fourteen files that currently hand-roll an `ErrorKind::BrokenPipe` branch
(`cmp.rs`, `cut.rs`, `env.rs`, `head.rs`, `hostname.rs`, `kill.rs`, `sed.rs`,
`sort/main.rs`, `stat.rs`, `tail.rs`, `tar.rs`, `tee.rs`, `uname.rs`,
`uniq.rs`) would all lose that branch and get status 141 for free.

It was rejected because it is **correct only on the host we test on**.
`design.txt` says plainly that SlateOS does not use Unix signals for process
control; there is no `SIGPIPE` on the target to restore. A restored signal
would make the Linux differential harnesses greener while making the *shipped*
binaries — the ones that run on SlateOS, where the write simply returns
`EPIPE` — behave in a way no test exercises. That is the exact failure mode
§374 was written to avoid, pointed the other way: a harness measuring a
configuration the product is never in.

Note also that A only *looks* like it deletes the fourteen branches. `tee`'s
`--output-error` modes and `cmp`, `sed` and `tar` all need the `EPIPE` value
in hand to decide what to do next; under A each would have to re-mask the
signal around its own writes, which is more machinery than the branch it
replaced, not less.

So the divergence is accepted and named. `stdfd::reader_gone` is the predicate,
and its doc comment carries the reasoning so the next converter does not have
to find this entry.

### The shape of the shared tail

Alongside it, `stdfd::close_stdout(program, out, earned)` is gnulib's
`atexit (close_stdout)` written as a return value: flush, return `earned` if
the flush arrived *or* if nobody was left to read it, otherwise print
`prog: write error: …` and return 1. `earned` is discarded on a real failure
because upstream's handler genuinely does override whatever status the run had
reached.

An earlier pass through this sweep rejected such a helper on the grounds that
it would hide the per-program failure status. That objection is answered by
carving the exceptions out explicitly rather than by refusing the helper:
`env` exits 125 on a write failure, `ls` and `sort` exit 2, `tty` exits 3, and
those four spell their tails out with `reader_gone` and `write_error`
directly. So do the utilities that must know the flush's verdict *before* they
finish deciding what to print — `printf`, whose write error outranks both a
`%z` diagnostic and a `\c` that asks for status 0, and `comm`/`join`/`tsort`,
which report a disordered input only if the output survived.

### The cost

Two ways to spell one thing, which is the usual price of a helper with
exceptions. It is bounded by the doc comment on `close_stdout` naming exactly
which utilities may not use it, and by the fact that the exceptions are
seven known binaries out of roughly eighty rather than an open-ended judgement
call at each site.
