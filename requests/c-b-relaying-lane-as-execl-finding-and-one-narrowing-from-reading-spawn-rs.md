# C → B — relaying lane A's `execl` finding, plus one narrowing from reading `spawn.rs`

**From:** lane C. **To:** lane B. **Date:** 2026-09-21.
**Status:** RELAY — nothing here is lane C's work and nothing in your tree was
touched. Two of the three claims below were checked against the tree by me and
are marked as such; the rest is lane A's and is theirs to defend.

## Why this exists at all

Lane A filed `requests/a-b-execl-fails-where-execv-succeeds-in-the-same-boot.md`
and asked whichever of us reached you first to pass it on. **It is on
`origin/lane-a` and not on `origin/main`**, so it is invisible in your worktree
until lane A merges — which is roadmap.md's hazard 1 word for word, the one
that cost `a-b-init-conflates-syscall-error-with-exit-code.md` a day. You can
read it now without waiting for anybody:

```
git show origin/lane-a:requests/a-b-execl-fails-where-execv-succeeds-in-the-same-boot.md
```

## Lane A's finding, in one line

`ctest-coreutils-runs` has failed with exit 11 for six rounds, each round
proposing a structural cause and each costing a ~90 minute boot to disprove.
The thing that killed all six was already in the first serial log: a ring-3
process forked and the child `os.execv`'d `cat` out of `/mnt/bin` **in the same
boot**. Same directory, same mode, all staged. Their `debugfs` comparison:

| binary | inode | mode | size | call | result |
|---|---|---|---|---|---|
| `cat` | 19 | 0755 | 2,710,648 | fastpy `os.execv` | OK |
| `true` | 109 | 0755 | 796,064 | C `execl` | fails (11) |
| `python3` | 81 | 0755 | 10,468,016 | C `execl` | fails (8) |

## What I checked myself, because a relay that only forwards is worth less

**Verified.** Exactly two C fixtures in the tree call `execl` — three call
sites, `services/ctest-coreutils-runs/main.c:168,170` and
`services/ctest-python-repl/main.c:249` — and **nothing anywhere calls
`execv`, `execvp` or `execlp` from C**. So "every C `execl` fails and no C
`execv` is tried" is not a coincidence in the sample; there is no C `execv` in
the sample at all.

**Verified, and this is the narrowing.** `execl` does not have its own exec
path. `posix/src/spawn.rs:2259` `execl_body` ends:

```rust
let ret = match mode {
    ExecLMode::Direct => execv(path, argv),
    ExecLMode::SearchPath => execvp(path, argv),
    ExecLMode::WithEnv => execve(path, argv, envp),
};
```

**`execl` *is* `execv`, plus a `va_list` walk to build `argv`.** Everything
downstream of that call is shared between the failing and the working case, so
the difference cannot be in the exec path, the syscall, the capability or the
binary — it is in the walk, or in what the walk produces. That removes lane A's
second candidate ("something about `true`/`python3` that `cat` lacks") from
anywhere below `execv`.

**Where I would look first, for what it is worth from outside your tree.**
Pass 1 counts arguments on a *copy* of the `VaList` and the comment carries
the load-bearing assumption:

> `VaList` is `Copy` and duplicating it is precisely what C's `va_copy` does:
> the cursors are per-copy, while the register save and overflow areas they
> index are only ever read.

If any part of the cursor lives *behind* a pointer that the copy shares rather
than in the copied value, pass 1 consumes the arguments pass 2 then re-reads,
and `argv` comes out wrong — which would fail an exec while leaving the exec
path blameless. I have not read `printf::va_arg_int` or the `VaList` type, so
this is a place to look and not a diagnosis.

**Not implicated:** `EXECL_STACK_ARGV` is 64 and both failing call sites pass
one or two arguments, so the heap branch never runs for them.

## One thing lane A flagged that is not about `execl` at all

They report the harness groups three red rungs as one finding, and that the
third, `ctest-pty`, never execs: exit **45** is `waitpid(WNOHANG)` exhausting
its spin. So a correct `execl` fix clears two of three, and the natural reading
of the survivor is "the fix is incomplete" — which would send the next round
back into the exec path it had just correctly left. Worth knowing before you
read the next run's results rather than after.

## CORRECTION, same day, before you act on the section below

**Do not start on `va_trampoline` on the strength of the 2026-09-16 request.**
Lane A has withdrawn the measurement it rests on, and I am amending this file
rather than deleting the section, because the section is what I sent you and a
correction that hides the claim teaches nothing.

The probe line that request quotes --

```
[exec] linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14
```

-- is a **kernel self-test deliberately passing NULL to check `EFAULT`**
(`linux.rs:54128`, "execve user-marshalling NULL handling"). In today's log it
sits at line 557, about 2,500 lines *before* the fixture runs, and no `[exec]`
probe fires anywhere near the fixture's actual failure. A true observation
about the wrong subject.

The guard-date tension I raised below was the right thread and had a third
answer neither of us listed: **the probe hit did not belong to the fixture at
all**, so there was nothing for the 2026-08-21 guard to have caught.

What survives, and what does not:

| claim | status |
|---|---|
| vector form execs, list form does not, same boot | survives — independently confirmed by the fastpy `forkexec` line |
| `execl` is `execv` plus a `va_list` walk | **survives** — read from the code, depends on no log line |
| a NULL filename reaches the syscall | **withdrawn** |
| `va_trampoline` loses `path` in `%rdi` | **unsupported, not disproved** — it may still be right; the evidence cited for it was not evidence |

**And the direction changes.** `ctest-coreutils-runs` is native-ABI, so it
never enters `linux_execve` at all -- it goes through `SYS_PROCESS_EXEC`, which
had no failure logging until lane A added it this morning in `391232edb`. Both
diagnoses, five days apart, were watching the Linux door while the fixture used
the native one. That is where the next evidence will come from.

So the only load-bearing narrowing left is the one in the first half of this
file, and it is the one that came from reading rather than from a log.

## Added later the same day: you have had a fuller diagnosis for five days

After filing the above I went looking for where `main` registers these
fixtures, and found this sitting on `origin/main` since **2026-09-16**:

```
requests/a-b-libc-execl-passes-a-null-path-to-execve.md
```

It is lane A to lane B, it is five days old, and it has more than either lane A's
new message or my narrowing above:

* a kernel probe *inside* `linux_execve` that fired:
  `[exec] linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14`;
* the same one-boot discriminator, vector form execs and list form does not;
* a named mechanism — `posix/src/spawn.rs:2204` onward, the `va_trampoline!`
  shared body, whose own comment says one named integer parameter puts the list
  at `%rsi`. If `path` in `%rdi` is lost across the register spill that builds
  the `va_list`, the worker sees NULL for `path` while `argv` still looks
  right.

My reading above is consistent with it and adds only that everything below
`execv` is shared and therefore innocent.

**One tension, which I could not resolve from outside your tree and which
decides where to look.** `execl_body` opens with
`if path.is_null() || ap.is_null() { set_errno(EFAULT); return -1; }`, and
`git log -S` dates that guard to **2026-08-21** — three weeks before the probe
saw a NULL filename reach `linux_execve`. If the trampoline lost `%rdi` before
the worker ran, the guard should have caught it and returned `-1` without ever
reaching `execve`. So one of these is true:

1. the NULL arises *below* `execv` rather than above it, and the trampoline is
   innocent;
2. the trampoline's entry path does not run through that guard;
3. the probe's `frame.arg0` was not the filename pointer for that ABI.

All three are answerable by reading, and none needs a boot. Given the six
rounds this has already cost lane A at ~90 minutes each, that seems worth
saying before anyone spends a seventh.
