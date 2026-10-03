## TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS (lane B, 2026-08-24) — **open**

**What it is.** Every coreutils binary reports its diagnostics with
`eprintln!`, and `eprintln!` panics when the write fails. The panic handler
then tries to print the panic message — to the same stderr — which fails
again, so the runtime gives up and calls `abort()`. The process dies of
`SIGABRT`: shell status **134**, `Aborted (core dumped)`, and on a terminal a
`note: run with RUST_BACKTRACE=1` line that GNU never prints. Measured under
WSL, with stderr on `/dev/full`:

| | ours | GNU 9.4 |
|---|---|---|
| `id --nope` | 134 | 1 |
| `uname --nope` | 134 | 1 |
| `logname x` | 134 | 1 |
| `whoami x` | 134 | 1 |
| `tty x` | 134 | 3 |

The same happens for `>&-` on stderr rather than `/dev/full`, for the same
reason: `guard_std_fds!` leaves the descriptor genuinely closed, so the write
returns `EBADF` and `eprintln!` panics on that instead.

**What GNU does instead**, pinned by measurement rather than recall. gnulib's
`close_stdout` closes *stderr* as well as stdout, and `_exit (exit_failure)`
if that close fails. The rule that falls out of it has three cases:

* **nothing was written to stderr** — the failure is invisible and the earned
  status stands: `id --help` 0, `uname` 0, `logname --version` 0, `tty -s` 1.
