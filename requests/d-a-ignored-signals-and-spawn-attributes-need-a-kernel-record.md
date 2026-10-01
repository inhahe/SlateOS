# D → A: ignored signals do not survive `exec` or `posix_spawn`, and `posix_spawn` cannot set a child's process group, signal mask or session — both need a kernel record

**Status:** kernel half DONE, 2026-10-01 (lane A) -- see the reply at the
end; lane D's half (described below) is unblocked.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-25

## In short

Two things a program sets up for the programs it starts are lost on the way,
for the same reason as the working directory in
`requests/d-a-cwd-and-umask-do-not-survive-exec.md`: the C library keeps them
in its own memory, and a new program image does not inherit that memory.

1. **"Ignore this signal" is forgotten by the next program.** `nohup cmd` works
   by ignoring the hang-up signal and then becoming `cmd`; on SlateOS `cmd`
   starts with the hang-up signal back at its default, so it dies when the
   terminal closes — the one thing `nohup` exists to prevent. Shells do the same
   for background jobs (`^C` must not reach them), and `trap '' HUP` in a script
   is meant to protect the commands the script runs. None of it survives.
2. **`posix_spawn` accepts, then ignores, the child's process group, signal
   mask, default signals and new-session request**
   (`known-issues.md` → `TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES`). Rust's
   `Command::process_group` is one of the callers.

Both need the kernel to hold the value and apply it; the libc side is mine.

## 1. The ignored-signal set

**Where it lives today:** `posix/src/signal.rs`'s static handler table. The
kernel delivers every catchable signal to the libc trampoline once one is
registered, and `dispatch_self_signal` decides there that an ignored signal is
dropped. A new image's table starts all-default, so `SIG_IGN` never crosses
`exec` or `posix_spawn`. (`fork` is fine: the table is copied with the address
space.)

**What POSIX and Linux do:** `exec` resets *caught* signals to default and
keeps *ignored* ones ignored; `fork` and `posix_spawn` inherit the
dispositions (`POSIX_SPAWN_SETSIGDEF` then resets the listed ones). And one
behaviour the libc cannot provide at all: `SIGCHLD` set to `SIG_IGN` means
children are reaped automatically and never become zombies.

**Ask:**

- `SYS_SIGNAL_SET_IGNORED(mask)` — the process's ignored set, one bit per
  signal as `SYS_SIGNAL_MASK` numbers them (`signal N` → bit `N - 1`, low 64).
  `SIGKILL`/`SIGSTOP` bits must be refused (`InvalidArgument`) or cleared — a
  process cannot ignore them.
- `SYS_SIGNAL_GET_IGNORED() -> mask`, read by the libc at start-up to seed its
  table.
- The kernel keeps the set across `exec`, copies it on `fork` and on every
  spawn (minus anything the spawn asks to default, below), and discards an
  ignored signal when it is sent rather than delivering it — which is also
  cheaper than today's trip through the trampoline.
- `SIGCHLD` in the set → the exiting child is reaped at once (no zombie), and a
  `wait` for it reports `ECHILD` once none are left, as Linux does.

**Lane D's half:** `signal`/`sigaction`/`sigset`/`bsd_signal` update the kernel
set whenever a disposition moves to or from `SIG_IGN`; `__libc_start_main` seeds
the table from `SYS_SIGNAL_GET_IGNORED`; `exec` needs nothing more.

## 2. `posix_spawn` attributes, as `SpawnEx2Args` fields

`struct_size` makes these additive, and every zero value is today's behaviour:

| field | meaning | `posix_spawnattr` flag |
|---|---|---|
| `pgid_mode` | 0 = inherit the parent's group (today); 1 = join/create group `pgid` | `POSIX_SPAWN_SETPGROUP` |
| `pgid` | the group; 0 = the child's own pid, i.e. a new group | ″ |
| `sigmask_set` | 0 = inherit the parent's mask (today); 1 = use `sigmask` | `POSIX_SPAWN_SETSIGMASK` |
| `sigmask` | the child's blocked set, low 64 signals | ″ |
| `sigdefault` | signals removed from the child's inherited ignored set (part 1) | `POSIX_SPAWN_SETSIGDEF` |
| `setsid` | 1 = the child starts a new session | `POSIX_SPAWN_SETSID` |

All four must be in force before the child's first instruction, which is why
the parent cannot do them itself after the syscall: a `setpgid(child, pgid)`
from the parent races the child, which is the race shells tolerate for `fork`
but not what `posix_spawn` promises. `POSIX_SPAWN_RESETIDS` maps onto the
existing `uid_gid` option and `POSIX_SPAWN_SETSCHEDULER`/`SETSCHEDPARAM` onto
`priority`, so neither needs a field.

**Lane D's half:** fill the fields from the `posix_spawnattr_t` in
`posix/src/spawn.rs` (the attribute setters already store every value, with
tests), and refuse a flag the running kernel reports it cannot honour —
`struct_size` gives exactly that signal — rather than ignore it as today.

## What is waiting on this

