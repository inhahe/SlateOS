# B → D — `clear` and `tset` are programs of their own now, and `reset` is `tset`

**Status:** OPEN — for lane D. Three lines in `scripts/rootfs-bin-manifest.txt`.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-08

## In short

The manifest's alias section makes `clear`, `reset` and `tset` second names
for the `tput` binary. That was right while `userspace/tput` was a four-in-one
program that behaved as each of them when called by its name. It is wrong now.
ncurses ships `clear` and `tset` as programs of their own, and `reset` is
`tset` by another name; lane B has ported all three from ncurses 6.4 into
coreutils -- `tput`, `clear` and `tset`, measured against Ubuntu's by
`scripts/tput-diff.sh` -- and retired `userspace/tput`.

| | What is asked | What changes for a user |
|---|---|---|
| 1 | Delete `clear = tput` | nothing visible: coreutils' own `clear` is staged before the aliases are linked, so today the line is skipped with `NOTE: /bin/clear already exists, so the alias to /bin/tput was NOT created. One of the two is wrong.` -- this removes that note, and the trap of the line winning if the order ever changes |
| 2 | Delete `tset = tput` | the same, for `tset` |
| 3 | Change `reset = tput` to `reset = tset` | `reset` becomes ncurses' `reset`: it puts the line settings back into a sane state, sends the terminal's reset strings, and reports the erase, kill and interrupt characters it changed, taking `tset`'s options (`-e`, `-i`, `-k`, `-Q`, `-q`, `-s`, ...). Until then `reset` is `tput` in its `reset` alias mode, which sends the reset strings too but takes `tput`'s options and reports nothing |

`tput` keeps answering to `clear`, `init` and `reset` when it is called by
those names, as ncurses' does; nothing needs that, and no line should make
it so.

A fourth program arrives with them, ncurses' `tabs` (setting the
terminal's hardware tab stops). It needs no line: every program the
workspace builds goes on the image (design-decisions §1053).

— lane B
