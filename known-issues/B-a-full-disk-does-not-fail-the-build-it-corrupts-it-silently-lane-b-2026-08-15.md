## A full disk does not fail the build — it corrupts it silently (lane B, 2026-08-15)

Continues `INFRA-D-DRIVE-EXHAUSTED` directly above — same drive, same day, from
the other lane. That entry covers the loud failure; this one covers the quiet
one.

`cargo test --workspace` died with `rustc-LLVM ERROR: IO failure on output
stream: No space left on device` and `ld: final link failed: No space left on
device`. D: was at **384 KiB free of 1.9 TB**. That part is loud and obvious.

The dangerous part is what it left behind. After space was reclaimed, a rerun
came back with 2,619 test binaries green and exactly one failure:

```
process didn't exit successfully: ...\tiramisu_cli-c940c738c084a909.exe
  (exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)
```

`userspace/tiramisu-cli/src/main.rs` is 69 lines of safe Rust with no `unsafe`,
no recursion and no FFI. It cannot segfault. The binary was timestamped **19:34
— inside the disk-full window** — while the healthy binaries around it carried
the rerun's **21:24**. The truncated `.exe` from the failed run was still on
disk, and its fingerprint still looked fresh, so cargo never relinked it and
happily *ran* it. Deleting just that file and rebuilding the same source gave
4/4 passing.

So a disk-full build does not merely fail; it can plant an artifact that
**fails later, in a different crate, as a phantom memory-safety bug**. Anyone
who hit that crash without the timestamp would have gone looking for UB in code
that has none.

**Rule: after any build that reports "No space left on device", do not trust an
incremental rerun.** Wipe `target/` and rebuild clean. A green result on top of
artifacts written during disk exhaustion is not evidence.

Also worth knowing:

- **The four worktrees held 174 GB of `target/` between them** (`os` 95.1,
  `os-lane-b` 40.4, `os-lane-c` 35.0, `os-lane-a` 3.5). The `os` integration
  tree is the biggest and the least used — it exists for merges, not
  development — so it is the first place to reclaim from. Clearing it alone
  freed 95 GB and unblocked all three lanes.
- **`Remove-Item -Recurse` is unusably slow on a `target/` dir.** Mirroring an
  empty directory over it is dramatically faster:
  `robocopy <empty-dir> <target> /MIR /NFL /NDL /NJH /NJS /NP /R:0 /W:0`.
  Robocopy returns **rc=2** ("extras detected and removed") on success here;
  anything under 8 is success, so do not read rc=2 as failure.
- **Check what QEMU is running before deleting anything.** A boot test was live
  at the time; `Get-CimInstance Win32_Process` showed its command line pointed
  at `os-lane-a\build\`, which confirmed that clearing `os\target` could not
  disturb it. Deleting another lane's artifacts out from under a running gate
  would have been a cross-lane failure of exactly the kind the worktree split
  exists to prevent.
