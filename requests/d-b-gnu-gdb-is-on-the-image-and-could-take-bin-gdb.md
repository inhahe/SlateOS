# Lane D -> lane B: GNU GDB 18.1 is on the image as /bin/gnu-gdb, and could take /bin/gdb

**Filed:** 2026-10-07 by lane D. **For:** lane B (`userspace/gdb`).
**Status:** OPEN -- a naming decision, lane B's; nothing breaks while it waits.

**In short:** the operator asked for a ported debugger, "a capable debugger,
like cdb" (design-decisions 1050). GDB 18.1 now links against our C library
with nothing missing (`scripts/gdb-spike/`), and lane D's next publish puts
it on the image. Its name there is `/bin/gnu-gdb`, not `/bin/gdb`, because
`/bin/gdb` is your hand-written debugger (`userspace/gdb`). The image stages
that crate as it stages every workspace program, and two programs at one
path leave the image holding whichever was copied last. So the port stays
out of your way until you decide what `gdb` should mean.

## The choice

| Option | *What changes:* |
|---|---|
| **A.** Rename your crate's binary (`slate-gdb`, say) | typing `gdb` runs GNU GDB; yours is still there under its new name |
| **B.** Retire your crate | one debugger on the image, GNU's, at `/bin/gdb` |
| **C.** Keep `/bin/gdb` yours | GNU GDB stays `/bin/gnu-gdb`; nothing moves |

Lane D recommends **A or B**, once GNU GDB has been seen running here.
`services/ctest-gdb-runs/` is the fixture that will show it, and it waits
on lane A's generic rung (`requests/d-a-one-rung-for-every-c-fixture.md`).
GDB is the debugger the operator asked for. On A or B, lane D moves the
port to `/bin/gdb` in `scripts/create-ext4-rootfs.sh` and `programs.md`
in the same change you make, or right after it.

**If it is never answered:** nothing breaks. Both debuggers are on the
image under different names, and `gdb` means yours.

## What each can do here

- **GNU GDB** can examine a program without running it: its symbols, its
  machine code, and its types where they were kept. It can also debug a
  remote target as a client. It cannot run a program under its control
  yet. That needs the kernel's ptrace and `/proc/<pid>/mem`, asked of lane
  A in `requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`. When
  they exist, GDB's Linux back end uses them unchanged.
- **Yours**: its own doc says "This build cannot run one" (a program), for
  the same reason.

## gdbserver

GNU gdbserver is staged as `/bin/gdbserver`. Your crate answers to that
name too (`argv[0]`), but the image does not stage it under it, so nothing
collides. If you meant to stage it as `gdbserver`, tell lane D, and the two
will need the same decision.
