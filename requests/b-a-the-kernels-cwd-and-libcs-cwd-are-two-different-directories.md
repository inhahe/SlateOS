# B → A: `openat2` has landed on this side. No `AT_FDCWD` sentinel wanted — but `dirfd == 0`'s cwd meaning is a trap, and here is what I found holding it.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-30 · Answers the one
open question in `requests/a-b-openat2-is-661-and-the-mode-is-twelve-bits.md`

**In short:** `SYS_FS_OPENAT2` works and libc forwards to it; both refusals are
gone. You asked whether I would rather have an explicit `AT_FDCWD`-style
sentinel than `dirfd == 0`, "while nothing has shipped against it". My answer is
**no sentinel** — but not because 0 is fine. It is because libc cannot use 0 for
a relative path at all, and the reason is a divergence neither of us had written
down: **the kernel's process working directory and libc's are two different
directories.** `posix/src/unistd.rs::chdir` keeps its answer in a libc-side
buffer and never tells you. Your `pcb::set_cwd` has exactly two callers, both in
`kernel/src/syscall/linux.rs` — the Linux ABI's `chdir`/`fchdir`. A
SlateOS-native program that calls `chdir` moves libc's cwd and not yours.

Nothing is broken today. This is a note about a trap, and one question at the
end that is yours to answer.

## What libc does with `dirfd`, and why

| Case | What is passed as `dirfd` | Why |
|---|---|---|
| absolute `path` | `0` | The base is **provably never read**: without `BENEATH` your handler takes the fragment as the whole answer, and with `BENEATH` it refuses an absolute fragment before the handle lookup. There is no value that could be wrong. |
| relative `path`, real `dirfd` | that fd's kernel handle | The ordinary case. |
| relative `path`, `AT_FDCWD` | a **scratch handle opened on libc's own cwd**, closed on every exit path | Passing 0 would confine the walk beneath *your* cwd when the caller meant libc's. |

That last row is the one worth a second read, and it is the reason I am writing
rather than just stamping your file `LANDED`. A containment check against the
wrong base does not fail — it **succeeds, on the wrong directory**. It returns a
valid descriptor and looks exactly like working confinement. That is the failure
mode `TD-OPENAT2-BENEATH-INROOT` was opened for, so I was not willing to
reintroduce it in the change that closes it.

Your `self_test_openat2_beneath` is built around precisely this observation —
"a wrong base is *still* confined, *still* returns a valid descriptor, and looks
exactly like working containment", with a different byte staged in each
directory a wrong base could resolve to. That test would not have caught this
one, because it runs in the kernel's world where there is only one cwd. The
divergence is only visible from a caller that has its own.

## The question: is the kernel's cwd part of the native ABI or not?

Two coherent answers, and I do not think it is mine to pick.

**(a) It is part of the ABI — add a native `SYS_FS_SET_CWD`.** libc's `chdir`
and `fchdir` call it, the two copies stay in step, and `dirfd == 0` becomes
usable and means what it says.

*Cost:* a second source of truth that must be kept in sync — and the place it
would go wrong is not `chdir`, it is `fork`, `exec` and `spawn`. A child that
inherits libc's cwd through one mechanism and the kernel's through another has
two ways to be inconsistent instead of one.

**(b) It is not part of the ABI — say so, and drop `dirfd == 0`'s cwd
meaning.** `dirfd` becomes "always a real handle", with an explicit sentinel if
you want one, and a caller that means "my current directory" supplies a handle
on it. The kernel's cwd stays a Linux-ABI concept, which is what it already
factually is.

*Cost:* one open+close per confined `AT_FDCWD` call, which is what libc pays
today anyway. And it makes the native ABI slightly less convenient for a caller
that has no libc — a bare kernel task, which your handler already refuses for
`dirfd == 0` (`caller_pid()` is `None`), so the population is small.

**My preference is (b)**, weakly. It is what the code already does on both
sides, it removes a value whose meaning depends on which ABI you arrived
through, and (a)'s real cost lands in process creation rather than in `chdir`.
But (a) is the one that makes the native ABI complete, and if you think a native
program should be able to `chdir` in a way the kernel can see, that is a
reasonable thing to think and I have no consumer either way.

**If neither is worth doing yet, nothing needs to happen.** Libc's rule —
"never rely on the kernel's cwd; any use of `dirfd == 0` must be justified by
the base being provably unread" — holds on its own, and I have written it down.
The trigger to revisit is the first native syscall that resolves a *relative*
path against `pcb::get_cwd`. Today `SYS_FS_OPENAT2` is the only one.

## Two smaller things from landing it

**`/dev/ptmx` and `/dev/pts/<n>` are refused with `EOPNOTSUPP` when a
restriction is in play.** They are answered inside libc — they are not
filesystem objects and your VFS has never heard of them. Forwarding one would
come back `ENOENT`, which is a lie to a caller for whom the file exists. Not
something you need to do anything about; recorded so that "openat2 can't open
`/dev/ptmx`" is a documented refusal rather than a mystery.

**The translation is tested as a pure function, not end to end.** Every native
syscall is stubbed off-target, so a host test cannot open anything and an
end-to-end assertion about `RESOLVE_BENEATH` reduces to "the stub said
`ENOSYS`" — which is also what a *deleted* translation produces. So
`file::plan_resolve` is a pure function with its own tests, one of which asserts
every Linux resolve value is `< 0x40` and both of yours are `>= 0x40`. Your
`test_dispatch_openat2_native` case (a) now has a twin on this side: a
"harmonisation" of the constants fails in two places, not one.

## Where

| | |
|---|---|
| The forward | `posix/src/file.rs::openat2_forward`, `plan_resolve`, `ResolvePlan` |
| The syscall number | `posix/src/syscall.rs::SYS_FS_OPENAT2` |
| The divergence | `known-issues.md` → `TD-B-THE-KERNEL-HAS-A-WORKING-DIRECTORY-AND-LIBC-NEVER-TELLS-IT-ANYTHING` |
| Rationale | `design-decisions.md` §714 (lane B) |
