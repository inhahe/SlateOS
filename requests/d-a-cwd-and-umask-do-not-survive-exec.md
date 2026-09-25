# D → A: a native program's working directory and umask do not survive `exec` or `posix_spawn` — every child starts in `/` with umask `022`

**Status:** open — needs lane A's kernel half; lane D's half is written up below and waits on it.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-24

## In short

When a SlateOS-native program starts another program, the new program always
starts in the root folder `/`, and always with the default file-creation mask,
no matter what the parent had. So the shell's `cd /tmp` is forgotten by every
command it runs: by the code, `cd /tmp && ls` lists `/`, and a command given a
relative path such as `rm -r build` acts on `/build`. Separately, `umask 077`
(permission bits a new file must *not* get) is dropped the same way, so files
end up readable by others when the user asked otherwise. The cause is that the
C library keeps both values in its own memory, which a new program image does
not inherit, and there is no native way to hand them to the kernel. I am asking
for three small native syscalls and one spawn field; the libc side is mine.

## What is wrong, measured by reading the code

| | where it lives | survives `fork` | survives `exec` | survives `posix_spawn` |
|---|---|---|---|---|
| working directory | `posix/src/unistd.rs` — a libc `static` buffer, initialised to `/` | yes (address space is copied) | **no** — the new image starts at `/` | **no** — same |
| umask | `posix/src/file.rs` `UMASK_VALUE`, initialised to `022` | yes | **no** | **no** |

`crt.rs`'s start-up never asks anyone for either value, because there is no one
to ask: no native syscall reads or writes `pcb.cwd` or `pcb.linux_umask`.

It is reached by everything that runs a program, not just C:

- **Rust's `std` on this target goes through this libc** (`x86_64-slateos` is
  `os: linux, env: musl`). On linux-musl, `std::process::Command::current_dir`
  always takes `posix_spawn` + `posix_spawn_file_actions_addchdir_np`
  (`library/std/src/sys/process/unix/unix.rs`, `get_posix_spawn_addchdir`
  returns `Some` unconditionally for musl). Our `addchdir_np` records the action
  and `posix_spawn` then ignores it — the child's libc cannot be told.
- **Oils calls `cmd.current_dir(self.cwd)` for every external command**
  (`userspace/oils/src/interp.rs:24281`, `:25026`), and `login`/`sshd` use it to
  start the user's shell in their home directory. All of those children start
  in `/`.
- C programs using `fork` + `exec*` lose both values at the `exec`.

Why no test has caught it: every ring-3 rung I could find runs its programs
with absolute paths, from `/`, which is the one directory where the bug and the
correct behaviour agree.

The kernel-side record is also wrong for *Linux* children of native parents:
`spawn_process` starts every child at `pcb` defaults (`cwd = "/"`,
`linux_umask = 0o022`) unless `SpawnOptions.cwd` is set, and native
`SYS_PROCESS_SPAWN_EX`/`_EX2` never set it. So a glibc program started by our
`posix_spawn` also begins in `/`.

## What I am asking for

The recommendation is that **`pcb.cwd` and `pcb.linux_umask` become the
process's record of both values for every ABI**, kept current by the native
libc, and inherited by the kernel exactly as it already does for `fork`.

1. **`SYS_PROCESS_SET_CWD`** — `arg0` pointer, `arg1` length. The value must
   already satisfy `pcb::set_cwd`'s invariants (absolute, normalised, no NUL,
   `<= CWD_MAX_LEN`); anything else is `InvalidArgument`. **No existence check
   and no capability check**: libc's `chdir` has already `stat`ed the target
   and normalised the path, and the value confers nothing (see below). Returns
   0.
2. **`SYS_PROCESS_GET_CWD`** — `arg0` buffer, `arg1` capacity. Writes the bytes
   (no NUL needed) and returns the length; `BufferTooSmall` (or whatever your
   convention is) when it does not fit. Called once, by `crt.rs` at start-up.
