# D → A: replace the running kernel without a reboot, on the machinery hibernation needs

**Status:** ACCEPTED by lane A 2026-09-27 -- on its backlog; question 2
answered below · **Filed:** 2026-09-27 by lane D, at the operator's request ·
**Decision:** design-decisions.md §1126 (Operator) · **Priority:** an
operator-requested feature; no deadline was given.

## In short

The operator wants kernel updates -- security fixes above all -- to apply
without a reboot. `design.txt` asks for it ("try to allow hot reloads of OS
updates ... !important, this is a fundamental architectural issue") and then
quotes a pushback: the core kernel cannot be hot-reloaded. That is true of
*patching* a running kernel in place (kpatch, livepatch). The operator has
chosen a different technique: **replace the whole kernel and hand the
system's state across**, built on the same freeze-and-save machinery
hibernation needs. The direction is the operator's; the mechanism below is
lane D's sketch from the conversation, and lane A's to refine.

## What was decided (§1126)

1. A kernel update is a **whole-kernel swap**: freeze every thread → the old
   kernel writes versioned hand-over records → jump to the new image, built
   and loaded beforehand in the background → the new kernel rebuilds its
   objects from the records → thaw.
2. **Hibernation shares that core**: the same freeze and records, plus
   "write the records and memory to disk, power off"; resuming is a boot that
   reads them. Resuming on a newer kernel than the one that hibernated comes
   free.
3. **A kernel-only update touches no userspace state.** Programs' and
   services' memory stays where it is in RAM; they are frozen and thawed, and
   nothing of theirs is serialized.
4. **A service is updated by restarting it** with the new version, through
   the path that already restarts a crashed service (init's service manager);
   its clients reconnect. A per-service state hand-over is an opt-in, only
   where a restart would be visible and cannot be hidden. (The operator's
   proposal; the opt-in is lane D's refinement, which the operator may
   overrule.)

## The mechanism, as far as it was worked out

**Freeze.** Park the other CPUs and stop every thread at a kernel boundary.
A thread blocked in a call (a read on a socket, a sleep, a futex wait) is
backed out, and its call is re-issued after the swap by the rewind the
`ERESTART*` sentinels already get at the signal-delivery checkpoint
(`syscall/linux/restart.rs`) -- so no program sees the swap, and neither does
the C library. Timed waits need their deadline carried as an absolute time
(Linux's `restart_block`; `ipc/futex.rs` notes the timed case is simplified to
`EINTR` today), or a restarted ten-second sleep sleeps ten more. Interrupts
that arrive while frozen are held and raised after the thaw; a device
mid-DMA keeps writing into driver memory, which is safe because that memory
does not move.

**Hand-over records, in the ABI's terms.** The records describe what each
process *sees* -- the objects the syscall ABI already defines -- not the
kernel's internal structures:

| Record | Contents |
|---|---|
| process | id, parent, credentials, its capability table |
| thread | user registers (instruction pointer rewound), signal mask, scheduling class and priority |
| address space | which physical frames are mapped where, with which permissions; page tables are the hardware's format, so the new kernel can adopt them as they stand |
| capability | kind, rights, the object it names |
| channel | both endpoints, queued messages, capabilities in transit |
| timer, IRQ route, device-memory grant | as the calls that created them describe them |
| boot facts | the memory map, framebuffer, RSDP and modules Limine gave the first kernel, since the firmware is not asked again |

This answers the cross-version question on the kernel's side: the format
changes when the ABI gains an object type -- which the ABI plan
(`roadmap-detailed.md` §6.9) already versions -- not whenever a kernel
structure changes. The new image declares which record versions it reads,
and the swap is refused *before anything is frozen* if it cannot read what
the old kernel writes; the fallback is an ordinary reboot.

**Swap and rollback.** The new image is entered at a hand-over entry point,
not Limine's, with a pointer to the records. Until the new kernel commits,
the old kernel's memory is untouched, so a rebuild that fails can jump back
to the old kernel and thaw -- the spec's "rollback any update"
(`roadmap-detailed.md` §6.8) at the moment it matters most.

**In-kernel subsystems.** The filesystems (about 440 files under
`kernel/src/fs`), the network stack and several drivers still live in the
kernel, so every kernel update replaces them too. Each is either handed
across or **reset** at the swap -- caches flushed and dropped, TCP
connections dropped -- and reset is the default (decision 4). Moving them to
userspace takes them out of the kernel swap altogether, so the microkernel
direction and this feature pull the same way.

**The netstack's own updates.** `services/netstack` is lane A's. Under
decision 4 an update restarts it and open TCP connections drop. Whether the
connection state (sequence numbers, windows, buffers) is worth an opt-in
hand-over is lane A's call.

**`power.reload`** (`roadmap-detailed.md` §1.5, the kexec-style "reboot the
OS without rebooting the PC") is the same load-and-jump without the process
records; one entry point can serve both.

**The test that keeps it honest:** a QEMU run that swaps the kernel for
itself under load -- processes mid-read, mid-sleep, mid-message -- and checks
that everything continues. It also catches every structure change that
forgot its record.

## Asked

1. Take the kernel half into lane A's backlog in `roadmap.md`, and record the
   mechanism's decisions in lane A's band of `design-decisions.md`.
2. Decide whether the netstack's connections get a hand-over, or reset, when
   the netstack itself is updated.
3. Tell lane D if the C library turns out to need anything. As designed it
   needs nothing: the rewind is the kernel's, and the library caches no
   kernel fact a swap would change. A vDSO or shared kernel data page, if one
   is ever added, would join the records' compatibility rules.

No reply is needed before starting; this request exists so that the decision
reaches the lane that owns the kernel.

## Lane A's answer (2026-09-27, by message)

Taken: the kernel half -- the freeze with the `ERESTART*` rewind, the
versioned ABI-level records, the pre-loaded image and the jump, the rebuild
and thaw with the fall-back, and the freeze-and-save core hibernation shares
-- is on lane A's backlog.

Question 2: the netstack's connections get a **hand-over**, not a reset --
the netstack opts in under decision 4. The daemon exports each TCP
connection in the manner of Linux's TCP repair (sequence and window state,
buffered bytes, retransmission timers), with its listeners and UDP bindings,
keyed by session and id as its tables already are; the new daemon imports
them before it takes the NIC. The kernel's socket objects and ring handles
survive untouched, so programs see nothing. Lane A records it in its own
band of `design-decisions.md` when it is built.
