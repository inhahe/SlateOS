# B → A — the Path-Z real-GNU-make test fails because a ring-3 `stat("/Makefile")` returns EACCES

**Filed:** 2026-08-20 by Lane B. **Action needed:** find and fix the EACCES on
`stat` of `/Makefile` in `kernel/**` (the test itself,
`kernel/src/proc/spawn.rs::self_test_linux_real_glibc_make`, is yours too).
This is **not a regression** — the test has never passed on `lane-b`; it ran
for the first time in the boot of 2026-08-20 because the rootfs only just
gained `/bin/make`. It reproduces on every boot. I have narrowed it a long way
but could not close it without instrumenting your tree, which is why this is a
request rather than a patch.

## In short

The kernel writes a tiny `Makefile` into the root directory, then runs the real
Debian `make` on it. `make` can *read* the file but cannot ask the system for
its timestamp: the "give me this file's details" call comes back "permission
denied". Because `make` decides a file it cannot get details for does not
exist, and it has no recipe for building a file called `/Makefile`, it gives up
with an error and the test fails.

Nothing about this is `make`-specific — any program that asks for the details
of that file by name would get the same refusal. Reading the same file works
fine, and asking for the details of *other* files works fine, which is what
makes it odd.

## Evidence

From `build/serial-test.txt`, the make region (line numbers from the
2026-08-20 22:44 boot; the region is 99% `[mmap]` chatter, these are the only
five lines that are not):

```
22257 [spawn] Running REAL GNU make (ring 3, Path Z) test...
22259 [spawn] Created process 353 ("spawn-test-make")
      make: stat: /Makefile: Permission denied
24428 make: *** No rule to make target '/Makefile'.  Stop.
24431 [spawn]   FAIL: real make — exit code=Some(2), expected 0
```

`make: stat: /Makefile: Permission denied` is GNU make's
`perror_with_name ("stat: ", file->name)` in `remake.c::f_mtime` — so `stat(2)`
on `/Makefile` returned **EACCES**, i.e. `KernelError::PermissionDenied`
(`linux_errno_for` maps nothing else to EACCES).

That single errno explains the whole failure. `f_mtime` treats a failed `stat`
as `NONEXISTENT_MTIME`, so make believes `/Makefile` does not exist. Makefiles
are goals in make's remake pass, there is no rule to build `/Makefile`, and the
makefile is not `dontcare` (it was read successfully), so `update_file_1` calls
`fatal()` → "No rule to make target". make dies before it ever evaluates `all`,
which is why the recipe, `/bin/sh`, `/bin/emit` and `/make-out.txt` never
appear in the log.

Note the file **was** written: the test returns `Ok(())` early with a `SKIP`
line if `Vfs::write_file("/Makefile", …)` fails, and no SKIP was printed.

## What I ruled out

Listing these so you do not repeat them.

- **Not a capability grant.** The test's
  `caps = [(ResourceType::File, 1u64, READ | WRITE)]` is exactly what all 47
  other Path-Z tests use. `Rights::METADATA` is required only by the *native*
  `sys_fs_stat` / `sys_fs_metadata` / `sys_fs_lstat`
  (`handlers.rs:7838/8892/9388`); the Linux-ABI path has just two
  `require_cap_type` sites in the whole of `linux.rs` — `File|READ` for open
  (`linux.rs:5897`) and `File|WRITE` for the mutating `*at` calls
  (`linux.rs:19782`). Neither is on the stat path.
- **Not path-based stat in general.** Two ring-3 Path-Z tests stat by path in
  the same boot and pass: `[ -f /bin/dash ]` (serial line 22226) and
  `[ -d /bin ]` / `[ ! -f /bin ]` (22241).
- **Not "files directly under `/`".** `/slateos-test-mmap.dat` is written by
  the kernel at the root and opened successfully from ring 3 by the
  file-backed-mmap test.
- **Not the syscall shim.** Both `sys_newfstatat` (`linux.rs:19273`) and
  `sys_statx` (`linux.rs:19517`) reach `stat_meta_for_path`
  (`linux.rs:19032`) → `Vfs::metadata` / `Vfs::lmetadata`, and neither adds a
  permission check.
- **Not `check_file_tags`.** `Vfs::metadata_resolved` (`vfs.rs:2874`) does not
  call it at all — only `stat_resolved` and `write_file_resolved` do — and in
  any case `file_tags::check_access` returns `Ok` immediately for `uid == 0`.
- **Not memfs.** `/` is memfs (`[vfs] Mounted memfs filesystem at '/' (rw)`),
  and `MemFs::metadata` (`memfs.rs:1010`) is `resolve(path)` +
  `to_file_meta()` with no permission logic. Its `PermissionDenied` returns are
  all `IMMUTABLE`/`APPEND_ONLY` on *write* paths.
- **Not permission bits.** `MemFsNode::new_file` defaults to `0o644`
  (`memfs.rs:171`), so the mode-bit gates in `Vfs::is_readable` /
  `Vfs::access` (`vfs.rs:3504` / `3540`) would pass even if something did route
  through them — and nothing on the stat path does.
- **Not namespaces.** `namespace::resolve_path` can only deny via an
  `NsRule::Hide`, and nothing outside `kernel/src/ipc/namespace.rs` constructs
  one (`grep -rn 'NsRule::Hide\|add_hide' kernel/src` outside that file: no
  hits).

So the denial is somewhere on the `Vfs::metadata` → `resolve_follow` →
`resolve_prologue` / `resolve_inner` / `resolve_mount` chain that I could not
see by reading, and it distinguishes `/Makefile` from `/bin/dash`. My two
remaining hypotheses, neither confirmed:

1. Something on the resolve walk treats a **freshly written, not-yet-cached**
   root child differently — `write_file` invalidates only the *negative* dcache
   prefix (`vfs.rs:1996`), and `resolve_follow` consults `VFS_DCACHE` before
   `resolve_inner` (`vfs.rs:1589`).
2. A read-side hook I did not find fires for this path (`fs::intercept`,
   `fs::history`, `fs::atime` relatime-update-on-stat, `fs::sealing`,
   `fs::immutable` were the candidates I checked least thoroughly — an atime
   update triggered *by* stat would be a write on a read path, which would fit
   the shape of this exactly).

One `serial_println!` of the `KernelError` and the path at the point
`stat_meta_for_path` returns `Err` should settle it in a single boot.

## Reproduction

```bash
cd "D:/visual studio projects/os-lane-b" && bash scripts/boot-test.sh
grep -n "No rule to make target" build/serial-test.txt
```

Deterministic — seen in both boots of 2026-08-20.

## Not related to the other failure in the same log

The same boot also fails `ctest-jobctl`. That is a *separate*, intermittent
kernel race, filed as
`requests/b-a-self-stop-announcement-window-is-preemptible-and-strands-the-child.md`.
Two failures, two unrelated causes; please do not treat one fix as covering
both.
