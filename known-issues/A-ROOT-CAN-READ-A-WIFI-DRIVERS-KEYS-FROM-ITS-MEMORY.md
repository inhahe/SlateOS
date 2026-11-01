## A-ROOT-CAN-READ-A-WIFI-DRIVERS-KEYS-FROM-ITS-MEMORY (lane A, 2026-10-31) — OPEN

**Status:** OPEN -- fixed on lane-a-wip, awaiting a boot on main: the
operator chose (2026-11-01) the narrow fix and the seal both. The netstack
daemon runs as its own user, uid 65532, everywhere it is started
(`spawn::NETSTACK_IDENTITY`; `requests/a-d-reserve-uid-65532-for-the-netstack-daemon.md`).
A process the kernel hands a secret -- one it starts in a sealed mount
namespace, the WiFi joining driver -- is sealed against inspection
(`pcb::seal_memory`): `pcb::may_access` refuses every reader but the
kernel and the process itself, root and capability holders included, and a
fork's child is sealed too. `linuxdrv.inspectable=1` on the kernel command
line leaves such a process inspectable, said, for debugging it.
`linuxdrv::namespaced_run` checks the seal in every boot test, with a root
process as the reader.

**In short:** the WiFi network file is now out of every other process's
reach while it is placed: it is in a mount namespace of the joining
driver's own, which no process may enter (design-decisions 1596). But the
driver itself holds the network's secrets in its memory -- the passphrase
from its read until it has parsed it, then the PMK (the key derived from
the passphrase, which joins the network as well as the passphrase does)
for as long as it serves the card. A root process may read any process's
memory, the driver's included, so root could still take the key. One does
run beside it: during the join the netstack daemon, which runs as root,
checks the joined network and serves sockets over it, parsing DHCP, ARP
and DNS from the real home network -- a daemon a hostile packet took over
could read the driver's PMK. Other driver processes, which share the WiFi
driver's user, may not: every driver process is made undumpable at its
start (`linuxdrv::start_in`), which takes away the own-user route.

**Where.** `kernel/src/proc/pcb.rs`, `may_access` (behind `may_inspect`):
the kernel, the target itself, uid 0, and the target's own user while it is
dumpable may read a process's memory -- `process_vm_readv`, `ptrace`,
`/proc/<pid>/mem`. The driver runs as `linuxdrv::DRIVER_UID`, not root, and
undumpable, and nothing marks it as a process root may not inspect.
`linuxdrv::wifi_join_run` starts the netstack daemon (`run_daemon`,
`serve_sockets`) as root while the driver serves the card.

**The proper fix.** A process the kernel starts holding a secret of the
system's -- a WiFi driver with a PMK, and any later one like it -- marked so
that no process may inspect its memory, root's included: `may_access`
refusing every reader but the kernel for it, as Linux's lockdown (`kernel
lockdown=confidentiality`) refuses even root a look at kernel memory. The
mark is the kernel's to set at spawn (`linuxdrv::start_in`), never a
process's own to clear. Whether root should be refused at all is a policy
question for the operator before it is built: it also stops a root debugger
or tracer on such a driver.

Two things narrow it short of the fix: the netstack daemon run as a user
of its own rather than root, and the daemon's own sandbox. Neither closes
it for every root process.

Trigger: now, for the join's root netstack daemon; and a WiFi driver that
serves for the system's life, or rejoins a lost link (known-issues
A-WIFI-REKEYS-AND-LOST-LINKS-ARE-NOT-ANSWERED), while other processes run.
