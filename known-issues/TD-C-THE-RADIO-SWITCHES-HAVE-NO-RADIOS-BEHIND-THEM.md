## TD-C-THE-RADIO-SWITCHES-HAVE-NO-RADIOS-BEHIND-THEM (lane C, 2026-10-06)

**Status:** OPEN -- waits on a wireless service the shell can reach.

**In short:** the notification pane's quick settings list Wi-Fi and
Bluetooth, and there is nothing behind either: no service the desktop can
ask to turn a radio on or off. The switches used to move and change
nothing; they now say "Not available yet" where the switch would be, and
take no press (design-decisions §1485). A user cannot turn Wi-Fi or
Bluetooth on or off from the desktop.

**Where:** `gui/desktop/src/lib.rs` (`RADIOS_UNAVAILABLE`, set as the shell
is made; `apply_pane_events` handles neither switch),
`gui/desktop/src/notif_pane.rs` (`set_unavailable`).

**To reproduce:** open the notification pane: the Wi-Fi and Bluetooth rows
say "Not available yet".

**The proper fix:** a service that owns the radios -- the network daemon
for Wi-Fi, a Bluetooth daemon -- reachable over a channel the shell can
open, with a switch's state read from it and a press sent to it; then the
two rows become switches again, showing the radio's state. Neither service
exists, and neither is lane C's to write.

**If never fixed:** the rows keep saying so; nothing pretends to work.
