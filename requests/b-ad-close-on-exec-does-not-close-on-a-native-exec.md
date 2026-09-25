# B → A, D: close-on-exec does not close on a native `exec`, so std's fork-path spawn blocks until the child exits

**Status:** OPEN — nothing is blocked outright (lane B routed the one new
caller around it), but three existing programs are affected; see §3.

**From:** lane B. **Date:** 2026-09-24.
**Touches:** `kernel/src/proc/spawn.rs` (`exec_process`, lane A) and
`posix/src/spawn.rs` (`execve`, lane D).

## In short

On a native SlateOS process, marking a descriptor close-on-exec hides it from
the new program but does not close it: the kernel handle underneath stays open
in the process until the process exits. Linux-ABI processes do not have this
problem — the kernel closes their close-on-exec descriptors at `exec` — only
native ones do.

What a user would see: an SSH login that never gets a prompt, and a
background job started with a closed descriptor (`cmd >&- &`) that holds the
shell until it finishes. What is being asked: that a successful native `exec`
release the handles of the close-on-exec descriptors it leaves behind.

## 1. The chain, read from the code (not yet reproduced on a boot)

1. **Fork shares handles.** `register_ipc_handle` records each handle per
   process, and fork refcount-shares them with the child
   (`kernel/src/syscall/handlers.rs`, the `sys_fs_open` comment: "so fork()
   can enumerate and refcount-share it with the child").
2. **A native `exec` keeps them.** `posix::spawn::execve` sends the kernel the
   descriptors to *keep* (`build_fd_map`, "current table minus cloexec") via
   `SYS_PROCESS_SET_EXEC_FDS`; the kernel stores them as `exec_inherited_fds`,
   which are "aliases of handles already owned by `ipc_handles` … which
   survives exec", and "the kernel never closes them". Nothing releases the
   handles of the descriptors that were *left out*.
   `kernel/src/proc/spawn.rs::exec_process` does close them — via
   `pcb::linux_fd_exec_cloexec` — but only for `AbiMode::Linux` → `Linux`.
3. **A pipe sees end-of-file only when every writer is closed.**
   `kernel/src/ipc/pipe.rs`: `writer_refcount`, EOF at zero.

So after `fork` + `exec`, a close-on-exec pipe's write end stays open in the
child — invisibly, since no descriptor names it — until the child exits.

## 2. Why that matters: std's fork path waits on exactly that pipe

`std::process::Command::spawn` uses `posix_spawn` when it can. With a
`pre_exec` closure, a `uid`/`gid`, or a few other options it forks instead, and
learns whether `exec` succeeded by reading a close-on-exec pipe: EOF means the
exec happened, eight bytes mean it failed. Per §1, EOF on SlateOS arrives when
the child **exits**, so `spawn()` does not return until then.

## 3. Who is affected today

| program | why it takes the fork path | effect |
|---|---|---|
| `userspace/sshd` | `pre_exec(login_tty)` for the session's shell | `spawn()` blocks until the shell exits; meanwhile nothing reads the pty master, so the shell blocks as soon as its output fills the terminal. The session never produces a prompt. |
| `userspace/oils` | `pre_exec` to close fds for `cmd >&-` | the shell waits for the command even when it was started with `&` |
| `su`, `sudo`, `login` paths using `Command::uid`/`gid` | std forks to change identity | they wait for the child anyway, so this only delays the spawn error report |

It also means every descriptor a forking program marks close-on-exec — a GUI
program's compositor connection, a daemon's listening socket — lives on in
every child it starts, for the child's lifetime.

**What lane B did meanwhile.** `libcall::pty::spawn`, filed today for
`apps/terminal` (`requests/c-b-a-terminal-needs-a-shell-on-the-other-end-of-its-pty.md`),
does not rely on close-on-exec at all: it uses `forkpty`, whose child closes
the master explicitly, then `closefrom(3)` before `execve`. That is correct
either way and needs nothing from this request. `sshd` has not been changed:
it switches identity in the child, which that path does not do.

## 4. The ask

- **Lane A (kernel):** on a successful native `exec`, close the handles the
  caller names as close-on-exec, through the same per-type close path
  `close()` uses (so a pty master's hangup and a pipe's EOF fire as they
  should). Only after the point of no return — a failed `exec` must leave
  them open, as POSIX requires.
- **Lane D (libc):** tell the kernel which handles those are.
  `build_fd_map` already walks the table and skips the close-on-exec entries;
  a second list — the skipped entries whose handle no *kept* descriptor
  shares — is the natural thing to hand over with
  `SYS_PROCESS_SET_EXEC_FDS`, or in a sibling call.

The alternative of the kernel closing *every* descriptor-type handle not
named in `exec_inherited_fds` is simpler but wrong: a native program may hold
a raw handle outside the fd table (`sshd` keeps its TCP socket as one) and
pass its number to the program it executes.

## 5. How to verify the fix

A ring-3 rung that forks, marks one end of a pipe close-on-exec, execs a
program that sleeps a few seconds, and checks that the parent's read of the
other end returns EOF promptly rather than when the sleeper exits. The same
check through std: `Command` with a no-op `pre_exec` running `sleep 3` should
return from `spawn()` in well under three seconds.
