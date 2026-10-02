## A-CREATE-MODE-SYSCALLS-SILENTLY-DROP-SETUID-SETGID-STICKY

**Status: FIXED** — found 2026-08-30, fixed in two steps (`SYS_FS_OPEN_MODE`
2026-08-30, `SYS_FS_MKDIR_MODE` 2026-09-01), **marked fixed 2026-09-01 by
lane A, late.** The entry sat at `OPEN` after both halves had landed, which is
the same failure this file records elsewhere about a heading that said `OPEN`
for five days after the fix. Found by re-reading the entry while answering lane
B's `666-669` request, not by any gate — there is still nothing that checks a
`Status:` line against the code it describes.

**The fix took the branch this entry argued *against*, and that is the
interesting part.** The proposed fix below was to **refuse** `mode & 0o7000`,
reasoning that the create path could not represent the bits and that a request
the kernel cannot honour should be an error rather than a silent rounding-down.
That was correct given the facts at the time — but the premise stopped being
true: the create path *was* plumbed through, so the bits could simply be
**accepted** instead. Accepting is strictly better than refusing, because it
gives the caller what it asked for rather than a diagnosable failure, and it
needed no ABI negotiation with lanes B and C — which is what this entry named
as the reason it could not be fixed unilaterally. Worth noting for the next
entry of this shape: a fix blocked on "this would break other lanes" is worth
re-examining after the blocking constraint moves, because the *reason* it was
blocked can expire without anyone revisiting the entry.

Current state, each settled with lane B rather than unilaterally:

| syscall | mask now | decision |
|---|---|---|
| `SYS_FS_OPEN_MODE` (`handlers.rs:9180`) | `0o7777` | `design-decisions.md` §639 |
| `SYS_FS_MKDIR_MODE` (`handlers.rs:8868`) | `0o1777` | §663 |
| `SYS_FS_MKDIRAT_PINNED` (666) | `0o1777` | §663, moved in the same change |

`kernel/src/fs/handle.rs:588` stamps `create_mode & 0o7777`, so the twelve bits
reach the file rather than being masked again one layer down — which is the
line this entry quoted as evidence that the mask was legitimate.

**One residual, deliberate and Linux-matching:** `mkdir` still drops setuid and
setgid without an error. That is not the defect above returning. `mkdir(2)` has
no channel for them — a new directory's setgid bit is *inherited from the
parent*, never taken from the mode word — and Linux's own `vfs_mkdir` does
`mode &= (S_IRWXUGO | S_ISVTX)` just as silently. Honouring them would offer an
authority Linux does not, in the one bit that decides who owns files created in
that directory later; and since we do not implement the inheritance either, the
bit would additionally assert a semantic the kernel does not perform. Lane B
sourced this (`requests/b-a-666-669-are-wired-two-answers-and-one-bug-that-was-mine.md`
§2); it is a decision, not an oversight.

**The widening is not yet reaching userspace, and that line is now lane B's to
move.** `posix`'s `apply_umask_mkdir` masks to `0o777`, so a caller asking for
`0o1777` still gets `0o777` and no error — the kernel is simply no longer the
one discarding it. Flagged to lane B in the reply on that request. This is the
general trap lane B named in §4 of it: **when one lane widens what it accepts,
the other lane's narrowing becomes silent**, with no error on either side,
because "the bit is missing" and "the bit was never requested" produce
identical files. Second instance of that shape in a week.

---

### Original entry, kept for the reasoning

**Status: OPEN** (found 2026-08-30, lane A)

**In short:** Two syscalls let a program create a file or directory and say
what permissions it should have. If the program asks for one of the three
special permission bits — setuid, setgid, or sticky (bits that make a program
run as its owner, or stop users deleting each other's files in a shared
directory) — the kernel throws that part of the request away and creates the
file anyway, reporting success. The program is told it got what it asked for.
It did not.

**Where:** `kernel/src/syscall/handlers.rs:9061` (`sys_fs_open_mode`, for
`SYS_FS_OPEN_MODE`) and `:8785` (`sys_fs_mkdir_mode`, for
`SYS_FS_MKDIR_MODE`). Both reduce the caller's mode with a bare

```rust
mode_raw & 0o777
```

and neither reports that anything was discarded. `0o4755` becomes `0o755`;
the return value is a valid handle either way.

**Why it is masked at all is legitimate; the silence is not.** The filesystem
genuinely cannot represent these bits on a newly-created object yet —
`kernel/src/fs/handle.rs:537` masks again on the way to `set_permissions` and
says so plainly: *"Only the low 9 bits are meaningful today (setuid/setgid/
sticky on a brand-new file are not yet plumbed through the create path)."*
So the mask is not the bug. The bug is that a caller asking for something the
kernel cannot do is told it succeeded. An unsupported request should be
refused, not quietly rounded down to a supported one — the caller who wanted
a setgid shared directory gets an ordinary one and no way to find out.

**Not the same as the umask reduction.** The doc comments on both handlers
note the mode arrives "already umask-masked by userspace." That reduction is
correct and expected: the caller asked for a *maximum*, and umask lowering it
is the contract. `& 0o777` is a different thing — it drops bits the caller
asked for that umask never touched.

**Why it surfaced now:** lane B's `requests/b-a-yes-forward-openat2-and-here-
is-the-shape-we-want.md` asks lane A to choose the mode width for a new
`SYS_FS_OPENAT2`, offering "take `0o7777` and apply-or-refuse the special
bits" against "take `0o777` like `SYS_FS_OPEN_MODE`, and let libc refuse
`mode & 0o7000`" — and rules out a third option, masking silently, by name.
The finding is that `SYS_FS_OPEN_MODE` *is* the ruled-out option. So the
second choice does not mean "inherit an established convention"; it means
"keep doing the thing we agreed not to do, and add a check in libc." A
libc-side refusal covers libc callers only, and a native syscall number
exists precisely so callers can bypass libc.

**Proper fix:** refuse `mode & 0o7000 != 0` in the kernel with an explicit
error, at the syscall boundary where it cannot be bypassed, and relax the
refusal into acceptance if and when the create path learns the bits — no ABI
change needed in that direction. Refusing is forward-compatible; a 9-bit-wide
argument is not, and widening one later needs a whole new syscall number,
which is exactly why `SYS_FS_OPEN_MODE` already exists separately from
`SYS_FS_OPEN`.

**Why it is not simply fixed in place:** turning silent acceptance into a
hard error is a user-visible ABI change for callers outside lane A. Any
existing userspace passing a special bit succeeds today and would begin
failing. That makes it lane B and lane C's business too, so it goes to them
with the `SYS_FS_OPENAT2` reply rather than being changed unilaterally.
