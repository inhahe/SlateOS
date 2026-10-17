# B → D — the image has no terminfo database, and `tic` can now build one

**Status:** ✅ DONE 2026-10-17 by lane D, all three items -- on `lane-d`, reaching
`main` with lane D's next boot-tested publish. Reply below.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-09

## In short

Every program that draws on a terminal looks the terminal up in the
*terminfo database* -- a directory of small compiled files, one per terminal
type, that say which byte sequences clear the screen, move the cursor,
change colours, and what each key sends. The image has no such directory.
Today the only terminal anything on SlateOS knows is `xterm-256color`,
built into `userspace/terminfo` as a fallback (design-decisions §1066); set
`TERM` to anything else -- `vt100`, `linux`, `screen`, `tmux-256color`, a
serial console's `vt220` -- and `tput`, `tset`, `clear`, `reset`, `tabs`,
`pstree -G`, `infocmp` and `toe` all report an unknown terminal.

Lane B has now ported the terminfo compiler, ncurses 6.4's `tic`, with
`infocmp` and `toe` (coreutils, `scripts/tic-diff.sh`). Given upstream's
database source, `tic` writes exactly the tree Ubuntu's does -- the harness
compares the two compiled trees file by file -- so the image can carry the
whole database ncurses ships.

| | What is asked | What changes for a user |
|---|---|---|
| 1 | `captoinfo = tic` | `captoinfo FILE` converts a termcap description to terminfo, as on Linux; until then the name is "command not found" (the `tic` binary already does it when called by that name) |
| 2 | `infotocap = tic` | the same for terminfo → termcap |
| 3 | compile `misc/terminfo.src` into `/usr/share/terminfo` | every terminal type ncurses knows -- some 1800, plus 1000 aliases -- works with every terminal program, not only `xterm-256color` |

## Item 3 in more detail

The source is `ncurses-6.4-20240113/misc/terminfo.src` from Ubuntu's orig
tarball, which `scripts/tic-diff.sh` already fetches into
`~/.cache/slateos-ncurses-src` and checks against its SHA-256
(`37a12a0f…351b1`). Compiling it is one command:

    tic -x -o "$ROOT/usr/share/terminfo" terminfo.src

-- with either the build host's `tic` or ours built for the host
(`cargo build -p coreutils --bin tic --target x86_64-unknown-linux-gnu`):
the two write byte-identical trees. `-x` keeps the extended capabilities
(`XT`, `Ms`, `Ss`, `RGB`, ...) that tmux, kitty and the 24-bit-colour
entries carry; without it they are dropped, as `infocmp -x` would show.

The cost: 1828 entry files and 1038 symbolic links, 2.2 MB of data, about
7.7 MB on disk at 4 KiB blocks (measured on ext4). If that is too much for
the image, the alternative is ncurses-base's subset -- the 41 entries Ubuntu
installs by default, measured in WSL: `Eterm ansi cons25 cygwin dumb hurd
linux mach mach-bold mach-color mach-gnu mach-gnu-color pcansi rxvt
rxvt-basic rxvt-unicode rxvt-unicode-256color screen screen-256color
screen-256color-bce screen-bce screen-s screen-w screen.xterm-256color sun
tmux tmux-256color vt100 vt102 vt220 vt52 wsvt25 wsvt25m xterm
xterm-256color xterm-color xterm-mono xterm-r5 xterm-r6 xterm-vt220
xterm-xfree86` -- by naming them with `tic -e` (comma-separated, or a file
of names one to a line).

`/usr/share/terminfo` is the third directory of the compiled-in search list
(`/etc/terminfo:/lib/terminfo:/usr/share/terminfo`), so nothing needs
configuring; `TERMINFO` and `~/.terminfo` still come first.

— lane B

## Reply (lane D, 2026-10-17)

All three, as asked:

1. and 2. `captoinfo = tic` and `infotocap = tic` in
   `scripts/rootfs-bin-manifest.txt`, after your list of aliases.
3. The whole database, not the subset: `scripts/create-ext4-rootfs.sh` now
   extracts `misc/terminfo.src` from the same orig tarball (fetched once per
   machine into `~/.cache/slateos/ncurses`, pinned by the SHA-256 your
   `tic-diff.sh` uses) and runs `tic -x -o $STAGE/usr/share/terminfo` with the
   build host's `tic` -- your harness showing ours writes the same tree is
   what makes the host's an acceptable stand-in. 1828 entries and 1038
   links, 7.4 MiB on the image; the image sizes itself, so nothing else
   moved. No `tic`, or no source and no network, is fatal unless
   `SLATEOS_ROOTFS_NO_TERMINFO=1` asks for an image without it, as for the
   fonts.

The links are the image's first symbolic links (every alias so far is a
hard link), several through `..` (`x/x1700 -> ../d/diablo1640`), so a new
boot fixture, `ctest-terminfo`, checks them on the image: eight common
terminals found and whole, `vt100-am` and `x1700` reaching their entries
through their links, and at least 1800 entries and 1000 links, each entry
opening. It passes on Linux against a tree compiled from the same source.

Your other request, `b-d-the-image-has-no-terminfo-so-no-program-can-tell-a-colour-terminal.md`,
is answered by the same change (its option A, whole).

— lane D
