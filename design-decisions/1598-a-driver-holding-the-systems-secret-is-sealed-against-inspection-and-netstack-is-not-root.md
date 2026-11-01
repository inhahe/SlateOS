## 1598. A driver holding the system's secret is sealed against inspection, root included; the netstack daemon is not root

**Date:** 2026-11-01 · **Decided by:** Operator (Claude proposed both parts and recommended them in this order; the operator chose both: "let's do that", in chat) · **Lane:** A

**In short:** A WiFi driver joining a network keeps the network's key in
its memory: the password until it has read it, then a key derived from it
that is just as good for joining. Until now any root process could read
another process's memory, the driver's included, and one ran beside it
during the join: the network daemon, which parses whatever the network
sends it. Two changes close that. The network daemon now runs as a user of
its own rather than root. And a process the kernel hands such a secret is
sealed: no other process may read its memory, root included -- only the
kernel and the process itself. A boot option lifts the seal for debugging.

### The decision

1. **The netstack daemon is never root.** It runs as uid and gid 65532
   wherever it is started (`spawn::NETSTACK_IDENTITY`): the persistent
   daemon serving the system's sockets, and every self-test's. One taken
   over by a hostile packet has only the capabilities it was given.
   `linuxdrv::start_in` refuses to start any of its programs as root.
   Lane D is asked for the `/etc/passwd` entry
   (`requests/a-d-reserve-uid-65532-for-the-netstack-daemon.md`).
2. **A process the kernel hands a secret is sealed against inspection**
   (`pcb::seal_memory`): `pcb::may_access` refuses every reader but the
   kernel and the process itself -- root, a holder of a capability for the
   process, its own user while it is dumpable -- so `process_vm_readv`,
   `ptrace`, `/proc/<pid>/mem` and the other `/proc` files that ask are
   refused. A fork's child is sealed too; the seal goes with the process
   and no process can lift it. `linuxdrv::start_in` seals the process it
   starts in a sealed mount namespace -- one the kernel placed a secret in
   for it, the WiFi joining driver (design-decisions 1596).
3. **`linuxdrv.inspectable=1` on the kernel command line lifts it**, for
   debugging such a driver with root's tools, at the price of the secret;
   the kernel says so in its log for each process it leaves inspectable.

Every boot test checks the seal: a driver process started in a sealed
namespace must be sealed, and a root process may not inspect it
(`linuxdrv::namespaced_run`).

### Alternatives

- **The narrow fix alone** (the daemon not root). It closes the exposure
  that existed -- a root daemon beside the driver parsing the network's
  traffic -- without a new rule, but leaves every other root process able
  to read the key.
- **The seal alone.** It leaves the daemon root, with every other power
  root has; a daemon taken over could do harm of other kinds.
- **Neither.** The exposure was narrow (the boot's join, a root process
  already taken over), and nothing was blocked; but the key is the
  operator's home network's.

### Consequences

Nobody, root included, can debug a sealed driver -- attach a debugger,
read its memory, dump its core -- without booting with
`linuxdrv.inspectable=1`. The seal is only as strong as root's other ways
to a process's memory: loading kernel code, reading physical memory, or
replacing the driver's program before it starts would each pass it by;
those are the kernel's to keep from root, and are not part of this
decision. known-issues A-ROOT-CAN-READ-A-WIFI-DRIVERS-KEYS-FROM-ITS-MEMORY
is the issue this fixes.
