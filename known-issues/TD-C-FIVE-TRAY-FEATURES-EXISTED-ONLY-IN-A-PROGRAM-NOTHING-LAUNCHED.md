## TD-C-FIVE-TRAY-FEATURES-EXISTED-ONLY-IN-A-PROGRAM-NOTHING-LAUNCHED

**Date:** 2026-09-14. **Lane:** C.
**Where:** nowhere any more — that is the point. They were in
`apps/systray`, 3 809 lines, deleted under `design-decisions.md` §845.

**In short:** the tray existed twice, and the operator's answer to C-Q12 is
that it belongs to the shell. The other copy was a whole program that
nothing ever launched. Most of what it did the shell now does — a tray a
program can put an icon in, arrange, click and hover. Five things it
offered have no home yet, and this is the record of them so that deleting
the program does not quietly delete the ideas.

**The five.** None was working before: the program was unreachable, so
every one of these was a control nothing could press. Nothing regressed.

| feature | what it was | why it did not move |
|---|---|---|
| **A volume popup** | master slider, mute, output device, and a **per-application volume list** | The shell has a volume *OSD*, which reports a change; this was a panel you adjust. Both want an audio service to talk to, and there is none — `apps/settings`' Sound page has the same list and the same gap |
| **A network popup** | connected state, SSID, signal strength, IP address | Needs a network daemon to ask. `net80211` exists; nothing serves a list of visible networks |
| **Airplane mode** | a quick-settings switch | Would have to reach the same radios Wi-Fi and Bluetooth do, which the shell's own quick settings already record as unreachable |
| **Battery saver** | a quick-settings switch | Needs a power manager. There are two dead `PowerConfig`s in the shell and no consumer for either — see `TD-C-ELEVEN-SETTINGS-PAGES-SAY-COMING-SOON` |
| **A brightness slider** | in quick settings | Needs a backlight. The compositor warms the screen now (§845's neighbour, night light) but cannot dim it: that is panel hardware, not a colour transform |

**What did move, and is already built.** The tray itself, across
2026-09-13: a control verb to register an icon, a registry in the
compositor reaped per client, a `TRAY` frame and subscription, the shell
drawing the row, clicks routed back to the owning program with the button
they were made with, drag to reorder, an overflow chevron with a menu, and
hover tooltips carrying the name the program gave. That is every tray
capability `apps/systray` had, reachable for the first time.

**Why deleting rather than porting the five.** Adding five controls to a
taskbar users can actually reach, that read nothing and change nothing, is
worse than deleting five nobody could reach — the argument
`TD-C-FOUR-DISPLAY-FEATURES-EXISTED-ONLY-IN-A-DEAD-SHELL-PANEL` makes about
its own four, and the same conclusion. Every one of the five needs a
service first.

**The one thing to check before building any of them.** Each is a *popup*
in a taskbar the shell owns outright, so the question that killed option B
of C-Q12 does not arise again: there is no protocol to negotiate, only a
service to call. Build the service, then the popup.
