## TD-B-SU-DIED-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE (lane B, 2026-09-06) — FIXED

**In short:** `su` — the program you use to become root — read its command line
with a Rust function that *crashes* when an argument holds a byte that is not
valid text. On this OS a file name may hold every byte except `/` and NUL, so
`su -s /bin/<a shell whose name has an odd byte> alice` did not refuse and did
not report anything: it died with a Rust panic before `run_su` executed a
single statement. Converted to carry argv and the environment as bytes end to
end. As with `sudo` the day before, the conversion is not the interesting part
— **five further defects came out of walking the lines it touched, and the
worst of them let any local user forge a login session record.**

**Where it lived:** `userspace/su/src/main.rs`, one line —

```rust
let args: Vec<String> = env::args().collect();
```

— the baseline entry `userspace/su/src/main.rs:argv-as-string` in
`scripts/argv-utf8-baseline.txt`, which is a ratchet and shrank 462 → 461.

### The five defects the conversion found

**1. `who` and `w` could be made to report a login session that does not
exist.** On a login switch, `su` writes `/run/sessions/<pid>` as a
`key=value\n` record:

```
user=alice
tty=pts/0
host=
time=1757...
pid=1234
```

The `tty=` value came from `detect_tty`, which reads the `/proc/self/fd/0`
symlink and strips `/dev/`. **Stdin is the caller's to choose.** A redirection
is an ordinary thing an unprivileged user may write, and a file name here may
contain a newline:

```sh
mkdir -p "/dev/shm/$(printf 'x\nuser=root\nhost=')" ...
su - alice < "/dev/shm/x
user=root"
```

The record then carries a second `user=` line, and every reader of that
directory reports a root session that nobody is in. Nothing in the path
validated the name, because nothing in the path was looking at it as anything
but a string to interpolate.

Fixed by screening the name for control bytes (`< 0x20` and `0x7f`) in
`tty_name_is_plausible`, which is split out of `detect_tty` so that it can be
tested — the real function reads `/proc/self/fd/0`, which a unit test cannot
arrange. **A suspicious name is reported as `?`, not sanitised.** Repairing it
would produce a name that looks real and is not, which is the same bug one
step further along.

**2. The unknown-user message interpolated the name raw.** `eprintln!("su:
unknown user: {}", opts.target_user)` — with the name coming straight from
argv, so

```sh
su "$(printf 'nobody\nsu: switched to root. #')"
```

wrote a convincing second line onto the caller's terminal. That is a phishing
primitive rather than a privilege escalation, but `su` is precisely the program
whose output a user reads to decide whether they are root. Now `quoteaf_os`,
as are the unknown-option and exec-failure messages, which had the same shape.

**3. `TERM` was read with `env::var`.** A terminal name that is not valid text
came back as `Err(NotUnicode)`, indistinguishable from unset, so the login
shell started with no `TERM` at all — no line editing, no arrow keys. `var_os`
now, and the value is handed to the child untouched: this program never needs
to read it as text.

**4. A computed `argv[0]` was built and thrown away.** The interactive branch
of `exec_as_user` computed `-bash` from the shell's basename — the convention
by which a shell decides to read its login profile — and then discarded it,
because `std::process::Command` sets `argv[0]` to the program path and offers
no way to override it. Its own comment conceded this ("best-effort"), which is
how it survived. Deleted rather than left in place: a computation whose result
is dropped reads to the next person as a feature that works, and `su -` not
running the login profile is a real bug that this dead code was hiding. The
convention needs an exec that takes `argv[0]` separately
(`SYS_PROCESS_SPAWN_EX2`); noted in `todo.txt`.

**5. `USER` was read with `env::var` in the caller-uid fallback.** Same
`Err(NotUnicode)`-is-unset confusion as (3). Harmless in outcome here — a
non-text name cannot appear in a YAML database either way — but it was written
as `if let Ok(...)`, which cannot tell the two apart, and that is the shape
that produced the real bug in `sudo`'s `EDITOR` handling.

### What the tests pin

`cargo test -p su` finishes at **43 passed, 0 failed** (was 34 before this
work). All nine additions are portable and were confirmed by name in the run,
which is the point of the paragraph below.

The argument-parsing suite moved from `Vec<String>` to `Vec<OsString>`, and six
non-text cases were added. They use the portable `not_text` fixture — an
unpaired surrogate on Windows, byte `0xff` on unix — rather than the
`#[cfg(unix)]` byte fixture, so that they actually execute on this project's
development host. See `TD-B-CFG-UNIX-GATED-TESTS-RUN-NOWHERE`: 198 tests across
40 files are gated the other way and have never once run here.

The sharpest of the six is
`a_leading_dash_is_an_unknown_option_even_when_the_rest_is_not_text`. Whether
an argument is an option is decided by its **first byte**, not by whether
`to_str()` succeeds. Deciding it the obvious way — falling into the positional
branch whenever the argument is not text — would make a mistyped option
containing a stray byte into a *username*, and because the last positional
wins, it would then **silently discard the real username typed after it** and
start a shell as somebody else. That is the failure this conversion could most
easily have introduced, and it exists only because the fixture runs.

Also pinned: the control-byte screen refuses `\n`, `\r`, NUL and the empty
name while still accepting `tty1`, `pts/0`, `ttyS0`, `console` and a
non-UTF-8 device name (the screen is for control characters, not for
non-UTF-8); and `is_safe_filename` refuses `""`, `.`, `..`, `../etc/passwd`,
`a/b` and an embedded NUL, which guards the `/tmp/.users/<username>` marker
this program writes and deletes as root.
