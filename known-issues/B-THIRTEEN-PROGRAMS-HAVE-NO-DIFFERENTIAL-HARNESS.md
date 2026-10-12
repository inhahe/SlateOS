## B-THIRTEEN-PROGRAMS-HAVE-NO-DIFFERENTIAL-HARNESS (lane B, 2026-10-07)

**Status:** OPEN (lane B), for `kill` alone: every other program here with
an upstream has a harness now (see Progress), and `fetch` has none to be
held to. `kill`'s waits on open question B-Q22, which decides whether it is
procps' or util-linux's.

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
* **`rmdir`, 2026-10-07** -- `scripts/rmdir-diff.sh`, written with the port of
  the two options it refused as not implemented: `-v`, and
  `--ignore-fail-on-non-empty`, which scripts and makefiles use and which
  stopped them at the first `rmdir`. Ported with them: upstream's
  `ignorable_failure` (an `EACCES` counts as "not empty" only when the
  directory really has an entry), `Symbolic link not followed` for `rmdir
  link/`, and `failed to remove` without "directory" for an ancestor that
  fails with `ENOTDIR`. 100 agree, 4 differ on purpose.
* **`readlink`, 2026-10-07** -- `scripts/readlink-diff.sh`. Its answers
  are absolute paths, so both sides run one after the other in the same
  directory, rebuilt between them, rather than in two copies side by side.
  Reading a link, the three canonical modes over dangling links, loops, a
  file in the middle of a name and `..` after a link, `-n` (refused for two
  operands), `-z`, last-wins `-q`/`-s`/`-v`, and every descriptor: 243 agree,
  0 differ, 4 differ on purpose. The 2026-10-03 port needed no change.
* **`realpath`, 2026-10-07** -- `scripts/realpath-diff.sh`, run the same way
  as `readlink`'s. The three modes, `-P`/`-L`/`-s` (last one wins),
  `--relative-to` and `--relative-base` alone and together -- inside,
  outside and equal to the base, through links, with a missing base -- `-z`,
  `-q`, and every descriptor: 284 agree, 0 differ, 4 differ on purpose. No
  change needed.
* **`true` and `false`, 2026-10-07** -- `scripts/true-diff.sh`, one harness
  for both as upstream is one program for both. Every argument ignored,
  options too, except `--help` and `--version` spelled in full and alone, and
  those two followed by `close_stdout`, so even `true --help >/dev/full` is a
  write error: 68 agree, 0 differ, 4 differ on purpose. No change needed.
* **`which`, 2026-10-07** -- `scripts/which-diff.sh`, against GNU which
  2.21 (`/usr/bin/which.gnu`; the table above had named debianutils', a
  different program). 65 agree, 0 differ, 15 differ on purpose: our help and
  version text, and the departures `B-WHICH-DIVERGES-FROM-GNU-IN-FOUR-MEASURED-
  PLACES` records. Its first run corrected that record twice -- a claimed
  `--skip-tilde` divergence GNU does not have, and a write-error divergence
  `which.rs` had made without recording it.
* **`renice`, 2026-10-07** -- `scripts/renice-diff.sh`, against util-linux
  2.39.3. Each side gets a target of its own, a `sleep` at niceness 2 in a
  session of its own, and the niceness it is left with is compared beside
  the words. Its first run found two things, one cause: `renice` closed its
  standard output by gnulib's rule rather than util-linux's, so `renice 5
  $$ >&-` reported `write error` where util-linux forgives a closed
  descriptor, and its success lines went out ahead of a later failure where
  util-linux's `warn` leaves them in the buffer. It uses `ulclosestream` now.
  87 agree, 0 differ, 3 differ on purpose (our version string).
* **`sleep`, 2026-10-07** -- `scripts/sleep-diff.sh`, which compares how long
  each side paused (to a tenth of a second, within two) beside the words:
  the number as `strtod` reads it (exponents, hex, leading space, `inf`), the
  suffixes, several operands summed, every bad operand named before one
  referral, and the descriptors. No change needed. When only the time
  disagrees it runs both sides twice more and compares each one's quickest:
  a busy machine can only slow a run down, and two early runs each had one
  stalled case (ours once, GNU's once) while another lane wrote a disk image
  in WSL. Measured directly, ours dies of `SIGTERM` at once under `timeout`.
