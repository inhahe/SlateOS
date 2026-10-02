## 1126. The kernel is replaced whole, on the machinery hibernation needs; a service is updated by restarting it

**Date:** 2026-09-27
**Lane:** D, recorded for lane A, whose kernel it is
(`requests/d-a-replace-the-running-kernel-without-a-reboot.md`)
**Decided by:** Operator for the kernel half (the operator's own proposal --
build kernel updates on hibernation's save and restore -- and the operator's
refinement that a kernel-only update touches no userspace state; Claude
worked out the mechanism, which is lane A's to refine). Claude
(operator-approved scope) for the service half: the operator proposed
restarting a service rather than carrying its state across versions; Claude
made that the default and kept a state hand-over as a per-service opt-in.

**In short:** installing a kernel update will not need a reboot. The new
kernel is loaded in the background; then every program is paused, the old
kernel writes down what it is managing -- which programs exist, which memory
each owns, who holds which permission, which messages are in transit -- and
the new kernel starts, reads that list, and resumes every program where it
paused. Programs' memory is never copied; it stays where it is.
Hibernation is the same procedure, with the list and the memory written to
disk before the power goes off. A *service* (the network stack, a driver) is
updated more simply: it is restarted with the new version, as it would be
after a crash, and the programs using it reconnect.

**What it reverses.** `design.txt`'s hot-reload line asks for exactly this
("!important, this is a fundamental architectural issue") and quotes a
pushback: "You can't hot-reload a new scheduler or a new memory manager ...
What you CANNOT safely hot-reload: core kernel code". That is true of
patching a running kernel in place (kpatch, livepatch), and stays true. It
is not true of replacing the kernel whole and handing its state across,
which is the technique chosen; `roadmap-detailed.md` §6.8's "NOT
hot-reloadable" item is annotated to say so. Precedents: Linux's kexec with
CRIU, Linux's kexec hand-over work for keeping virtual machines alive across
a host-kernel swap, and MINIX 3's live update, which is a microkernel's.

**The kernel half.**

- **Freeze** every thread at a kernel boundary. One blocked in a call is
  backed out and its call re-issued after the swap, by the rewind the
  `ERESTART*` sentinels already use, so programs do not see it.
- **Hand over** in records that describe each process's objects in the
  ABI's terms -- mappings, capabilities, channels and their queued messages,
  threads' registers, timers with absolute deadlines, IRQ routes -- not the
  kernel's internal structures. The format then changes when the ABI gains
  an object type (which `roadmap-detailed.md` §6.9 already versions), not
  when a kernel structure changes: that is the answer to "compatible across
  versions" on the kernel's side.
- **Swap**: jump to the new image, loaded beforehand. The old kernel's
  memory is untouched until the new one commits, so a rebuild that fails
  falls back to the old kernel; a record version the new image cannot read
  is caught before anything is frozen, and falls back to a reboot.
- **Userspace is frozen, not touched.** A kernel-only update serializes
  nothing of any program's or service's.
- **In-kernel subsystems** -- the filesystems, the network stack and the
  drivers still in `kernel/src` -- are replaced along with the kernel, so
  each is handed across or reset at the swap; reset is the default, as for
  services. Moving them to userspace takes them out of the swap entirely.
- **Hibernation** is the same freeze and records, written to disk with the
  memory; resuming is a boot that reads them.

**The service half.** An update restarts the service with the new version,
through the path that restarts a crashed one (init's service manager already
does, with backoff). There is no state format, so nothing to keep compatible
across versions, and the path is one that has to work anyway. What a restart
costs depends on the service:

| Service | What a restart looks like |
|---|---|
| input, storage-controller and USB drivers | nothing visible: the hardware is probed again, and disk requests in flight are retried by the layer above |
| audio | a short gap |
| GPU driver, compositor | a flicker, and programs rebuild their GPU resources -- which is how Windows updates a display driver without a reboot |
| network stack | open TCP connections drop |
| a filesystem service | it flushes first; open files survive only if the protocol keeps the session on the client's side, as NFS does |

Where a restart would be visible and a reconnect cannot hide it, the service
may opt into a state hand-over: the old instance writes its state and the new
one reads it -- a format private to that service -- and the live endpoints
(channels, the IRQ, device memory) pass to the new instance as capabilities,
so its clients keep their connections. Reconnecting belongs in the client
libraries (the C library, the toolkit), written once, not in every program.

| Option | For | Against |
|---|---|---|
| Patch the running kernel in place | no freeze; a small fix applies at once | cannot change a data structure; every patch built by hand |
| **Replace the kernel whole (chosen)** | anything can change, the scheduler and memory manager included; shares its machinery with hibernation | a hand-over format to keep readable -- cheap when it is the ABI's |
| Reboot and restore the session | no new kernel machinery | every program restarts, and what it did not save is lost |
| Services: carry every service's state across | nothing visible on any update | a versioned format per service, forever, and the crash path still needed |
| **Services: restart; hand-over by opt-in (chosen)** | one path for updates and crashes | visible for a few services -- which is what the opt-in is for |

**Who builds what.** Lane A: the freezer, the records, the swap, hibernation,
and whether the netstack's connections get a hand-over -- which lane A
answered the same day: they do, exported and imported in the manner of
Linux's TCP repair (the request file has the detail). Lane B: "restart with
the new version" in the service manager. Lanes C, D and F: reconnecting in
the toolkit, the C library and the compositor, as the services they talk to
become restartable. As designed, the C library needs nothing for the kernel
swap itself.

**How to reverse.** Nothing is built; the entry records a direction. The
service half can be changed service by service at any time.
