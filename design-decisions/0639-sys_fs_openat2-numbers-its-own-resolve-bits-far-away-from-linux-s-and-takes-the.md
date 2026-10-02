## 639. `SYS_FS_OPENAT2` numbers its own `resolve` bits far away from Linux's, and takes the full twelve-bit create mode

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous, lane A) — answering the one width lane B
left to us in `requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md`,
and settling the one they did not think to ask about.

**In short:** Linux has a system call, `openat2`, that opens a file *and* takes
a set of "resolve" rules constraining how the path is allowed to be walked —
notably "do not follow symbolic links" and "do not escape this directory". Our
kernel is growing a native equivalent so lane B's libc can forward to it instead
of refusing. Two numbers had to be chosen: what the resolve rules are numbered,
and how many permission bits the call accepts when it creates a file. We
deliberately numbered the resolve rules so that they *cannot* collide with
Linux's, and we accept all twelve permission bits rather than the nine our
sibling call accepts.

### Decision 1 — the `resolve` bits are `1 << 16` and `1 << 17`, not Linux's `0x04` and `0x08`

```rust
pub const RESOLVE_NO_SYMLINKS: u64 = 1 << 16;
pub const RESOLVE_BENEATH:     u64 = 1 << 17;
```

Linux numbers its six from `0x01` (`RESOLVE_NO_XDEV`) to `0x20`
(`RESOLVE_CACHED`), with `NO_SYMLINKS = 0x04` and `BENEATH = 0x08`. We support
exactly two rules and reject every other bit with `InvalidArgument`.

**For copying Linux's values:** the libc forward becomes a pass-through with no
translation table to get wrong or to drift, and anyone reading both sources at
once sees one number rather than two.

**Against, and this is what decided it:** a pass-through is precisely the thing
that fails *silently* when it is wrong. If we reused Linux's numbering and lane
B ever forwarded the raw `open_how.resolve` — by intent, or by a refactor that
drops a translation line — then `RESOLVE_NO_XDEV` (`0x01`, "do not cross a
mount point") would arrive as some other rule of ours, and `RESOLVE_CACHED`
(`0x20`, "fail rather than do I/O") as another. The caller would be told the
open succeeded under the containment it asked for. It would not be. That is the
same failure class as `TD-OPENAT2-BENEATH-INROOT` and as the fail-open libc bug
that started this whole exchange: **the security promise is what is silently
wrong, and nothing downstream can detect it.**

Numbering ours at bit 16 and above makes that mistake impossible to make
quietly. Every Linux resolve value lies in `0x00..=0x3f`; any of them passed
through untranslated has *no* known bit set and *at least one* unknown bit set,
so the kernel returns `InvalidArgument` on the first call. A forgotten
translation becomes a hard, immediate, first-test failure instead of a
confinement that is quietly anchored somewhere else. `resolve == 0` means the
same thing in both numbering schemes — no extra rules — which is the one value
where a pass-through is harmless, and it stays harmless.

This is also the precedent the native `SYS_FS_*` family already sets:
`OpenFlags::NO_SYMLINKS` is `1 << 8` and shares no value with Linux's
`O_NOFOLLOW`, for the same reason.

**What it costs:** two lines in `posix/src/file.rs` that lane B was writing
anyway — they already validate unknown resolve bits to `EINVAL` in step 4, so
the set of bits is enumerated on their side no matter which numbering we pick.
The translation is not new work; it is the same work, spelled explicitly.

### Decision 2 — the create mode is `0o7777`, and the three extra bits are *applied*, not refused

Lane B offered two acceptable answers and asked us to rule out a third with
them: take `0o7777` and honour it, or take `0o777` and have libc refuse
`mode & 0o7000` with `EOPNOTSUPP` — but never mask silently.

We take `0o7777` and honour it, because **the storage layer already does**.
`ext4`'s `set_permissions_ino` writes `permissions & 0o7777` and its
`metadata` reads `i_mode & 0o7777`; `memfs` stores the `u16` unmasked. Setuid,
setgid and sticky round-trip through `stat` today. There is nothing to refuse:
the nine-bit width of `SYS_FS_OPEN_MODE` is not a limit of the filesystem, it
is a `& 0o777` in one line of one handler.

Refusing in libc (lane B's option 2) was the weaker choice for a reason worth
recording: it leaves *native* callers — anything that calls `SYS_FS_OPENAT2`
directly rather than through the POSIX layer — with the silent mask, because the
refusal would live in a library they do not link. A rule enforced in libc is a
rule enforced for POSIX programs only, and the whole point of the native
`SYS_FS_*` family is that it is callable without one.

**A note on setuid not being enforced.** Nothing in `exec` honours `S_ISUID`
today. Storing a bit no one acts on is nevertheless safe *in this direction*:
an unenforced setuid bit grants no privilege, so the failure mode is
"less privilege than the metadata claims", which is fail-closed. The reverse —
enforcing a bit we do not store — would not be.

### Decision 3 — `SYS_FS_OPEN_MODE` and `SYS_FS_MKDIR_MODE` are widened to match

Having decided the mask is wrong, leaving it in place on the two sibling calls
would mean `open(O_CREAT, 0o4755)` and `openat2(..., 0o4755)` create files with
different permissions, which is a worse outcome than either width consistently
applied. Both are widened to `& 0o7777`.

This is an observable change to two existing syscalls, and it is recorded here
rather than treated as a bug fix because of that. The change can only *stop*
bits from being discarded — a caller passing `0o0644` is unaffected, and a
caller passing `0o4755` currently gets `0o0755` with no indication that the
`0o4000` was dropped. No caller can reasonably depend on the drop, because no
caller can observe that it was asked for.

### Decision 4 — six flat arguments, no `open_how`, at syscall number 661

Lane B's shape, adopted verbatim:

```
SYS_FS_OPENAT2(path_ptr, path_len, flags, mode, resolve, dirfd) -> fd
```

Their argument is recorded in full in their request and in their §705, and it
is right: `open_how` exists in Linux to be extensible *by struct size*, which is
a solution to a problem `design.txt` already solves differently and better with
**versioned syscall tables**. Taking the struct would give one call two
extensibility mechanisms and pin three field widths neither lane chose. The
first four arguments are `SYS_FS_OPEN_MODE`'s, in its order, so the forward
reads line for line against its sibling.

`dirfd == 0` means "resolve relative to the process's cwd" — native file handle
0 is never valid, so no in-band sentinel is being invented. 661 is the first
free number; 600–660 are contiguous.