* **a diagnostic was attempted and lost** — `exit_failure`, *silently*, since
  there is nowhere left to complain: `id --nope` 1, `logname x` 1, `whoami x`
  1, `tty x` 3 (each utility's own failure status, not a fixed 1).
* **stdout unwritable too** — the same, and it is stderr that has the last
  word: `pwd x 2>/dev/full` is GNU 1 against our 0, so a lost diagnostic
  overrides even a success.

**Scope.** 354 `eprintln!`/`eprint!` sites across 72 of the 84 bins. Nineteen
of them are binaries *already converted* by the closed-descriptor sweep, which
fixed stdout and left stderr exactly as it was: `basename`, `cat`, `comm`,
`dirname`, `expand`, `fold`, `join`, `logname`, `nice`, `nohup`, `paste`,
`printf`, `pwd`, `seq`, `tsort`, `tty`, `unexpand`, `whoami`, `yes`. So this
is not a backlog item behind the sweep — it is a hole *in* it.

**The proper fix**, in this order:

1. A diagnostic writer in `coreutils::stdfd` that cannot panic: raw `write(2)`
   to fd 2, no `std::io::Stderr`, the result *recorded* rather than
   propagated. Call it `stdfd::diag(program, args)` behind a macro shaped like
   `eprintln!` so the 354 call sites convert mechanically.
2. A process-global "a diagnostic was lost" flag it sets, and an extension of
   `stdfd::close_stdout` to consult it: when set, the returned status becomes
   the utility's failure status even if the run had earned success. The four
   utilities that spell their tail out by hand (`env`, `ls`, `sort`, `tty`)
   need the same check written into theirs.
3. Retrofit the nineteen converted bins, then the rest as the sweep reaches
   them.
4. Harness cases. `pwd-diff.sh`, `whoami-diff.sh`, `logname-diff.sh` and
   `tty-diff.sh` all exercise a closed and a full *stdout* and none of them
   touches stderr, which is exactly why this survived four ports. Every
   harness wants `2>/dev/full` and `2>&-` against both a quiet run and a
   diagnostic-producing one.

### Done, 2026-08-24 — steps 1–4 for the converted set

`stdfd` grew `diag!`/`diag_line`/`diag_bytes` over raw `write(2)`, the
`DIAGNOSTIC_LOST` flag, and `close_stderr`, and all nineteen converted bins
plus `digest.rs` now go through them. `pwd-diff.sh` 108/0/5, `whoami-diff.sh`
35/0/6, `logname-diff.sh` 33/0/6, `tty-diff.sh` 58/0/7 — every one of those
files gained a `2>/dev/full` and `2>&-` group, and every case in it was a 134
before the change.

Two decisions worth carrying forward, because the remaining ~65 bins inherit
them:

* **`close_stderr` wraps `main`; it is not called per exit path.** Upstream
  reaches the verdict from an `atexit` handler, so *every* exit gets it —
  including the early `usage (EXIT_FAILURE)` that never returns from `main`.
  Rust has no `atexit` that can change the status, but exactly one value
  leaves `main`, so each bin is now `fn main() -> ExitCode {
  stdfd::close_stderr(run_main(), FAILURE) }`. The inner is `run_main` and not
  `run` only because `fn run` is already a worker name in about thirty of
  these files. A call at each `return` would be the same rule written N times,
  and the (N+1)th exit path someone adds later would not have it.
* **`io::stderr()` is as much of a lie as `eprintln!`, and quieter about it.**
  `pwd` and `tsort` thread their diagnostics through an `impl Write` so the
  tests can read them out of a `Vec`; both were still handing the production
  side `io::stderr()`, whose `EBADF` the runtime maps to success and whose
  `ENOSPC` the `let _ =` throws away. So no flag was set, and `pwd foo 2>&-`
  stayed at 0 against GNU's 1 *after* the funnel was in place — the harness
  caught it, which is the whole reason the group was written. The fix is
  `Stream::stderr()`, whose `record` sets the same flag: it implements `Write`,
  so the threading is untouched. Any bin the sweep reaches that names
  `io::stderr()` needs the same substitution; `cp`, `du`, `find`, `ln`,
  `mkdir`, `mkfifo`, `ls`, `mv`, `od`, `readlink`, `realpath`, `rm`, `rmdir`,
  `sed`, `sort`, `touch` and `awk` still do.

### Done, 2026-08-24 — step 3, the rest of the bins

**290 `eprintln!` sites in 55 files converted to `diag!`**, plus the four that
were not mechanical, so **no `eprintln!` or `eprint!` remains in the crate
outside comments**. No bin can abort on an unwritable stderr any more.

The mechanical part was a regex over `src/bin/**`, skipping comment lines, plus
a `use coreutils::diag;` per file. Six files keep their sites inside an inner
`mod imp`/`mod net`, which needs its *own* import — a `use`d `#[macro_export]`
macro is a name in that module, not in its children. In five of those
(`chmod`, `chown`, `cmp`, `id`, `ls`) the only *top-level* site is the
`#[cfg(not(unix))]` stub `main`, so the file-level import is now
`#[cfg(not(unix))] use coreutils::diag;` — unused on the real target
otherwise. Only the x86_64-slateos build compiles `#[cfg(unix)] mod imp` at
all, so the Windows host build cannot find any of this; `cargo +nightly build
--bins` is the one that can.

The four hand edits, and why each is not a plain substitution:

| site | was | now | why |
|---|---|---|---|
| `find.rs` | `eprintln!()` | `diag!("")` | `diag!` is `format!` underneath and `format!()` has no format string |
| `time_cmd.rs` | `eprintln!()` | `diag!("")` | same |
| `more.rs` ×2 | `eprint!` + `io::stderr().flush()` | `prompt()` → `stdfd::write_all(2, …)` | no trailing newline, **and it must not set the flag** |

`more`'s `--More--` is the one deliberate exception to "everything on
descriptor 2 goes through `diag`". It is a prompt, not a complaint — the
pager's half of a conversation with the terminal — so losing it does not mean
the file was paged wrongly and must not turn a clean run into a failing
status. `stdfd::write_all` is the same `write(2)` without the flag. The flush
went with it: unlike `io::stderr()` there is no buffer to flush.

**Non-diagnostic stderr output *should* set the flag, though** — checked
against the real binaries rather than assumed, because it looked like the same
exception as `more`'s and is not:

```
dd if=/dev/zero of=/dev/null count=1 2>/dev/full → 1     (2>&- → 1)
ls / 2>/dev/full → 0    wc … 2>/dev/full → 0    seq 3 2>/dev/full → 0
```

`dd`'s `N+0 records in` statistics are its *output*, not a complaint, and the
copy itself succeeded — yet GNU still exits 1, because `close_stdout` is
registered with `atexit` and does not care *what* the unflushable bytes were.
The utilities that write nothing to stderr keep their earned 0. So `dd`'s
statistics, `fetch`'s `-v` trace and `time`'s timing line are all correctly
routed through `diag!` now; the distinction that matters is not
diagnostic-vs-output but whether the bytes were *the program's* at all, and
`more`'s prompt is echo to a terminal rather than output.

### Done, 2026-08-24 — the `io::stderr()` substitutions

All twenty of them, in `awk`, `cp`, `du`, `find` ×2, `ln`, `ls`, `mkdir`,
`mkfifo`, `mv`, `od` ×2, `readlink`, `realpath`, `rm`, `rmdir`, `sed`, `sort`
and `touch`. Three named `io::stderr()` sites remain in the crate and are all
correct: two are `is_terminal` queries, which ask rather than write, and the
third is `stdfd`'s own non-libc fallback, where there is no `write(2)` to call
— now commented as the one deliberate use.

They did not all want the same replacement, and which one they wanted turned on
a question the code could not answer:

| shape | replacement | why |
|---|---|---|
| `let mut err = io::stderr()`, threaded as `&mut impl Write` (15 sites) | `Stream::stderr()` | `Stream` implements `Write`, so the threading is untouched, and its `record` sets the flag |
| a `/dev/stderr` **sink** the program writes its output to, returning `io::Result` (`awk`, `find -fprint`, `sed w`) | `stdfd::write_all(2, …)` | the caller already reports the error and maps it to a status; it only needed to *see* one |
| a raw-byte diagnostic with a `let _ =` (`od` ×2) | `stdfd::diag_bytes` | nowhere left to report to, so record it and let `close_stderr` speak |

**`find`'s `-ok` prompt was the interesting one**, because it looks exactly
like `more`'s `--More--` and behaves oppositely. Both are prompts rather than
complaints, and the standing comment in `find` said the write error was
"dropped deliberately". Measured:

```
find q.txt -maxdepth 0 -ok true {} \; </dev/null 2>/dev/full → 1
find q.txt -maxdepth 0 -ok true {} \; </dev/null 2>/dev/null → 0
```

So GNU does fold the lost prompt into the status — `close_stdout` runs from
`atexit` and does not care that the unwritable bytes were a question. It goes
through `diag_bytes`. `more` keeps `write_all` because it is not a GNU utility
and registers nothing; that is the difference, not prompt-versus-diagnostic.

Three sinks were also assembled into one buffer and written once, instead of
two-to-five `write(2)` calls: `find`'s prompt, `sed`'s `w /dev/stderr` line and
its newline, matching upstream's single `fprintf`. Separate calls let another
process's output land in the middle of a line on a shared terminal.

**Still open**, and re-scoped to the closed-descriptor sweep rather than to
this entry: the `close_stderr` funnel for the ~65 bins that still lack one, and
step 2's hand-written tails in `env`, `ls` and `sort` (`tty`'s is done). Until a
bin has the funnel its flag is set and never read, so the work so far converts a
134 into the *old* status rather than into GNU's — an improvement with no
regression, but not yet parity.

