## `tee` was five options short and dropped the log the moment the pipeline's reader left (lane B, 2026-08-24)

**In short:** `tee` copies what it reads to standard output *and* to every file
you name — that is why `build 2>&1 | tee build.log` both shows you the build and
keeps a copy. The entry above
(`B-tee-REPORTS-SUCCESS-AFTER-LOSING-THE-DATA`, two days earlier) stopped it
reporting success after a failed copy. This entry is the rest of the program.
It still accepted only `-a` and file names: `-p` and `--output-error` — the two
options that exist precisely to say what should happen when a copy fails —
were not implemented, `-i` was rejected outright, and `--help`/`--version` did
not exist. It still panicked on a file name holding a byte that is not valid
UTF-8, which this OS allows everywhere but `/` and NUL. And worst of the three:
when the program reading its standard output went away, it stopped copying **to
the files as well**, leaving the log truncated at the last 8 KiB boundary, and
exited 0 — the same "durable copy that is not there, reported as success" that
the earlier entry was written about. Rewritten as a port of GNU coreutils
9.4's `src/tee.c`, read rather than recalled, with `scripts/tee-diff.sh`
running it against the real GNU binary case by case inside WSL.

### Why the earlier fix could not have found these

That fix repaired the code that was there. It never opened `tee.c`, and its own
closing paragraph shows the cost: *"The one thing deliberately left as success:
`BrokenPipe` on stdout"* is upstream's reasoning for its default mode applied to
half the program. Upstream decides on the **errno**, and then drops that one
output and **keeps copying to the others**. Ours reached the same verdict about
the status and then called `process::exit`, which ends the copy to every file
too. The right conclusion, drawn from the wrong place, still lost the data.

The unit tests could not help either: twelve of them, all on `parse_args`, all
agreeing with each other about an option set that was five options short. A
test written against the implementation can only find disagreements *inside*
it.

### The defects the rewrite fixed

1. **A broken stdout abandoned the files.** `process::exit(status)` sat inside
   the stdout-write error branch, above the loop over `files`. `yes | tee log |
   head -1` left `log` holding one 8 KiB block of a stream that had not ended,
   and exited 0. Upstream drops the failed output and copies to whatever is
   left until the input is exhausted; only when *every* output has been dropped
   is there nothing more to do.
2. **A read error skipped the flushes.** `process::exit(1)` in the read arm,
   above the `out.flush()` at the bottom of `main`. Up to 8 KiB of input that
   had already been copied was sitting in stdout's `LineWriter` and went
   nowhere — a read failure turned into a *write* loss.
3. **stdout was buffered; upstream makes every output unbuffered.** `tee.c:255`
   is `setvbuf (stdout, NULL, _IONBF, 0)`, and `tee.c:275` does the same for
   each opened file. Two things follow, and both matter: a `tee` that returns
   from a write before the byte has reached the descriptor can report success
   for data still in memory, and a buffered `tee` breaks the reason it is in
   the pipeline at all — `cmd | tee log | grep -q x` should see each block as
   it arrives, not at the next 8 KiB boundary. Now every chunk is flushed to
   stdout before the next output is written.
4. **`-p` and `--output-error` were missing entirely.** Five modes upstream
   (`warn`, `warn-nopipe`, `exit`, `exit-nopipe`, and the default), and the
   whole of the decision is three lines of `fail_output`:
   `errno != EPIPE || output_error == exit || output_error == warn`. Note what
   that tests — the **errno**, not the file. `nopipe` does not mean "this
   output is a pipe"; a unix socket whose peer has gone reports `EPIPE` and is
   treated identically. Without these options there was no way to ask `tee` to
   stop at the first failed write (`--output-error=exit`, which is what a
   backup script wants) and no way to ask it to stay quiet about a reader that
   left (`-p`, which is what an interactive pipeline wants).
5. **`-i`/`--ignore-interrupts` was rejected, on a rationale that was
   backwards.** The module doc argued that there is no `SIGINT` here, so the
   option should not exist. But the option asks for `SIGINT` to be *ignored*,
   and a system with no `SIGINT` has already granted the request. Rejecting it
   failed the whole pipeline over something that was already true. It is now
   accepted and does nothing, which is the accurate translation.
6. **`--help` and `--version` were missing**, and the failure path invented a
   `Usage:` line GNU does not print.
7. **A bad option was diagnosed in wording nothing matches.** `unknown option:
   -Z` where GNU says `tee: invalid option -- 'Z'` followed by
   `Try 'tee --help' for more information.`, and `unknown option: --zzz` where
   GNU says `tee: unrecognized option '--zzz'`.
8. **No long-option abbreviation, and `--append=x` was mis-diagnosed.** GNU
   accepts `--app`, and rejects `--append=x` with `option '--append' doesn't
   allow an argument`; ours took the whole string as one unknown option. Table
   order is observable too — `scripts/getopt-ambiguity-check.py tee` now checks
   ours against GNU's as a *sequence*, because glibc names `pfound` (the first
   table entry an ambiguous prefix matched) before the rest.
9. **Errors were rendered with `Display`.** `tee: 'log': No such file or
   directory (os error 2)` instead of `strerror`'s bare text.
10. **argv was `Vec<String>`.** `env::args()` panics on the first byte that is
    not valid UTF-8, so `tee $'\xff'` aborted before any of the above could
    matter. Removed from `scripts/argv-utf8-baseline.txt`; 30 findings remain.
11. **A short-circuited read was fatal.** No `ErrorKind::Interrupted` retry, so
    a read the kernel cut short ended the copy and truncated it. Upstream's
    `errno == EINTR` loop; there are no signals here, but a kernel is still
    free to return early.
