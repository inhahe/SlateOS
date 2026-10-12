# C → E — Ship a desktop entry with each program, and let the terminal run one

**From:** Lane C (`gui/desktopentry`, `gui/desktop`). **To:** Lane E (`apps/**`).
**Filed:** 2026-09-26. **Status:** DONE (lane E, 2026-10-09; reply at the end) --
on `main` with lane E's next publish.

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

## Lane E -- 2026-10-09: both done

**1. An entry for each program with a window: 136 of them.** Each is
`apps/<crate>/org.slateos.<Id>.desktop`, in the specification's reverse-DNS
form: `org.slateos.Calculator.desktop`, `org.slateos.Files.desktop` for the
explorer, and so on.

- **`Exec`** names the program as it is built: the package's name, so
  `tmux-app` and `sysinfo-app` for those two, whose packages are named so.
- **Field codes:** an entry has one only where its program opens what it is
  given. Nine do, through `app::ArgsOs`:
  - `%F` (several files): the editor, hex editor, music player, PDF viewer and
    video player;
  - `%f` (one): the image viewer, archive manager, explorer (a folder) and disk
    analyzer (a folder).
- **`MimeType`** is listed only for formats each program is known to open:
  - video player: WebM and Matroska, not MP4, whose usual H.264 does not decode
    yet;
  - image viewer: PNG, JPEG, GIF and BMP;
  - PDF viewer: PDF;
  - editor: plain text;
  - hex editor: any file;
  - archive manager: zip, tar, 7z, gzip, bzip2 and xz;
  - explorer and disk analyzer: folders.
- **Categories** start with a main category for every program, so none lands
  under Other. **Keywords** say what a search should find each by.
- **`NoDisplay=true`** for the lock screen and the app launcher, which are
  parts of the desktop rather than programs to start from a menu.
- **Icons** are names from the Icon Naming Specification. Two are not in the
  specification, though common themes carry them: `network-vpn` (VPN
  Connections) and `accessories-screenshot` (Screenshot). Your built-in set
  may lack these, and also `preferences-desktop-remote-desktop`,
  `x-office-drawing`, `internet-news-reader` and `weather-few-clouds`; the
  generic picture shows where it has none. If any is worth a pictogram, the
  list of every name used is
  `grep -h ^Icon= apps/*/org.slateos.*.desktop | sort -u`.

`apps/desktopentries`, a crate that is only tests, keeps them true through
your `desktopentry` reader. Every program with a window has exactly one entry
that `App::from_entry` accepts. Its `Exec` names that program's binary, and its
first main category is one the menu knows. It has an icon and a comment and is
not `Terminal=true`. An entry offers files (`%f`/`%F`) only if its program
reads `ArgsOs`'s arguments, and lists file types only if it offers files. No
two entries share an id, and no crate without a window has one. Its mutation
table breaks one entry seven ways, and each is caught.

**2. The terminal runs the command it is given.** `terminal [--display ADDR]
-e PROGRAM ARG...` runs PROGRAM on the terminal in place of the shell, with
each argument its own. It is never run through a shell, so nothing in the
arguments is split or expanded. PROGRAM is found along `PATH` as `execvp` finds
it (`termchild::find_program`), and its `argv[0]` is the name as given.
Everything after `-e` is the program's, `--display` included; only what comes
before `-e` is the terminal's. `-e` with nothing after it, or an argument the
terminal does not take, is refused by name. No entry here says
`Terminal=true` yet: every lane E program has a window of its own.
