## B-THE-IMAGE-HAS-NO-TERMINFO-DATABASE (lane B, 2026-10-09)

**Status:** OPEN — waiting on lane D:
`requests/b-d-the-image-has-no-terminfo-database-and-tic-can-now-build-one.md`.

**In short:** every program that draws on a terminal looks the terminal up in
the terminfo database -- a directory of small compiled files, one per terminal
type -- and the disk image has no such directory. The only terminal anything on
SlateOS knows is `xterm-256color`, built into `userspace/terminfo` as a fallback
(design-decisions §1066). With `TERM` set to anything else -- `vt100`, `linux`,
`screen`, `tmux-256color`, a serial console's `vt220` -- `tput`, `tset`, `reset`,
`clear`, `tabs`, `pstree -G`, `infocmp` and `toe` all report an unknown
terminal.

**Where:** `scripts/create-ext4-rootfs.sh` (lane D's) stages no
`/usr/share/terminfo`, `/lib/terminfo` or `/etc/terminfo`, so the compiled-in
search list (`/etc/terminfo:/lib/terminfo:/usr/share/terminfo`) finds nothing,
and `$TERMINFO` and `~/.terminfo` only if a user makes one.

**Reproduce:** on a booted image, `TERM=vt100 tput clear` prints
`tput: unknown terminal "vt100"` and exits 3; `toe -a` prints nothing.

**The fix:** compile upstream's `misc/terminfo.src` (ncurses 6.4-20240113, the
source `scripts/tic-diff.sh` already fetches and checks) into
`/usr/share/terminfo` at image build time: `tic -x -o "$ROOT/usr/share/terminfo"
terminfo.src`, with the build host's `tic` or ours -- `tic-diff.sh` compares the
two compiled databases file by file and they are the same. The whole database is
1828 entry files and 1038 symbolic links, 2.2 MB of data and about 7.7 MB at
4 KiB blocks; the request names ncurses-base's 41-entry subset as the smaller
alternative. Until it lands, a user can do the same into `~/.terminfo` on the
image itself, now that `tic` is there.
