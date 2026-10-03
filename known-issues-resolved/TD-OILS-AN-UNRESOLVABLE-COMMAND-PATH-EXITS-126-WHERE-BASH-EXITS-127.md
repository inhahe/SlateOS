### TD-OILS-AN-UNRESOLVABLE-COMMAND-PATH-EXITS-126-WHERE-BASH-EXITS-127 — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, where a spawn failure is turned into a
diagnostic and an exit status.

**Reproduce:**

```text
x=$(printf 'b\\'); "$x"; echo rc=$?

  bash: b\: No such file or directory        rc=127
  osh:  b\: program path has no file name    rc=126
```

**What is wrong.** Two things, and the status is the one that matters. A command
word ending in a `\` is, on Windows, a path with no file name, and osh reports
the OS's own words for that and charges it to 126 — the "found but not
executable" status. bash treats the failure as ENOENT and exits **127**. The
message text is the lesser half (osh is naming a Windows error where bash names a
POSIX one), but the status is observable to every caller.

`nosuchcmd` — a name with no separator in it at all — already agrees
(`command not found`, 127) in both, so this is specifically the path-shaped word.

**What the fix turned out to be — bigger than the original reading.** Measuring
the whole family first (probes over eleven then thirteen shapes) showed the
mapping the entry proposed is wrong for one of them, and that a *second*,
unrelated divergence sits in the same code:

| word | bash 5.2.37 | osh before |
|---|---|---|
| `b\`, `nosuch/`, `nosuch\`, `./d/x/` | `No such file or directory`, 127 | ❌ `program path has no file name`, 126 |
| `dir/`, `dir\`, `./`, `/` | `Is a directory`, 126 | ❌ `program path has no file name`, 126 |
| `./dir` (no separator on the end) | `Is a directory`, 126 | ❌ `Permission denied`, 126 |
| `reg/`, `reg\` (a plain file) | `Not a directory`, 126 | ❌ `program path has no file name`, 126 |

So the failure is not one errno to remap. bash does not read the failure off the
`errno` at all — it asks the *filesystem* what the file is, in `shell_execve`:

```c
      last_command_exit_value = (i == ENOENT) ?  EX_NOTFOUND : EX_NOEXEC;
      if (file_isdir (command))
	internal_error (_("%s: %s"), command, strerror (EISDIR));
      else if (executable_file (command) == 0)
	{ errno = i; file_error (command); }        /* execute_cmd.c:5970-5981 */
```

`file_isdir` is why `./dir` reads `Is a directory` on *both* hosts even though
neither kernel says so — `execve` on a directory is EACCES on Linux and
`CreateProcess` on one is ERROR_ACCESS_DENIED on Windows, i.e. `Permission
denied` either way. And only ENOENT exits 127; everything else in this family is
126.

The trailing-separator rows are the other half. Such a path never reaches the
host at all, because the standard library refuses it up front:

```rust
    // Early return if there is no filename.
    if exe_path.is_empty() || path::has_trailing_slash(exe_path) {
        return Err(io::const_io_error!(
            io::ErrorKind::InvalidInput, "program path has no file name")) }
                                    // std::sys::pal::windows::process::resolve_exe
```

That one error stood for four different situations. POSIX distinguishes them —
a trailing `/` asserts that the name in front of it resolves to a directory — so
the errno is reconstructible: missing name → ENOENT, plain file → ENOTDIR,
directory → the `file_isdir` arm above.

**The fix.** A `SpawnTarget` enum (`Dir` / `Missing` / `NotDir` / `Other`) and a
`spawn_target(cwd, word, resolved)` that answers it by `stat`ing the file the
spawn was to run — the `$PATH` search's answer where there was one, otherwise the
word joined onto the shell's *own* directory, since the child is given
`current_dir`. A bare word that resolved to nothing named no file and is not
asked about, so a `$PATH` miss is still `command not found`. `spawn_error_message`
takes the answer and lets it override the host's: `Missing` joins the ENOENT arm
(127), `Dir` forces `Is a directory`, `NotDir` forces `Not a directory`. Only a
failure pays for the `stat`. All three spawn-failure call sites — the simple
command, the pipeline stage, and the background job — pass it.

**Known residue: `reg/` and `reg\`, where the base is a plain file.** osh now
matches bash's *text* (`Not a directory`) and status (126) but not its prologue:
bash prints `C:/Program Files/Git/usr/bin/bash: reg/: Not a directory`, with no
`case.sh: line N:`. That is because Cygwin's `execve` is laxer than its `open` —
it tolerates the trailing slash far enough to return **ENOEXEC**, which sends
bash down the "run it as a shell script" path, and the message then comes from
the *re-exec'd* interpreter (`/bin/sh` if the file has a `#!`, bash itself if
not), whose `$0` is its own full path. Matching that would mean reproducing an
inconsistency inside one C library and printing another program's `argv[0]`, so
the two rows are deliberately left out of the corpus case.

**Regression cover:**
`userspace/oils/tests/corpus/a-command-path-that-will-not-run-is-read-off-the-file-not-the-errno.sh`
— the missing-with-separator shapes (including the `$(printf 'b\\')` one this
entry was opened for), the directory shapes plain and with either separator,
`./` and `/`, the bare-word `$PATH` miss, and the same answers off a pipeline
stage and a background job. Unit tests
`a_failed_spawn_is_worded_off_the_file_rather_than_the_errno` and
`a_program_path_is_asked_about_relative_to_the_shells_own_directory`.
