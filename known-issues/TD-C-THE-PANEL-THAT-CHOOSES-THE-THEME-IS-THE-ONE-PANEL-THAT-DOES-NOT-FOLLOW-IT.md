### TD-C-THE-PANEL-THAT-CHOOSES-THE-THEME-IS-THE-ONE-PANEL-THAT-DOES-NOT-FOLLOW-IT — 2026-08-24 — OPEN

**In short.** `appearance_settings.rs` is the settings page where the user
picks their theme and accent colour. It is the only shell module of the fifty
that never receives a `Palette` — the object holding the colours the user just
chose — and instead draws itself from hardcoded colour constants. So the page
that chooses the theme is the one page that ignores it: switch to a light theme
or a different accent and that page keeps rendering in the stock dark one. It
is the last unconverted module of the 49-module palette conversion, and it is
unconverted precisely because it is the awkward one.

**Where:** `gui/desktop/src/appearance_settings.rs`. Verified 2026-08-24: zero
occurrences of `Palette` and zero `p.<role>` references in the file; 34 draw
sites reference hardcoded role constants (`TEXT`, `GREEN`, `SURFACE2` and
friends) imported from `appearance`.

**Why it is awkward rather than merely last.** This panel legitimately needs to
draw colours that are *not* the current theme — the swatch grid previewing the
fourteen accents, and the light/dark preview tiles, must show colours the user
has not chosen yet. So a mechanical substitution of `CONST` to `p.role` is
wrong here in a way it was not for the other 48: some of these constants are
the panel's *subject matter* and must stay literal, while the rest are its
chrome and must follow the theme. Telling the two apart is a judgement per
site, not a sweep.

**The proper fix:** thread `&Palette` into the panel's render, convert the 34
sites one at a time deciding subject-vs-chrome for each, and add the standard
`assert_drawn_from` sweep with the preview swatches declared as the exemption
(the same escape hatch `mouse_settings` uses for a derived colour, used here
for a deliberately-foreign one). The panel's *own* switches were already
converted to `crate::switch` in `66ec4d983`; they pass hardcoded `GREEN` and
`SURFACE2` as the track and are commented as the one remaining site that will
need revisiting when the panel is converted.

**If never fixed:** a user who picks a light theme sees every settings page
respect it except the one they picked it on, which reads as the setting not
having applied. It is the most visible remaining instance of
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
