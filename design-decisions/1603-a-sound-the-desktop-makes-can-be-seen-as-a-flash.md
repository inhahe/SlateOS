## 1603. A sound the desktop makes can be seen as well as heard: a flash of the window in front, or of the screen

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** someone who cannot hear the desktop's sounds -- a notification's
chime, a device plugged in, the volume changing -- now can ask to see them:
`sounds.visual_alert` in `appearance.yaml` is `off` (the default), `window`
(the window in front flashes) or `screen` (the whole screen flashes). Each
flash is one faint pulse of a sixth of a second, and a run of sounds flashes
at most two and a half times a second. It was one of the four accessibility
features the shell's deleted accessibility panel had a switch for and nothing
behind (`known-issues/TD-C-FOUR-ACCESSIBILITY-FEATURES-EXISTED-ONLY-AS-A-DEAD-PANELS-CONTROLS.md`),
waiting on "a signal that a system sound played" -- which the shell's event
sounds (§1476) now are.

**Where:** `gui/appearance/src/lib.rs` (`VisualAlert`,
`SoundSettings::visual_alert`), `gui/desktop/src/lib.rs`
(`DesktopShell::sound_event`, `flashing`, `flash_settling`, the flash in
`render_osd`), `gui/desktop/src/session.rs` (frames while a flash is up or
settling).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A flash for every sound the desktop makes**, through the one call each sound goes through (`sound_event`) | a flash for some events only | One rule a user can predict; an event the user turned silent flashes nothing, so the per-event switches already choose. | A user cannot have an event heard but not flashed. |
| **Flash when the sound cannot be heard** -- no sound device, the volume down | only when sound actually plays | That is when the flash is most wanted; on SlateOS today no native program reaches the sound card, and the flash still works. | -- |
| **One pulse of 150 ms, at most one per 400 ms** | a pulse per sound | A flashing screen can bring on a seizure past three flashes a second (WCAG 2.3.1); a held volume key sounds at the key's repeat rate. | A burst of sounds shows as one flash. |
| **Faint, in the text's ink** (alpha 96 of 255) | white, or the accent | Seen against a dark or a light theme alike, and what was on the screen stays readable through it; the accent is the user's choice of colour, not a warning. | Less striking than a full-white flash. |
| **On the overlay surface**, which takes no clicks | a surface of its own | A press during the flash reaches what is under it; the volume's own overlay is drawn over the flash its sound makes. | -- |
| **The window in front, else the screen** | the window's title bar, as Windows also offers | The window's frame is the compositor's to draw; its whole box is the shell's to cover. | No title-bar-only choice. |

**Next:** the switch on the Settings application's accessibility page, asked
of lane E (`requests/c-e-a-visual-alert-for-sounds-on-the-accessibility-page.md`).
