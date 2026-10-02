## TD-B-SUDO-DIED-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE (lane B, 2026-09-06) — FIXED

**In short:** `sudo` — the program that hands out root — read its command line
with a Rust function that *crashes* when an argument contains a byte that is
not valid text. On this OS a file name may hold every byte except `/` and NUL,
so `sudo rm <a file with an odd byte in its name>` did not refuse, did not log,
and did not reach a single line of sudo's own code: it died with a Rust panic
message before `main`'s first statement. It read the *environment* the same
way, so a single odd byte in any exported variable killed it too — no argument
needed. Converted to carry argv and the environment as bytes end to end. The
conversion uncovered five further defects that have nothing to do with text
encoding, **three of them security bugs**, and those are the part of this entry
worth reading.

**Where it lived:** `userspace/sudo/src/main.rs`, two lines:

```rust
let args: Vec<String> = env::args().collect();   // argv
for (key, val) in std::env::vars() { ... }       // the environment
```

Both were baseline entries in `scripts/argv-utf8-baseline.txt`
(`userspace/sudo/src/main.rs:argv-as-string` and `:env-as-string`), which is why
this was picked up first out of the 29 privileged programs the gate's scope
extension found — see
`TD-B-SSHD-DIES-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE` above.
The baseline is a ratchet and shrank 464 → 462.

### The five defects the conversion found

None of them is about UTF-8. They are what the conversion walked past.

**1. The audit log could be forged, by three independent routes — and one of
them needed no odd bytes at all.** The log is one record per line, fields
separated by ` ; `, ending `RESULT=ALLOWED` or `RESULT=NOT_ALLOWED`. Three
fields were written into that line verbatim and each could carry a newline:

| Field | How a caller sets it |
|---|---|
| `COMMAND=` | argv — chosen outright |
| `TTY=` | `$TTY`, a plain environment variable; nothing validated it |
| `PWD=` | the working directory — one `mkdir` of a name containing a newline and one `cd` |

So anyone who could run `sudo` at all — including someone whose every attempt
was *denied* — could append a fabricated `RESULT=ALLOWED` record naming another
user, into the very file whose only purpose is to say who ran what. The `PWD=`
route is the notable one: it is reachable with an ordinary command line
containing only printable characters, because the newline lives in a directory
name rather than in anything sudo was passed. Path names here may hold every
byte but `/` and NUL, so that directory is legal, not malformed.

Fixed by putting all five variable fields through `escape_os`, whose output is
valid UTF-8 by construction — a newline becomes a two-character backslash-`n`
escape, a byte that is no part of a character becomes three octal digits — so a
record is text no matter what went into it, and a field that was already plain
comes through unchanged. `username` and `target_user` are escaped too; they
come from the user database rather than from argv, but "a step further from the
caller" is not a property worth relying on in an audit log.

The record formatting was split out of `log_command` into `format_log_record`
purely so the property could be *tested*: writing the log needs a root-owned
`/var/log`, which no test has, and a security property checkable only by
reading the source is one that comes back. Three tests now pin it, including
one asserting that an ordinary record is byte-for-byte unchanged by the
escaping.

**2. `sudoedit` could copy your edit back over the wrong file.** It built its
temporary path as

```rust
original_path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
```

so *every* basename that is not valid UTF-8 collapsed onto the single path
`/tmp/sudoedit-<pid>-file`. Editing two such files in one invocation therefore
gave both of them one temp file, and the copy-back wrote the second edit over
the first original. A wrong-file write is the worst outcome an editor wrapper
can have, and it needed only a file name nobody chose to type. The basename is
now carried across as an `OsStr`.

**3. `sudoreplay -l` silently omitted recordings.** `list_sessions` obtained
the session id with `file_name().and_then(|n| n.to_str())` and `continue`d on
`None`. A session directory whose name is not UTF-8 therefore did not fail to
replay — it failed to *appear*, and the listing showed no sign that anything
had been left out. The recording existed on disk the whole time.
`SessionEntry::id` is an `OsString` now, and the listing renders it through
`escape_os` because it is a fixed-width table: a session name containing a tab
or a newline would otherwise rewrite the rows below it.

**4. A non-UTF-8 `EDITOR` was silently ignored, and `$TTY` silently became
`unknown`.** Both used `env::var` in a fallback chain:

```rust
env::var("SUDO_EDITOR").or_else(|_| env::var("VISUAL")).or_else(|_| env::var("EDITOR"))
```

`env::var` reports a non-UTF-8 value as `Err(NotUnicode)`, which an `or_else`
chain **cannot tell apart from "unset"**. So an `EDITOR` that was set, and
valid, and pointed at a real program fell through to the built-in default, and
the user's `sudoedit` opened in a different editor than every other command
they ran, with nothing said. The `$TTY` instance of the same shape wrote
`TTY=unknown` into the audit log for a terminal that had a perfectly good name.
Both now use `env::var_os`, where "unset" is the only `None`.

**5. The editor was handed a path that did not exist.**
`.arg(temp_path.display().to_string())` — `Path::display` substitutes U+FFFD for
every byte it cannot decode, so whenever the original's name was not UTF-8 the
editor was launched on a different path from the one `sudoedit` had just
written. It is now `.arg(&temp_path)`, which passes the bytes.

