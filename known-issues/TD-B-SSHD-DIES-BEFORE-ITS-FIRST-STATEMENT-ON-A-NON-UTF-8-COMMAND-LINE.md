## TD-B-SSHD-DIES-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE (lane B, 2026-09-05)

**In short:** on this OS a filename may hold any byte except `/` and NUL — that
is written down in `design.txt`, not an accident. `sshd` reads its command line
as text that must be valid Unicode, so `sshd -f /etc/ssh/conf-<0x80>` does not
report a bad filename: it aborts with a Rust panic message before running a
single line this repository wrote. The same goes for `-h <hostkey>`. The daemon
does not start, and the panic text says nothing about which argument caused it.

Nobody can trigger this remotely — argv comes from an init script written by
root, not from the network — so it is a robustness defect, not a
vulnerability. But an init script is exactly where an odd byte goes unnoticed
for months, and a daemon that fails to boot with a panic backtrace is the worst
available way to report a bad path.

### Where it lives

`userspace/sshd/src/lib.rs`:

```rust
fn parse_args() -> Result<Self, i32> {
    Self::parse_from(env::args().skip(1))     // <-- panics on a non-UTF-8 argument
}
```

`std::env::args()`'s iterator is documented to panic on an argument that is not
valid Unicode; its body is a literal `unwrap`. This is the same defect that
`scripts/argv-utf8.py` gates for the 84 shipped coreutils, and for the same
reason — but that gate's stated scope is `userspace/coreutils/`, so `sshd` is
outside it and nothing catches this.

### Why it is not simply `args_os()`

The path does not stop at the parser. `CliOptions::config_file` and
`host_key_file` are `String`, and so is `SshdConfig::host_key_file`, which is
*also* filled from the config file's own text — so a host key path can arrive by
two routes and both are UTF-8-only. Fixing only `parse_from` would move the
panic to the first `fs_read_file` and change nothing a user sees.

### The proper fix

Carry the two paths as `OsString`/`PathBuf` from `parse_from` through
`CliOptions`, `SshdConfig` and the config parser to `fs_read_file` /
`HostKey::load_from_file`, which is the shape `coreutils::getopt` already uses
for the utilities that are clean. Then extend `scripts/argv-utf8.py`'s scope, or
add sshd to it explicitly, so the next daemon does not reintroduce it.

### How it was found

Opening the `#[expect(clippy::indexing_slicing, clippy::arithmetic_side_effects)]`
on the argument parser during the sshd panic-lint audit. The suppression was
covering `args[i]` and `i += 1`; the `String` in the line above them was not
what the lint was pointing at, and is the larger defect of the two.

### Fixed 2026-09-05 — and the second route was the worse of the two

Done as prescribed, in three commits, working inwards from `open` so that each
one compiled and tested on its own:

| Commit | Layer |
|---|---|
| `20f8f07fa` | `fs_read_file`, `fs_write_private_file`, `fs_set_mode`, `HostKey::{load_from_file, generate_and_persist}` and the OpenSSH writers take `&Path`. Diagnostics name the file through `Path::display`, so the *message* is lossy and the bytes handed to `open` are not. |
| `76d60980d` | `SshdConfig::{host_key_file, banner_file}` are `PathBuf`; `SshdConfig::parse` takes `&[u8]` and splits lines itself. New dependency on `userspace/quoting` for `os_from_bytes` rather than a fourth private copy of the `#[cfg(unix)]` bytes↔`OsString` dance. |
| `2895bdce6` | `parse_args` uses `env::args_os()`; `parse_from` takes `OsString`; `CliOptions::{config_file, host_key_file}` are paths. |

**The `SshdConfig` route was not merely a second way in — it was the one that
did real damage, and it did it with no argv involved at all.** `run_cli` read
the configuration file through `String::from_utf8_lossy`, so a `HostKey` line
naming a file whose name held byte 0x80 reached the opener with U+FFFD in it,
and opened nothing. What follows is the whole point: an unreadable host key
path is *deliberately* treated as a first start, so that a fresh machine comes
up with a key. So the daemon generated a **new host key**, and every client
reported that the host identity had changed — the exact warning host key
verification exists to raise, produced by the daemon itself, on a machine
nobody had touched. A conversion three functions away defeated a policy the
code states in its own comment.

Two things fell out of the conversion that were not in the plan:

