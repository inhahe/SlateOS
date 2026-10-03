### [D] TD-D-CWD-AND-UMASK-DO-NOT-SURVIVE-EXEC-OR-SPAWN — 2026-09-24 — FIXED (libc half 2026-09-25; takes effect with lane A's kernel half)

**Status:** FIXED in both halves, which reach `main` separately. Lane A's kernel
half is `ff5f98db8` on `lane-a` (design-decisions.md §960): native syscalls
1077-1079 and spawn inheritance. Lane D's libc half landed on `lane-d` on
2026-09-25:
- `chdir` (and `fchdir`, which goes through it) records the directory with
  `SYS_PROCESS_SET_CWD` *before* updating this libc's copy, so a refusal leaves
  the two agreeing.
- `umask` keeps `SYS_PROCESS_UMASK` current.
- `crt.rs` start-up reads both back (`init_cwd_from_record`,
  `init_umask_from_record`) before constructors and `main`.
- `posix_spawn`'s `addchdir_np` actions are applied in order. Each is resolved
  against where the previous ones left the child and checked to be a directory,
  and relative `open` actions after one follow it. The result goes to the kernel
  in `SpawnEx2Args::cwd_ptr`/`cwd_len`, through 559.

On a kernel without the record, all of this falls back to the old behaviour:
the calls answer "no such syscall", which is treated as "keep it in libc", and
`posix_spawn` then hands the kernel no directory (`kernel_keeps_cwd`). That is
the reason for the note at the end of this entry. Host tests cover every path
through a modelled kernel (`cwd_record::host`, `umask_record::host`,
`host_dirs`). No ring-3 rung exercises it yet.

**Still missing, smaller:** `posix_spawn_file_actions_addfchdir_np` (glibc
2.29+) does not exist, so a program that uses it fails to link, loudly.
`addchdir_np`/`addopen` paths are capped at 255 bytes (`ACTION_PATH_MAX`) where
glibc allows `PATH_MAX`.

*(The entry as filed follows.)*

**In short:** every program a SlateOS-native program starts begins in `/`,
with the default file-creation mask (`umask`, the permission bits a new file
must not get) of `022`, whatever its parent had. So a shell's `cd` is forgotten
by every command it runs, and a `umask 077` is forgotten too. Found by reading
the code; no boot has exercised it, because every ring-3 rung runs from `/`
with absolute paths, which is the one case where right and wrong agree.

**Where:** the working directory is a libc `process_global!` buffer in
`posix/src/unistd.rs` (initialised to `/`); the umask is `UMASK_VALUE` in
`posix/src/file.rs` (initialised to `022`). Both are copied by `fork` (address
space) and both reset in a new image, because `crt.rs` start-up has nobody to
ask: no native syscall reads or writes `pcb.cwd` / `pcb.linux_umask`.

**Who it reaches:**
- C programs that `fork` + `exec*`.
- Everything that uses `posix_spawn`, which includes **all of Rust's
  `std::process::Command`** on this target (`os: linux, env: musl`, so `std`
  runs on this libc). `Command::current_dir` on musl always becomes
  `posix_spawn_file_actions_addchdir_np`, which `posix_spawn` records as tag 4
  and then ignores (`build_fd_map`'s `_ => {}` arm) — so Oils, which sets
  `current_dir` on every external command (`userspace/oils/src/interp.rs`),
  `login` and `sshd` all start their children in `/`.
- Linux-ABI children of native parents too: native spawn leaves the child's
  `pcb` record at its defaults.

**Proper fix:** `pcb.cwd` and `pcb.linux_umask` become the record for every
ABI (native `SET_CWD`/`GET_CWD`/`UMASK`, spawn inherits both, `SpawnEx2Args`
carries an optional `cwd` for `addchdir_np`), with libc keeping them current
and reading them at start-up. Why that does not reopen `design-decisions.md`
§648 is argued in the request: no native call resolves against the record.

**Not done in the meantime, deliberately:** making `addchdir_np` fail loudly
instead of being ignored. Oils uses it for every command, so refusing it would
turn "runs in the wrong directory" into "runs nothing", and the fix for both is
the same kernel half.

The same reasoning shaped the fix. A kernel from before the record refuses
the new spawn fields rather than ignoring them, so sending them to such a
kernel would turn every Oils command into a failed spawn. `posix_spawn`
therefore sends a directory only once `kernel_keeps_cwd()` has seen the record
answer.
