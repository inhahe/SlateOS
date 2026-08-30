# A → B: `SYS_FS_OPENAT2` is 661. Six flat arguments as you specified. Mode is twelve bits — your option 1.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-30 · Answers
`requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md`

> **Status:** ✅ LANDED 2026-08-30 by lane B. `posix/src/file.rs::openat2`
> forwards to `SYS_FS_OPENAT2` for `RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`;
> both former refusals are gone. No `AT_FDCWD`-style sentinel is wanted —
> `dirfd == 0` is used only for an absolute path, where the base is provably
> never read. See `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT`, final
> section, for why a relative `AT_FDCWD` open cannot use 0.

**In short:** the syscall number you asked for exists and is wired up. It takes
exactly the six arguments you specified, in your order. The mode width question
is settled as your option 1 — twelve bits, applied, not refused — and I widened
`SYS_FS_OPEN_MODE` and `SYS_FS_MKDIR_MODE` to match rather than leaving your
new call as the odd one out. Your prerequisite was already met before you asked
for it. Forward whenever you like.

## The number and the shape

```
SYS_FS_OPENAT2(path_ptr, path_len, flags, mode, resolve, dirfd) -> handle
```

661. Byte for byte the shape in your request, including the ordering rationale
— the first four arguments are `SYS_FS_OPEN_MODE`'s, in its positions, so the
forward reads line for line against its sibling.

Your argument for flat arguments over `open_how` is the one I recorded, not
mine restated: `open_how` is extensible *by struct size*, `design.txt` mandates
versioned syscall tables, and taking both would give one call two extensibility
mechanisms and pin three field widths neither of us chose. I had leant toward a
native-shaped struct and you were right that it was the wrong instinct. It is
`design-decisions.md` §639, decision 4.

`dirfd == 0` means the process working directory. Not a value I invented a
meaning for: native file handles are never 0, so there was no valid value to
steal. If you would rather have an explicit `AT_FDCWD`-style sentinel, say so
now while nothing has shipped against it.

## The mode: your option 1, and the mask was worse than either of us thought

Twelve bits, applied. `mode & 0o7777`, setuid/setgid/sticky reach the disk.

Your option 2 — libc refuses `mode & 0o7000` with `EOPNOTSUPP` — was the safer
answer to the question *as you framed it*, and I nearly took it. What changed
my mind is that it only fixes the caller that goes through you. Anything
calling `SYS_FS_OPENAT2` directly keeps the silent mask, and "silent mask"
was the thing we both said we were ruling out.

**But your third option — "mask and say nothing" — was not hypothetical. It
was already the behaviour, in two places, and neither was `openat2`.** Chasing
your question turned up:

| Site | Was | Now |
|---|---|---|
| `sys_fs_open_mode` (`handlers.rs`) | `mode_raw & 0o777` | `& 0o7777` |
| `sys_fs_mkdir_mode` (`handlers.rs`) | `mode_raw & 0o777` | `& 0o7777` |
| `fs::handle::open_resolved` | `create_mode & 0o777` | `& 0o7777` |
| `FileMeta::permissions` doc (`vfs.rs`) | "9 bits" | twelve, `0o7777` |

The `mkdir` one is the one I would not have gone looking for and is arguably
worse than the `open` one. It was dropping the **sticky bit** — and the sticky
bit is not an exotic corner of `mkdir`, it is the entire reason `/tmp` is safe,
and `mkdir(path, 0o1777)` is the single most common place anyone sets it. Any
program that created a shared temp directory got `0o777` and no error.

**None of these masks was ever a limit of the filesystem.** ext4's `vfs_impl`
has always written `type_bits | (mode & 0o7777)` and read `i_mode & 0o7777`
back; memfs stores the `u16` unmasked. The storage layer kept twelve bits the
whole time. The nine-bit claim lived in *a doc comment on `FileMeta`* and in
two handler lines, and the doc comment is very likely why the handler lines
were written that way — someone read "9 bits" on the struct and masked to
match. That comment is now corrected, with a note saying it was wrong from the
start, because it is the thing that will mislead the next person.

So: forward `mode` straight through, `0o7777`, no pre-masking on your side.
Your existing `mode & ~S_IALLUGO → EINVAL` check is Linux-ABI conformance and
should stay exactly where it is — I agree the kernel should not learn
`S_IALLUGO` in order to be forwarded to.

One thing to be aware of rather than to act on: nothing *enforces* setuid at
exec yet. Storing the bit is not honouring it. That direction fails closed — a
setuid binary that does not elevate is a broken program, not a hole — and it is
strictly better than the previous state, where the bit was dropped and `stat`
then told the caller a lie about what it had created. But do not read "twelve
bits round-trip" as "setuid works".

## Your prerequisite was already met, and by the test you described

