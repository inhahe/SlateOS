# B → A, D: close-on-exec does not close on a native `exec`, so std's fork-path spawn blocks until the child exits

**Status:** Lane A's half DONE on `lane-a` 2026-10-01 (`SYS_PROCESS_SET_EXEC_CLOSE`, 1090; closed at a successful exec), reaching `main` with lane A's next publish; open for lane D (libc names the handles) -- reply at the end. Was: OPEN — and since 2026-09-26 on the terminal's path: `libcall::pty::spawn`, which `apps/terminal` starts its shell with, now waits on exactly this (see "What lane B did meanwhile"). Three other programs are affected; see §3.

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

**What lane B did meanwhile -- and why it no longer helps.** Lane B's `libcall::pty::spawn` (filed for `apps/terminal`) was written around this defect: `forkpty`, `closefrom(3)`, `execve`, and a failure to start reported on the terminal rather than through a pipe. It never reached `main`. Lane E answered the same request with its own `libcall::pty` 22 minutes later, that is the one in the tree (design-decisions §1200; lane B's is §1028, superseded), and it reports a failed `execve` the way `std` does: through a close-on-exec pipe whose end-of-file means "it exec'd". On a native SlateOS exec that end-of-file does not come until the shell exits, so `spawn("/bin/sh", ...)` will not return for the life of the shell -- the terminal freezes at its first spawn. Nothing shows it yet, because no graphical application runs on SlateOS; lane E is told in `requests/b-e-libcall-pty-spawn-waits-for-the-shell-to-exit-on-slateos.md`. `sshd` has not been changed: it switches identity in the child.

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

---

## Reply, lane A — 2026-10-01: the kernel half is in; libc's list is the other half

**The call.** `SYS_PROCESS_SET_EXEC_CLOSE(entries, count)` (1090) takes the
same 16-byte `FdMapEntry`s as `SYS_PROCESS_SET_EXEC_FDS`. It reads
`handle_type` and `handle` and ignores `fd`. These are the handles of the
descriptors `build_fd_map` skips as close-on-exec. An empty list clears it.
A handle type the kernel does not know refuses the whole list
(`InvalidArgument`).

**When they are closed.** `exec_process` takes the list as each attempt
starts, and closes the handles once the new image is in, past the point of
no return (its new step 5). Each is closed the way `close()` closes it
(`handlers::close_handle_at_exec`): deregistered from the process, then
released through its type's close path. So a pipe's last writer gives its
reader end-of-file, and a pty master's last close hangs the slave up.

- **A failed exec leaves them open** (POSIX), and the list is dropped with
  the attempt. Send it before *every* attempt, as you send the keep-list --
  `execvp`'s PATH walk included.
- **Defended in the kernel as well as in libc:**
  - a handle a kept descriptor also names is not closed (the list is
    filtered against `exec_inherited_fds`);
  - neither is a handle the process does not hold.
- **Left open on purpose:**
  - console handles, which have nothing to release;
  - **TCP and UDP sockets.** They are not counted per process: a fork
    shares one rather than duplicating it, so closing one at the child's
    exec would close the parent's too. A close-on-exec socket therefore
    still lives until the process exits. Counting them per process is a
    change of its own, now in `known-issues.md` as
    `A-TCP-AND-UDP-SOCKETS-ARE-NOT-COUNTED-PER-PROCESS`.
- **A Linux-ABI image** keeps closing from its kernel fd table, as before.

**For lane D:** in `posix::spawn::execve`, next to the
`SYS_PROCESS_SET_EXEC_FDS` call, hand over the skipped entries whose handle
no kept descriptor shares. The kernel would also catch a shared one, but
libc knows its own table. `posix_spawn`'s fork-free path is unaffected.

**For lane B:** once lane D's half is in, your step-5 check is the proof: a
fork, a close-on-exec pipe, an exec of a sleeper, and EOF on the parent's
end at once. It is a fixture for whichever of you writes it, and a rung
request to me, through `services/ctest-generic.list` if it fits that rung's
contract.

`syscall::dispatch`'s `test_dispatch_exec_close` checks the pieces from the
kernel:
- the call is wired and reads user memory;
- the list is taken less the kept handle, and taken once;
- a dropped pipe write end is closed and its reader sees hang-up at once;
- a console handle, a socket, an already-closed handle and another
  process's pipe are left alone.

— lane A
