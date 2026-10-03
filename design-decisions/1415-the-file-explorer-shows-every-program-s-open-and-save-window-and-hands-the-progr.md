## 1415. The file explorer shows every program's Open and Save window, and hands the program only the file chosen

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended this option, C-Q30's A) &middot; **Lane:** C, with E, F, A and D

**In short:** A program's Open and Save window will no longer be drawn by the
program. The program asks; the file explorer -- a trusted part of the system --
opens its own window in "choose a file" form above the program; the user
browses with the explorer's columns, thumbnails and layouts; and the program is
handed the file chosen and nothing else. Today each program draws the window
from toolkit code and reads your folders itself to fill it, so it has to be
able to read any folder you might browse, and it sees every name in each one.
Under this decision a program needs no access to your files at all until you
choose one -- the arrangement macOS uses for its sandboxed programs and Linux's
Flatpak for its "portals", and the one that fits `design.txt`'s rule that a
program holds only what it was handed.

**The question and the options:** `open-questions.md` C-Q30 (now resolved).
The design says the explorer is "used as a file save or file(s) load dialog
for applications", and the detailed roadmap that the dialog "IS the file
explorer component"; what existed was a separate, plainer dialog in the
toolkit. Of the three ways offered:

| Option | What it would have meant | |
|---|---|---|
| **A. The explorer shows the window** | one implementation; a program handed only the file chosen | **chosen** |
| B. Every program carries the explorer's view | the same look, but 45 programs grow by the explorer's file readers and still need to read every folder you might browse | not taken |
| C. Keep two, restyle the dialog | they look alike; the dialog keeps three columns and one layout | not taken |

**What it takes, by lane:**

| Part | Lane |
|---|---|
| The explorer opens as a dialog: choose a file to open, or a place and name to save, with the program's filters, and Open/Save and Cancel | E (`apps/explorer`) |
| A program asks for one and gets the answer back: the request, the reply, and the toolkit call programs already make (`guitk::dialog::FilePicker` asks the explorer instead of drawing a dialog) | C (`gui/toolkit`), with E |
| The dialog is kept above the program that asked, and belongs to it | F (the window system) |
| "Handed only that file": passing an open file from the explorer to the program | A and D |

**Until the last part exists** the explorer hands back the file's name, which
is no weaker than today; the security gain arrives with it.

**Meanwhile:** the toolkit's own dialog stays, as the fallback where no
explorer can be asked. Lane C does not restyle its layout further -- it is to
be replaced -- but keeps it working: the typed paths of §1412 and the buttons
of §1414 stay.