**Found on the way, and not the same bug.** `wc >/dev/full` exits **0**
silently — it never checks the write at all. `head >/dev/full` reports the
failure as `head: error reading '/etc/hostname'`, blaming the input for an
output error. `nl >/dev/full` prints `nl: No space left on device` without
GNU's `write error:` prefix. All three are stdout defects for the sweep to
pick up; recorded here so they are not lost.

**How it was found.** Probing `/dev/full` while porting `tty`, after a first
probe that used `>/dev/full 2>&1` and so pointed the finger at stdout — the
134 came from stderr all along.

### 2026-10-03 -- where it stands

Measured against each program's reference, the stderr half is now settled
wherever upstream has a rule, and is absent where upstream has none:

| program | upstream's rule | here |
|---|---|---|
| `tar` | its own `main` tail: `close_stdout ()` when the member list is on stdout, else `ferror (stderr)` raises the status to 2 | ported (`conclude`), with `stdopen` |
| `test` / `[` | `atexit (close_stdout)`, failure status 2 | ported: `close_stdout_with(prog, out, earned, 2)` |
| `sed` | checks stdout itself (`ck_fflush`, `ck_fclose`), never stderr | no stderr funnel is the faithful answer; its *stdout* layer is not GNU's and is next |
| `patch` | checks neither at exit | agrees as it stands (measured: `2>/dev/full` statuses equal) |
| `hostname` (Debian 3.23) | checks neither | agrees as it stands |
| `more` | util-linux's `close_stdout` | waits on `TD-B-more-HAS-NO-INTERACTIVE-COMMANDS-BEYOND-SPACE-ENTER-AND-q`; ours is not a port yet |

The other half of this is the closed-*descriptor* guard (`guard_std_fds!` and
`stdfd::restore`), without which Rust's runtime quietly replaces a closed
descriptor with `/dev/null` before `main` and a program cannot see `>&-` at
all. Forty-four programs still lack it: `bc chmod chown cmp cp csplit cut date
df dir du ed env expr find id install kill ln ls mkdir mkfifo mktemp more mv od
patch readlink realpath rmdir sed shuf stat tac tail tee touch tr uname uniq
vdir awk hostname sort`. Each wants measuring against its reference before it
is converted -- `tar`'s answer to a closed stdout (a reason-less `write error`,
because GNU tar reopens it read-only first) is not `wc`'s -- which is why this
is done program by program rather than as one edit.
