## B-hostname-DOES-NOTHING-IN-EITHER-DIRECTION (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** The `hostname` command did not work at all, in either direction,
and said nothing about it. Asked for the machine's name it always answered
`localhost`, whatever the machine was really called. Asked to *change* the
name — `hostname newbox` — it changed nothing and exited reporting success.
And because it had no notion of options, `hostname --help` did not print help:
it tried to rename your machine to the literal string `--help`. Now rewritten:
it reads and writes the same two files as the rest of the system, understands
all nine standard options, and refuses names that are not legal host names.

### Why it did nothing

The old version (`userspace/coreutils/src/bin/hostname.rs`, 125 lines) called
the C functions `gethostname()` and `sethostname()` from our POSIX layer. Those
look like system calls, and on Linux they are. Ours are not. In
`posix/src/unistd.rs` they are backed by:

```rust
process_global! {
    /// Initialized to "localhost" — can be changed via `sethostname()`.
    fn hostname_buf_ptr() -> [u8; HOST_NAME_MAX + 1] = { /* "localhost" */ };
```

and `process_global!` (`posix/src/perprocess.rs:82`) expands to `static mut
STORAGE` — a plain variable **inside the calling program's own memory**.
Nothing about it crosses a process boundary. So:

| Command | What it did | What the machine saw |
|---|---|---|
| `hostname` | read that variable | printed `localhost`, always |
| `hostname newbox` | wrote that variable, then exited | nothing changed, exit status 0 |

The second row is the dangerous one. A first-boot or provisioning script that
runs `hostname "$NAME"` and checks the exit status was told it succeeded. The
machine kept whatever name it had, and the script had no way to find out.

### It also disagreed with every other program

The rest of the tree keeps the host name in two files —
`/proc/sys/kernel/hostname` (the live value) and `/etc/hostname` (the value
that survives a reboot). `dhcpcd` writes both when a DHCP lease supplies a
name; `getty` shows the name in its login banner; `osh` fills `$HOSTNAME` from
exactly this pair, in exactly this order; `sysctl` maps `kernel.hostname` onto
the first; `hostnamectl`, `logger`, `snapper2` and `sudo` all read them. The
`hostname` command was the only program in the system not using them — so it
was also the only one that could not see a name `dhcpcd` had just set.

### Everything else that was wrong

| Defect | Consequence |
|---|---|
| **No option parsing whatsoever** | every option was read as a new host name. `-s`, `-f`, `-d`, `-i`, `-I`, `-F`, `-b`, `-V`, `--help` — all of them. `hostname -s` (the single most common use, "just the short name") tried to rename the machine to `-s`. |
| **No validation** | `hostname ""`, `hostname "my box"`, `hostname $'a\nb'`, a 5000-byte name — all passed straight through. A newline is the worst: it makes the *second* line of `/etc/hostname` look like a separate valid name to anything that reads the file line-wise. |
| **`String::from_utf8_lossy` on the result** | forbidden outright by CLAUDE.md rule 7 ("silent data corruption"). A name containing a stray byte printed as `a<?>b`, indistinguishable from a name that genuinely contained U+FFFD. |
| **errno discarded** | one string, `failed to set hostname`, for every cause. "You are not root", "that name is too long" and "the file is read-only" were the same message. |
| **Extra operands ignored** | `hostname foo bar` used `foo` and said nothing about `bar`. |
| **`env::args()`** | panicked before any of the above on a non-UTF-8 argument (see the sweep entry below). |
| **`println!`** | panics when stdout cannot be written, so `hostname \| head -1` could end in a Rust panic message rather than a broken pipe. |

### The fix

Rewritten to 700 lines, 7 tests → 35. Reads `/proc/sys/kernel/hostname` then
`/etc/hostname`; writes both, replacing the persistent one by rename so a crash
part-way leaves the old name rather than half of the new one (a truncated
`/etc/hostname` is read at boot as a *different valid name*, which is worse
than an unchanged one). Full option set including `-F`/`--file`, `-b`/`--boot`,
and `--`, which the standalone twin lacks — without `--` there is no way to be
sure an argument taken from a variable is treated as a name rather than an
option.

**Validation is done on bytes, and that is what also makes it panic-proof.**
A non-UTF-8 argument contains a byte ≥ 0x80; that byte is not ASCII
alphanumeric; so the *same* RFC 1123 rule that rejects a space in a host name
rejects it — with a diagnostic naming the byte, not a crash. The byte-safety
and the standards-compliance turned out to be one check, not two.

**There is no `#[cfg]` in the new file at all**, and that is the headline
lesson repeated from `kill` and `env`. The old version put its entire working
body inside `#[cfg(target_os = "linux")]`, so `cargo test` on the Windows
development host compiled *none* of it; its seven tests all exercised one
4-line buffer-decoding helper, and passed. That is precisely why nobody
noticed the program did nothing. The new implementation is file I/O and byte
manipulation, which compiles and runs identically on the host — the paths
merely do not exist there, which the code must handle anyway because they may
not exist on the real system either.

**Nine for nine.** Every shipped `coreutils` binary examined so far against a
larger standalone twin has had a silent-wrong-behaviour bug. This is the most
complete one: previous entries were tools that got some cases wrong, whereas
this one had no working path in either direction while reporting success.
