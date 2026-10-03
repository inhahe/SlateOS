## 879. The start menu in two columns: the programs on the left, the places and the power on the right

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous), within the operator's direction that the Aero reference is the default look (§815, §816) &middot; **Lane:** C

**In short:** The start menu was one list, 300 by 400 pixels, with the search
field at the top and Power, Settings and Terminal squeezed into a footer. It is
now the reference's two columns (`Aero Desktop (offline).html`,
`.aero-start-menu`): the programs on the left with the search field at their
foot, and on the right who is signed in, their folders, Settings, a terminal
and the power button.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Size | 524 by 566, scaled, and clamped to the room above the taskbar | keep 300 by 400 | the reference's; a second column needs the width, and the list keeps its row height, so more programs show at once. |
| Where is the search field? | at the foot of the programs, as in the reference | at the top, where it was | the reference's, and where a hand coming from the taskbar already is. Typing with the menu open searches, as before; the field's place changes nothing about that. |
| What is in the places column? | the user; Home, Documents, Pictures, Music, Downloads; Settings; a terminal; the power button at the foot | the reference's full list (Games, Control Panel, Devices, Help and Support) | only places this system has: a folder opens in the file manager, and Settings and the terminal are `design.txt` line 721's. A place that opened nothing would be the defect the power menu had. |
| Which folders? | `$HOME/<Name>`, the names the desktop's Documents icon uses | XDG user-dirs (`user-dirs.dirs`) | nothing on SlateOS writes a user-dirs file, and the icon and the place should open the same folder. When there is one, both should read it -- one change. |
| The user's picture | the first letter of their name on the accent, as the login screen draws a user without a picture | an account picture | none is saved yet (`TD-C-THE-ACCOUNT-PICTURE-IS-CHOSEN-AND-NEVER-SAVED`). The name is the account's display name, set when they sign in, or the process's user when there is no login screen. |
| A menu too short for every place | the lower places are left out | shrink them, or let them run over the power button | a place drawn over the power button would take its press; a shrunken one could not be read. Scrolling the places is not worth it for seven. |
| The two columns' colours | the programs `base`, the places `mantle`, both at the panel's transparency | a second accent-tinted colour | two palette roles, so a light theme and a dark theme each get a pair that belongs together, and a colour theme can set both. |

### What is not done here

The pinned programs as a three-wide grid of tiles, jump lists from them, a
"Shut down" button with the other power actions behind a caret -- the
reference's -- and icons, which need an icon theme to draw from.

The Shut down button is held back on purpose, not for time. It shuts the
machine down on one press, and `powerctl` does not yet ask programs to close
first -- an application can now decline a close to ask about unsaved work,
but nothing asks it at shutdown -- so a stray press would lose whatever was
not saved. The power button opens the list, and Shut down is a second,
deliberate press, until shutting down asks first.

The tiles are held back for their icons: a tile is an icon above a name, and
without an icon theme a grid of names in boxes is a worse list.
