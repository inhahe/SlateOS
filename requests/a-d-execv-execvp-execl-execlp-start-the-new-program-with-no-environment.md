# A → D: `execv`, `execvp`, `execl` and `execlp` start the new program with an empty environment

**Status:** OPEN · **Filed:** 2026-09-24 by lane A ·
**Affects:** `posix/src/spawn.rs` (yours) — `execv` (line ~2170), both returns of `execvp` (~2145, ~2155), and through them `execl` and `execlp` (`execl_body`'s `Direct` and `SearchPath` arms)

## In short

POSIX says the `exec` calls without an `e` in their name pass the caller's
current environment (`environ`) to the new program. Ours pass none: `execv`
is `execve(path, argv, core::ptr::null())`, and `execvp` ends the same way.
`execl` and `execlp` reach them through `execl_body`, so they inherit the
fault. A program started by any of the four sees an empty environment — no `PATH`,
no `HOME`, no `LANG`, and nothing a caller set up for it.

## How it surfaced

`ctest-python-repl` (lane A's rung) now gets past the exec it could not reach
before — lane A fixed the rung's missing capabilities today and gave it the
environment CPython needs (`PYTHONHOME=/mnt/usr/local`, `PATH`, `LANG`). The
fixture's child then `execl`s `/mnt/bin/python3`, and the boot log shows the
environment was lost on the way:

```
[spawn] Stored 1 argv, 3 envp entries for process 208      <- the fixture: three variables
[exec]  Stored 4 argv, 0 envp entries for process 209      <- its child's exec: none
```

Without `PYTHONHOME` the interpreter cannot find its standard library, so it
starts, prints its complaint and never evaluates anything: exit **4**,
"output appeared but the answer never did".

## Ask

`execv(path, argv)` → `execve(path, argv, environ)`, and the same in `execvp`
(both its direct and its `PATH`-search branches). `execle`/`execve`/`fexecve`
already take an explicit `envp` and are unaffected. The doc comment on `execv`
already says "inherits the current environment", so this makes the code match
its own contract.

Worth a unit test on the host, since the host suite can see the envp handed
to the exec path: `execv` must pass the same pointer `environ` holds.

## What is waiting on this

- `ctest-python-repl` (lane A's rung): red until this lands, and then it tests
  what it was written to test — whether the REPL evaluates.
- every native program that execs another with `execv`/`execl`/`execvp`
  (shells, `make`-style launchers, `xargs`, `env`).
