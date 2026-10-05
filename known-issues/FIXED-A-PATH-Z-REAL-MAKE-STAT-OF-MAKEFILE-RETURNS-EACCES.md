## FIXED-A-PATH-Z-REAL-MAKE-STAT-OF-MAKEFILE-RETURNS-EACCES (found by lane B, 2026-08-20; fixed by lane A, 2026-08-21)

**Owner: lane A** (`kernel/**`). Filed as
`requests/b-a-path-z-real-make-fails-because-stat-of-Makefile-returns-eacces.md`,
answered in `requests/a-b-make-eacces-was-the-abi-switch-your-rootfs-change-made.md`.

**Cause: `/bin/make` is not a Linux binary any more.** `create-ext4-rootfs.sh`
stages the Debian glibc `make` and then *overwrites* it with
`build/spike/make-slateos.elf` (GNU make 4.4.1 linked against our `libc.a`), so
the binary that runs is static, non-PIE and speaks the **native** syscall ABI.
Native `sys_fs_stat` requires `Rights::METADATA`; the Linux-ABI stat path checks
nothing. The test granted `READ | WRITE`, which was sufficient only while make
came in the Linux door.

**Fix:** the test grants `READ | WRITE | METADATA`, as every other native Path-Z
test in the file already does, and its doc comment now names the ABI switch.
`Rights::METADATA`'s own doc — which said "Modify metadata" while all eight
gates that check it are metadata *reads*, and the two metadata *writes* are
gated on `WRITE` — was a contributing cause of the misdiagnosis and has been
corrected to describe the bit the code implements.

**The fix reached one of two tests; completed 2026-08-21 in `e066c8990`.**
`0153b147c` fixed `self_test_linux_real_glibc_make` and left
`self_test_linux_real_glibc_make_cc` (same file, ~1600 lines later) still
granting `READ | WRITE`, so `make: stat: /cap.mk: Permission denied` kept
failing that rung of the boot test for a day. The reason is worth naming
because it will recur: the *report* named one test, and the report was treated
as the extent of the bug. It was not — it was the extent of what lane B
happened to be running. The habit that catches this is to grep for the
*mechanism* (`ResourceType::File` grants that omit `METADATA` in a native-ABI
launch) rather than to fix the symptom that was reported. These are the only
two tests in the tree that run `make` (`grep 'b"make"' spawn.rs` — two hits);
both now grant `METADATA`.

Running that mechanism-grep to its end is *not* cheap, which is the honest
caveat: `Rights::READ | Rights::WRITE` without `METADATA` matches ~85 sites in
`spawn.rs` alone, and the overwhelming majority are Linux-ABI tests that
correctly need nothing. Only a native-ABI binary that stats is affected, and
there is no grep that separates those two populations — which is precisely why
Q56 exists. What bounds the live damage is the boot test rather than the grep:
it exercises every one of those launch sites, so any other instance would
already be failing a rung with a `stat`/`Permission denied` symptom. As of
`e066c8990` none does.

**The fix worked, and the rung still fails — for a different reason.** Worth
recording precisely, because "still red after the fix" reads like a failed fix
and this was not one. Before, `make-drives-tcc` died at `make: stat: /cap.mk:
Permission denied` without parsing anything. After, make parses the makefile
and gets as far as running `/bin/tcc -c /cap-a.c -o /cap-a.o` — then the child
faults at `rip=0x10315d4, addr=0x8, exit code=Some(-8)`. That triple is
*byte-identical* to the one its sibling `self_test_linux_real_glibc_make` hits,
i.e. lane B's `BUG-POSIX-SPAWN-FILE-ACTIONS-IS-4624-BYTES-IN-AN-80-BYTE-SLOT`.
So the boot's failure *count* did not drop, but the number of distinct causes
went from two to one: both `make` rungs now fail in the same place, in lane B's
tree, and nothing lane A owns is implicated. A fix that converges two symptoms
onto one known cause is progress even though the tally is unchanged — and the
tally is why it would have been easy to record this as "no effect."

**What it exposed and did not fix:** the same operation requires a capability
under the native ABI and nothing at all under the Linux one. Put to the operator
as **Q56** in `open-questions.md` rather than decided, because closing it means
granting `METADATA` at ~50 launch sites across two lanes.

**Diagnostic note worth keeping.** Two probes were added while chasing this — on
the `Err` return of `stat_meta_for_path`, and on the return value of every
stat-family Linux syscall — and *both staying silent* is what identified the
cause. An absent log line was the evidence; the same is true of the
`Detected Linux x86_64 ABI binary` line, whose absence in the make region is the
one-line tell for this whole class of confusion.

**Original report follows.**

**What.** `self_test_linux_real_glibc_make` (`kernel/src/proc/spawn.rs:27361`)
fails on every boot. The kernel stages a two-line Makefile at `/Makefile` with
`Vfs::write_file` and runs the real Debian `make -f /Makefile all` on it; make
prints

```
make: stat: /Makefile: Permission denied
make: *** No rule to make target '/Makefile'.  Stop.
```

and exits 2. The first line is GNU make's `perror_with_name("stat: ", …)` in
`remake.c::f_mtime`, so a ring-3 `stat("/Makefile")` returned **EACCES**
(`KernelError::PermissionDenied` — nothing else maps to EACCES in
`linux_errno_for`). `f_mtime` treats a failed `stat` as "does not exist", the
makefile is a goal in make's remake pass with no rule to build it and is not
`dontcare`, so make calls `fatal()` and dies before evaluating `all`. That is
why nothing downstream — the recipe, `/bin/sh`, `/bin/emit`, `/make-out.txt` —
appears in the log.

**Not a regression.** The test has never passed on `lane-b`: it ran for the
first time in the 2026-08-20 boot, because the rootfs only just gained
`/bin/make`. Deterministic across both boots that day.

**Ruled out** (details and line citations in the request): the capability
grant (`(File, 1u64, READ|WRITE)` is what all 47 Path-Z tests use, and the
Linux-ABI stat path has no `require_cap_type` at all); path-based stat in
general (`[ -f /bin/dash ]` and `[ -d /bin ]` both pass in the same boot);
"files directly under `/`" (`/slateos-test-mmap.dat` is kernel-written at the
root and opens fine from ring 3); the syscall shim (`sys_newfstatat` and
`sys_statx` both reach `stat_meta_for_path` with no permission check);
`check_file_tags` (not called by `Vfs::metadata_resolved`, and it bypasses for
uid 0); memfs (`MemFs::metadata` has no permission logic, and new files default
to `0o644`); and namespaces (`NsRule::Hide` is the only denial and nothing
outside `namespace.rs` constructs one).

**Remaining hypotheses.** Something on `resolve_follow`/`resolve_inner` treats
a freshly written, not-yet-cached root child differently (`write_file`
invalidates only the *negative* dcache prefix), or a read-side hook fires —
`fs::atime`'s relatime update on stat is the best-shaped candidate, since it
would make a read path perform a write.

*(Both hypotheses were wrong; neither was involved. So was the "ruled out"
verdict on the capability grant — accurate for a Linux-ABI process, and the
process was not one. See the cause section at the top.)*

**How to close it in one boot:** log the `KernelError` and path where
`stat_meta_for_path` (`kernel/src/syscall/linux.rs:19032`) returns `Err`.
