## `nohup` ignored no hangup, and sent `nohup cmd > out.txt` to `nohup.out` instead (lane B, 2026-08-24)

**In short:** `nohup` exists to let a command outlive the terminal it was
started from. Ours did not do that: it never told the kernel to ignore the
hangup signal, so a command run under it died with the terminal exactly as it
would have without it — the program was a no-op with respect to its own name.
It also captured the command's output into `nohup.out` *always*, including
when the user had explicitly redirected it somewhere else, so
`nohup cmd > out.txt` left `out.txt` empty and the output in a file the user
had not asked for. Rewritten from GNU coreutils 9.4's `src/nohup.c`, checked
against the real GNU binary across 77 cases, 41 of them under a
pseudo-terminal.

### The name was the specification, and the code did not implement it

The old file contained no signal code at all. It spawned the command and
waited for it:

```rust
match Command::new(&parsed.cmd).args(&parsed.cmd_args)...spawn() {
    Ok(mut child) => match child.wait() { ... }
```

Two separate faults sit in those two lines.

**No `SIGHUP` disposition was ever set.** `nohup` is one system call in a
trench coat — `signal(SIGHUP, SIG_IGN)` before the command starts, so that the
`SIGHUP` the kernel sends to a terminal's foreground group on hangup is
discarded rather than killing the process. Without it the command dies on
hangup, which is the exact event the user ran `nohup` to survive. Nothing else
the program does matters if this is missing, and it was missing.

**It spawned and waited instead of `exec`ing.** GNU replaces itself with the
command, so once `nohup` has done its work no trace of it is left. Ours stayed
alive as the command's parent — a process that itself had no `SIGHUP`
protection, so the hangup that the child was (notionally) shielded from killed
the shield. It also cost a process for the command's whole lifetime, and it
lost the exit status of a command that died from a signal, mapping every such
death to `126` via `status.code().unwrap_or(126)`.

### `nohup cmd > out.txt` wrote to `nohup.out`, and the code said why

```rust
// Try to redirect stdout to nohup.out if it's a terminal.
// In our minimal environment, we'll always redirect since we
// can't easily check isatty.
let output_file = OpenOptions::new().create(true).append(true).open("nohup.out");
```

Every clause of that comment is wrong, and the last one is checkable. The
whole of `nohup`'s redirection behaviour is conditional on `isatty`: output is
diverted to `nohup.out` **only** when it would otherwise go to the terminal.
A user who redirects explicitly has already said where the output goes, and
GNU's own `--help` tells them to — `To save output to FILE, use 'nohup COMMAND
> FILE'.` Our `--help` said the same thing, above code that made it false.

"We can't easily check `isatty`" was not true when it was written.
`posix/src/ioctl.rs` — which exports `isatty` — was added on 2026-05-08.
`nohup.rs` was added on 2026-05-17, nine days later. The comment is a guess
about the tree recorded as a fact about it, and it survived three months
because nothing tested the branch it excused.

### The rest of the old program

- **`nohup --help` tried to run a command called `--help`**, failing with `No
  such file or directory` and status 127. The parser recognised no options at
  all; the help text it did contain was unreachable.
- **stdin and stderr were untouched.** GNU makes stdin unusable and points
  stderr at wherever stdout went, both conditionally on `isatty`.
- **No `$HOME` fallback.** GNU tries `./nohup.out` and then `$HOME/nohup.out`.
- **`nohup.out` was created world-readable** (`0666 & ~umask`), rather than
  `0600`. The output of a command you detached from your terminal is not
  something to hand to the rest of the machine by default.
- **The exit statuses were invented**: `127` for every spawn failure rather
  than only for not-found, `126` for a failed `wait`, and `125` — `nohup`'s
  own "I failed before the command started" status — used for nothing but the
  argument error.
- **`env::args()` as `Vec<String>`**, so any argument holding a byte that is
  not valid UTF-8 aborted the process. That is what put this file on the
  `argv-utf8` list, and it is the smallest of the faults above.

### Six defects in the *replacement*, found by measuring GNU rather than recalling it

The port was written from `nohup.c` and smoke-tested, and it looked right. It
was then probed against the real `/usr/bin/nohup` seven times before a single
harness case was written, and each round found something. None of these would
have been caught by reading the source I was porting from, because each of
them lives in a detail the source states obliquely or not at all.

| What ours did | What GNU does | How it was found |
|---|---|---|
| Said `ignoring input` whenever stdin was a terminal | Says it only when *neither* stdout nor stderr is being redirected — otherwise the fact is folded into the longer sentence | Ran both with stdin a tty and stdout a pipe |
| Opened `/dev/null` **read-only** for stdin | Opens it **write-only**, on purpose, so the command's *reads fail* rather than returning end-of-file | `ls -l /proc/self/fd/0` under each; `nohup cat` gives `cat: -: Bad file descriptor` |
| Printed one `failed to open` line | Prints one per path tried, so the reader can see `$HOME` was tried too | Made both paths unwritable |
| Requested mode `0600`, which `umask` then filtered | Brackets the open with `umask(0177)`, making the mode exact | Compared modes under `umask` 000, 077, 0200, 0466 |
| Created no `nohup.out` when stdout was *closed* | Creates one, because the `dup2` that redirects stderr needs something on descriptor 1 | `nohup true >&-` vs `nohup true >&- 2>err` |
| Exited 0 when its own diagnostic could not be delivered | Exits 125 — an unwritable stderr means the user cannot be told where the output went | `nohup true 2>&-` |

