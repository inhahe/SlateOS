### A-RESTART-SENTINELS-MATCHED-512-BYTE-RETURNS-AND-THREE-NATIVE-ERRORS -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** when a signal interrupts a blocking system call, the kernel
marks the call's return value as "restart me" with a special number, as
Linux does (`ERESTARTSYS` and three others, 512-516). Every system call's
return passes a check for those numbers on its way back to the program, and
the check had two flaws. It accepted the number with either sign, so a call
that *succeeded* with a result of 512, 513, 514 or 516 -- a 512-byte read, a
512-byte write -- was taken for "restart me" and run again: the read's data
was lost to the next read, and the write was written again, forever. And
three native error codes are those very numbers -- `CrossDevice` (-512),
`StaleHandle` (-513), `NoAttribute` (-514) -- so a native `rename` across
devices, a stale handle, or a `getxattr` of a missing attribute restarted
forever instead of failing; and a Linux program asking for a missing
attribute got neither `ENODATA` nor an error, the case boot rq23 caught.

**Where:** `kernel/src/syscall/linux.rs`, `restart::is_sentinel` /
`sentinel_magnitude` / `restart_result`, called for every return from
`kernel/src/syscall/entry.rs`.

**Fixed:** a sentinel is now carried as `-(2^40 + n)` (`restart::encode`), a
value no syscall returns and no errno or native error code reaches, and only
a negative value can be one. Every producer already went through
`restart_result`, so nothing else changed. The restart self-test now checks
that 512-516 of either sign and the three native codes are not sentinels.
