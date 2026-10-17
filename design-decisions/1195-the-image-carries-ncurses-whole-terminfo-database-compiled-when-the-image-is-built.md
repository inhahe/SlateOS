## 1195. The image carries ncurses' whole terminfo database, compiled when the image is built

**Date:** 2026-10-17 · **Lane:** D · **Decided by:** Claude (autonomous) -- lane B's
request left the choice between the whole database and a subset to lane D.

**In short:** programs that draw on a terminal look the terminal's type
(`TERM`) up in the terminfo database, which says what bytes clear the screen,
move the cursor or change colours. The image had none, so every terminal but
the one built into `userspace/terminfo` (`xterm-256color`, §1066) was
"unknown". The rootfs recipe now compiles ncurses' whole database -- 1828
terminal types and 1038 other names for them, 7.4 MiB on the image -- into
`/usr/share/terminfo`, from upstream's source pinned by its SHA-256, with
the build host's `tic`. A user sees `linux`, `screen`, `tmux-256color`,
`vt220` and the rest work, as on Linux.

**Alternatives:**

- **ncurses-base's 41 entries** (what Ubuntu installs by default; `tic -e`).
  About a tenth of the space; but `ssh` from an older terminal or an unusual
  emulator meets "unknown terminal", which is what the request was about, and
  Debian's split exists to keep minimal containers small, which this image is
  not.
- **Our own `tic`, built for the host, compiling it.** The image would not
  depend on the build host's ncurses at all; but the recipe would need a
  host Rust build of `coreutils` in WSL, which nothing else in it needs, and
  lane B's `scripts/tic-diff.sh` already shows the two writing the same tree
  byte for byte. The recipe prints the host `tic`'s version beside the count.
- **Compiled files committed to the tree.** No build-time tool or network;
  but 2866 binary files in git's history for good, as §1112 refused for the
  fonts.
- **Compiled at first boot by our `tic` on SlateOS.** Exercises the port;
  but every boot until then lacks the database, and the image's root is not
  where a first boot should be writing 7 MiB.

**What holds it:** the recipe refuses an image without it unless
`SLATEOS_ROOTFS_NO_TERMINFO=1` asks, checks six common entries were written,
and refuses a source whose SHA-256 is not the pinned one. The boot fixture
`ctest-terminfo` checks the database on the booted image, including the
image's first symbolic links, which the other names are.