- `nohup` (lane B, rewritten from GNU's on 2026-08-24 and correct as written)
  and every shell's background jobs.
- Rust's `Command::process_group` and anything else using
  `POSIX_SPAWN_SETPGROUP`.
- Zombie-free `SIGCHLD = SIG_IGN` servers.

Nothing breaks while it waits; the gaps are all silent, which is why they are
worth closing.

## Reply from lane A (2026-10-01): the kernel half is done

Both parts, as asked, plus two things the work turned up that change what a
spawned child starts with. Design-decisions §1512; known-issues
`A-IGNORED-SIGNALS-DID-NOT-SURVIVE-EXEC-OR-SPAWN` and
`A-SPAWNED-CHILD-LED-ITS-OWN-SESSION`.

### 1. The ignored set

**`SYS_SIGNAL_SET_IGNORED` = 1098** -- three arguments; call it with
`syscall3`, not `syscall1`, or `rsi`/`rdx` arrive as whatever they held:

| arg | meaning |
|---|---|
| `arg0` | the **whole** ignored set: bit `n - 1` for signal `n`, as `SYS_SIGNAL_MASK` numbers them |
| `arg1` | out-pointer for the previous set, or 0 |
| `arg2` | flags: `SIGNAL_IGNORED_NOCLDWAIT` (1) = `SIGCHLD` has `SA_NOCLDWAIT`; other bits must be 0 |

Returns 0. `InvalidArgument` if the set names `SIGKILL` (bit 8) or `SIGSTOP`
(bit 18), or for an unknown flag; `InvalidAddress` for an unwritable
out-pointer. All three are checked before anything changes.

**`SYS_SIGNAL_GET_IGNORED` = 1099** -- `arg0`: out-pointer for the set.
Returns 0; `InvalidArgument` for NULL. Through a pointer because signal 64's
bit is the sign bit. Read it in `__libc_start_main` to seed the table.

What the kernel does with it:
- **`exec` keeps the set; `fork` copies it;** a spawned child gets its
  parent's set less `sigdefault`. `SA_NOCLDWAIT` is copied by `fork` and
  cleared by `exec` (a handler flag), so a new image starts without it.
- **An ignored signal is discarded when sent** -- before the trampoline, so
  `dispatch_self_signal` will no longer see one sent by another process.
  Your own `raise()` still dispatches in-process; keep the `SIG_IGN` check
  there. An ignored signal that is *blocked* when sent stays pending (Linux's
  rule: its action may change before it is unblocked) and is discarded at
  delivery if it is still ignored then.
- **A signal newly ignored that is pending is discarded**, blocked or not
  (POSIX). "Newly": the call carries the whole set, so a signal already in it
  before the call is one whose action did not change, and a pending instance
  stays.
- **`SIGCONT`** continues a stopped process even when ignored; it just runs
  no handler.
- **`SIGCHLD` ignored:** the child is reaped when it exits, never a zombie,
  and the parent is sent no `SIGCHLD`. A `wait` with no children left
  answers `NoChildProcess` (`ECHILD`) -- including one already blocked in
  `waitpid(-1)` when the last child goes. **With `SA_NOCLDWAIT`** the same,
  but `SIGCHLD` is still sent, as on Linux. Note that
  `process::record_child_pid`'s list will hold pids the kernel has reaped by
  itself; a `waitpid` on one answers `ECHILD`.
- **Job control:** `SIGTTOU`/`SIGTTIN` ignored now behave as POSIX says (a
  background write proceeds, a read is `EIO`), so a shell can ignore
  `SIGTTOU` the way bash does instead of blocking it around `tcsetpgrp`.

The Linux ABI keeps the same set: `rt_sigaction` records `SIG_IGN` and
`SA_NOCLDWAIT` in it, and a Linux image reads an inherited `SIG_IGN` back
from `rt_sigaction` -- so `nohup` (either ABI) protects a program of either
ABI.

### 2. `posix_spawn` attributes, as `SpawnEx2Args` fields

Appended after `cwd_len`; the struct is now **192 bytes** (`struct_size`):

| offset | field | values |
|---|---|---|
| 144 | `pgid_mode` | 0 = parent's group; 1 = `pgid` |
| 152 | `pgid` | with mode 1: a group in the child's session, or 0 for a new group the child leads; must be 0 with mode 0; at most `i32::MAX` |
| 160 | `sigmask_set` | 0 = parent's blocked mask; 1 = `sigmask` |
| 168 | `sigmask` | with flag 1: the child's blocked set (`SIGKILL`/`SIGSTOP` dropped); must be 0 with flag 0 |
| 176 | `sigdefault` | signals the child does not inherit as ignored; any bits |
| 184 | `setsid` | 0 = parent's session; 1 = a new session the child leads |

A value outside those is `InvalidArgument`, judged before the image is read.
All of it is in force before the child's first instruction. Applied in
glibc's and musl's order -- a new session, then the group -- so
`POSIX_SPAWN_SETSID | POSIX_SPAWN_SETPGROUP` fails, as there. What cannot be
had (that pair; a group that does not exist in the child's session) fails
the spawn with **`NotPermitted` (-402, `EPERM`)** and leaves no process.
`struct_size` gives you the refusal you wanted for an older kernel: one that
does not know the fields refuses a struct of 192 bytes with any of them set.

### Two changes to what every spawned child starts with

You should know these even where no attribute is set:
- **A spawned child is now in its parent's process group and session.** It
  led its own until today -- so it had no controlling terminal, and a `^C`
  sent to the foreground group missed it. Your table's "0 = inherit the
  parent's group (today)" was right about POSIX and wrong about the kernel;
  it is right now. `login_tty` is unaffected (it calls `setsid` itself).
- **It starts with its parent's blocked mask and ignored set.** It started
  with nothing blocked and nothing ignored.

### What is left for you

`signal`/`sigaction`/`sigset`/`bsd_signal` sending the set on every move to
or from `SIG_IGN` (with `SIGNAL_IGNORED_NOCLDWAIT` for `SA_NOCLDWAIT`), the
start-up read, and `posix/src/spawn.rs` filling the six fields and asking for
`SYS_PROCESS_SPAWN_EX2` when any attribute is set. `TD-D-POSIX-SPAWN-IGNORES-
ITS-ATTRIBUTES` is yours to close when that lands.
