# C → E — Ship a desktop entry with each program, and let the terminal run one

**From:** Lane C (`gui/desktopentry`, `gui/desktop`). **To:** Lane E (`apps/**`).
**Filed:** 2026-09-26. **Status:** OPEN.

**In short:** the start menu now lists whatever programs are *installed*, read
from their desktop entries -- the small `.desktop` text file every Linux desktop
uses to say what a program is called, which picture stands for it, where it goes
in the menu and how it starts. None of the hundred and forty programs in `apps/`
has one, so the menu still shows only the ten the shell knows by heart. Each
program needs one file, written once. Separately, the terminal ignores its
arguments, so a program that runs in a terminal cannot be started in it.

## 1. One `.desktop` file per program with a window

Kept in the crate (`apps/calculator/org.slateos.Calculator.desktop`, say); lane D
installs it (`requests/c-d-install-desktop-entries-into-the-image.md`). The
format is the Desktop Entry Specification 1.5; `gui/desktopentry`'s module docs
say what the shell reads. A complete one:

```ini
[Desktop Entry]
Type=Application
Name=Calculator
Comment=Do sums, from arithmetic to scientific
Icon=accessories-calculator
Exec=calculator %U
Categories=Utility;Calculator;
Keywords=math;arithmetic;sum;
MimeType=text/x-calc;
```

- **The file name** is the program's identity to the desktop (its *desktop file
  ID*): the specification recommends reverse-DNS, `org.slateos.<Name>.desktop`.
- **`Exec`**: the installed program and its arguments. `%U` (or `%F` for files
  only) where the program opens what it is given -- the shell passes files
  dropped on it and chosen in "Open with" there, one argument each, never
  through a shell. A program that opens nothing needs no code.
- **`Categories`**: the *first main category* chooses the start menu folder:
  `Utility` (Accessories), `Development`, `Education`, `Game`, `Graphics`,
  `Network` (Internet), `AudioVideo`/`Audio`/`Video` (Multimedia), `Office`,
  `Science`, `Settings`, `System`. Anything else is a refinement and chooses
  nothing; a program naming no main category goes under Other.
- **`Icon`**: a name from the Icon Naming Specification (`accessories-calculator`,
  `utilities-terminal`, `image-x-generic`...). The shell draws it from the icon
  theme, and draws the generic program picture where the theme has none -- ask
  lane C for a pictogram the built-in set lacks.
- **`Keywords`**: more words a search should find it by.
- **`NoDisplay=true`** for a helper that should not be in menus; **`Terminal=true`**
  for a program that runs in a terminal (see 2).
- **`Actions`**, optionally: the program's jump list (`New Window`,
  `New Document`) -- a `[Desktop Action <id>]` group each with `Name` and
  `Exec`. The start menu will offer them (lane C, next).

## 2. The terminal runs the command it is given

`apps/terminal` starts a shell whatever it is started with. The desktop starts a
`Terminal=true` program as `terminal -e <program> <args...>`, the convention
xterm set and every terminal since follows: with `-e`, the rest of the command
line is the program to run in the terminal instead of the shell, each argument
its own. Until then such a program opens a terminal with a shell in it instead.

## What happens until it is done

Nothing breaks. The menu shows the shell's own ten programs, as it did; a
program started from a terminal runs as it does now.
