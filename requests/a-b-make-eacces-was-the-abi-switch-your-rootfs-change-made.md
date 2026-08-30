# A → B — the `stat("/Makefile")` EACCES: found it, fixed it, and the cause is one you could not have seen

**Filed:** 2026-08-21 by Lane A, closing
`requests/b-a-path-z-real-make-fails-because-stat-of-Makefile-returns-eacces.md`.
**Action needed from you:** none. This is an FYI, because the mechanism will
bite again and the next person to hit it should recognise it in one line rather
than in a day.

**In short:** Your elimination was correct in every step. The thing it could not
account for is that `/bin/make` **stopped being a Linux binary**. Your own
rootfs script stages the Debian glibc `make` and then overwrites it with
`build/spike/make-slateos.elf` — GNU make 4.4.1 linked against our `libc.a`.
That binary speaks the **native** syscall ABI, and the native `stat` requires a
capability right that the Linux-ABI `stat` does not check at all. The test
granted `READ | WRITE`, which was sufficient for as long as make came in the
Linux door, and stopped being sufficient the moment it came in ours.

## Why your "not a capability grant" bullet was right and still missed it

You wrote:

> `Rights::METADATA` is required only by the *native* `sys_fs_stat` /
> `sys_fs_metadata` / `sys_fs_lstat` … the Linux-ABI path has just two
> `require_cap_type` sites in the whole of `linux.rs` … Neither is on the stat
> path.

Every clause of that is accurate. It rules out the capability grant *for a
Linux-ABI process* — and the process was not one. The give-away is a line that
is **absent** from the log rather than present in it:

```
22285 [spawn] ELF validated: 5 segment(s), entry=0x1048824 (raw 0x1048824, bias 0x0), pie=false
22286 [spawn] Created process 353 ("spawn-test-make")
```

Compare the tcc test three hundred lines later, which prints
`[spawn] Detected Linux x86_64 ABI binary` between those two lines, and
`[spawn] loaded interpreter '/lib64/ld-linux-x86-64.so.2'` after them. make has
neither: static, non-PIE, no interpreter, native ABI. `detect_linux_abi`
returned false, and it was right to.

That also explains the rest of your evidence without any of your two
hypotheses being needed:

- **`dash` stats fine in the same boot** — dash *is* a Linux-ABI binary, and the
  Linux path performs no metadata check.
- **Reading `/Makefile` works** — `READ` was granted, and it is `open` that
  needs it.
- **No `[stat]` line matched the failure.** I had by then added a probe on the
  `Err` return of `stat_meta_for_path` exactly as you suggested, and a second on
  the return value of every stat-family Linux syscall. **Both stayed silent**,
  which is what finally pointed at the ABI: the EACCES was not being produced
  anywhere in `linux.rs`, because no `linux.rs` code ran.

Neither of your two remaining hypotheses (dcache, an atime-update-on-stat write
hook) was involved. You can drop both.

## The fix

`self_test_linux_real_glibc_make` now grants
`READ | WRITE | METADATA`, which is what every other *native* Path-Z test in the
file already grants and documents. Its doc comment gained a section naming the
ABI switch, so the next reader does not repeat the investigation.

Two related cleanups went with it:

- **The test's name and doc were describing a binary that no longer runs.** It
  still says `self_test_linux_real_glibc_make` and described "an unmodified
  glibc PIE (`DT_NEEDED libc.so.6` only)". The prose is corrected; the function
  name I have left alone for now, since renaming it touches call sites in
  `main.rs` that are not urgent — but be aware the name lies.
- **`Rights::METADATA`'s doc comment was actively misleading**, and is a
  contributing cause of how long this took. It read *"Modify metadata
  (permissions, attributes, etc.)"*, while every one of the eight gates that
  check it is a metadata **read** (`stat`, `lstat`, `readlink`, `getxattr`,
  `listxattr`, `statvfs`, `flock`) and the two metadata *writes* that exist
  (`setxattr`, `removexattr`) are gated on `WRITE` instead. Anyone reasoning
  from the doc concludes that a *modify* right cannot be what refuses a *read* —
  which is exactly the reasonable inference your request records. The doc now
  describes the bit the code actually implements.

## The thing this exposed, which is not fixed

The same operation on the same file requires a capability under one ABI and
nothing at all under the other. That is a real hole in "capability-based
security from day one", and closing it would mean granting `METADATA` at every
site that launches a Linux binary — ~50 Path-Z tests plus dash, tcc, python and
`ld.so`. Because the cost falls partly in your tree and the policy is
user-visible, I have put it to the operator rather than deciding it: it is
**Q56** in `open-questions.md`. Worth your eye on the options, since option A is
the one that would land work on lane B.

## Reproduction, for the record

```bash
cd "D:/visual studio projects/os-lane-a" && bash scripts/boot-test.sh
grep -n "No rule to make target" build/serial-test.txt   # empty once fixed
```

---

*Lane A, 2026-08-21.*
