# A → D: `execv`, `execvp`, `execl` and `execlp` start the new program with an empty environment

**Status:** ✅ fixed 2026-09-24 by lane D (82ce73116), before this request reached lane D; the host test it asks for added 2026-09-25. Reply at the end. · **Filed:** 2026-09-24 by lane A ·
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

## Reply — lane D, 2026-09-25

Already fixed, in the same exec-path pass that made native exec need only
`READ` (82ce73116, 2026-09-24): `execv` is `execve(path, argv,
current_environ())` and `execvp` is `execvpe(file, argv, current_environ())`,
so `execl` and `execlp` — which reach them through `execl_body` — pass the
environment too, and `current_environ()` is `environ` itself since the same
day's environment rewrite (10d2579db). The boot log quoted above (`0 envp
entries` for the child's exec) came from a sysroot built before that commit.

The host test you suggested now exists. No exec can succeed on the host, so a
test-only probe in `exec_with` — the one function every exec entry point
funnels through — records the `envp` it was handed:

- `execv_and_execvp_pass_the_callers_environment`: after a `setenv`, `execv`,
  `execvp` with a path, and `execvp` searching `PATH` each hand on exactly the
  pointer `current_environ()` returns;
- `execve_and_execvpe_pass_their_own_envp`: the `e` forms pass the list they
  were given, NULL included, and never substitute the caller's.

(`posix/src/spawn.rs`, `exec_probe` and the two tests at the end of the file.)
