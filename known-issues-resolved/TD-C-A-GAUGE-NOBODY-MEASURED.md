## TD-C-A-GAUGE-NOBODY-MEASURED -- FIXED 2026-09-16

**In short:** the desktop's system-monitor widget drew a CPU bar at 45%, a
memory bar at 62% and a disk bar at 38%. Those were three constants in the
drawing code. Every desktop, every frame, every machine, the same three
figures. Nothing measured anything.

**Date:** 2026-09-15. **Lane:** C. **Decided by:** Claude (autonomous).

**Why a gauge is the worst place for this.** A number in a settings page is
read once, deliberately, by somebody who came to look at it. A gauge on the
desktop is read at a glance and believed without thinking -- that is what a
gauge is *for*. The reader never forms the question "is this measured?",
because the whole point of the shape is to answer a question they did not have
to ask.

**How it was found, and the searchable form.** Following
`scripts/find-echoed-settings.py` into `apps/` turned up several programs that
*describe themselves* as placeholders, so I grepped the lane for that:

    In the real OS|For now, we|simulated state|placeholder for|In a real system

Twelve hits, four real. This class is almost always **documented** -- whoever
wrote it knew, said so in a comment, and the comment aged into furniture. Lane
A's `dmevent.rs` seeded devices and lane C's `ChildProcess` were both labelled
the same way.

**Then the same shape searched for directly**, since a fabricated gauge does
not have to carry a comment:

    width: (width|w|bar_w|full_w) \* 0\.[0-9]+
    (progress|percent|usage|level|fill)\w* = 0\.[0-9]+

Nine hits. **Three real and six legitimate**, and the six matter as much as
the three because they define the discriminator:

| hit | verdict |
|---|---|
| `gui/desktop/widgets.rs` CPU/memory/disk bars | **fabricated readings** |
| `apps/diagram` `w * 0.8`, `w * 0.5` | shape geometry -- a cloud drawn from overlapping rounded rects |
| `apps/undelete` `overall_progress = 0.2 / 0.8 / 0.9` | phase milestones, each set *after* its phase actually ran |

**A fraction of a shape is geometry. A fraction of a gauge is a reading.**
That is the whole test, and it is cheap enough to apply by eye.

**The fix, and why it is an `Option`.** `LiveReadings` already existed --
"the readings a widget shows that the widget layer cannot derive, supplied by
the caller each frame" -- and the clock already used it. The monitor bypassed
it. It now takes `cpu_fraction`, `memory_fraction`, `disk_fraction` as
`Option<f32>` and draws a bar only when there is a reading.

`Option`, not a default of `0.0`, and this is the part worth keeping: **a bar
at zero is a reading.** "The processor is idle" is a different claim from
"nothing measured the processor", and a default of zero would have been a
quieter version of the same defect. The trough is still drawn when there is no
reading, because an empty gauge is the honest shape of a gauge with no needle,
and a line says which it is.

**Closed 2026-09-16: two of the three are supplied, and the third says it is
not.** Re-read rather than assumed. `ShellState::sample_system` builds a
`procinfo::ProcFs`, reads `memory()` and `cpu_stats()`, and fills
`memory_fraction` and `cpu_fraction` from them; `prev_cpu` holds the previous
sample because a processor fraction is a ratio over an interval, so the first
call after start-up is `None` and the meter reads "CPU (not measured)" for that
one second -- which the code documents as the honest answer rather than a gap.
`disk_fraction` stays `None` with the reason written at the assignment: nothing
in the tree reports how much of a disk is *in use*, only its capacity, so the
meter says so instead of showing a plausible fraction of a number it does have.

The paragraph below is what was true on 2026-09-15, kept because it records why
the seam was typed as three `Option`s before anything could fill them:

**Was open:** nothing supplies the three. `gui/desktop` has no `procinfo`
dependency, so the shell cannot read `/proc/stat` or `/proc/meminfo` today.
Wiring it is a contained job -- `apps/sysinfo`, `apps/procexplorer` and
`apps/sysmonitor` all read through `procinfo` already -- and the seam is now
*typed* rather than simulated: three `Option`s to fill, with the compiler and
the widget both telling the truth until they are.
