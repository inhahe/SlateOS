## B-kill-CANNOT-SIGNAL-A-PROCESS-GROUP (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** `kill` could not send a signal to a *process group* — the normal
way a shell shuts down a whole job — and, worse, it did not say so. A process
group is a set of related processes (a pipeline, a background job) that the
system lets you signal as a unit; you name one by writing its number with a
minus sign, so `kill -TERM -1234` means "terminate group 1234". Our `kill` read
that leading minus as introducing a *signal name*, so it quietly decided the
signal was number 1234 and then complained it had been given no process to
signal. In the form that appears in real scripts — `kill -9 -1234 567` — it did
something worse than fail: it sent signal *1234* to process *567*. The wrong
signal, to the wrong target, with no hint that anything had been misread.

Fixed: `userspace/coreutils/src/bin/kill.rs`, rewritten. 22 → 39 tests.

### The parse

The old parser had one loop over *every* argument:

```rust
for arg in args {
    if arg.starts_with('-') && arg.len() > 1 {
        match resolve_signal(&arg[1..]) { Some(n) => signal = n, ... }
    } else { pids.push(arg.clone()); }
}
```

Anything shaped like `-X` was a signal, wherever it appeared. So a PID could
never be negative, and a negative one was not rejected — it was *absorbed*:

| Typed | Old result | Correct |
|---|---|---|
| `kill -TERM -1234` | signal := 1234; then `kill: missing PID` | SIGTERM to group 1234 |
| `kill -9 -1234 567` | `kill(567, 1234)` | SIGKILL to group 1234 **and** to 567 |
| `kill -- -1` | `kill: unknown signal: -` | SIGTERM to group 1 |
| `kill 100 -9 200` | signal 9 to 100 and 200 | signal is argv[0] only; `-9` is a bad PID |

The last row had a test asserting the old behaviour —
`parse_signal_after_pids_still_applies`, with the comment "order doesn't matter
for collecting". **The third time this audit has found a test that pins the
wrong answer** (after `chmod`'s `parse_recursive_lowercase_r_accepted` and
`dd`'s `parse_size_garbage_zero`). The signal goes first in POSIX for exactly
the reason this bug exists: if it could go anywhere, a negative PID would be
ambiguous with it.

The POSIX layer was never the limitation. `posix/src/signal.rs:1053 kill()`
routes `pid <= 0` to the kernel's group fanout over the membership `setpgid()`
writes, handles `pid == 0` as the caller's own group, and reports EPERM/ESRCH
honestly (§314). All of that was reachable from C and unreachable from the
command line.

### Five more defects

1. **Twenty of the thirty-one signals did not exist.** The table had twelve
   entries. Missing: `ILL`, `TRAP`, `BUS`, `FPE`, `SEGV`, `TTIN`, `TTOU`,
   `URG`, `XCPU`, `XFSZ`, `VTALRM`, `PROF`, `WINCH`, `IO`, `PWR`, `SYS`,
   `STKFLT` — and `USR1`/`USR2`, which after TERM/HUP/KILL are the signals
   scripts send most (`kill -USR1` is how you ask a daemon to reopen its logs).
   `kill -USR1 $pid` was `unknown signal: USR1`. The table is now 1..=31, and a
   test asserts it is dense and matches `posix/src/signal.rs`'s constants
   one for one — a `kill` whose `TERM` was not the platform's `SIGTERM` would
   be a much quieter disaster than a missing name.

2. **`-s SIGNAL` did not work.** This is POSIX's own spelling, the one the
   standard's synopsis leads with. `-s` fell into the signal branch, so
   `kill -s TERM 123` died with `unknown signal: s`. `-n`, `--signal` and
   `--signal=` were likewise absent.

3. **Every failure printed the same sentence.** `No such process or permission
   denied` — for ESRCH, for EPERM, for EINVAL alike. The one thing a script
   most needs to distinguish (the process is gone → fine; I am not allowed →
   not fine) was deliberately blurred. Now `No such process` /
   `Operation not permitted` / `Invalid argument`, chosen by errno, in GNU's
   wording and with GNU's quoting.

4. **`kill -l 9` ignored the `9`.** `-l` was taken as the whole command and its
   operands discarded, so it listed everything instead of printing `KILL`.
   `kill -l $?` — the reason the synopsis says `EXIT_STATUS` — now works,
   including the 128 subtraction that turns a wait status back into a signal.
   `-l`'s format was also one-signal-per-line rather than GNU's single
   space-separated line that `$(kill -l)` expects to read as a word list;
   `-L` gives the numbered table.

5. **`kill -l | head -1` panicked.** `print!` panics when the write fails, so
   a reader closing the pipe produced a Rust panic message. Same family as
   `tee`, `tar`, `dd` and `stat`: writes are checked now, with `BrokenPipe`
   the one deliberate success.

Also: an out-of-range numeric signal (`kill -1234 $$`) used to reach `kill()`
and fail with EINVAL, reported against the *PID*. It is rejected up front now,
naming the signal.

### Two judgment calls

- **The signal universe is 0 and 1..=31 — no real-time signals.** Linux has 34
  through 64, but `posix/src/signal.rs`'s `SIGNAL_NAMES` stops at 31, so
  `strsignal` cannot describe them. Listing names for signals the platform
  cannot name would make `kill -l` a catalogue of things that do not work.
  `SIGNALS` is the single place to extend when the POSIX layer grows them.

- **`-segv` is SIGSEGV, not `-s egv`.** getopt's rule is that an option taking
  an argument swallows the rest of the token, which would make `kill -segv $$`
  send whatever `egv` means — nothing — and it is reachable by accident because
  this program accepts lower-case signal names everywhere else. The whole body
  after `-` is tried as a signal spec first, and only then re-read as an
  attached option argument. `-s9` and `-n15` still work; no valid GNU spelling
  changes meaning.

**Not verified end-to-end.** `send_one` is `cfg(target_os = "linux")` and needs
QEMU to exercise. The `cfg` region is now three lines — the diagnostics, the
parser, the tables and the listing are all outside it and all tested on the
host, which is the point: the old code built its error messages *inside* the
`cfg`, so nothing about them was compiled on the development machine. Fifth
consecutive argument for a filesystem-level harness that runs the shipped
binaries inside the OS.

**Seven for seven.** Every shipped `coreutils` bin read against its bigger
standalone twin — `chmod`, `dd`, `tee`, `tar`, `chown`, `stat`, `kill` — has
had a silent-wrong-behaviour bug. The hit rate is not a coincidence: the
standalone twin is bigger *because* it was written against the real problem,
and the shipped one is smaller because it was written against a summary of it.
