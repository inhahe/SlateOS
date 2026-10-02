## 1534. A way into a running guest: a virtio-serial port, and an agent in the kernel

**Date:** 2026-10-02 · **Decided by:** Claude (operator-approved scope: C-Q11
idea 1, the operator's own idea, which lane A took on 2026-09-27 in
`requests/a-c-testing-without-a-full-boot-lane-a-takes-both.md`) · **Lane:** A

**In short:** to try a changed program today, a lane boots the whole system:
about two and a half hours, nearly all of it checks that run before the boot.
The operator asked for a way into a copy of SlateOS that is already running,
to copy a program in and run it. This builds one. QEMU gives the running
guest a private wire to the host (a virtio-serial port, a named byte channel
between the two). A small server in the guest kernel (the agent) answers
requests on it: put a file, get a file, run a program with stated
permissions and send back its output and exit code, or run a kernel-shell
command. It is for trying a change, not for publishing one: the full boot
stays the test for `main`.

**The channel.** Lane A's 2026-09-27 answer named virtio-serial or vsock. vsock
is out on this host: QEMU's vsock device needs Linux's vhost, and the host is
Windows. virtio-serial works here, carries bytes both ways without the network
(which may be what is under test), and names its ports, so the agent's port
cannot be mistaken for another. The driver is `virtio::console`.

**The agent, and where it runs:**

| | For | Against |
|---|---|---|
| **In the kernel, as a task (chosen)** | works today, with the boot rungs' own machinery: spawning with named capabilities, capturing output to a file, `capture_command` for shell commands; no lane waits on another; nothing in the guest can reach the port, which has no device file | a server in the kernel, against the grain of a microkernel |
| A userland program (the 2026-09-27 plan) | the microkernel's place for a service | needs device files for the ports with blocking reads (a new handle kind, poll support), a way to start it with every capability, and lane B to write it -- weeks of waiting for a tool meant to save waiting |

The kernel-side choice adds no authority. The host that attaches the port
controls the whole virtual machine already (its memory, its disks), so
nothing it asks the agent for is more than it has. And the agent is no more
than the kernel shell, which is in the kernel already. If ports later get
device files for other uses, the agent can move out with no change to its
protocol.

**The protocol** is in `guestagent`'s module doc: a line of words, then the
byte fields its numbers measure, so a path or a file may hold any byte.
Programs run with the C fixtures' grant words (`file`, `secureboot`), so a
program tried here holds what its fixture would at boot.

**Limits:** two ports at most (the virtio descriptor pool, `ada::MAX_QUEUES`,
holds 16 queues for every device together, and a port costs two); output comes
back when a program ends, not as it runs; a `run` lasts at most an hour.
