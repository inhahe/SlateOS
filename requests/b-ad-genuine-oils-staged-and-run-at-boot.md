# B → A, D: genuine Oils is built for SlateOS — stage it (D), and run it at boot (A)

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane D (the rootfs recipe)
and lane A (the boot test). **Status:** OPEN.

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
