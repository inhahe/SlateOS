### [B] TD-B-LS-WRITES-A-DIAGNOSTIC-WITHOUT-FLUSHING-THE-LISTING-FIRST — 2026-08-22 — FIXED 2026-08-22
**Status:** FIXED 2026-08-22

**What it is.** `ls` accumulates its whole listing in `Out::buf` and writes it
once, at the end of `main`. Diagnostics go to stderr the moment they happen. So
when both streams land in the same place — `ls -R t 2>&1 | …`, or a terminal —
every diagnostic appears *before* the entire listing instead of at the point in
it where the failure occurred.

GNU does not do this: gnulib's `error()` calls `fflush (stdout)` before it
writes, so a message lands between the lines already printed and the ones still
to come.

**How to see it.** With an unreadable subdirectory `t/noperm`:

```text
$ ls -R t 2>&1
GNU:   …23 lines of t's listing…  ls: cannot open directory 't/noperm': …
ours:  ls: cannot open directory 't/noperm': …  …23 lines of t's listing…
```

It is the one remaining non-deliberate failure in `scripts/ls-diff.sh`.

**What the proper fix looks like.** `Out` gains a `flushed: usize` (bytes
already written) and a sink, `Out::mark` uses `flushed + buf.len()` so
`--dired`'s offsets are unaffected, and every write to stderr flushes first.
The four diagnostic sites are `Listing::file_failure`, the
`not listing already-listed directory` branch in `Listing::print_dir`, and
`gobble_file`'s two — the last needs the sink plumbed in beside its existing
`err`, which is the only awkward part.

Flushing *more* often than GNU is unobservable (the bytes and their order are
identical), so the fix does not have to match gnulib's flush points — only to
guarantee that no stderr write happens while stdout has unflushed bytes.

**Fixed 2026-08-22**, exactly as described above. `Out` became `Out<'a>` with a
`sink: Option<&'a mut dyn Write>`, a `flushed: usize` and a `broken: bool`;
`Out::mark` counts from `flushed + buf.len()`; `Out::flush` writes the buffer
and clears it. `gobble_file` took an `out` parameter beside `err`, and all four
diagnostic sites flush first. `main` holds the stdout lock, flushes once at the
end, and exits 2 if the sink ever refused a write — a listing that could not be
written is an exit status, because the only place to report it is the stream
that just failed.

Two tests cover it:
`a_diagnostic_lands_where_it_happened_and_not_in_front_of_the_listing` points
both streams at one buffer and asserts the byte-for-byte interleaving, and
`flushing_does_not_move_the_dired_offsets` asserts a flush between two `mark`s
leaves `--dired`'s offsets where they were. `scripts/ls-diff.sh`'s `ls -R t`
case dropped its `!` deferral and now passes: 159 cases, 148 passed, 0 differed.

One measurement corrected while writing the test: there is **no blank line**
before the diagnostic. `print_dir` emits the separating newline only in front
of a heading it is about to print, and `t/noperm` never gets one, because the
`opendir` that would have led to that heading is the thing that failed. GNU
9.5, `ls -R t 2>&1 | cat -A`, prints `t:$ a$ noperm$ z$` then the message. The
exit status is **1**, not 2: `t/noperm` was reached by recursing rather than
named on the command line, and GNU reserves 2 for the latter.
