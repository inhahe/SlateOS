## 1196. Whether a service starts at boot is kept in `/etc/startup.disabled`, apart from `/etc/startup.conf`

**Date:** 2026-10-17 · **Lane:** D · **Decided by:** Claude (autonomous)

**In short:** `service disable sshd` must make sshd not start at the next
boot, and `service enable sshd` undo that. init now answers both (it answers
`org.slateos.ServiceManager`). It records the choice in a small file of its
own, `/etc/startup.disabled` -- one service name a line -- rather than by
editing the user's `/etc/startup.conf`. A disabled service is still read from
`startup.conf` and registered, so `service start sshd` works on it; it is
only not started at boot. A list init cannot read, or one too long, disables
nothing: every service starts, and the console says why.

**Alternatives:**

- **Comment the service's line out in `/etc/startup.conf`** (or add a
  keyword such as `enabled:no`). One file says everything about a service.
  But init would rewrite the file a person maintains, every enable and
  disable, from a `no_std` program with a whole-file write: a power cut
  mid-write could lose service definitions, not just a preference; and a
  commented-out line is no longer registered, so `service start` could not
  reach a disabled service.
- **The separate list** (chosen). It is what systemd does -- enablement
  apart from the unit file. A damaged list costs at worst a service that
  starts when it should not, never a service's definition; and the two
  files can be edited by hand independently.

**Where it bites:** a service that others `depends:` on, disabled, leaves
them waiting at boot as a dependency that never becomes ready does today;
`init` reports unmet dependencies as it always has. `EnableService` and
`DisableService` change only the next boot, as `systemctl enable` and
`disable` do; `StartService` and `StopService` change now.

**Where it is:** `services/init/src/lib.rs` (`STARTUP_DISABLED`,
`names_disabled`, `set_disabled`, host-tested), `main.rs`
(`read_disabled_list`, `set_disabled_at_boot`).
