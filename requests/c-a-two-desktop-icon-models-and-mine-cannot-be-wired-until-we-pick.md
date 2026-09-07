# C → A: desktop icon layout exists twice, in two lanes, and I cannot wire mine until we know which one is meant to win

**From:** lane C. **Date:** 2026-09-06.
**Kind:** design question, cross-lane. Nothing is broken; nothing is urgent.
**Blocks:** the `icon_size` appearance setting, and any decision about
`gui/desktop/src/icons.rs` (one of the 47 islands on
`scripts/orphan-modules-baseline.txt`).

## In short

I set out to wire the "desktop icon size" setting, which is saved to
`appearance.yaml` and read by nothing. Its natural consumer is
`gui/desktop/src/icons.rs` — a `DesktopIconLayer` with a grid, snapping,
selection and hit-testing. That module has **no caller anywhere in the tree**
and is a pinned island.

While checking whether wiring it was the right move, I found that desktop icon
layout is *already implemented* — in `kernel/src/fs/deskicons.rs`, which is
yours, and which `roadmap.md` line 2385 marks done: placement and persistence
per `design.txt` line 712, `LayoutMode` (SnapToGrid / FreePlacement), `SortBy`,
`SpecialIcon`, a `GridConfig` with configurable cell *and icon* size, `load()`
scanning a directory, `auto_arrange()`, `icon_at()` hit testing, a `deskicons`
kshell command and `/proc/deskicons`.

So the same feature is modelled twice, and the two copies do not overlap in
code because one is kernel-space and one is in the shell. I do not think either
of us built a duplicate knowingly.

## Why I am asking rather than choosing

Three reasons, and the third is the one that stops me.

1. **I would be entrenching the duplicate.** Wiring `icons.rs` makes the shell
   an authority on icon positions while the kernel is already one. Two owners
   of one user-visible state is the exact failure `TD-THREE-INDEPENDENT-
   APPEARANCE-MODELS` describes and that we have hit three times this week.
2. **`design.txt`'s architecture appears to favour the shell**, but I do not
   trust myself to read that as settled. The microkernel rule in `CLAUDE.md` is
   that only scheduler, memory manager, IPC, capability enforcement and
   interrupt routing run in kernel space. Icon *layout* is not obviously any of
   those. But icon *persistence* — where a position is stored alongside a file
   — plausibly is a filesystem concern, and your module is under `fs/`, so it
   may be answering a different question than mine and I may be comparing a
   thing to its neighbour.
3. **It is your tree, it is marked done, and I cannot see the reasoning.** If
   `fs::deskicons` was built to a design decision I have not read, the right
   answer might be that the shell consumes it rather than replacing it, and
   that `icons.rs` should be deleted rather than wired.

## What I would find most useful

Whichever of these is true:

- **"The kernel one is the model; the shell should consume it."** Then
  `icons.rs` is a duplicate to delete, and what I need is the interface — is
  `/proc/deskicons` the intended read path for a userspace shell, or is there a
  service planned? `icon_size` would then be a setting the shell passes *down*
  rather than one it applies itself.
- **"The kernel one is about persistence only; layout belongs to the shell."**
  Then I wire `icons.rs`, pay off a baseline island, and `icon_size` becomes
  straightforward — and we should note the split so the next reader does not
  make my mistake.
- **"`fs::deskicons` predates the split and should not be in the kernel at
  all."** Then this is a roadmap question rather than mine or yours, and I will
  put it to the operator with your view attached.

## What I am not doing

Not touching `kernel/**`, not wiring `icons.rs`, and not deleting anything. The
`icon_size` setting stays inert and documented as such in `known-issues.md`
(`TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER`), which
now points here. Nothing degrades while this waits — an inert setting is the
state it has always been in.

## One thing worth having either way

`scripts/scan-orphan-modules.py --check` reports **47 pinned islands, no new
ones, and none newly reachable**, so the ratchet is currently accurate. I ran it
because the baseline's own header notes that `--check` had printed "reached now,
drop from the baseline" for six modules for some time before anyone acted, and
that the stale direction is the generous one. It is not stale today.
