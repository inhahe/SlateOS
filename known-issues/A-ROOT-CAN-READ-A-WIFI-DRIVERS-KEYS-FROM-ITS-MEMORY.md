## A-ROOT-CAN-READ-A-WIFI-DRIVERS-KEYS-FROM-ITS-MEMORY (lane A, 2026-10-31) — OPEN

**Status:** OPEN

**In short:** the WiFi network file is now out of every other process's
reach while it is placed: it is in a mount namespace of the joining
driver's own, which no process may enter (design-decisions 1596). But the
driver itself holds the network's secrets in its memory -- the passphrase
from its read until it has parsed it, then the PMK (the key derived from
the passphrase, which joins the network as well as the passphrase does)
for as long as it serves the card. A root process may read any process's
memory, the driver's included, so root could still take the key. Today no
such process runs while a driver holds one, since the boot's join comes
before every long-lived process starts.

**Where.** `kernel/src/proc/pcb.rs`, `may_access` (behind `may_inspect`):
the kernel, the target itself, uid 0, and the target's own user while it is
dumpable may read a process's memory -- `process_vm_readv`, `ptrace`,
`/proc/<pid>/mem`. The driver runs as `linuxdrv::DRIVER_UID`, not root, and
nothing marks it as a process root may not inspect.

**The proper fix.** A process the kernel starts holding a secret of the
system's -- a WiFi driver with a PMK, and any later one like it -- marked so
that no process may inspect its memory, root's included: `may_access`
refusing every reader but the kernel for it, as Linux's lockdown (`kernel
lockdown=confidentiality`) refuses even root a look at kernel memory. The
mark is the kernel's to set at spawn (`linuxdrv::start_in`), never a
process's own to clear. Whether root should be refused at all is a policy
question for the operator before it is built: it also stops a root debugger
or tracer on such a driver.

Trigger: a WiFi driver that serves for the system's life, or rejoins a lost
link (known-issues A-WIFI-REKEYS-AND-LOST-LINKS-ARE-NOT-ANSWERED), while
other processes run.