### Three duplications removed as part of the fix, not around it

* `current_pwd()` — the expression
  `current_dir().map(|p| p.display().to_string()).unwrap_or_else(...)` appeared
  at **six** call sites, spelled out by hand at each. Six copies of a lossy
  conversion is six places to fix it and six chances to miss one.
* `editor_command()` — `sudoedit` and `visudo` had a copy each of the
  `SUDO_EDITOR` → `VISUAL` → `EDITOR` → default order. Two copies of a
  preference order is a way to end up with two preference orders.
* `format_log_record()` — split out for the testability reason under defect 1.

### One behavioural change that is not a bug fix

The `env_check` scan — which drops a preserved variable whose value contains
`/` or `%` — now runs over the raw bytes instead of a decoded `String`. `/` and
`%` are single bytes in UTF-8 and can be no part of a multi-byte character, so
the byte scan finds exactly what the string scan found; it additionally works
on values that could not be decoded, which are precisely the ones a caller
would try to smuggle something through in. Before the conversion those values
did not reach the scan at all — `env::vars()` panicked first.

### Testing

The new tests are in two blocks. **"argv is bytes"** covers what could not
previously be written down at all: arguments that are not valid text, which is
impossible through `&str` and impossible to produce on the Windows development
host through a real process spawn. They pin that a command survives parsing
unchanged, that two commands differing around a non-text unit do not collapse
together, that everything after `--` is taken as command even when it is not
text, that a value-taking flag refuses an argument that is not text, that
`visudo`'s file and `sudoreplay`'s directory and session are carried through,
and that the personality is chosen from a basename that need not be text.

**A `#[cfg(unix)]` fixture would have run nowhere that matters, and the first
draft of these tests found that out the hard way.** They were written on
`quoting::os_from_bytes`, which is byte-exact under `#[cfg(unix)]` — where an
`OsStr` *is* its bytes — and goes through `String::from_utf8_lossy` off unix.
So on the Windows host `b"tool\xff"` arrived as `tool\u{fffd}`: valid text, and
precisely the substitution the tests exist to rule out. All seven failed on
exactly that, which is the good outcome; had the assertions been one degree
weaker they would have passed while testing the host's lossy conversion against
itself. Gating them `#[cfg(unix)]` would have been the easy answer and the
wrong one — Windows is where this code is actually built and run, so a case
gated out there is a case that runs nowhere.

The fix is a `not_text(prefix, suffix)` fixture that asks each platform for the
cheapest thing its `OsString` can hold that `to_str()` refuses: byte `0xff` on
unix, and **an unpaired surrogate `U+D800` on Windows**, which `OsString` stores
happily because it is WTF-8 there. The fixture asserts its own result is not
valid text, so a platform where that stopped being true would fail loudly
rather than quietly assert nothing. The byte-exact assertions are kept as
*additional* `#[cfg(unix)]` tests alongside the platform-free ones, because
byte-exactness is the property that matters on the target, where the command is
handed to `exec` as bytes and one substituted byte is a different program.
**This technique is the thing to copy into the other 28 conversions**; every
`OsString` conversion in this tree written before it has its non-text coverage
confined to unix and therefore, in practice, to nowhere. The size of that hole
was measured the same day and has its own entry:
`TD-B-CFG-UNIX-GATED-TESTS-RUN-NOWHERE` — **198 `#[cfg(unix)]` tests across 40
files**, none of which have ever executed on the development host.

**"The audit log is one record per line, and stays that way"** (three tests) is
the guard on defect 1: a forged record attempted through the command field, the
same attempted through `$TTY` and through the working directory, and the
byte-for-byte control proving an ordinary record is undisturbed.

`detect_personality` was made generic over `AsRef<OsStr>` rather than taking
`&OsStr`, so that its existing call sites read `detect_personality("visudo")`
and not `detect_personality(OsStr::new("visudo"))`. The ceremony is what stops
cases being added.

`cargo test -p sudo` finishes at **253 passed, 0 failed** (was 234 before this
work), with three of the additions kept deliberately `#[cfg(unix)]` as the
byte-exact controls described above.

### Why it survived

The same reason as every other entry in this audit: the development host is
Windows, where argv arrives as UTF-16 and no test can hand a process an invalid
argument; and `cargo test` never runs the binary at all, only its internal
functions. `main` is the least-tested line in the file. **This is the pattern
holding for the seventh consecutive conversion** — three unrelated bugs in
`rm`, four in `mv`, six in `cp`, four in `ln`, four in `mkdir`, nine in `ed`,
and now five in `sudo`, none of them about UTF-8. What makes `sudo` the
sharpest instance is *which* bugs: three of the five are exploitable by an
unprivileged local user, and the log-forging one is exploitable by a user whose
sudo access is being *denied* — the case the log exists to record.

### Still to do — the other 28

`su`, `login`, `doas`, `passwd`, `useradd`, `chpasswd`, `getty`, `newgrp`,
`chage`, `chroot`, `firejail`, `unshare`, `nsenter`, `capsh` and the network
daemons all have the defect `sudo` had, and are recorded in
`scripts/argv-utf8-baseline.txt`. `sudo`'s five defects are the argument for
converting each of them properly rather than swapping `env::args()` for
`env::args_os()` and moving on: the panic is only the part of an untested
`main` that a checker can see.
