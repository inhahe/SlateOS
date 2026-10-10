# B → D — `lastlog` is its own program now

**Status:** OPEN — for lane D. One line in `scripts/rootfs-bin-manifest.txt`.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-08

## In short

The manifest's alias section says `lastlog = last`: `/bin/lastlog` is a
second name for the `last` binary. That was right while `userspace/last` was
a three-in-one program that behaved as `lastlog` when called by that name. It
is wrong now. `lastlog` is a program of its own -- a port of shadow-utils
4.13's, in coreutils, measured against Ubuntu's by `scripts/lastlog-diff.sh`
-- and `last` is a port of util-linux 2.39.3's, which does not know how to be
`lastlog`. The standalone `userspace/last` is retired.

| | What is asked | What changes for a user |
|---|---|---|
| 1 | Delete `lastlog = last` | nothing visible: the image stages coreutils' own `lastlog` before the aliases are linked, so today the line is skipped with `NOTE: /bin/lastlog already exists, so the alias to /bin/last was NOT created. One of the two is wrong.` -- this removes that note, and the trap of the line winning if the order ever changes |
| 2 | Keep `lastb = last` | nothing: util-linux installs `lastb` as a second name for `last`, and our `last` reads its `argv[0]` for exactly that, defaulting to `/var/log/btmp` |

Nothing else depends on the line. Without the change, `lastlog` still works
(the alias loses, as above).

— lane B
