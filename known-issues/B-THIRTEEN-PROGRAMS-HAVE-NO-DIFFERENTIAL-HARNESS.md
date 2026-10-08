## B-THIRTEEN-PROGRAMS-HAVE-NO-DIFFERENTIAL-HARNESS (lane B, 2026-10-07)

**Status:** OPEN (lane B).

**In short:** almost every program in `userspace/coreutils` is checked against
its upstream by a `scripts/*-diff.sh` harness, which runs both on the same
inputs in WSL and compares output, messages and status. Thirteen are not, so
nothing has ever measured whether they behave as the programs they replace:
their messages, their `-v` output, their exit statuses, and what they do with
a closed or full standard output. Found on 2026-10-07 while converting `chmod`
to the descriptor guard: its `-v` output and its help go through `println!`,
which panics when the write fails (`chmod -v 644 f >/dev/full` is Rust's panic
message, status 101, where GNU says `chmod: write error: No space left on
device`, status 1), and no harness was there to show it.

**Where:** found by listing `userspace/coreutils/src/bin/` against the
`DIFF_PROG`/`DIFF_BINS` of every harness:

| program | upstream to measure against |
|---|---|
| `rmdir`, `readlink`, `realpath`, `sleep`, `true`, `false` | GNU coreutils 9.4 (`DIFF_GNU_SOURCE=9.4`, as `stat-diff.sh` builds it) |
| `which` | Debian's `which` (debianutils) |
| `renice` | util-linux 2.39.3 |
| `kill` | procps or util-linux -- open question B-Q22 decides which, so its harness waits for that |
| `fetch` | none: a SlateOS program, so its tests are its own |

**The fix:** a harness for each, written the way the rest are -- every option,
every message, and the descriptor cases (`<&-`, `>&-`, `>/dev/full`, `2>&-`) --
and each divergence it finds fixed. `chmod` first, since it also still lacks
the descriptor guard (`TD-COREUTILS-AN-UNWRITABLE-STDERR-ABORTS-THE-PROCESS`).

### Progress

* **`chmod`, 2026-10-07** -- `scripts/chmod-diff.sh`, which compares the tree
  each side leaves (every path's type and mode) as well as what it said. Its
  first run found nine differences: the panic above, the same panic part-way
  through a `-v -R` walk with the rest of the tree unvisited, a closed
  standard output passed as success, help and version written with
  `println!`, and `cannot access` where upstream's `fts` says `cannot operate
  on dangling symlink` for a link named on the command line. All fixed; it
  now agrees with GNU 9.4 on every case but its own help and version text,
  including standard error full and closed.
* **`mkdir`, 2026-10-07** -- `scripts/mkdir-diff.sh`, the same shape, with a
  set-group-ID parent and eight umasks. Its first run found 28 differences,
  and they were one cause: `mkdir` had a walk of its own. `-p` named the
  wrong component or gave the wrong reason when it stopped (`File exists`
  for a file in the way, where upstream's step into it says `Not a
  directory`), failed `./a/./b/.`, and made ancestors under the bare umask;
  `-m` set special bits upstream leaves to the kernel and kept ones it takes
  away. It is a port of `mkdir.c` over the crate's `mkdirp` now -- gnulib's
  `make_dir_parents`, which `install` already used -- with upstream's two
  umasks, and `-Z`/`--context` as upstream takes them without SELinux
  (design-decisions §1064). The walk fix reached `install` too, whose harness
  gained the cases. 284 agree, 4 differ on purpose (help and version text).
* **`mkfifo`, 2026-10-07** -- `scripts/mkfifo-diff.sh`. Converted with it:
  `-Z`/`--context` (§1064), and the mode set as upstream sets it -- the umask
  read and left alone, the FIFO made under it and `lchmod`ed to `-m`'s mode --
  where the umask used to be zeroed for the rest of the process. 144 agree, 5
  differ on purpose (help and version text).
