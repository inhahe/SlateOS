### A-SPAWNED-CHILD-LED-ITS-OWN-SESSION -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a program started with `posix_spawn` (or any spawn call) led a
process group and a session of its own, where POSIX puts it in its parent's.
So it had no controlling terminal, a `^C` typed at the terminal -- sent to
the foreground group -- did not reach it, and it started with nothing
blocked, not its parent's mask. `fork` was right; only spawn was wrong.

**Where:** `kernel/src/proc/spawn.rs` (`start_job_and_signals`), and
`pcb::create`, which makes every process a leader -- right only for one the
kernel starts.

**Fixed:** a spawned child with a parent starts in the parent's group and
session with its blocked mask and ignored set (`pcb::inherit_job`,
`signal::start_spawned`), and the `posix_spawn` attributes
(`SpawnEx2Args`' six new fields) change them before it runs. Nothing in the
tree relied on the old behaviour: `login_tty` calls `setsid` itself. Tests:
`spawn`'s `test_spawn_job_and_signals` and `test_ex2_attrs`, `pcb`'s
`test_inherit_job`, the ring-3 probes `0x26`-`0x30` of
`build_spawn_ex2_abi_test_elf`.
