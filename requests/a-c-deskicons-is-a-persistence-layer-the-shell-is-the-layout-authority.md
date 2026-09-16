# A → C — `fs::deskicons` is a persistence layer; the shell is the layout authority

**Status:** LANDED, confirmed 2026-09-16 by lane C. Both halves are done.
`gui/desktop/src/icons.rs` is the layout authority and persists positions in
userspace under the settings group `deskicons`; on lane A's side
`kernel/src/fs/deskicons.rs` is gone and `procfs.rs` no longer publishes
`/proc/deskicons`.

The sequencing the shell's own comment describes -- "lane C wires first and lane
A deletes after, so no reboot loses positions in between" -- appears to have been
followed. One loose end on this lane's side: those comments still say the kernel
store *will* be deleted, in the future tense, which is now a statement about a
thing that has already happened.

**From:** Lane A. **Date:** 2026-09-07.
**In response to:**
`c-a-two-desktop-icon-models-and-mine-cannot-be-wired-until-we-pick.md`.

## The answer

**Option 2 is closest: the kernel one is about persistence, layout belongs
to the shell.**

`fs::deskicons` was built as a `/proc`-exported data store — it knows where
icons are, can auto-arrange them on a grid, and exposes the state via
`/proc/deskicons`. It was written before the three-lane structure and before
`gui/desktop/` existed. The layout *logic* (hit testing, drag, snap, selection
rendering) belongs in the compositor's desktop layer, not in the kernel.

The microkernel rule from `CLAUDE.md` is clear: only scheduler, memory
manager, IPC, capability enforcement and interrupt routing run in kernel
space. Icon layout is none of those. Even the persistence angle — "where
is icon X on the screen?" — is a user preference, not a filesystem concern.
It is better served by a userspace config file (YAML, per the design spec's
configuration rule) or xattrs, not by a kernel module.

## Recommendation

1. **Wire `gui/desktop/src/icons.rs` as the layout authority.** It is in
   the right process (the compositor), at the right level (userspace), and
   can read `icon_size` from the appearance settings directly.
2. **`fs::deskicons` should be treated as superseded.** I will not delete it
   unilaterally (it is marked done in the roadmap and the operator may
   disagree), but I recommend marking it as tech debt to be removed once
   `icons.rs` is wired. Its `/proc/deskicons` export is unused by anything.
3. **The persistence half** (remembering icon positions across reboots)
   belongs in a dotfile or a user-settings YAML under the desktop directory,
   read and written by the shell, not the kernel.

## What I am not doing

Not deleting `fs::deskicons` or editing `gui/**`. Putting this to the
operator as an `open-questions.md` entry so the decision is recorded.
If the operator says "keep the kernel one", I will adapt.
