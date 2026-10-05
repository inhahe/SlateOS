## `TD-B-OSH-DECIDED-A-COMMAND-WORD-BY-THE-ERRNO-RATHER-THAN-BY-THE-FILE` (lane B, 2026-08-26) — ✅ **FIXED** 2026-08-26 (`d31a8743b`, `d00f55270`, `b7d18d969`, `3d6bcce68`, `71932f742`)

**In short:** when you typed the name of a program the system would not run, osh
printed whatever complaint the operating system handed back. bash does not do
that — it goes and *looks at the file* first, and words the complaint from what
it finds there. The two shells therefore disagreed about eight different things
at once, from "you don't have permission to run this" (osh silently ran it
anyway) to which of `126` and `127` a failed command exits with. All eight are
now measured against real bash and fixed.

This was found while migrating the shell's differential harness off MSYS bash
(a Windows port) onto glibc bash — see
`TD-B-THE-SHELL-HARNESS-STILL-MEASURES-AGAINST-MSYS-BASH`. Three of the eight
were not osh bugs at all until that migration: the old reference *was* the
Windows port, and had certified the Windows answer.

### The eight, as measured against bash 5.2.21 (glibc)

| # | The case | bash | osh, before | Fixed in |
|---|---|---|---|---|
| D1 | a text file without the execute bit, run by path | `Permission denied`, 126 | **ran it**, rc=0 | `b7d18d969` |
| D2 | the script path given to a `#!` interpreter | `[./show.sh]` — the spelling you typed | `[/tmp/…/./show.sh]` — absolutised | `3d6bcce68` |
| D3 | a word ending in a backslash, e.g. `nosuch\` | `command not found`, 127 | `No such file or directory` | `d31a8743b` |
| D4 | a `#!` naming an interpreter that is not there | `SCRIPT: cannot execute: required file not found`, 127 | `SCRIPT: No such file or directory` | `71932f742` |
| D5 | a `#!` naming one that will not run | `SCRIPT: INTERP: bad interpreter: Permission denied`, 126, **no `line N:`** | `line N: SCRIPT: Permission denied` | `71932f742` |
| D6 | `exec`'s second line | a bare `WORD: Success` | `exec: WORD: cannot execute: No error` | `71932f742` |
| D7 | a `$PATH` match that exists but is not executable | `Permission denied`, 126; `type` names it; it gets hashed | `command not found`, 127 | `b7d18d969` |
| D8 | `type ./nox.sh` on a `chmod 644` file | `not found`, exit 1 | reported it as a file | `b7d18d969` |

### The one rule underneath all of them

bash's `shell_execve` (`execute_cmd.c:5960-6010`) never prints the errno on its
own. When `execve` refuses, it asks the filesystem what the file is and lets the
*file* choose the wording:

```c
last_command_exit_value = (i == ENOENT) ? EX_NOTFOUND : EX_NOEXEC;
if (file_isdir (command))            internal_error ("%s: %s", command, strerror (EISDIR));
else if (executable_file (command) == 0) { errno = i; file_error (command); }
… else if (sample is a #!)           sys_error ("%s: %s: bad interpreter: %s", …);
```

So a directory reads `Is a directory` however the kernel spelled its refusal —
and every kernel spells it differently (`execve` on a directory is `EACCES` on
Linux and `ERROR_ACCESS_DENIED` on Windows, i.e. `Permission denied` on both).
`ENOENT` alone exits 127; everything else is 126.

### Three notes worth keeping

**A backslash is an ordinary filename byte** (D3). SlateOS paths admit every
byte but `/` and NUL, and so does Linux. `word_is_path` is therefore
`contains('/') || (cfg!(windows) && contains('\'))` — a word holding a
backslash and no slash has no separator in it at all, so it is a plain `$PATH`
lookup and never a `stat`. The corpus case that certified the opposite
(`a-command-path-that-will-not-run-is-read-off-the-file-not-the-errno.sh`) was
recording an MSYS fact, and was corrected in `d00f55270`.

**The `$PATH` walk keeps a losing candidate** (D7). bash's `file_to_lose_p` is
the first candidate that exists but is not executable; when the walk ends
without a winner, *that* is returned, with `errno = EACCES`, which is how the
failure comes out as `Permission denied`/126 rather than `command not
found`/127. A directory may claim the slot but can never fill it. `$EXECIGNORE`
is consulted **first**, so a name it hides neither ends the search nor claims
the slot.

**On Linux, D4 and D5 are only reachable after the fact.** The kernel honours
`#!` itself, so osh's `shell_script_indirection` returns `None` and the shell
never learns the interpreter's name before the spawn. `SpawnTarget::Executable(Option<Str>)`
carries the `#!` line read back off the file *after* the refusal — which is
exactly the position bash is in. `file_interpreter` renders a trailing carriage
return as the two characters `^M`, measured with a directory literally named
`adir\r`: `./cr.sh: ./adir^M: bad interpreter: Permission denied`.

### `exec`'s second line (D6), measured rather than recalled

A matrix over every shape of program the OS refuses gives one rule:

| bash's first line | verdict | `exec`'s second line |
|---|---|---|
| `cannot execute binary file: Exec format error` | 126 | `WORD: Success` (bare) |
| `INTERP: bad interpreter: REASON` | 126 | `WORD: Success` (bare) |
| `Is a directory`, `Permission denied`, … | 126 | `exec: WORD: cannot execute: REASON` |
| `cannot execute: required file not found` | 127 | *(none)* |
| `No such file or directory` | 127 | *(none)* |
| `not found` (bare word) | 127 | *(none)* |

The bare form is exactly the two verdicts bash reaches *by reading the file*,
and the `Success` in it is `strerror(0)` — glibc's spelling. It is `No error` on
newlib, so `strerror_ok()` asks the host rather than hard-coding either.

### Coverage

Two corpus cases were added or corrected —
`a-command-path-that-will-not-run-is-read-off-the-file-not-the-errno.sh` and
`a-path-search-with-nothing-runnable-in-it-answers-with-what-it-could-not-run.sh`
— and `shebang-interpreter-line.sh` and `exec-shebangless-script.sh` now match
bash byte for byte. Six new unit tests; three existing expectations corrected to
the measured answers.
