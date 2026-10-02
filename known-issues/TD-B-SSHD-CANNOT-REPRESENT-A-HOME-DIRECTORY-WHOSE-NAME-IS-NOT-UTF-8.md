## TD-B-SSHD-CANNOT-REPRESENT-A-HOME-DIRECTORY-WHOSE-NAME-IS-NOT-UTF-8 (lane B, 2026-09-05)

**In short:** on this OS a directory name may hold any byte except `/` and NUL.
`sshd` reads `/etc/passwd` as text, replacing any byte that is not valid Unicode
with U+FFFD (the "replacement character" a terminal draws as a black diamond
with a question mark in it). It does not fail; it produces a *different name*,
three bytes long where the original was one. So an account whose home
directory has an odd byte in it gets its `authorized_keys` looked for in a
directory that does not exist, and **public key authentication silently stops
working for that user** — they are asked for a password instead, with nothing in
the log to say why. If the account has no password, they are locked out of the
machine.

Nothing here is remotely triggerable: `/etc/passwd` is written by root. But a
home directory acquires an odd byte the ordinary way — a name typed in a
non-UTF-8 locale, a directory restored from a backup taken on another system, a
`useradd` driven from a script — and the failure it produces looks nothing like
its cause.

### Where it lives

`userspace/sshd/src/lib.rs`:

```rust
fn lookup_passwd(username: &str) -> Option<PasswdEntry> {
    let data = fs_read_file(Path::new("/etc/passwd")).ok()?;
    let content = String::from_utf8_lossy(&data);   // <-- here
    ...
}

struct PasswdEntry {
    username: String,
    uid: u32,
    gid: u32,
    home: String,      // <-- and here
    shell: String,
}
```

The lossy string then reaches three places that each turn it back into
something the kernel is asked to act on:

| Site | What it does with `home`/`shell` |
|---|---|
| `authorized_keys_path` | joins `home` with the expanded `AuthorizedKeysFile` pattern and opens the result — this is the one that breaks publickey auth |
| `expand_path_tokens` | substitutes `%h` (home) and `%u` (user name) into that pattern |
| the session-start path | `chdir`s to `home` and `exec`s `shell` |

Note that `parse_passwd` itself takes `&str` and splits on `:`, so making this
right is not a one-line change at the `from_utf8_lossy`: the parser, the record,
the two path helpers and the session-start path all move together.

### Why it was left when the rest of sshd's paths were fixed

`TD-B-SSHD-DIES-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE` (above)
converted the command line, the configuration file and the host key path to
`OsString`/`PathBuf` on 2026-09-05. `SshdConfig::authorized_keys_file` was
deliberately *not* converted in that work, and this entry is why: it is only
ever used by being expanded against `PasswdEntry::home` and `::username`. Making
it a `PathBuf` while those are `String` would move the lossy conversion one call
later — from the config parser into `authorized_keys_path` — without removing
it, and would leave the code looking as though the problem were solved.

### The proper fix

`PasswdEntry::{home, shell}` become `PathBuf` and `username` an `OsString`;
`parse_passwd` takes `&[u8]` and splits on `b':'` (the same shape
`SshdConfig::parse` was given in `76d60980d`, and for the same reason);
`expand_path_tokens` builds an `OsString` by pushing byte slices;
`authorized_keys_path` returns a `PathBuf`; `SshdConfig::authorized_keys_file`
becomes a `PathBuf` at the same time. `uid`/`gid` are already numbers and are
unaffected.

Two constraints the conversion must respect:

* **`expand_path_tokens`'s single-pass property is the security property**, not
  a performance one: replacement text is pushed and never re-read, so a `%h`
  that arrives *inside a user name* — which at that point in the connection is
  a string the unauthenticated peer chose — is not expanded. A byte-oriented
  rewrite must keep that shape; scanning for `%` in the *output* would
  reintroduce exactly the hole the current code is written to avoid.

* **The comparison in `authorized_keys_path` must stay `Path::has_root()`**,
  not `starts_with('/')` and not `is_absolute()`. On the target the three are
  the same predicate; on the Windows development host they diverge, and
  `has_root` is the only one under which the tests exercise the target's rule.
  The existing comment says so — keep it.

A user name that is not UTF-8 also needs a decision at the protocol edge: SSH
carries the user name as bytes on the wire, and `handle_userauth_request`
currently does `String::from_utf8_lossy` on it before anything compares it to
`/etc/passwd`. Two lossy names that differ in the original bytes compare
*equal* after that conversion, which is an authentication question rather than
a path question. Convert the wire name to `OsString` in the same pass so the
comparison is over bytes.

### How it was found

Bounding the scope of the `OsString` conversion above: `authorized_keys_file`
was the one path-shaped field in `SshdConfig` that could not be converted with
the others, and following why led here.
