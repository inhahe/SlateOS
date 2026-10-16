### A-KSHELL-TOUCH-DATED-FILES-1970 -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED on lane-a-wip 2026-10-02, awaiting a boot.

**In short:** kshell's `touch` of an existing file without `-d` set its
times to `hpet::elapsed_ns` -- the time since boot -- so the file showed a
date in the first minutes of 1970. It now asks for `TIME_NOW`, which the VFS
makes the wall clock.