12. **"Every flush is checked" was true and vacuous for the files.** Rust's
    `File::flush` is `Ok(())` — the write *is* the syscall, there is no buffer
    to empty — so the check the earlier entry added could not fail. What
    actually catches a full disk is the error from `write_all`, which that fix
    had also added; the flush was doing nothing. The only real buffer was
    stdout's, and #3 is what removes it.

### What the differential harness found: nothing, and that is the result

`scripts/tee-diff.sh` reports **71 passed, 0 differed, 6 differ on purpose** in
about 85 seconds. `OURS=$(command -v tee) scripts/tee-diff.sh` flips exactly
the six to XPASS and nothing else, which is what shows it discriminates. It is
the fifth harness of this shape (`du`, `find`, `ls`, `cmp`) and builds the
subject *for Linux inside WSL*, sharing `$HOME/.cache/slateos-diff-target`
with the others (`design-decisions.md` §374). Unique to `tee`: each case gets a
fresh directory per side, snapshotted and compared afterwards, because `tee`'s
side effects *are* its output.

Unlike `cmp` — where building the harness turned up three defects that
thirty-three unit tests had missed — every difference this one reported was a
defect in **itself**. They are worth listing because they generalise to the
next harness:

* **A case whose runtime depends on the program's own output is not a case.**
  `tee -a self < self` is unbounded on both sides; the first run spent twelve
  minutes at 100% CPU producing 188 MB (ours) and 166 MB (GNU) before `timeout`
  stepped in.
* **`od -An -c` on an unbounded capture** turns 188 MB into a gigabyte of octal
  inside a shell variable. Everything is now rendered through one `render()`
  helper that prints a size, then `od` only under 512 bytes and `md5sum` above.
* **A `/dev/urandom` fixture built once per side** hands the two programs
  different input, so the comparison is meaningless — and it fails *randomly*,
  which is the worst way for a harness to be wrong. Found by inspection, not by
  a failure. Now a deterministic `seq` stream.
* **`$?` is clobbered by the next command, including a `[ ... ]` test.**
  `if [ "$side" = ours ]; then o_rc=$?; else g_rc=$?; fi` records the status of
  the *test*: 0 for one side and 1 for the other, every time, agreeing with the
  truth exactly when the truth happened to be 0 and 1. It reported 59 of 68
  cases as differing and cost a full run. `rc=$?` goes on the very next line.
* **A wrong `xfail` is a claim nobody rechecks.** Four cases about how a file
  name is rendered were written as `xfail` on the assumption carried over from
  `cmp-diff.sh` that GNU emits raw bytes where we quote (§373). That is true of
  diffutils, which interpolates with a bare `%s`. It is **not** true of
  coreutils' `tee`, whose three `error` calls all spell the name
  `quotef (files[i])` — which is exactly what our `quotef_os` is. The harness
  said XPASS on the first clean run; they are ordinary passes now, and the
  header says so, because deleting a wrong xfail silently would leave the next
  reader to make the same assumption.

### `tee out <&-` diverges, and the reason is our runtime, not our `tee`

With stdin closed, GNU prints `tee: read error: Bad file descriptor` and then
`tee: standard input: Bad file descriptor` (the second from gnulib's
`close_stdin` atexit hook) and exits 1. Ours prints nothing and exits 0.

The first explanation written into the harness was that we lack `close_stdin`
because `coreutils` links no libc — and it was wrong, because it did not
account for the *missing first line* or for the status. The real cause is that
**Rust's std reopens any closed standard descriptor onto `/dev/null` before
`main` is entered**, so our `main` is handed an empty stdin, copies zero bytes,
and is right to call that a success. Nothing in `tee.rs` can see the
difference.

Measured rather than argued: `tee /proc/self/fd/0 <&-` asks the process about
its own descriptor table, since that symlink exists only while the descriptor
is open. It opens successfully for us and is `No such file or directory` for
GNU.

**What that leaves open, and it is not about `tee`:** the reason Rust does this
is a real hardening — if descriptor 1 is closed at `exec`, the first file the
program opens *becomes* its standard output, and everything it prints is
written into that file. A grep of `posix/` and the userspace runtime finds no
equivalent (`sanitize_standard_fds`, a `POLLNVAL` sweep of 0/1/2, or a
`/dev/null` fallback at spawn), so on SlateOS proper a program started with a
closed standard descriptor gets whatever the descriptor allocator hands it.
Logged in `todo.txt`; it belongs in the process-spawn path, not in any
individual utility.

### Deliberate divergences, all recorded as `xfail` in the harness

* `--help` omits the GNU project's `Report bugs to:` block; `--version` names
  SlateOS.
* **A broken stdout under the default mode.** Upstream's default leaves
  `SIGPIPE` fatal, so GNU is killed by it (status 141, no diagnostic). SlateOS
  does not use Unix signals for process control and Rust masks `SIGPIPE`
  anyway, so "die from `SIGPIPE`" has no translation. The faithful reading is
  that the default then becomes its own `EPIPE` path, which is `fail = false` —
  drop that output, stay quiet, keep copying to the rest. That is upstream's
  own code for the case, not an invention, and it is what `cut`, `head`, `tail`
  and `uniq` in this tree already do with a broken stdout. Visible consequence:
  `yes | tee log | head -1` keeps writing `log` until the input ends, where
  GNU's `tee` dies with the pipeline.
* `tee FILE <&-` — the closed-stdin case above.

### One thing deliberately not implemented

**`iopoll`.** Upstream watches the first live output *while blocked on the
read*, so a `nopipe` run whose outputs have all become broken pipes ends at
once rather than at the next read. That needs `poll(2)`, and `coreutils` links
no libc. The bytes copied are identical either way; what differs is the moment
a doomed run gives up, and only while stdin is idle.
