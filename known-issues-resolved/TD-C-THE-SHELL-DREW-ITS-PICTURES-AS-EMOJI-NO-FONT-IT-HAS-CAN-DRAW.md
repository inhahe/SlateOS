## TD-C-THE-SHELL-DREW-ITS-PICTURES-AS-EMOJI-NO-FONT-IT-HAS-CAN-DRAW (lane C, 2026-09-26) -- FIXED the same day

**Status:** FIXED 2026-09-26 -- the taskbar's start button, bell and tray
chevron are icons (design-decisions §881), and so is every overlay's picture
(volume, brightness, media, lock keys, devices, screenshots, microphone,
network, battery, and the ten generic ones), the login screen's (an account's
default picture, the password eye, the bar's power, accessibility and keyboard
buttons, the power menu), and the widgets' (each kind's title icon, the
picker's rows, the battery by state, a placeholder).

**In short:** the shell drew its small pictures as characters -- a bell, a
speaker, a sun, a padlock, a person, a power symbol, the start button's `≡` --
and no font it has can draw most of them. The built-in font covers Basic Latin,
box drawing and block elements; Inter and DejaVu Sans, the UI faces it looks
for, have no emoji. So each was drawn as the replacement box: on the taskbar,
in every volume change, on the login screen, in every widget's title bar.

**Where:** `gui/desktop/src/lib.rs` (`render_taskbar`), `osd.rs`,
`login_screen.rs`, `widgets.rs`, `focus_assist.rs`.

**The fix:** each picture is a named icon from the icon theme (§880), with the
built-in set drawing every one (`appearance::icons`), and every surface uploads
the icons its frame names before sending it (`ShellSession::send_frame`).

**Not in reach here:** the icons *programs* put in the tray are characters they
send (`guiremote::tray::TrayIcon::glyph`); naming a theme icon there is a wire
change, lane F's -- requested in
`requests/c-f-let-a-tray-icon-name-a-theme-icon.md`.
