### [C] TD-C-THE-TOAST-DAEMON-DRAWS-IN-ITS-OWN-COLOURS -- 2026-09-29

**Status:** OPEN -- lane C's next task after the motion policy
(design-decisions §1446).

**In short:** the notification daemon -- the pop-up toasts and the
notification centre, `gui/notifications` -- draws in a fixed copy of the dark
palette and never reads the user's theme: not light mode, not the accent, not
a colour theme, not high contrast. Its window is handed the palette like
every application's (`App::theme_changed`); until 2026-09-29 it ignored it,
and now it takes only the palette's motion from it.

**Where:** `gui/notifications/src/main.rs` -- the seventeen `const NAME:
Color` at the top of the file (`BASE`, `MANTLE`, `CRUST`, `SURFACE0`..`2`,
`TEXT`, `SUBTEXT0`/`1`, `BLUE` and the rest) and every draw site that uses
them.

**The proper fix:** keep the palette `theme_changed` hands over and draw from
its roles, as the shell's modules were converted
(`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`), with a
test that every colour drawn comes from the palette (`assert_drawn_from`'s
shape), so a constant cannot creep back.

**Also found, not yet looked into:** nothing in the tree starts the daemon.
It is not staged into the image by the rootfs recipe, and neither the
session nor init launches it (searched 2026-09-29: no `notifications` in a
launch list anywhere), so the toasts it draws are not on screen in a booted
system at all. Whether it is meant to run beside the shell's own
notification pane (`gui/desktop/src/notif_pane.rs`), or be folded into it,
is the question to answer before wiring it.
