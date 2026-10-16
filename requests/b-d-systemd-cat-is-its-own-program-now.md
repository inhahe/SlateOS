# B → D — `systemd-cat` is a program of its own now, not a name of `systemctl`

**Status:** OPEN — for lane D. One line in `scripts/rootfs-bin-manifest.txt`.

**From:** lane B · **To:** lane D · **Filed:** 2026-10-09

## In short

The manifest's alias section makes `systemd-cat` a second name for the
`systemctl` binary (`systemd-cat = systemctl`). That was right while
`userspace/systemctl` answered to `systemd-cat` by reading its own name -- a
personality that copied standard input into the journal and did nothing else.
Lane B has ported systemd 255's `systemd-cat` into coreutils
(`userspace/coreutils/src/bin/systemd-cat.rs`): it runs a command with its
output in the journal, as upstream does, measured against Ubuntu's by
`scripts/systemd-cat-diff.sh`. `systemctl` no longer answers to the name.

| | What is asked | What changes for a user |
|---|---|---|
| 1 | Delete `systemd-cat = systemctl` | nothing visible: coreutils' own `systemd-cat` is staged before the aliases are linked, so today the line is skipped with `NOTE: /bin/systemd-cat already exists, so the alias to /bin/systemctl was NOT created. One of the two is wrong.` -- this removes that note, and the trap of the line winning if the order ever changes: `systemctl` called by that name now behaves as `systemctl` |

Nothing else needs adding: like every program the workspace builds, the new
`systemd-cat` goes on the image without a line of its own (design-decisions
§1053, §1164).
