## TD-A-SOMETHING-DRAWS-STRIPES-OVER-THE-SCREEN-LOG (lane A, 2026-10-27) — OPEN

**Status:** OPEN -- the drawer is not yet identified.

**In short:** on the test PC, with the kernel's log on the screen
(`log.screen=1`), something draws coloured vertical stripes -- short
gradients, green to red and blue to violet -- over the right half of the log
pane, a few rows at a time, and redraws them as the system runs. The log text
on the left stays readable. Whatever draws them writes the framebuffer without
taking the display: a mode set or page flip would have stopped the pane
(`console::hide_hint` -> `screenlog::stop`). Nothing else is known to be wrong
with the boot.

**Seen:** the test PC behind the PiKVM, boots of d7261a3c5 and 6b12ad112 on
2026-10-27 (`bare-metal-boot.md` §9), from the benchmark suite on (after the
shell prompt) for as long as the system ran -- 20 minutes. Screenshots of the
d7261a3c5 boot were in `build/pikvm/watch-d7261/` on the operator's machine.

**What it is not:** nothing in `kernel/src` draws gradients (searched for
gradient, hue, rainbow and colour-bar drawing); the log pane draws only glyphs
in three colours. So the likely source is a userspace program or test drawing
to the framebuffer through a mapping of it -- which would also be drawing over
a desktop, were there one.

**How to find it:** a QEMU boot with `log.screen=1` and a monitor screendump
after the benchmarks, if QEMU's framebuffer shows the same; otherwise the full
serial log from the test PC's serial card (`log.pci_serial=1`, once fitted),
read for what runs when the stripes appear. Then the owner lane decides
whether that program should take the display first (and so stop the pane) or
not draw to the screen at all during a boot.

**Where:** unknown; the pane is `kernel/src/screenlog.rs`, which stops only
when the display is taken.
