### TD-OILS-EXEC-SHEBANGLESS-SCRIPT. osh will not run a shebang-less, non-executable file as a shell script the way bash does — 2026-08-02 — ✅ **RESOLVED 2026-08-02**

**Resolution.** osh now decides *before* the spawn rather than after: every
external command whose word the shell resolved to a real file has its first 128
bytes classified by bash's `check_binary_file` rule
(`shell_script_indirection`), and a shebang-less text file is run as an argument
of a fresh copy of this shell — `own_binary()`, i.e. `current_exe`, because bash
re-execs *itself* here and a script picked up this way reports the running
`$BASH_VERSION`.

The decision has to be made before the spawn because `std::process::Command`
cannot be re-aimed at a different program once built, and its stdio plan cannot
be recovered from a failed one. A 128-byte read is noise beside the milliseconds
a process launch costs.

All three external-spawn sites were unified onto one builder,
`Shell::external_command`, so a pipeline stage, a plain command and a `&` job
cannot disagree about what runs. The pipeline stage now resolves through the
shell's own `$PATH` and `hash` cache like the others (it previously handed the
bare word to the OS), and gained the `argv[0]`-as-typed override the others had.

A file the OS *does* refuse now reports bash's wording rather than the host
errno — `WORD: cannot execute binary file: Exec format error`, status 126, with
`exec` adding its own `exec: WORD: cannot execute: Permission denied` second line
(bash overwrites `errno` with EACCES before returning its verdict). `exec`'s
reported path also stopped keeping the `./` of a relative word, which bash's
`sh_makepath (…, MP_RMDOT)` drops.

Covered by `tests/corpus/exec-shebangless-script.sh` (byte-identical to bash
5.2.37) plus `only_a_shebangless_text_file_is_run_by_this_shell` and
`a_file_the_os_will_not_run_is_reported_as_a_binary` in `interp.rs`. Full suite
green (1149 + 4 + 39 + 7 + doctests), clippy clean, corpus sweep 262 matched /
0 failed.

Three divergences the corpus case ran into on the way out were tracked
separately and have since been fixed: TD-OILS-NUL-IN-SOURCE,
TD-OILS-PREFIX-PATH-LOOKUP and TD-OILS-SHEBANG-INTERPRETER. The first of those
restored this case's `latenul.sh` section; the last took the `#!` files it had
excluded into a case of their own.

**Where:** `userspace/oils/src/interp.rs` — the external-command spawn path.
The OS is asked to execute the file directly and its refusal is reported
verbatim; there is no fallback that re-reads the file as shell source.

**What:** when execution of a command file fails because it is not a valid
executable format, bash falls back to interpreting it as a shell script (POSIX
`execvp` ENOEXEC handling — bash's `execute_disk_command` retries via
`shell_execve`'s script check). osh reports the OS error instead.

```
$ cat >c.sh <<'EOF'
echo hi
EOF
$ bash -c './c.sh'    →  hi                                (rc 0)
$ osh  -c './c.sh'    →  osh: ./c.sh: %1 is not a valid Win32 application. (os error 193)
                                                            (rc 126)
```

**Proper fix.** On a spawn failure whose OS error means "not an executable
image" (Windows `ERROR_BAD_EXE_FORMAT` = 193; POSIX `ENOEXEC`), re-run the file
as shell source in a child of this shell — bash's rule is to check the first
line: a `#!` line is the kernel's business (and on Windows osh must honour it
itself), anything else means "feed it to the shell". Data that is plainly not a
script (a leading NUL / an ELF or PE magic) must still fail with the original
error rather than be parsed.

**Impact.** Any script invoked by path without a shebang fails under osh.
On Windows this is broader than on Unix, because a shebang alone does not make
a file executable there — so essentially *every* `./script.sh` invocation goes
through this path. Found while trying to make a corpus case re-exec itself.
