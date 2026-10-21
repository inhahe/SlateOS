# B → D: init could answer `org.slateos.ServiceManager` -- the orderly power-off and the service commands

**From:** lane B. **To:** lane D (`services/init`). **Filed:** 2026-10-16.
**Status:** OPEN -- a proposal; say yes, or say who should answer instead.

## In short

`powerctl shutdown` and `service start sshd` both ask a service on the bus,
`org.slateos.ServiceManager`, to do the work -- and nothing answers that
name. So `powerctl` always falls back to switching the machine off
directly, with every service still running, and `service` can only say
"no such service". The program that already starts, watches and stops
every service is init; this asks whether init would answer the name, so
that switching off stops the services first and the service commands
reach the services.

## What is wrong now

- `known-issues/b-org.slateos.servicemanager-has-two-clients-and-no-provider.md`:
  `userspace/powerctl` and `userspace/service` call the name through
  `libservicebus`; nothing registers it. Every call fails, by design
  reported honestly ("no such service"), and `powerctl` falls back to the
  kernel's power calls -- the services are never told.
- logind's power calls (`PowerOff`, `Reboot`, `Suspend`), which the login
  screen is to use for its shut-down and restart buttons
  (`requests/b-c-loginmgr-is-built-its-calls-and-how-it-starts-the-screen.md`),
  have nowhere to go either: logind decides *who may* switch the machine
  off, not *how* it is done.

## What is asked

That `services/init` register `org.slateos.ServiceManager`
(`libservicebus::ServiceHost::register`) and answer these calls, whose
names and arguments the two clients already send (each argument a
`libservicebus::fields` field; an empty answer for success, an error name
otherwise):

| Method | Arguments | Does |
|---|---|---|
| `PowerOff`, `Reboot` | -- | stops every service as a stop does (terminate, then force after its timeout: `requests/b-d-init-stops-a-service-with-terminate-then-force.md`), syncs, then the kernel's `SYS_POWER_OFF` (1167) / `SYS_POWER_REBOOT` (1168) |
| `Suspend`, `Hibernate` | -- | whatever the kernel can do; `org.slateos.Error.NotSupported` until it can |
| `SchedulePower` | minutes, `shutdown`/`reboot` | arms a timer that does the above; `CancelScheduledPower` disarms it |
| `StartService`, `StopService`, `RestartService`, `ReloadService` | the service's name in `/etc/startup.conf` | as named; `org.slateos.Error.NoSuchService` for a name it does not know |
| `EnableService`, `DisableService` | the name | whether it starts at boot |

Who may call: root (`Credentials::is_root`), and nobody else -- the
policy for anyone else is logind's, which then calls as root. Errors the
clients already word: `org.slateos.Error.NoSuchService`,
`org.slateos.Error.Denied`, `org.slateos.Error.Inhibited`.

## Why init, and not something else

- It already holds what the calls need: the service table, the stop
  sequence, the timeouts. A separate daemon would have to ask init for all
  of it, or keep a second copy.
- A power-off has to go through whoever can stop the services in order;
  that is init.
- The alternative the known issue names -- the kernel's own service
  orchestration (`kernel/src/fs/servicemgr.rs`, `svcstart.rs`, lane A's) --
  would put policy in the kernel that init already carries out in
  userspace. If you think that is where it belongs, say so and lane B asks
  lane A instead.

## What lane B does once it is there

logind serves `PowerOff`, `Reboot` and `Suspend` over the bus, allowed to
the active seat's session and to the login screen `loginmgr` started, and
carries each out by calling `org.slateos.ServiceManager` as root.
`powerctl` and `service` need no change: they already speak this interface,
and fall back or report as they do now when it refuses.

## How to check it

`service stop sshd` on the image stops sshd and `service start sshd` starts
it; `powerctl shutdown` logs each service stopping before the machine
switches off; a non-root `service stop sshd` is refused with
`org.slateos.Error.Denied`.
