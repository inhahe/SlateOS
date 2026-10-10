# B → A, D: genuine Oils is built for SlateOS — stage it (D), and run it at boot (A)

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane D (the rootfs recipe)
and lane A (the boot test). **Status:** lane D's half DONE 2026-10-05 -- staged (reply at the end); lane A's rung is written (it skipped while the binary was unstaged) and, with the binary now staged, runs from lane A's next boot.

## In short

The operator decided (design-decisions §1043) that the real Oils shell -- OSH,
which runs bash scripts, and YSH, its newer language -- becomes SlateOS's
default shell, built from upstream's C++, with our Rust re-creation of OSH kept
as a fallback. Lane B has done the first step: upstream Oils 0.38.0, unmodified,
cross-compiles and links against SlateOS's C library with nothing missing
(`scripts/oils-spike/`, README there). What cannot be checked off the machine
is whether it *runs*. This asks lane D to put it on the image and lane A to
run it at boot.

## Lane D: stage it

- **What:** `build/spike/oils-for-unix-slateos.elf` (about 4 MB, debug info
  stripped, symbols kept), built by `bash scripts/oils-spike/run.sh` in WSL.
  Like the CMake fixture, the recipe can rebuild it when it is missing or older
  than `libc.a`; it takes two or three minutes, and the script refuses a stale
  sysroot itself.
- **Where:** `/bin/oils-for-unix`, and the same file as `/bin/ysh`. The program
  picks its language from the name it was run by (`osh`, `ysh`), or from its
  first argument (`oils-for-unix osh -c ...`).
- **Not `/bin/osh` yet.** That name is our Rust OSH's until genuine Oils has
  run here and the Rust one has been renamed -- a later step, with `init/` and
  this recipe's `/bin/sh`, in that order.
- If the binary is absent, leave it off the image and say so, as for
  `python3` without its standard library.

## Lane A: a rung that runs it

Once it is staged. Each line below was measured from the same release built
natively on Linux (glibc), so it is what upstream's shell does:

| command | stdout | stderr ends with | status |
|---|---|---|---|
| `oils-for-unix osh -c 'echo osh-ok; [[ ab == @(ab\|cd) ]] && echo extglob-ok; f() { return 3; }; f; echo status=$?; x=$(echo sub); echo "$x"; case abc in @(x\|abc)) echo case-ok;; esac'` | `osh-ok` `extglob-ok` `status=3` `sub` `case-ok`, one per line | (nothing) | 0 |
| `oils-for-unix osh -c 'echo before; : ${undefined_var?boom}; echo not-reached'` | `before` | `[ -c flag ]:1: fatal: Var undefined_var is unset: 'boom'` | 1 |
| `ysh -c 'json write ({x: 42})'` | `{`, `  "x": 42`, `}` | (nothing) | 0 |
| `ysh -c 'var x = 1 / 0'` | (nothing) | `[ -c flag ]:1: fatal: Divide by zero` | 3 |

What each one proves, so a failure points somewhere:

- **The first** is the ordinary path, a command substitution (a fork, a pipe,
  a wait) and **extended globs** in `[[ ]]` and `case`, which only work because
  SlateOS's `fnmatch` implements `FNM_EXTMATCH` -- musl's does not.
- **The second and fourth are C++ exceptions.** Oils reports every shell error
  by throwing and catching one. If libunwind cannot find the program's unwind
  tables the shell does not print that message, it terminates (an abort, not
  status 1 or 3). `services/ctest-cxx-throw` tests the same chain in
  isolation.
- **The third** is YSH and its JSON writer.

## What lane B does next

With the rung green: Oils' spec tests on SlateOS, then the switch -- the Rust
OSH renamed, genuine Oils as `/bin/osh`, the default `sh` and the login shell
-- each as its own request where it touches another lane.

## Reply, lane A -- 2026-10-02: the rung is written, and waits for the binary

`proc::spawn::self_test_oils` ("Genuine Oils shell" in the boot log) runs your
four lines exactly as the table gives them, on `lane-a` now and on `main`
with lane A's next green boot.

- **Until lane D stages them,** it skips, counted, through the same
  `pathz_missing` every rootfs rung uses: the image's `/bin/oils-for-unix`
  and `/bin/ysh` are both required. While the self-tests run, the image is
  at `/mnt`, so the rung reads `/mnt/bin/...`, as the bash and CPython rungs
  do. It is a visible skip, not a quiet pass.
- **Once both are there,** each case must match:
  - the status exactly;
  - stdout byte for byte;
  - stderr either empty, or ending with your text once trailing newlines are
    trimmed.
- **How each case runs:** argv[0] is the path, so `ysh` picks YSH by name;
  `oils-for-unix osh -c` for the OSH cases. cwd is `/tmp`, the environment is
  `PATH=/bin LANG=C HOME=/tmp`, and the capability is File READ/WRITE/METADATA,
  as the CMake and bash rungs use.
- **A failure** prints the case's number and name, status, stdout and stderr,
  so an abort from a missing unwind table reads as cases 2/4 with no message
  and an odd status.

Nothing more is needed from lane A for this request. The spec-test runner
(`requests/b-ad-oils-spec-tests-on-the-image.md`) is a separate piece.

-- lane A

---

## Reply, lane D — 2026-10-05: staged; the rung is lane A's

**On the image:** `/bin/oils-for-unix`, and `/bin/ysh` as a hard link to it
(the program reads the name it was run by, so a link is all YSH needs; a copy
if the link fails, as the recipe's multi-call aliases do). Not `/bin/osh`,
not `/bin/sh`, as you asked. `build/spike/oils-for-unix-slateos.elf`,
`--strip-debug`, about 4 MB (`scripts/create-ext4-rootfs.sh`, after CMake's
block; its `# PROGRAM:` line puts both names in `programs.md`).

**Kept current the way bash and CPython are,** not by rebuilding from source:
- `scripts/oils-spike/slatelink.sh` (new) links the objects `run.sh`
  compiled against the current `libc.a`, checks the result -- nothing
  undefined, nothing duplicated, `PT_GNU_EH_FRAME` present, and the SlateOS
  ABI note, without which the kernel would run it as a Linux program -- and
  stages the stripped copy. Seconds.
- `run.sh` now builds and then runs it. The recipe's rebuild pass runs it
  alone when the artifact is behind `libc.a`, and an artifact older than
  `libc.a` still stops the image (`ALLOW_STALE_FIXTURES=1` aside), as for
  every other port. Absent is a NOTE.

**One change to your script, a fix:** its link went through zig's `c++`
driver with `-nostdlib`, which still puts zig's musl `libc.a` behind every
link, so a function our library lacked would have come from musl instead of
failing the link (`known-issues-resolved/D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC.md`).
It links through `scripts/lib/worktree.sh`'s `slate_make_link_wrappers` now,
as every other port's does: zig's `ld.lld` with exactly its inputs, zig's C++
runtime ahead of our `libc.a` and its `compiler_rt` behind. Relinked so:
MISSING_COUNT=0 and DUPLICATE_COUNT=0, 4,020,952 bytes stripped (yours was
3,952,264).

**Lane A:** the four commands in "Lane A: a rung that runs it" above are
yours to run; the binary is on the image from lane D's next publish to `main`.

— lane D
