## TD-B-EVERY-BINARY-IS-WRONG-ABOUT-A-CLOSED-STANDARD-DESCRIPTOR — `coreutils::stdfd` exists now; the sweep does not (lane B, 2026-08-24)

**Status: mechanism FIXED and shared; two binaries converted; the rest
unaudited.**

### The two lies

Rust's runtime tells every binary in this crate the same two untruths about
descriptors 0, 1 and 2, and both of them push the program toward reporting
success:

1. **`sanitize_standard_fds`** runs before `main` and reopens any *closed*
   standard descriptor on `/dev/null`. By the time `main` sees the world,
   `prog >&-` and `prog >/dev/null` are indistinguishable.
2. **`handle_ebadf`** in `StdoutRaw`/`StderrRaw`'s `Write` impls turns `EBADF`
   into `Ok(buf.len())`. Even with (1) defeated, a write to a closed
   descriptor is reported as a write that fully succeeded.

Together: `prog >&-` writes nothing, notices nothing and exits 0, where every
GNU utility prints `prog: write error: Bad file descriptor` and exits nonzero.
This is not a corner — gnulib registers `close_stdout` with `atexit` in
essentially every one of its programs precisely because a report that did not
arrive is a failure, and a pipeline that silently discards output is the thing
that rule exists to prevent.

### The answers, now in the library

`userspace/coreutils/src/stdfd.rs`:

- **`guard_std_fds!()`** — a `macro_rules!` the *binary* expands, installing an
  `.init_array` ELF constructor that records which of 0/1/2 were closed. It
  has to be a macro rather than a plain library item: an `.init_array` entry
  inside an rlib is not reliably pulled into the link, because inclusion is by
  object file and rustc's codegen-unit partitioning is not a stable interface.
  The constructor is the one window in which the truth is still available —
  it runs from `__libc_start_main`, before `lang_start` calls
  `sanitize_standard_fds`.
- **`stdfd::restore()`** — re-closes, at the top of `main`, whatever the
  constructor saw closed.
- **`stdfd::Stream`** — a buffered writer over a raw descriptor that calls
  `write(2)` directly, so `EBADF` arrives as `EBADF`. It reproduces gnulib's
  `close_stream` rule exactly, which is subtler than "did any write fail":
  failure is `prev_fail || (fclose_fail && (some_pending || errno != EBADF))`.
  That is why `nice true >&-` is 0 (nothing was owed) while `nice >&-` is 125
  (a number was owed), and both were measured before being coded.

### What is left

`nohup` and `nice` are converted and covered by their harnesses. **Every other
binary in the crate is still wrong by default**, in the silent direction.
The remaining work is a bin-by-bin sweep: for each, decide whether its exit
status or behaviour depends on a standard descriptor being open — for anything
that writes to stdout, it does — and if so expand `guard_std_fds!()`, call
`stdfd::restore()` first thing in `main`, route output through
`stdfd::Stream`, and add `prog >&-` / `prog >/dev/full` cases to its harness.
`cat`, `echo`, `yes`, `printf`, `seq`, `head`, `tail`, `wc`, `sort` and the
`sum`/`digest` family are the ones a pipeline is most likely to notice.

Recorded in `todo.txt` as well.

### A general defect it exposed on the way

`errmsg::strerror` fell back to `io::Error`'s `Display` for any errno stable
`ErrorKind` cannot name, and that `Display` appends ` (os error N)`. `EBADF`
is such an errno, so the first thing the new code printed was
`nice: write error: Bad file descriptor (os error 9)` against GNU's
`nice: write error: Bad file descriptor`. The suffix is std's, not POSIX's,
and no utility this crate imitates has ever printed it — so *any* unnamed
errno would have given the port away. `errmsg::host_text` now strips it,
which fixes the whole family rather than `EBADF` alone.
