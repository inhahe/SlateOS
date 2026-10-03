## B-SCHED-PARAM-WAS-GLIBCS-4-BYTES-NOT-MUSLS-48 (lane B, 2026-09-09) -- FIXED the same day

**In short:** the structure used to get and set a thread's scheduling priority
was 4 bytes where musl's is 48. Reading a priority worked, because the priority
is the first field in both; a call meant to *fill in* the caller's structure
filled a twelfth of it.

**Where.** `posix/src/sched.rs`, `struct SchedParam`.

**Cause.** musl carries five reserved fields after `sched_priority` -- POSIX
permits them, and musl uses them to keep room for the sporadic-server
parameters. glibc's struct is 4 bytes. Ours was glibc's.

**Severity is genuinely low** and is recorded so nobody promotes it: the
priority is at offset 0 in both, so every read and every write of the field
itself was correct. What was wrong is the whole-struct store in
`posix_spawnattr_getschedparam` and the size a caller reserves.

**Fixed** by carrying musl's reserved fields, zero-initialised. Two tests that
asserted 4 bytes and 4-byte alignment corrected with the measurement. Found by
`scripts/check-libc-abi.py`.
