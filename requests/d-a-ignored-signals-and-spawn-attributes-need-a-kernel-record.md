# D → A: ignored signals do not survive `exec` or `posix_spawn`, and `posix_spawn` cannot set a child's process group, signal mask or session — both need a kernel record

**Status:** open — needs lane A's kernel half; lane D's half is described below
and waits on it.

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
