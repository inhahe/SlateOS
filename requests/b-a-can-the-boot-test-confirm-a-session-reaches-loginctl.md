# B → A: can the boot test confirm a login session reaches `loginctl`?

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
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
