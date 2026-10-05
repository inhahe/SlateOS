## 1459. Cursor themes are the ones every Linux desktop uses: XCursor files, found by name

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C, with F and E

**In short:** The pointer could be any size and any of three colourings, but
only ever one shape set -- the compositor's own drawings. A user can now
choose a cursor theme, and the pointer's pictures come from it, animated ones
included (a spinning busy pointer). The themes are the format every Linux
desktop already uses, so a cursor theme made for GNOME or KDE -- Adwaita,
Breeze, Bibata and hundreds more -- installs here unchanged. With none chosen
the pointer is drawn as it is today. Reading the themes is done; drawing their
pictures is the compositor's (lane F) and the choice in Settings is lane E's.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| The format | XCursor, the one every Linux cursor theme ships | SVG cursors, as the icon axis uses SVG | a theme here should not have to be redrawn for this desktop; XCursor carries several sizes, hot spots and animation frames, which is everything a pointer needs. Scalable SVG cursors (KDE's `cursors_scalable`) can be added beside it later |
| Where themes are found | the SlateOS theme roots first, then `$XDG_DATA_HOME/icons`, `~/.icons` and each `$XDG_DATA_DIRS/icons` | the SlateOS theme roots only | the second list is where other desktops' themes are installed, in libXcursor's order; a theme folder here can still carry `cursors/` beside its `theme.yaml` |
| How a cursor is named | by its CSS name (`default`, `pointer`, `ns-resize`), then its older X11 and Qt names (`left_ptr`, `hand2`, `size_ver`), then each theme the `index.theme` inherits from | the exact name only, as libXcursor looks | themes ship both generations of names, and older ones only the older; trying a theme's every name before the next theme keeps one theme's look together |
| The size | the nominal size nearest the pointer's, the first of the nearest on a tie, every frame of it | scaling to the exact size | libXcursor's rule, so a theme looks here as it does elsewhere; the pictures come at their own size and scaling them further is the compositor's choice |
| No theme, or a cursor no theme has | no picture: the compositor's own drawing | X11's `default` theme, as libXcursor falls back to | this desktop's default pointer is its own, drawn at any size; a stray system-wide `default` theme should not override it |
| Trust | every position, length, side and hot spot checked; 32 MiB per file, 16 Mi pixels decoded, sixteen themes per lookup | trust the files | a theme is whoever installed it; a crafted file must not crash the compositor or make it allocate gigabytes, and inheritance must not loop |
