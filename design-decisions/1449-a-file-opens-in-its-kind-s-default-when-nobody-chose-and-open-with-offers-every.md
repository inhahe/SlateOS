## 1449. A file opens in its kind's default when nobody chose, and "Open with" offers every program that opens it

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** Double-clicking a picture on the desktop now opens it in the
Image Viewer even if you never chose a program for pictures -- before, the
desktop said nothing was set to open it, though SlateOS names a default for
every kind it knows. And a file's right-click menu has "Open with", listing
every program on the machine that can open it, the one a double-click would
use first, so a file can be opened in something else without changing any
setting.

**Open**, in order (`DesktopShell::open_path`):

| | |
|---|---|
| 1. The user's choice | `fileassoc.yaml`, by extension (`gui/associations`) |
| 2. A program | an executable file runs -- before the default, so a script is run as it always was, not opened in the editor |
| 3. The kind's default | `programs::default_for` (design-decisions §1425), through the program's own command line; a kind of text with no default of its own takes plain text's |
| 4. Nothing | "Nothing is set to open files of this kind", as before |

**Open with** (`DesktopShell::offer_open_with`): every program whose desktop
entry's `MimeType` lists the file's kind -- or `text/plain` for a kind of
text, or `application/octet-stream` for any file but a folder, the two
sub-classes the shared MIME-info specification gives every type -- the
program Open would start first, the rest by name, each with its picture.
For several files, the programs that open all of them; chosen, a program
starts on all of them as its command line says (`%F` once, `%f` once each).
Between Open and the icon's own items, before what programs add
(design-decisions §1448). Not offered when its one program is Open's own --
a folder's file manager -- since it would say only what Open does; offered
with one program when Open has none, as for a kind nothing opens by
default.

| Alternative | For | Against |
|---|---|---|
| **The default after running an executable** (chosen) | a script still runs on a double-click, as it did | a script with no executable bit opens in the editor, while one with it runs -- which is what the bit is for |
| The default before running an executable | a double-click never runs code by surprise | changes what every executable icon on every desktop does |
| Open with lists only the default | short | the point of the menu is the others |
| Open with lists every program | nothing hidden | a calculator offered for a PDF |

**Revisit if** associations come to be kept by MIME type rather than by
extension (the first step would then read the same key the third does), or
when the Settings application's program chooser exists -- a "Choose another
program" row, and an "always use this" choice, belong to it.
