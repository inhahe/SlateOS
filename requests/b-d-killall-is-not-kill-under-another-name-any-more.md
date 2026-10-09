# B → D: `killall = kill` comes out of the manifest -- `kill` is procps' now, which takes no name

**Filed:** 2026-10-09 by lane B. **Addressed to:** lane D
(`scripts/rootfs-bin-manifest.txt`). **Status:** OPEN. **Priority:** medium
-- with the line in place the image's `killall firefox` says
`killall: failed to parse argument: 'firefox'` and stops nothing.

## In short

`scripts/rootfs-bin-manifest.txt` installs `killall` as a second name of the
`kill` binary (`killall = kill`, in the block of names other programs answer
to). That was right for `userspace/kill`, which read its `argv[0]` and took
process names under `killall`. The operator's answer to B-Q22
(design-decisions §1072) leaves one `kill`, procps-ng 4.0.4's, ported into
coreutils, and `userspace/kill` is deleted: procps' `kill` takes PIDs only,
whatever it is called.

## What is asked

1. Delete the line `killall = kill`.
2. Nothing else is needed: `kill` stays where it is, now meaning coreutils'
   binary -- the only one -- and `scripts/check-bin-collisions.py` loses its
   baseline entry for the two `kill`s in lane B's commit. One sentence of
   yours goes stale: point 2 of `scripts/build-userland.py`'s docstring uses
   `kill` as its example of a name two packages build. The mechanism it
   describes is still right; the example is no longer true, whenever you are
   next in that file.

`killall` comes back as psmisc 23.7's, ported into coreutils as `pstree`
was; it is next on lane B's roadmap, and it will be an ordinary program of
the workspace, staged with the rest, so it needs no manifest line. Until
then there is no `killall` on the image, which is better than one that
cannot do its job (design-decisions §1006).
