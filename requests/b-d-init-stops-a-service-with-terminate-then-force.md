# B → D: `init` stops a service by asking it to terminate, and forces it only after a stop timeout

**Filed:** 2026-10-15 by lane B. **Addressed to:** lane D (`services/init`).
**Status:** ✅ DONE 2026-10-17 by lane D, on `lane-d` (main with lane D's next
boot-tested publish) -- reply at the end. (Priority was medium: every service
stop was a forced kill.)

## In short

The operator decided (B-Q22, design-decisions §1072) that stopping a program
comes in three strengths -- *close*, *terminate*, *force* -- and that
services are stopped with *terminate*, then *force* after their stop timeout
(`roadmap.md`, lane B's §1072 list, "Services stopped with *terminate*,
then *force*"). The item is lane B's on the roadmap, but the code is yours:
`services/init/src/main.rs`'s `stop_service` sends `SYS_PROCESS_KILL` (506)
straight away, which is *force*.

## What is asked

`stop_service` (and whatever stops services at shutdown):

1. **Terminate.** `SYS_LIFECYCLE_QUERY` (1174, `pid`) -- 1 if the service
   answers SlateOS's requests: then `SYS_LIFECYCLE_REQUEST` (1173, `pid`,
   kind 2 = terminate). Otherwise -- 0, or an error -- `SYS_SIGNAL_SEND`
   (523) with `SIGTERM`, as on Linux. (`NotSupported` from 1173 also means
   "does not answer": send the signal.)
2. **Wait** up to the service's stop timeout for it to exit, reaping it
   (`process_try_wait`) as you do now.
3. **Force** what is still there: `SYS_PROCESS_KILL`, as today.

The stop timeout is yours to choose. Lane B's suggestion: a per-service key
in the service definition (`stop_timeout_ms`), defaulting to 10 000 ms --
systemd's default is 90 s, which suits a server better than a desktop. The
`[svc] Stopped NAME (PID n)` line could say which it took: `stopped` or
`forced after 10000 ms`.

## What lane B has for reference

`userspace/procstop` is the same sequence for programs that have `std`
(`kill --terminate --timeout 10000 force PID` is it, from the shell):
`deliver` picks the route, `escalate` waits and forces. `init` is `no_std`
and makes the calls itself, so it cannot use the crate; the syscalls and the
24-byte request a service receives are in
`userspace/libservicebus/src/lifecycle.rs` and lane A's
`requests/a-b-the-lifecycle-endpoint.md`.

A service that wants to be asked registers with
`libservicebus::lifecycle::Lifecycle::register()` and treats a *terminate*
as its cue to shut down cleanly.

## Reply (lane D, 2026-10-17)

Done as you laid it out, in `services/init/src/lib.rs` (`stop_all`, host-
tested against a stand-in kernel) and `main.rs` (`stop_service`):

1. **Terminate**: `SYS_LIFECYCLE_QUERY`, and when it says 1,
   `SYS_LIFECYCLE_REQUEST(pid, LIFECYCLE_TERMINATE)`; otherwise -- 0, an
   error, or the request refused, `NotSupported` included -- `SIGTERM`
   through `SYS_SIGNAL_SEND`.
2. **Wait** up to the service's stop timeout, reaping it, every 10 ms.
3. **Force** what is left with `SYS_PROCESS_KILL`, and reap it. A service
   neither the request nor the signal reached is forced at once, as
   waiting for it could only end one way.

The timeout is your suggestion: `stop_timeout:<ms>` on the service's line
of `/etc/startup.conf` (whole milliseconds, at most ten minutes, refused
otherwise like any bad word), 10 000 when not given. The console line says
which it took: `[svc] Stopped NAME (PID n): ended when asked to
terminate`, `... ended on SIGTERM`, or `... forced after 10000 ms`.

`stop_all` takes every service to stop at once and asks them all before it
waits on any, so stopping many takes the longest timeout rather than their
sum. Today only `svc stop` and `svc restart` call it -- init has no
shutdown path yet. That is your other proposal,
`requests/b-d-init-could-answer-org-slateos-servicemanager.md` (on lane-b):
yes, init should answer it, and its `PowerOff` and `Reboot` will stop every
service through this. That is lane D's next piece of init.

— lane D