3. **`SYS_PROCESS_UMASK`** — `arg0` a new mask in `0..=0o777`, returns the old
   one, as `umask(2)`; `arg0 = u64::MAX` queries without changing. It should
   read and write the same `linux_umask` the Linux `sys_umask` uses.
4. **Native spawn inherits both from the parent**, for 517 and 559 alike, when
   `parent != 0` — as `fork_create` already does. POSIX specifies that a
   `posix_spawn` child inherits the working directory and the umask, exactly as
   after `fork` + `exec`.
5. **`SpawnEx2Args` gains `cwd_ptr` / `cwd_len`** (zero = inherit, per 4), for
   `posix_spawn_file_actions_addchdir_np`. `struct_size` means an older caller
   is unaffected. libc resolves the `chdir` actions itself, in order, against
   the parent's directory, `stat`s the result, and passes the final absolute
   path; the kernel applies it with `pcb::set_cwd`, as it already does for
   `SpawnOptions.cwd`.

Numbers are yours to pick; the highest native process number I can see is
`SYS_KEYLAYOUT_SET = 1074`.

## Why this does not reopen §648

§648 rejected a native `SYS_FS_SET_CWD` because `dirfd == 0` then meant
"resolve against the directory I happen to be in" — a base the caller did not
name and could not have been denied: ambient authority. That rule should stay
exactly as it is, and nothing here touches it. **No native call resolves a path
against `pcb.cwd` after this change, just as none does today.** libc keeps
resolving every relative path to an absolute one itself, and each resulting
path is checked against the caller's capabilities as it is now.

What changes is only what `pcb.cwd` is *for* on the native side: a **record**,
used for three things, none of which is a lookup base for a native call —

- **inheritance** across `fork` (already), `exec` (already — exec does not
  reset it) and `spawn` (item 4);
- **`/proc/<pid>/cwd`**, which today reports a native process's spawn-time
  directory forever, whatever it has `chdir`ed to;
- **a native process that `exec`s a Linux binary.** The Linux image resolves
  against `pcb.cwd`, which is the Linux ABI's own semantics and not something
  this changes — but today it gets `/` instead of where its parent was.

A string that names a directory confers nothing a capability check does not
already decide, so recording it is not authority. The objection to the old
proposal was to *reading* the base implicitly; this only keeps a record
accurate.

## Alternatives, and why I prefer the above

| option | what changes for a user | cost |
|---|---|---|
| **A. `pcb` is the record (above)** | children start where their parent was; `umask` holds; `/proc/<pid>/cwd` is right; native→Linux `exec` is right | one extra syscall per `chdir`/`umask`, which are rare |
| B. hand the values over only at the image boundary (a `SET_EXEC_FDS`-style staging call before `exec`, plus the spawn field) | children start where their parent was; `umask` holds | `/proc/<pid>/cwd` stays wrong; native→Linux `exec` needs a separate fix; a second channel for state the kernel already stores |
| C. smuggle them through the environment | as A for native children | visible to every program, breaks `env -i`, and is exactly the kind of workaround CLAUDE.md forbids |

**If this is never answered:** every program started by a native parent runs in
`/` with umask `022`. That is silent — a relative path usually still names
*something* under `/` — and it gets worse as more of the userland runs real
commands in real directories. Nothing is blocked outright, which is why it has
gone unnoticed.

## What lane D does once it lands

- `chdir`/`fchdir` call `SYS_PROCESS_SET_CWD` after validating, and fail if it
  fails, so libc and the record cannot disagree.
- `umask` goes through `SYS_PROCESS_UMASK`; `crt.rs` start-up reads both values
  before `main`.
- `posix_spawn` resolves `addchdir_np` actions (and relative `addopen` paths
  after them, which POSIX resolves in the child's new directory) and passes
  the result in `SpawnEx2Args`.
- A C fixture under `services/` that `chdir`s, `umask`s, then `fork`+`exec`s
  and `posix_spawn`s a child that reports both — so a rung can assert the
  child saw the parent's values, not `/` and `022`.

I have not touched `kernel/**`.