The last one is the one worth dwelling on. `nohup`'s messages are not
decoration: `appending output to 'nohup.out'` is the *only* record of where a
detached command's output went. A `nohup` that cannot say it and starts the
command anyway has silently lost the output. GNU treats that as a failure to
perform the job, and now so do we.

Two of these lose user data outright — the read-only `/dev/null` turns a
failed read into an empty one, and the closed-stdout case discards the
command's stderr instead of saving it.

### Two places the Rust runtime lies about the standard descriptors, and how each is defeated

Both faults below are the same shape: the standard library papers over a
closed standard descriptor because for almost every program that is a
kindness, and for *this* program it is the subject matter.

**`sanitize_standard_fds`.** Before `main` runs, Rust reopens any of
descriptors 0, 1 and 2 that is closed onto `/dev/null`. So by the time our
code could look, `nohup cmd >&-` was indistinguishable from
`nohup cmd >/dev/null` — and the two must behave differently, the first
creating `nohup.out` and the second not.

There is no runtime switch for this, so the answer is recorded *before* the
runtime can destroy it, from an ELF constructor that `__libc_start_main` runs
ahead of Rust's `lang_start`:

```rust
#[used]
#[unsafe(link_section = ".init_array")]
static RECORD_CLOSED_STD_FDS: extern "C" fn() = record_closed_std_fds;
```

`run` then closes again whatever the constructor saw closed. That the
constructor really does run first was measured, not assumed — a standalone
`rustc` probe reported `early(ctor)=100 late(main)=000` for `2>&-`.

**`handle_ebadf`.** `StderrRaw`'s `Write` impl passes its result through the
standard library's `handle_ebadf`, which turns `EBADF` into `Ok(buf.len())`:
a write to a closed stderr is reported as a write that fully succeeded. This
is why `nohup true 2>&-` exited 0 against GNU's 125 even after the descriptor
bookkeeping above was correct — the write genuinely failed and the standard
library said it had not. The fix is to stop asking it: the diagnostic path
calls `write(2)` directly through a small `write_all_fd`, and reports what the
kernel reported.

**This is a hazard every binary in this tree shares**, not a `nohup`
peculiarity. Any utility whose exit status or behaviour depends on whether a
standard descriptor is open — and by POSIX that is a large family — will be
wrong by default in Rust, silently and in the safe direction, which is the
hard kind to notice. The two mechanisms above are the general answer; they are
written in `nohup.rs` with enough commentary to be lifted.

*Update, same day:* they **were** lifted, into `coreutils::stdfd`, when `nice`
turned out to need them too; `nohup.rs` no longer carries its own copies. See
`TD-B-EVERY-BINARY-IS-WRONG-ABOUT-A-CLOSED-STANDARD-DESCRIPTOR` below for the
shared module and the bin-by-bin sweep that is still outstanding, and
`design-decisions.md` §375 for why the startup hook has to be a macro.

### Why the harness needs a pseudo-terminal

Every branch that makes `nohup` `nohup` is guarded by `isatty`. Run from a
pipe — which is how a test harness normally runs anything — the program
redirects nothing, says nothing and creates no `nohup.out`. A harness built
like `cut-diff.sh` would therefore exercise the argument parser and *nothing
else* while reporting a full green column, which is a worse outcome than
having no harness at all. `scripts/nohup-diff.sh` runs the interesting half of
its cases under `script -qec`, so `isatty` answers yes and the redirections
actually happen. All seven combinations of (stdin, stdout, stderr) being a
terminal are covered.

The comparison is also not text-only. `nohup`'s real output is a *file*, so
every case compares a snapshot of the directory it ran in — each path with its
octal mode and size — plus the bytes of every file. A port that printed the
right sentence and created `nohup.out` world-readable, or empty, or in the
wrong directory, would pass a text-only comparison and fail this one.

Two harness bugs were found by running it with `OURS=/usr/bin/nohup`, which
must turn every `xfail` into `XPASS` and change nothing else: the per-side
temporary directory was leaking into the compared text, so the two
`$HOME`-fallback cases could never have agreed; and the deliberate
`kill -TERM` case emitted a `Terminated` line from the harness's own shell.

### One latent bug the descriptor work exposed

With descriptor 1 genuinely closed, the `open` of `nohup.out` lands *on*
descriptor 1, because it is the lowest free number. The redirect helper would
then `dup2(1, 1)` and drop the `File` — closing the descriptor it had just put
in place, leaving the command with no stdout at all. It is fixed by taking the
raw descriptor and returning early when it is already the target. The case is
reachable only because the `.init_array` work above made it reachable; before
that, the runtime's `/dev/null` was occupying descriptor 1 and the bug was
merely waiting.

### Deliberate divergences, both recorded as `xfail`

The family's two: `--help` omits the GNU project's `Report bugs to:` block,
and `--version` names SlateOS. Confirmed discriminating by running the harness
with `OURS=/usr/bin/nohup`, which turns both into `XPASS` and nothing else.

Final: **75 passed, 0 differed, 2 differ on purpose**, plus 18 unit tests —
one of which is an eight-row table over every `(stdin, stdout, stderr)`
terminal combination, asserting the exact ordered list of lines GNU produced
for each.
