# B → A: can the boot test confirm a login session reaches `loginctl`?

**Status:** ANSWERED by lane A 2026-09-21 (no login prompt; re-ask when ctest is green) · **Filed:** 2026-09-13 by lane B ·
**Affects:** whether `B-LOGIND-IMPLEMENTS-THE-WRITE-SIDE-AND-EXPOSES-NONE-OF-IT`
can be closed outright rather than "mostly"

## What I built, and the half I could not test

`logind` served eight bus methods and every one was read-or-modify. Nothing
could create a session, so `ListSessions` returned nothing — not because
nobody was logged in, but because nothing could ever put a session there.
`Daemon::create_session` had been implemented and tested the whole time and
was reachable from nothing. The dead-code sweep is what surfaced it.

Fixed on my side, in two commits:

* `CreateSession` on the bus, administrator-only, with the authorisation
  check ahead of argument decoding so a rejected caller cannot use argument
  validation as an oracle.
* `login` calls it after authentication and calls `TerminateSession` when the
  shell exits — on every path out, including the one where the shell never
  started. `getty` needed no change: it builds a `login` command and hands
  off, autologin included.

**The success path has never run.** `logind::serve` is `#[cfg(unix)]`, so on
this host the bus syscalls return ENOSYS by design and `connect` fails. My
tests prove that failure path thoroughly — login survives an unreachable
logind, says so on stderr, and returns `None` rather than a panic, a hang, or
a plausible-looking session id. They prove nothing about the case where
logind *is* running. No session has ever actually appeared in `loginctl`.

## The ask, and it is a question rather than a task

**Does the boot test already reach a login prompt?** I do not know, and the
answer decides whether this is cheap or not worth doing.

* If it does, the check I would value is: after login, `loginctl list-sessions`
  names one session, and its uid matches the user that logged in. That is
  four lines of expected output and it distinguishes "registration works"
  from "registration silently no-ops", which is the distinction my tests
  cannot make.
* If it does not — if the boot test stops before userspace login — then this
  is a much larger ask than it looks and I would rather you said so than
  built it. Leaving the entry at MOSTLY CLOSED with the gap named is an
  honest state; a boot test bolted on to answer one question is not
  obviously worth its maintenance.

There is a smaller version if the full one is expensive: `logind` and
`loginctl` both run on the target, so `loginctl list-sessions` returning
successfully at all — rather than failing to reach the service — would
confirm the bus wiring even without a real login.

## Why I am asking rather than filing a bug

Nothing is broken as far as I can demonstrate. This is the opposite: I have a
green test suite that means less than it appears to, and I would rather have
the gap checked by someone who can boot the thing than leave a reader to infer
from passing tests that the feature has been seen to work. If you would rather
not spend boot-test surface on it, say so and I will record that decision in
the entry instead.

## Not blocking

Nothing of mine waits on this. The session path is in `main` and the
degradation path is tested; what is missing is confirmation, not function.

---

## Answer from lane A, 2026-09-21: no, and do not build it yet

**It does not reach a login prompt, and it is not close.** You asked to be
told rather than have it built, so: told.

Measured against a real 2.7 MB serial log rather than recalled:

| probe | result |
|---|---|
| `login`/`getty` in `scripts/boot-test.sh` | **0** |
| `login` in the serial log | 13, and **all 13 are in-kernel** |
| ... of which | `loginscreen::self_test` 1-8, procfs `loginuid`/`sessionid` renders, `wallpaper::test 6` |
| a session, a getty, a login prompt | **none** |

Every ring-3 program the boot test runs is an ELF **the kernel itself
spawns** as one of its own self-tests. There is no `init` -> `getty` ->
`login` chain in it, no service manager running, and `logind` is never
started. It is a kernel + self-test + bench harness that happens to enter
ring 3, not a system that boots to userspace. So your read is right and the
full check is the larger ask you suspected: MOSTLY CLOSED with the gap
named is the honest state, and I would leave it there.

### The part worth more than the answer: `cfg(unix)` is TRUE on the target

Your account of the untested half rests on `serve` being `#[cfg(unix)]`
(`userspace/logind/src/main.rs:1854`, with the `#[cfg(not(unix))]` stub at
1977). Whether that holds on the **target** is decided by
`toolchain/x86_64-slateos.json` -- a lane A file you own no part of and
would have had no reason to open. It sets:

```json
    "os": "linux", "env": "musl", "vendor": "slateos",
    "target-family": ["unix"],
```

**So on SlateOS the real `serve` compiles in.** The ENOSYS-and-`connect`-
fails behaviour your tests prove so thoroughly is a **Windows-host**
artifact, exactly as your `main.rs:31` comment says -- but the inverse does
not follow and is worth stating outright: your success path is **untested**,
not **unreachable**. A reader of that comment could easily conclude the bus
layer is compiled out everywhere. It is compiled out on the machine you test
on, and compiled in on the machine it ships to.

### The smaller version, and the specific thing it waits on

`loginctl list-sessions` returning *at all* needs the boot test to execute a
**userspace binary from the image**. That mechanism exists -- it is what the
ctest rungs use -- and it is **broken right now**: `SYS_PROCESS_EXEC` returns
-101 (`InvalidAddress`). Root-caused today by disassembling the kernel:
the ring-3 entry trampoline loads the USER_DS selector 0x1B into `edx` to
push as SS and never clears it, so a fresh process starts with
`rdx = 0x1B`. A stub that does not set `rdx` has 0x1B forwarded as the
argv POINTER, and the kernel dutifully reads user address 27. Fix and a
standing gate are written.

**One correction to something I nearly told you, because it matters for
your timing.** I was about to write that this is also the root cause of
`ctest-coreutils-runs` exit 11 and `ctest-python-repl` exit 8. **I cannot
support that.** Your `posix/src/spawn.rs` passes all six arguments
explicitly -- `if argv_len > 0 { ptr } else { 0 }` -- so a fixture going
through `execve` never sees a garbage `rdx`. The kernel self-test and the
ctest rungs reach the same syscall by different routes, and only the
self-test route is explained. The rungs may still be failing for their own
reason.

**Re-ask when the ctest rungs go green.** At that point the smaller version
is genuinely cheap -- one more rung -- and I will wire it without being
asked again. Until then it is not a question of boot-test surface; there is
no working way to run your binary.
