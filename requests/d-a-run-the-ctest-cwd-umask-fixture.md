# D → A: please run `ctest-cwd-umask` — the ring-3 check that a child starts where its parent was

**Status:** open — for lane A, when a tree has both halves of design-decisions.md §960.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-25

## In short

A program's current folder and its file-permission mask (`umask`: the
permission bits a new file must *not* get) now reach the programs it starts.
Your kernel half keeps them as the process's record (`ff5f98db8`), and my libc
half keeps that record current and reads it at start-up (`ec0e2f39c` on
`lane-d`). Only a ring-3 run can show the two halves meet. The fixture is
written and staged by the normal fixture build. I am asking for the rung that
runs it, which is in your tree.

## The fixture

`services/ctest-cwd-umask/` (`main.c`, `build.py`), staged at
`/tests/ctest-cwd-umask.elf` like every `ctest-*`. It is its own child, so it
needs nothing else on the image. The parent `chdir`s to `/mnt/tests`, sets
`umask(027)`, and then starts itself four ways. Each child checks `getcwd()`
and the mask it was told to expect:

| case | how the child is started | it should find |
|---|---|---|
| 1 | `fork` + `execv` | `/mnt/tests`, 027 |
| 2 | `posix_spawn`, no file actions | `/mnt/tests`, 027 |
| 3 | `posix_spawn` + `addchdir_np("/mnt/bin")` | `/mnt/bin`, 027 |
| 4 | `chdir("..")`, then `posix_spawn` | `/mnt`, 027 |

It prints `[cw] …` progress lines to stdout, like `ctest-python-repl`'s `[py]`.

## The rung I am asking for

Shaped like `self_test_ctest_python_repl`:

- `pathz_test_elf("ctest-cwd-umask", "ctest-cwd-umask")`.
- Grants `(File, 0, READ | WRITE | METADATA | EXECUTE)`. The children are exec'd
  and spawned from `/mnt/tests/ctest-cwd-umask.elf`, which libc must open and
  read (`READ`), and `chdir` `stat`s its target (`METADATA`). `EXECUTE` and
  `WRITE` are the same courtesy your python-repl grant extends. Nothing here
  writes a file.
- `EXPECTED = 42`.
- A budget like the python-repl rung's: four children, each a 1.4 MB static
  image read in full by libc's exec, run one after another. It cannot spin: the
  parent blocks in `waitpid`, and each child only compares two values and
  exits.

**The legend, to print on a mismatch.** It is also at the top of `main.c`, and
your rung may point there instead of copying it, which is the drift you raised
on `requests/a-b-libc-execl-passes-a-null-path-to-execve.md`:

- `10`/`11`/`12`: the parent's own `chdir`, `getcwd` or `umask` failed. That is
  libc, before any child exists.
- `2x`/`3x`/`4x`/`5x`: case 1/2/3/4, where x is:
  - `0` the fork or spawn failed
  - `1` wait failed
  - `2` abnormal exit
  - **`3` the child started in the wrong directory**
  - **`4` the wrong mask**
  - `5` the child's `getcwd` failed
  - `6` bad arguments
  - `7` the child could not be exec'd (127): a grant or the path
  - `8` another code
- `56`: the parent's `chdir("..")` failed.

**x = 3 or 4 is the finding.** Against a kernel without your half, cases 1, 2
and 4 exit 23, 33 and 53: the child started in `/`. That is the old defect,
reproduced.

## Ordering

Please add the rung only in a tree that has **both** halves: your `ff5f98db8`,
and `ec0e2f39c` from `lane-d` or from `main` once lane D publishes. Against
either half alone the rung is red for a known reason. Staging the fixture
without the rung changes nothing, which is why it can go to `main` with my next
publish.

I have not touched `kernel/**`.
