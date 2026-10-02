## `B-COREUTILS-DIAG-DOES-NOT-FLUSH-STDOUT-FIRST` — a diagnostic can appear above output that was written before it — ✅ FIXED 2026-08-24 (lane B)

*(lane B, 2026-08-24)*

**In short:** when a utility both prints something and complains about
something, the complaint can come out *before* the printed lines instead of
after, if the two are sent to the same place. `nl a nosuch > log 2>&1` shows
the complaint about `nosuch` first and the numbered lines from `a` second; GNU
shows them the other way round, which is the order they actually happened in.
Nothing is lost — every byte still arrives — but a log read top to bottom tells
the story backwards.

### Why

glibc's `error()` — the function behind every GNU diagnostic — calls
`fflush (stdout)` before it writes to stderr, precisely so that the two
interleave in real order when they share a destination.

Our `diag!` cannot do that. Standard output here is a `coreutils::stdfd::Stream`
that lives as a local variable in `run_main`, so the macro has no way to reach
it; it writes to descriptor 2 while the pending output is still sitting in a
buffer that will not be drained until `close_stdout` at exit.

### The fix, which is structural

Make descriptor 1's buffer process-global — stdio's own model: a `Mutex`-guarded
shared buffer, error flag and buffering mode, with `Stream::stdout()` becoming a
handle onto it rather than an owner of it. `diag_line`/`diag_bytes` can then
flush it before writing, exactly as `error()` does.

The one API consequence: `Stream::error()` currently returns
`Option<&io::Error>`, which cannot outlive a lock guard, so it has to return an
owned snapshot instead. Call sites: `src/bin/cat.rs`, `src/bin/head.rs`.

### Reproducing it

```
$ nl f nosuch > log 2>&1     # ours: the complaint is line 1
$ /usr/bin/nl f nosuch > log 2>&1   # GNU: the complaint is last
```

Verified against our own build, not from recall. Affects every utility that can
write output and a diagnostic in the same run — `nl`, `cat`, `head`, `wc`,
`md5sum`, `sort`, and so on.

### How it was fixed

Structurally, as prescribed above: `Inner` — buffer, buffering mode and sticky
error — was split out of `Stream`, and descriptor 1's copy became the
process-global `static STDOUT: Mutex<Inner>`. `Stream::stdout()` is now a
*handle* onto it (`own: None`) rather than an owner; every other descriptor
still owns its own `Inner`. `Stream::error()` returns `Option<io::Error>`, an
owned snapshot, since a borrow could not outlive the lock guard.

**Two things the diagnosis above did not anticipate:**

1. **`diag!` was not the only way to reach descriptor 2.** `tsort` reports its
   loop through a `Stream::stderr()` handed around as `&mut dyn Write`, not
   through `diag!` — so a flush installed only in `diag_to` left
   `tsort cycle >log 2>&1` still printing the loop report above the vertices it
   had already ordered. `nohup`, `env` and `nice` build their messages the same
   way. The rule was therefore attached to the **descriptor** rather than to the
   call: `Inner::put` and `Inner::drain` both begin with `before_diagnostic(fd)`,
   which flushes descriptor 1 whenever `fd == 2`. This is wider than glibc, which
   flushes only inside `error()` — see `design-decisions.md` §378.

2. **`cat` passed the whole time, and that was misleading.** It flushes
   explicitly after each operand for reasons predating this bug, so it was the
   one utility that looked correct while the shared machinery was still broken.
   A harness that had only covered `cat` would have reported green.

### Where it is enforced

`scripts/interleave-diff.sh` — a new cross-utility harness, and deliberately
cross-utility: the per-utility harnesses capture standard output and standard
error into *separate* files, which is the one arrangement in which an ordering
bug cannot be seen. It runs each case twice per side, `>out 2>err` to establish
that the content agrees at all (if it does not, the case is `n/a` — content is
the per-utility harnesses' business) and then `>log 2>&1`, compared byte for
byte. 21 cases over 13 utilities; no accepted divergences.

It was checked for discrimination, not just for green: with the flush disabled
it reports **5 passed / 16 differed**, with it restored **21 / 0**.

As bins convert onto `Stream` they must be added to its `DIFF_BINS`. Everything
still on `std::io::Stdout` is line-buffered unconditionally — even to a file,
where stdio would block-buffer — so it interleaves correctly by accident and has
no ordering to regress.
