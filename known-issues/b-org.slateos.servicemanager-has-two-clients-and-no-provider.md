## B: `org.slateos.ServiceManager` has two clients and no provider

**2026-10-01: the clients now really ask; there is still nobody to answer.**
Both used to "open" the name with syscall 200, which is `SYS_CHANNEL_CREATE`:
it took the name's address as its flags and returned a fresh channel
connected to nothing, so neither ever reached any service even in principle
(lane F's
`requests/f-b-logind-refuses-every-caller-because-libservicebus-never-asks-who-it-is.md`,
point 3). Both now go through `libservicebus` (`Connection::connect`, then
method calls with `fields` arguments: `PowerOff`, `Reboot`, `Suspend`,
`Hibernate`, `SchedulePower`, `CancelScheduledPower`; `StartService`,
`StopService`, `RestartService`, `EnableService`, `DisableService`,
`ReloadService`), so today they fail with "no such service" -- the true
reason -- and fall back as described below. Those method names are the
interface a provider must implement; they were chosen on the clients' side
because there was no provider to follow. One behaviour changed: `powerctl`
no longer treats a *refusal* as "nobody answered" and forces the direct
fallback; it reports the refusal and exits 1. The rest of this entry -- no
provider, and the open design question of who should be one -- stands.

`userspace/powerctl` (`SERVICE_MANAGER_NAME`, main.rs:98) and
`userspace/service` (main.rs:79) both open a channel to the well-known name
`org.slateos.ServiceManager`. **Nothing anywhere registers that name.** A
tree-wide search for the string finds exactly those two call sites, both
clients. `init/servicebus` registers activation entries for
`org.slateos.compositor`, `.audio.mixer`, `.network.manager` and
`.power.manager`, pointing at `/usr/lib/slateos/*` exec paths -- a different
naming scheme, and none of them this one.

So every IPC call either tool makes fails at `SYS_CHANNEL_OPEN`. What that
means per caller:

* **`powerctl shutdown|reboot|suspend|hibernate`** -- `orderly_shutdown`
  returns `false` and the direct-syscall fallback runs. This is the reason the
  sync defect fixed alongside this entry mattered as much as it did: the
  "fallback" is not a rare path taken when the service manager is wedged, it
  is the *only* path, on every invocation.
* **`powerctl schedule N shutdown`** -- nothing arms a timer, and nothing in
  the system ever reads `/run/powerctl/scheduled` except `powerctl status`.
  The scheduled shutdown never happens. Before this change the command printed
  "Scheduled shutdown in N minutes." and "Run 'powerctl cancel' to abort."
  *before* attempting the notification, then demoted the failure to a trailing
  `note:` -- so the case where the machine certainly will not shut down was
  presented as success with a footnote. It now reports the failure and exits
  non-zero, and `powerctl status` distinguishes pending from overdue from
  unknown-clock rather than printing nothing for the latter two.
* **`userspace/service`** -- not yet audited at this depth. Its
  `send_service_command` does return `Err` on connect failure, so it is at
  least capable of reporting the truth; whether every caller does is the open
  part.

**What the proper fix is, and why it is not done here.** Something must
provide the name. That is a power/service manager daemon under `services/`,
which is lane B's, but there is **no roadmap item for one** -- writing it
would be inventing an unspecified subsystem, and the kernel already has
`fs/servicemgr.rs` and `svcstart.rs` doing service orchestration on lane A's
side, so the right answer may be that these tools should talk to *that*
rather than to a userspace daemon that has never existed. That is a design
question, not an implementation one.

Until it is answered, the rule these tools now follow is the only one
available to them: **do not report that something is scheduled when nothing
can run it.** Reaching the fallback is fine; claiming the orderly path
succeeded is not.
