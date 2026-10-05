## 1401. Windows' titles on the taskbar are an option, on by default, switched from the bar itself

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** `design.txt` asks for an "option to show app name along with app
icon in taskbar". There is one now: with it on -- the default -- a window's
tile is its picture and its title, as the Aero reference draws it; with it
off, a window's tile is its picture alone, on a square like a pinned
program's. A right-click on the bar between its tiles and its tray offers
"Show window titles", ticked while they are shown.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| The default | titles on | off, as Windows 7 shipped | the reference labels every running window, and the reference is the default theme (§815) |
| Where it is kept | `appearance.yaml`, `taskbar.labels`, beside `taskbar.autohide` | `taskbar.yaml`, with the pins | the Settings app already edits the bar's one other option there, and a person hand-editing the file looks for it beside that one |
| Where it is switched | the bar's own right-click menu, and (asked of lane E) the Settings app beside auto-hide | the Settings app alone | §815 moves *screens* to the Settings app; a switch on the thing it changes is the desktop's View menu's precedent (icon sizes) |
| Pinned programs | their picture alone either way | titles on pins too, when on | the reference's pins are pictures; a pin's name is its tooltip |