* **A non-UTF-8 value on a directive that is *not* a file name is now refused,
  naming the directive**, where the lossy read substituted U+FFFD and carried
  on. `AllowUsers al<0xff>ice` used to become a pattern matching no account,
  silently. A daemon that will not start is strictly better than one that
  quietly means something the administrator did not write.

* **`-f` now records that a configuration file was *asked for*, not which one**
  (`ba0f3be56`; `CliOptions::config_file` is an `Option<PathBuf>`). `run_cli`
  refuses to start when a named config cannot be read but falls back to
  built-in settings when none was named, and it told the two apart by comparing
  the path against the default's *spelling*. That was wrong in both directions:
  `-f /etc/ssh/./sshd_config` was refused where omitting `-f` would have
  worked, and `-f /etc/ssh/sshd_config` on a machine with no such file started
  on built-in settings and said nothing — the administrator asked for a
  configuration by name, got none, and was not told. Which half a deployment
  hit depended on how its init script happened to spell a path.

Ten tests, of which six pin byte-exactness under `#[cfg(unix)]`, where an
`OsStr` *is* its bytes. None of them can call `env::args()`, so none can
reproduce the original panic directly; the guard against its return is the item
type, since `parse_from` taking `OsString` means `parse_args` cannot be written
with `env::args()` without a type error.

**Not done, and deliberately so:** `SshdConfig::authorized_keys_file` is still
a `String`. It is expanded against `PasswdEntry::{username, home}`, which come
from `/etc/passwd` read through `String::from_utf8_lossy` — converting this one
field alone would move the conversion one call later rather than remove it. See
`TD-B-SSHD-CANNOT-REPRESENT-A-HOME-DIRECTORY-WHOSE-NAME-IS-NOT-UTF-8` below.
**The `scripts/argv-utf8.py` scope extension called for above is done.** The
gate no longer covers `userspace/coreutils` and nothing else; it covers every
crate under `userspace/` that does not *declare itself* unimplemented, and the
declaration is a dependency on `userspace/notimpl` — something a crate says,
not something the script infers about it. Inference was tried and does not
work: `abiword-cli` prints canned text and declares no dependencies at all,
while `getty`, `telnet` and `dnsmasq` declare none either and are real
programs, so no property of a manifest separates them.

474 of the 2760 crates there are now in scope, against the one the old rule
could see. The number that matters for this entry is what that turned up:
**464 findings in 450 crates**, where the old scope reported 4. `sudo`, `su`,
`login`, `doas`, `passwd`, `useradd`, `chpasswd`, `getty`, `ftpd`, `ftp`,
`sftp`, `scp`, `ssh`, `ssh-keygen`, `syslogd`, `crond`, `logind`, `inetd`,
`telnet`, `ntpd`, `dhcpcd`, `dnsmasq`, `chroot`, `firejail`, `unshare`,
`nsenter`, `capsh`, `newgrp` and `chage` all have the defect `sshd` had —
every one of them a program that dies before its first statement on an
argument holding a byte that is legal in a filename here. They are recorded in
`scripts/argv-utf8-baseline.txt`, which is a ratchet and only shrinks; the
next `sshd` cannot be added silently, which is what this follow-up was for.

**`sudo` and `su` are done (2026-09-06), leaving 27 of that list; the baseline
is at 461.** The list above is left as it was measured, because it is the record of
what the scope extension found; the live count is whatever
`python scripts/argv-utf8.py --check` prints. The `sudo` conversion is written
up under
`TD-B-SUDO-DIED-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE` below,
and it is the one to read before starting any of the other 27: it turned up
five unrelated defects, three of them exploitable by an unprivileged local
user, including an audit log that could be forged from the *working directory*
by a user whose sudo access was being denied.

`su` followed the same day and found five more (see
`TD-B-SU-DIED-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE` below).
The pattern is now established well enough to state as a prediction rather
than a surprise: **the conversion is not the value; walking every line that
touches a command-line string is.** Both programs' worst defects were
injection into a line-oriented file or terminal, by a value that reached it
verbatim — and in both cases the line in question was the one the system keeps
in order to say who did what. Expect the same in `login`, `passwd` and
`chage`, all of which write records of exactly that shape.

Roughly half of the 450 are crates that print canned output and never said so
— they import nothing that could touch a file, a socket or a subprocess. The
fix for those is the same one line as their 2286 siblings,
`notimpl::guard(env!("CARGO_PKG_NAME"))`, which also fixes the panic: the
guard runs before `env::args()` is ever called. The rest are real work, and
are the backlog this baseline exists to count.
