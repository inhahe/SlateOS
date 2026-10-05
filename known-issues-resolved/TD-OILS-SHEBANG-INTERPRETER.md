### TD-OILS-SHEBANG-INTERPRETER. osh does not interpret a `#!` line itself where the OS will not — 2026-08-02 — ✅ **RESOLVED 2026-08-02**

**Resolution.** `shell_script_indirection` now answers with an `Interposed`
rather than a bare path: either `Shell(own_binary)` for a shebang-less text file
as before, or `Interpreter { program, arg }` for a `#!` line — the latter only
where `!cfg!(unix)`, which is bash's own `#if !defined (HAVE_HASH_BANG_EXEC)`
condition. The SlateOS target is `target-family: ["unix"]` and its
`posix/src/linux_binfmt.rs` knows `SCRIPT_MAG`, so the OS honours the line there
and the shell stays out of it; only the Windows development host needs the
interposition. (The premise recorded below — that this "fixes the SlateOS
target" — was wrong, and checking it is what scoped the fix.)

The line is split the way a *kernel* splits one (`shebang`, following Linux's
`fs/binfmt_script.c`): truncate at the first newline, trim spaces and tabs from
both ends, take the first blank-delimited word as the interpreter and hand the
entire remainder over as **one** argument. bash's own fallback splits into words
and keeps only the first — the older BSD reading, and not what anything osh runs
on actually does, so the kernel's rule is the one followed. Two consequences fall
out and are tested: a `#!` line cannot name a path with a space in it, and a CRLF
file's `\r` stays on the last word, so such a script names an interpreter that
does not exist.

A `#!` that names *no* interpreter (`#!` alone, or nothing but blanks) is
`ENOEXEC` on a kernel that reads the line — which is a fall-through, not a
refusal — so osh reads the file as shell source, exactly as for a file with no
line at all.

The interpreter is resolved against the shell's working directory and never
searched for on `$PATH`, because that is what the kernel does; the resolution has
to happen in the shell because Windows' `CreateProcess` resolves a relative
application name against the *calling* process's directory rather than the
child's. The diagnostic keeps the spelling from the `#!` line regardless.

The diagnostics needed a distinction osh did not previously carry. bash has two
error printers with different prologues: `report_error` prints the shell name
*and* `line N:`, while `sys_error` — which exists to append `strerror(errno)` —
prints only the shell name. The `bad interpreter` line comes from the second, so
it is the one command diagnostic with no line number in it. `spawn_error_message`
therefore returns `Vec<SpawnDiag>` (`Line` / `Bare`) instead of `Vec<Str>`, and
`Shell::spawn_diag` puts the right prologue on each. The wordings, all measured:

| what happened | line | status |
|---|---|---|
| interpreter is not there | `SCRIPT: cannot execute: required file not found` (`Line`) | 127 |
| interpreter will not run | `SCRIPT: INTERP: bad interpreter: REASON` (`Bare`) | 126 |
| …under `exec`, second line | `SCRIPT: No error` (`Line`) | 126 |

That last one is bash reporting an `errno` it has already cleared. It is
reproduced anyway: `strerror(0)` is not the same string on every libc, but this
whole reading is only reachable on the one host where the OS does not honour a
`#!` line, so that host's wording is the only one there is to match.

Covered by `tests/corpus/shebang-interpreter-line.sh` (byte-identical to bash
5.2.37 — argument placement, the single-argument rule, blank trimming, the
space-in-the-interpreter rule, both failure wordings, and all of plain command /
pipeline stage / `&` job / `exec` / script-starts-script) plus
`a_shebang_line_splits_the_way_a_kernel_splits_one`,
`a_shebang_that_will_not_start_is_blamed_on_the_interpreter` and the extended
`only_a_shebangless_text_file_is_run_by_this_shell` in `interp.rs`. Full suite
green (1158 + 4 + 39 + 7), clippy clean, corpus sweep 265 matched / 0 failed.

The corpus case interposes a *copy* of the real `printf` under a name of its
own: the two shells spell the one on `$PATH` differently, it lives under a path
with a space in it, and `printf`'s output is its arguments and nothing else — no
`argv[0]` of its own leaks in. That last point matters because the `argv[0]` an
interposed interpreter receives is the host's business: `std` has no `arg0`
override on Windows, so osh passes the resolved path where a kernel would pass
the `#!` line's spelling. Only the interpreter's *own* diagnostics can see the
difference.

**What it was, below.**

**Where:** `userspace/oils/src/interp.rs` — `shell_script_indirection`, which
deliberately returns `None` for a file beginning `#!` and leaves it to the OS.

**What:** bash carries the `#if !defined (HAVE_HASH_BANG_EXEC)` half of
`execute_shell_script`: where the kernel does not honour `#!`, bash reads the
line itself, splits it into an interpreter and at most one argument, and execs
that with the script appended. osh leaves the file to the OS, which on the
Windows *development* host refuses it — so a `#!/bin/sh` script that runs fine
under host bash (the MSYS runtime emulates `#!`) reports
`cannot execute binary file: Exec format error` under osh.

**Proper fix.** Parse the `#!` line and spawn the interpreter. Note the
diagnostics that come with it: a missing interpreter is
`SCRIPT: cannot execute: required file not found` with status **127** (not 126),
and an interpreter that is itself unrunnable keeps its own error.

**Interacts with:** on the Windows host the interpreter named is almost always a
POSIX path (`/bin/sh`, `/usr/bin/env`) that does not exist there, and osh has no
MSYS-root translation — `osh -c '/bin/echo hi'` fails the same way from *any*
context. So implementing this fixes the SlateOS target and a Unix host but
cannot be diffed against host bash, and the corpus case therefore excludes `#!`
files. Do not paper over it with a basename `$PATH` fallback: bash does not do
that, and a `#!python3` file run as *shell* source would be worse than failing.
