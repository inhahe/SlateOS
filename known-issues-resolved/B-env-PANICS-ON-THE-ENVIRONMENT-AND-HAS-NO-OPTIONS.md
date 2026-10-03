## B-env-PANICS-ON-THE-ENVIRONMENT-AND-HAS-NO-OPTIONS (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** Two things. First, `env` — the program whose whole job is to
carry environment variables around — *crashed* if any variable in the
environment was not valid text. Our OS lets a variable, like a filename, hold
any byte at all, so this is reachable, and what the user saw was a Rust crash
message instead of their environment. Second, `env` had no options whatsoever.
`env -i somecommand`, the standard way to run a program with a clean
environment (what a build script or a security boundary reaches for), was read
as "run the program called `-i`" and failed.

Fixed: `userspace/coreutils/src/bin/env.rs`, rewritten. 11 → 29 tests.

### The panic is one `unwrap` in std, and 54 utilities inherit it

`std::env::args()` is not a safe reading of the command line. Its iterator, in
`library/std/src/env.rs`:

```rust
fn next(&mut self) -> Option<String> {
    self.inner.next().map(|s| s.into_string().unwrap())
}
```

A literal `unwrap`, documented as such: *"The returned iterator will panic
during iteration if any argument to the process is not valid Unicode."*
`env::vars()` is the same. So the first line of the old `env`,

```rust
let args: Vec<String> = env::args().skip(1).collect();
```

panics *during `collect`* — before a single line of the program's own logic
runs — if any argument holds a byte sequence that is not UTF-8. And its print
loop, `for (key, value) in env::vars()`, panics if any *variable* does.

This is CLAUDE.md's rule 7 ("never force UTF-8 on filesystem paths,
environment variables, or pipe data") violated at the point where it costs the
most. `env` now uses `args_os` and `vars_os`, and is `OsString`/`&[u8]` end to end,
with the `NAME=VALUE` split done on bytes so a name or a value may be anything
the OS allows.

**This is not just `env`.** 52 of the 84 shipped `coreutils` bins opened with
`env::args()`, including every one that takes a filename: `cp`, `mv`, `rm`,
`ls`, `ln`, `mkdir`, `rmdir`, `touch`, `find`, `grep`, `du`, `df`, `readlink`,
`realpath`, `basename`, `dirname`, `stat`, `chmod`, `chown`, `tar`, `xargs`.
On an OS whose paths are byte strings, `rm <name-with-a-non-UTF-8-byte>`
panics before it does anything. Tracked separately as
**`B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`** below, because it is a sweep
rather than one fix.

### The options

POSIX defines exactly two for `env`, and this had neither:

| Typed | Old behaviour | Correct |
|---|---|---|
| `env -i prog` | runs a program named `-i`; `No such file or directory`, exit 127 | `prog` with an empty environment |
| `env -u FOO prog` | runs a program named `-u` | `prog` without `FOO` |
| `env -- -i` | runs a program named `--` | runs the program named `-i` |

These failed loudly rather than quietly, which is the one mercy — but `env -i`
not existing is a larger hole than any single wrong answer, since the whole
reason to reach for it is to establish a known-clean environment, and a script
that believes it did but did not is a security problem rather than a bug.

Added: `-i`/`--ignore-environment` (and bare `-`, GNU's historical synonym),
`-u`/`--unset`, `-0`/`--null`, `-C`/`--chdir`, `--`, short-option bundling
(`-i0`, `-iuFOO`), and GNU's rule that option parsing stops at the first
operand so `env FOO=1 prog -i` passes `-i` to `prog`.

### Three more

1. **Exit status 127 for everything.** GNU distinguishes 127 (no such command)
   from 126 (found it, cannot run it — permissions, or not an executable
   format), and scripts test for them. The old code returned 127 for both, so
   a non-executable file was reported as a missing one. `env`'s *own* failures
   are 125, a third number, which is why the distinction needs one.

2. **A signalled child reported `1`.** `status.code().unwrap_or(1)` — so a
   command killed by SIGKILL looked like one that returned failure. The shell
   convention is `128 + signal`; `env` must not change the answer merely by
   standing in front of the command. Now 137 for SIGKILL, 143 for SIGTERM.

3. **The two paths built the environment differently.** The print path applied
   assignments with `unsafe { env::set_var }` and mutated the process's own
   environment to do it; the exec path used `Command::env`. Two implementations
   of one question — what does `env -u FOO FOO=bar` leave `FOO` as? — is two
   chances to answer it differently. There is now one `effective_env` that both
   call, and the `unsafe` block is gone.

Also: `println!` panics on a write error, so `env | head -1` produced a panic
message. Checked writes, `BrokenPipe` the one deliberate success — the same
family as `tee`, `tar`, `dd`, `stat` and `kill`.

**`-S`/`--split-string`: implemented 2026-09-14**, which took `env-diff.sh` to
59 passed / 0 differed. The grammar was measured first (58 cases,
`scripts/probe-env-split-string.sh` plus four companion probes) rather than
written from the documentation, and three rules would have been wrong
otherwise: `\t` puts a tab *inside* an argument where a raw tab separates;
only `${NAME}` expands, never re-splitting; and an unset variable contributes
no argument where a set-but-empty one contributes an empty one. `\a`, `\b` and
`\e` are rejected by GNU despite being standard C escapes.

The `todo.txt` design for it was wrong on its central point — it held that
`${VAR}` must expand against the environment `env` is building, and GNU
expands against the *process* environment, ignoring `-i`, `-u` and assignments
alike.

**Eight for eight.**
