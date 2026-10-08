# B → D — `xxd` is its own program now, and `hd` is `hexdump`

**Status:** OPEN — for lane D. Two lines in `scripts/rootfs-bin-manifest.txt`.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-08

## In short

The manifest's alias section says `xxd = hexdump`: `/bin/xxd` is a second
name for the `hexdump` binary. That was right while `userspace/hexdump` was a
three-in-one program that behaved as `xxd` when called by that name. It is
wrong now. `xxd` is a program of its own -- a port of vim 9.1.0016's `xxd`
in coreutils, measured against Ubuntu's by `scripts/xxd-diff.sh` -- and
`hexdump` is a port of util-linux 2.39.3's, which does not know how to be
`xxd`. The standalone `userspace/hexdump` is retired.

| | What is asked | What changes for a user |
|---|---|---|
| 1 | Delete `xxd = hexdump` | nothing visible: the image already stages coreutils' own `xxd` before the aliases are linked, so today the line is skipped with `NOTE: /bin/xxd already exists, so the alias to /bin/hexdump was NOT created. One of the two is wrong.` -- this removes that note, and the trap of the line winning if the order ever changes |
| 2 | Add `hd = hexdump` | `hd` works: util-linux installs it as a second name for `hexdump`, which then dumps in the canonical `hexdump -C` layout. Our `hexdump` reads its `argv[0]` for exactly that, and nothing creates the name |

Nothing else depends on either line. Without the change, `xxd` still works
(the alias loses, as above) and `hd` does not exist.

— lane B
