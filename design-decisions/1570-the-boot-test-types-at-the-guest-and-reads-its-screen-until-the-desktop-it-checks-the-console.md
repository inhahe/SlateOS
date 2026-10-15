## 1570. The boot test types at the guest and reads its screen; until the desktop starts, it checks the console

**Date:** 2026-10-14 · **Lane:** A · **Decided by:** Claude (operator-approved scope) -- the operator chose the goal, a boot test that boots to the desktop and checks it (F-Q15, §1392); how it checks the screen before there is a desktop is lane A's call.

**In short:** at the end of every boot test, while the emulated machine is
still running, the test now types a command at it through QEMU's monitor --
`echo` and a random word -- and waits for the word to appear on the screen,
read back from QEMU's picture of it. Until now a boot "passed" on a line
printed to the serial port, which proves nothing about what a person would
see or whether the keyboard works. When the desktop starts, the same test
types at the desktop instead.

**What was built.** `scripts/screen.py`: capture the screen (QEMU's
`screendump`), press keys (`sendkey`, or QMP's `send-key`), read the
console's text off the picture, and `probe`, which does the three in turn.
`boot-test.sh` runs `probe` at the marker before stopping QEMU, through the
HMP monitor it already attaches, and fails the boot when the word is not
drawn within 180 seconds, saying which stage failed: nothing drawn, keys
never arrived, or the shell did not answer. The last screen is kept as
`build/screen/probe.png`. The probe's self-test runs before the build.

**Choices, and what was set against them:**

| Choice | Taken | Against it |
|---|---|---|
| What stands in for the desktop | init's shell on the kernel console -- the screen a person sees today | Couples the gate to lane D's `echo` builtin; the probe changes when the shell does |
| How the screen is read | exact matching of the console's 8x16 font, parsed from `kernel/src/font.rs` | Reads only that font; the desktop's text will need another reader (or a marker the compositor draws) |
| Which screen | QEMU's console 0 -- what its window shows | On the boot test's machine that is the std-VGA framebuffer Limine hands the kernel, not the virtio-gpu the kernel's DRM layer makes primary; the desktop's test must name the device it draws on |
| Gate or report | a gate: the boot fails | A new way for every lane's boot to fail; mitigated by the self-test before the build and `SCREEN_PROBE=0` to switch it off |
| Monitor | HMP, which the boot test already has | QMP is the structured one; `screen.py` speaks both, for `guest.py --qmp` |

**Reading the text by the font rather than comparing pictures** makes the
check independent of where the shell happens to be on the screen, what
scrolled past, the mouse pointer (one cell reads `?`) and the colours; a
reference picture would break on every change to the boot's output.
