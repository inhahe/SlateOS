## 933. Desktop icon layout is not a kernel concern; `fs::deskicons` goes

**Date:** 2026-09-12 · **Decided by:** Operator · **Lane:** A

Answering A-Q8, option C. Claude recommended B or C and called C the clean one.

**In short:** a user's desktop icons have positions on screen, and two modules
modelled that: `kernel/src/fs/deskicons.rs`, which exports `/proc/deskicons`, and
`gui/desktop/src/icons.rs`, which was never wired because wiring it would have
duplicated the kernel's state. The kernel module goes. Icon layout lives in the
shell end to end, persisted in userspace (a dotfile or YAML), and `/proc/deskicons`
goes with it.

**Why, in one line:** the microkernel rule lists what belongs in the kernel --
scheduler, memory manager, IPC, capability enforcement, interrupt routing -- and
icon coordinates are not on it. `fs::deskicons` predates the split.

**The alternatives, and what they cost.** (A) keep the kernel module as the model
and make the shell a renderer for it: the duplicate goes, but the kernel keeps
state it has no business holding, and the microkernel rule stays violated with a
tidier shape. (B) make the shell the authority and demote `fs::deskicons` to
persistence: minimum viable, but it leaves a kernel module whose only job is
storing a userspace concern -- the same violation, smaller. C removes the question
rather than relocating it.

**What it costs, stated plainly:** `fs::deskicons` is marked done in the roadmap
and this deletes it. That is the right direction even so -- a module being
finished is not an argument for it existing -- but the roadmap entry needs to say
removed rather than done, or the next reader will think coverage was lost.

**Split of work.** Lane A: delete `kernel/src/fs/deskicons.rs`, its `/proc`
entry, its self-tests and its wiring. Lane C: wire `gui/desktop/src/icons.rs` and
add userspace persistence, which also makes the `icon_size` appearance setting
live for the first time. Neither half is useful alone: deleting the kernel module
before the shell persists anything loses icon positions across a reboot, so lane C
goes first and lane A follows.
