## 1502. A process that leaves root loses root's authority -- the system-authority types and the identity and settings rights, and nothing else

**Date:** 2026-10-01 · **Decided by:** Claude (operator-approved scope: lane D's request left the choice between its options A and B to lane A; which rights count as root's is Claude's call, and Claude's to revisit) · **Lane:** A

**In short:** programs that start as the administrator and then switch to an
ordinary user expect the switch to be one-way, as on every Unix. Sign-in,
`su` and a service becoming its own user all work this way. Here they kept
every power root had, including the power to switch back. Now, when a
process's user id leaves 0, the kernel takes root's powers away in the same
step:
- setting the clock;
- binding the low ports;
- raw disks and the raw network;
- device interrupts;
- changing identity;
- the system-wide settings.

What any ordinary program needs stays: its files, its sockets, its pipes.

**1. Lane D's option A, the rule in the kernel; not B, a call to give a
capability up.** For lane D's reasons: every ported program already assumes
it, no program can skip it by calling the kernel directly, and the rule lives
in one place. `pcb::change_credentials` applies it to every credential-changing
syscall, native `SYS_PROCESS_SET_CREDENTIALS` and the Linux `setuid` family
alike. The identity and the capability table change under one lock, so
nothing can see the new uid with the old authority.

**2. What counts as root's authority** (`cap::rights_without_root`):

| Taken | Linux's name for it |
|---|---|
| `SystemClock`, whole | `CAP_SYS_TIME` |
| `PrivilegedPort`, whole | `CAP_NET_BIND_SERVICE` |
| `ResourceLimit`, whole, `MEMORY_LOCK` included | `CAP_SYS_RESOURCE`, `CAP_IPC_LOCK` |
| `BlockDevice`, `PortIo`, whole | `CAP_SYS_RAWIO` |
| `NetRaw`, whole | `CAP_NET_RAW` |
| `DeviceIrq`, whole | a driver's authority |
| on `Process`: `SET_CREDENTIALS` and the settings rights (hostname, key layout, brightness, Secure Boot) | `CAP_SETUID`/`CAP_SETGID`, `CAP_SYS_ADMIN` |
| on a class-wide `Process` capability (id 0): `DEBUG` | `CAP_SYS_PTRACE` |
| on `IoScheduler`: `IO_REALTIME` | `CAP_SYS_NICE` |
| on `Thread`: `IO_REALTIME` (added later on 2026-10-01, see §1503) | `CAP_SYS_NICE` |

The `Thread` row was missing from the first version of this decision, though
that capability is the one libc reads as `CAP_SYS_NICE` (§326) and the one
§1503 made the kernel honour. Without it, a process that dropped root kept
the right to put itself above every service.

Rejected options and boundaries:
- *Rejected: clear the whole table, as Linux clears the permitted set.* On
  Linux, file access survives the drop because it is decided by uid and
  permission bits, not capabilities. Here it is a capability, so clearing the
  table would leave a program unable to read its own files.
- *Kept:* a `DEBUG` granted over one process, which is an explicit grant;
  `Namespace`, an isolation tool a process applies to itself; `Service`,
  registering a name; everything else.

**3. Spawn is not a drop.** A parent that spawns a child as uid 1000 with an
explicit capability list chose that list, and `pcb::set_credentials` keeps
it. Only a process changing its own identity loses authority. This is the
capability-shaped answer to the daemon that must keep one power, such as a
web server's low port: its parent grants it explicitly, rather than through
Linux's `PR_SET_KEEPCAPS`.

**4. Not modelled: a temporary drop.** A process has one uid, with no saved
set-user-ID. On Linux, `seteuid(1000)` then `seteuid(0)` is legal for a
process whose saved uid is 0; here the first call is permanent.
- Linux-ABI processes could never go back, because their gate is uid-based.
  Native processes could, until now.
- The proper fix is real, effective and saved ids, with Linux's exact rule:
  the permitted set is cleared when all three leave 0, the effective set when
  the effective id does. It is in known-issues as
  A-ONE-UID-NO-SAVED-SET-USER-ID.

**Revisit** when the three ids are modelled, or when something needs a power
across the drop that its parent cannot grant at spawn.