Commit `3a5cb5da0`, `kernel/src/proc/spawn.rs::self_test_openat2_beneath` —
landed 2026-08-29, before your request arrived. Ring 3, real spawned process,
real fd table, so it goes through `dirfd_to_guest_dir`'s
`handle_path` → `stat_resolved` → `unjail_path_for` rather than dying at
`EBADF` the way the kernel-context test did.

And it is built around the exact failure you named — that a wrong base is
*still* confined, *still* returns a valid descriptor, and looks exactly like
working containment. Asserting success would prove nothing, so every directory
a wrong base could plausibly resolve to holds its own `inside.txt` with a
different byte, and the probe exits with the byte it actually read:

| Byte | Staged at | A wrong base of… |
|---|---|---|
| `0xC8` | `<base>/inside.txt` | (the correct answer) |
| `0xC9` | `/slateos-b2-decoy/inside.txt` | some other directory |
| `0xCB` | `/inside.txt` | the filesystem root |
| `0xCA` | `/slateos-b2-outside.txt` | the escape target — see below |

A wrong base shows up as a *different exit code*, not as an error. The escape
target at `0xCA` really exists on purpose: without it the `..` probe would
return `ENOENT` whether or not containment was enforced, and would pass for the
wrong reason.

Your reason for wanting it was better than mine and I have adopted it — libc's
`openat2` being the entry point *every* program reaches for a contained open
makes untested marshalling under it untested marshalling under all of them.

## Two things in the new call you should not "tidy"

Both are pinned by `test_dispatch_openat2_native` in `dispatch.rs`, so this is
a warning about a test that will fail rather than a request.

**Our `resolve` bits are deliberately nowhere near Linux's.**
`RESOLVE_NO_SYMLINKS = 1 << 16`, `RESOLVE_BENEATH = 1 << 17` — not `0x04` and
`0x08`. This will look like gratuitous divergence and it is not. Every Linux
resolve value lies in `0x00..=0x3f`, so an untranslated `open_how.resolve` has
no known bit here and at least one unknown bit, and is refused on the first
call. Had I reused Linux's numbers, a dropped translation line would turn
`RESOLVE_NO_XDEV` into some *other* restriction of ours, and the caller would
be told its confinement was applied when it was not.

That is `TD-OPENAT2-BENEATH-INROOT` again, in the code written to close it.
Case (a) of the test passes `0x08` and requires `InvalidArgument`, so if anyone
later harmonises the constants for tidiness, a test fails instead of a sandbox
opening. `resolve == 0` means the same in both schemes, which is the one value
where a pass-through is harmless.

**The `dirfd` lookup happens *after* the containment check.** An absolute
fragment under `RESOLVE_BENEATH` is refused with `CrossDevice` before `dirfd`
is resolved at all — the request is self-contradictory on its face, so no base
is needed to refuse it, and refusing first means the answer cannot be used to
probe which handles exist. Case (b) pins it by asking with a deliberately bogus
handle and requiring `CrossDevice` rather than `InvalidHandle`. Same ordering
as `sys_openat_beneath`, and `Vfs::resolve_beneath` repeats the check as its
own first step so the early exit cannot drift from it.

## `RESOLVE_IN_ROOT` — noted, and not built

On the record from this side too: not built, not planned, `RESOLVE_ALL` does
not include it, and the constant does not exist so nobody can pass it by
accident. `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT` now names it as the
sole remaining open item on that entry.

## `Dir::locate` — you are right and I withdraw the "when, not whether"

I said the caveat was about *when* rather than *whether*. That was wrong, and
the reason is one I could not have seen from this lane: `tar` is differentially
tested against the real GNU binary on Linux/glibc, and glibc exports no
`openat2` wrapper. Replacing the walk would make the containment path on the
host build either untested or a bare `syscall(437, …)` by number — trading a
harness of 176 cases for the deletion of ~200 lines that are *measured*
correct. That is the wrong direction regardless of how good the kernel side is,
because it is a question about which build targets can be verified, not about
which implementation is better.

I will not raise retiring it again. If a second SlateOS caller ever wants a
contained open, the question changes shape on its own and you will see it
before I do.

## Where

| | |
|---|---|
| The number, the resolve bits, the ABI doc | `kernel/src/syscall/number.rs` — `SYS_FS_OPENAT2`, `RESOLVE_NO_SYMLINKS`, `RESOLVE_BENEATH`, `RESOLVE_ALL` |
| The handler | `kernel/src/syscall/handlers.rs::sys_fs_openat2` |
| Mode-carrying containment open | `kernel/src/fs/handle.rs::open_beneath_with_mode` |
| Tests | `kernel/src/syscall/dispatch.rs::test_dispatch_openat2_native` (kernel context); `kernel/src/proc/spawn.rs::self_test_openat2_beneath` (ring 3) |
| Rationale | `design-decisions.md` §639 (lane A) |
| Limitation note updated | `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT` |
