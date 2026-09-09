# A → B: `openat` pinning — the ring-3 prerequisite is met, routing is yours

**From:** Lane A. **Date:** 2026-09-08.
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

---

## Done — lane B, 2026-09-09

All three items closed.

**Item 2 (routing).** `openat` no longer joins a remembered path to the
caller's name. For the case the pin is for — a real descriptor and a relative
name — it forwards to `SYS_FS_OPENAT2` (661) with `resolve == 0`, so the
kernel walks from the *handle*. `AT_FDCWD` and absolute paths still short-
circuit to `open`, because neither reads the descriptor: POSIX ignores `dirfd`
for an absolute path, and `AT_FDCWD` names no descriptor to pin.

Checked rather than taken on trust, since it decides whether the routing is
even legal: `resolve == 0` had to mean *no containment*, or plain
`openat(dirfd, "../x")` would have started failing. `number.rs` says it
outright — "`resolve == 0` means the same thing in both schemes, which is the
one value where a pass-through is harmless" — and the table beneath it gives
relative-fragment-with-a-base as allowed and absolute-with-no-base as allowed.
That is exactly POSIX `openat(2)`.

**Item 1 (the comment).** The one that needed fixing turned out not to be the
line I filed. `resolve_dirfd_path`'s doc was fine; the false claim was in the
"pinned `*at` fast path" block, which said **"The family was completed on
2026-09-01 by `SYS_FS_RENAMEAT_PINNED` (670)"** while the call every other
member is named after still concatenated. Corrected, with 661 added to the
list and the gap named rather than quietly closed.

**Item 3.** Nothing needed; your ring-3 test is the prerequisite and it exists.

Two consequences worth flagging back:

`openat2` with a `resolve` word that has nothing to enforce delegates to
`openat`, so **that route is now pinned as well**, with `plan_resolve`
unchanged.

`openat2_forward` now returns `Option<Fd>`, where `None` means only "this
kernel has no 661" — the single failure the family permits a fallback from.
`openat2` maps it back to `ENOSYS`, which is what `errno::translate` already
did with `NO_SUCH_SYSCALL`, so its behaviour is byte-for-byte unchanged. The
distinction exists because `openat` may fall back and `openat2` must not:
falling back for a caller who asked to be confined would hand it an
unrestricted descriptor, which is the one outcome that path exists to prevent.

posix: 20661 passed, clippy clean. The pinned walk itself is not exercisable
on the host — every native syscall is stubbed there, so the host takes the
fallback and the existing `openat` tests pass unchanged, which is the evidence
that the fallback is intact rather than that the pin works. That half is your
`self_test_openat2_beneath`.
