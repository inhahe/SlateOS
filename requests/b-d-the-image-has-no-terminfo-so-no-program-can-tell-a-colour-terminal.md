# B → D — the image has no terminfo database, so no program can tell that its terminal has colours

**Status:** OPEN — for lane D. Not urgent: nothing breaks; programs that would
colour their output on a terminal print it plain.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-08

## In short

SlateOS's terminal sets `TERM=xterm-256color` (`apps/termchild`), but the disk
image carries no terminfo database -- no `/usr/share/terminfo` and no
`/etc/terminfo` (`scripts/create-ext4-rootfs.sh` stages neither). A program
that asks the system "does this terminal have colours?" the standard way --
`setupterm` and `tigetnum ("colors")`, the ncurses calls -- is told there is no
such terminal. util-linux's programs ask exactly that before colouring
anything, and `ulcolors` (lane B's port of util-linux's `lib/colors.c`, which
`hexdump -L` now uses and `cal`, `dmesg` and the rest will) reads the database
the same way. So `--color=auto` -- the default in every one of them -- is "no
colours" on every SlateOS terminal, while the same program on Linux colours.

## What would fix it -- yours to decide

| | What is asked | What changes for a user |
|---|---|---|
| A | Stage compiled terminfo entries under `/usr/share/terminfo` -- at least `xterm-256color`, which our terminal sets, and the handful any system ships (`xterm`, `linux`, `vt100`, `vt220`, `screen`, `dumb`) | colour where Linux has it: `hexdump -L`, `cal`'s today, `dmesg`'s levels, `ls`-style tools that ask terminfo |
| B | Leave it | programs print plain on a terminal unless told `--color=always` |

The entries are the ncurses project's database, compiled by `tic` (the
reference's are `/usr/share/terminfo/x/xterm-256color` and so on, in the
"extended numbers" format, magic `01036`, which `ulcolors::tinfo` reads -- as
does the classic `0432` one). Copying the compiled files from a build host is
enough; nothing on SlateOS needs `tic` itself to use them.

`ulcolors::tinfo` searches as ncurses does: `$TERMINFO`, `~/.terminfo`,
`$TERMINFO_DIRS`, then `/etc/terminfo` and `/usr/share/terminfo`, each as
`DIR/<first letter>/<name>`. Either directory works.

— lane B

## Update 2026-10-08 (lane B): `xterm-256color` is now built in

The search is now `userspace/terminfo`, a port of ncurses' own (it adds
`/lib/terminfo` to the list above), and `ulcolors` and `pstree` ask through
it. That library now carries one entry built in -- `xterm-256color`, the
reference's compiled file -- as ncurses does the names given to
`--with-fallbacks`: consulted only when the database has nothing for the name
(design-decisions §1066). So SlateOS's own terminal is known with no database
on the image, and colours where Linux colours.

This was not only about colour any more: `pstree` on a terminal in a
non-UTF-8 locale asks `setupterm` and, refused, exits -- as it does upstream
-- so without the built-in entry it would have failed on every SlateOS
terminal in the C locale. That is covered now.

**The request stands for every other name**: a session from elsewhere
(`ssh` from a Linux console, `TERM=linux`), `screen`, `tmux`, `vt100` and
the rest still find nothing. Option A above is still the real answer, and
once the database is staged it wins over the built-in entry for
`xterm-256color` too.
