### TD-OILS-EXTERNAL-ARGV0-IS-THE-FULL-PATH. an external command sees its resolved path as `argv[0]`, not the word that named it — OPEN — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — the external-spawn path passes the
resolved executable path as the program *and* as `argv[0]`.

**Reproduce:**

```sh
cat nosuchfile
```

bash: `cat: nosuchfile: No such file or directory`.
osh: `/usr/bin/cat: nosuchfile: No such file or directory`.

**Impact.** Every diagnostic printed by every external tool is prefixed with an
absolute path instead of the command name, so the output of a script that runs
`grep`, `sed`, `cat`, … does not match bash's byte-for-byte. It also breaks the
multi-call convention (`busybox`-style binaries and `bash`-as-`sh` decide their
personality from `argv[0]`).

**Only the Windows host.** On Unix the split is already made:
`external_command` calls `std::os::unix::process::CommandExt::arg0` with the
word as typed, so the child is told the name the user wrote. Everything below
is about the host `osh` is developed on, not about the target it is for.

**Confirmed 2026-08-03 — the child really does read what we write.** A child
spawned from a native (non-Cygwin) parent reconstructs its `argv` from
`GetCommandLineW`, so the first token of the command line *is* its `argv[0]`:

```
$ bash -c 'cat nosuchfile; bash -c "echo 0=\$0"'
cat: nosuchfile: No such file or directory
0=bash
$ osh  -c 'cat nosuchfile; bash -c "echo 0=\$0"'
/usr/bin/cat: nosuchfile: No such file or directory
0=/usr/bin/bash
```

So this is not a host limitation to be written off — the command line is ours
to write. What is missing is a way to say it through `std::process::Command`.

**Why `Command` cannot express it.** `sys::process::windows::spawn` derives
*both* halves from the single `program` field:
`resolve_exe(&self.program, …)` becomes `lpApplicationName` (the file that is
actually run) while `make_command_line(&self.program, &self.args, …)` puts that
same string at the head of the command line (the parameter is even named
`argv0`). There is no `arg0` on the Windows `CommandExt` — only `raw_arg`,
which appends *after* the program. Naming the short word instead would hand
resolution back to `resolve_exe`, whose search order (child `PATH`, then parent
`PATH`, with its own `.exe` rules) is not the shell's and would ignore the
`hash` cache — i.e. the shell would stop running the file it resolved.

**Proper fix.** Call `CreateProcessW` directly on Windows, with
`lpApplicationName` = the resolved file and a command line built by
[`crate::wincmd`] whose first token is the word as typed. That means the
Windows spawn path can no longer be `std::process::Command`, and `Stdio` goes
with it: the ~40 `cmd.stdin/stdout/stderr(…)` sites in the redirect code
(around `interp.rs:14148`–`14585`) would have to hand over raw handles, and
`Stdio::piped()` callers would need pipes of their own. That is the whole
Windows child-process plumbing, for a fidelity gain on the development host
only — which is why it has not been done, not because it cannot be.
