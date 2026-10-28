## TD-A-SOMETHING-DRAWS-STRIPES-OVER-THE-SCREEN-LOG (lane A, 2026-10-27) — OPEN

**Status:** OPEN -- found and fixed on lane-a-wip (`console::lend_display`),
awaiting a boot on main.

**In short:** on the test PC, with the kernel's log on the screen
(`log.screen=1`), coloured vertical stripes -- short gradients, green to red
and blue to violet -- covered the right half of the log pane for the rest of
the boot. They were the kernel's own DRM self-test's test pattern, left on the
screen when the test finished. The log text on the left stayed readable;
nothing else was wrong with the boot.

**Seen:** the test PC behind the PiKVM, boots of d7261a3c5 and 6b12ad112 on
2026-10-27 (`bare-metal-boot.md` §9). Screenshots of the d7261a3c5 boot are in
`build/pikvm/watch-d7261/` on the operator's machine. Counting their colourful
pixels: none until `watch-006`, then a screenful at once, shrinking over the
next three frames as the pane wrote over its rows, and from `watch-010` on
exactly the same count in every frame to the end. So the stripes were drawn
once and never again -- not redrawn as the system ran, as this entry first
said.

**What drew them:** `drm::self_test`, which on the test PC (Intel UHD 630,
no native display driver) tests the firmware-framebuffer backend: its mode
sets, page flips and plane compositions (`drm/compose.rs`, `pattern_a`:
`(x & 0xFFF) << 12 | (y & 0xFFF)`, which is green ramps 16 pixels wide, red
rising across them and blue down the screen) are copied straight to the
framebuffer the console and the log pane are drawn on. The test drives the
device directly, not through the system calls whose mode sets and flips stop
the pane (`console::hide_hint`), so the pane went on drawing -- each row only
as far as its text reaches, which left the pattern beside the text. Turning
the CRTC off at the end blanks nothing on a firmware framebuffer. The search
that missed it looked for gradient, hue and rainbow drawing, and the pattern
is an arithmetic one.

**The fix:** `console::lend_display` runs the DRM self-test with the pane
paused (`screenlog::pause`), so the pane neither draws into frames the test
reads back nor is covered by them, then repaints the console from its text
and begins the pane again, blank, at its top (`screenlog::resume`). The lines
logged during the test are on the serial log only.

**Where:** `kernel/src/console.rs` (`lend_display`, `repaint_on`),
`kernel/src/screenlog.rs` (`pause`, `resume`, `Pane::restart`),
`kernel/src/drm/mod.rs` (`self_test`).
