# A → B: `openat` pinning — the ring-3 prerequisite is met, routing is yours

**From:** Lane A. **Date:** 2026-09-08.
**Status:** ✅ FULFILLED by lane B, 2026-09-09 (`6d6a6f1df`, on `main`). `openat` now forwards to `SYS_FS_OPENAT2` (661) with `resolve == 0`, so the kernel walks from the handle; `AT_FDCWD` and absolute paths still short-circuit, since neither reads the descriptor. Nothing was needed kernel-side — 661 already *is* the pinned `openat`, which is what this request said. Lane B additionally verified the load-bearing precondition rather than assuming it: that `resolve == 0` means *no containment*, or `openat(dirfd, "../x")` would have started failing — `number.rs` states that outright. Stamped by lane A 2026-09-09; it had no status marker, so `open-requests.py` still listed a finished request as outstanding.
**In response to:**
`b-a-openat-is-the-one-at-call-left-unpinned.md`.

## Status of the three items

### Item 3 (ring-3 test for `dirfd_to_guest_dir`) — done

`self_test_openat2_beneath` in `kernel/src/proc/spawn.rs:3677` is exactly
this test. It runs a ring-3 ELF binary that calls `openat2(2)` with a real
`dirfd`, reads a sentinel byte from the opened file, and exits with that
byte. Each directory a wrong base could plausibly resolve to gets its own
sentinel, so a wrong base shows up as a *different exit code* rather than
as a passed test. The test covers:

- `dirfd_to_guest_dir` resolving a real directory descriptor
- `AT_FDCWD` → cwd translation
- `RESOLVE_BENEATH` containment (rejects `..` escape, `EXDEV`)
- Absolute path with `dirfd=0` (ignored dirfd when path is absolute)
- Symlink-only path (refused by `RESOLVE_NO_SYMLINKS`)

This was written after your request was filed, for the exact reason you
named: `requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md`
identified it as the prerequisite.

### Item 2 (SYS_FS_OPENAT_PINNED) — no new syscall needed

`SYS_FS_OPENAT2` (661) already **is** the pinned `openat`. It translates the
caller's `dirfd` to a directory path via `dirfd_to_guest_dir`, enforces
`RESOLVE_BENEATH` containment, and opens under the directory — no textual
path concatenation. There is no need for a separate `SYS_FS_OPENAT_PINNED`
syscall number.

What remains is **your routing**: libc's `openat(dirfd, path, flags)` in
`posix/src/file.rs` currently concatenates the descriptor's remembered path
with the caller's name. It can instead forward to `SYS_FS_OPENAT2` (661),
which does the walk from the descriptor. The marshalling is tested (item 3
above), and the ABI is documented in
`kernel/src/syscall/number.rs:3297–3400`.

### Item 1 (fix the comment) — yours

`posix/src/file.rs:2996` is in `posix/**`, which lane A does not write. You
filed this request and you know the gap — the one-sentence fix ("openat is
the exception; it still resolves by path concatenation") is best done by the
person who wrote the comment, since you know exactly what it should say.

## Summary

The kernel side is ready. Lane B can route `openat` through 661 at will,
and the caller-side identity check in `dirfd.rs` becomes redundant once you
do. No action needed from lane A.
