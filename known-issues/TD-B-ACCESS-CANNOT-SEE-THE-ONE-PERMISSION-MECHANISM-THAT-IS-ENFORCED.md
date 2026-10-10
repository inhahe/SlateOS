## TD-B-ACCESS-CANNOT-SEE-THE-ONE-PERMISSION-MECHANISM-THAT-IS-ENFORCED (lane B, 2026-09-13) -- FIXED 2026-10-02
**Status:** FIXED by lane A on lane-a-wip 2026-10-02, awaiting a boot -- the Linux `access` answers the gates a real open or exec meets (`Vfs::access_gates`): `EPERM` for `W_OK` on an immutable file, the ACLs and capability tags (`EACCES`), `EROFS` for `W_OK` on a read-only mount; not the mode bits (design-decisions §1524)

**In short:** `access(path, W_OK)` answers "yes" for any file that exists. On a
stock system that is correct. On a system where someone has set an ACL or a
capability file-tag, it is wrong in the dangerous direction: the program is told
it may write, and the `open()` that follows returns `EACCES`.

### What is and is not enforced, measured

The comment on `access()` said *"our OS doesn't have a permission system yet"*.
That has two mechanisms' worth of counter-evidence:

| mechanism | stored? | enforced? |
|---|---|---|
| traditional rwx mode bits | **yes** — `fchmodat` (`SYS_FS_FCHMODAT_PINNED`, 665) records them, `stat` returns them | **no** — `S_IWUSR`, `S_IRUSR` and `S_IXUSR` appear **nowhere** in `kernel/src` |
| POSIX ACLs | yes (`kernel/src/fs/acl.rs`) | **yes** — via `vfs::check_path_access` |
| capability file-tags | yes (`cap::file_tags`) | **yes** — same gate |

`vfs::check_path_access` documents itself as *"the single permission gate every
path operation passes through"*, and exists because the ACL check previously
"was called from none of" the sixteen call sites, so `setfacl` reported success
while governing nothing. It is a gate that was itself once unwired.

### Why `access()` must NOT simply consult `st_mode`

This is the trap, and it is the reason this is written down rather than fixed in
five minutes. The obvious fix — read `st_mode` from the `stat` that `access()`
already performs, and answer from the rwx bits — would **introduce** a defect.
Nothing enforces those bits, so a file with mode `0o444` is genuinely writable;
answering `W_OK` = denied would refuse access that a write would get. A wrong
answer in the safe-looking direction is still wrong, and it would break callers
that probe before writing.

The answer `access()` owes is *"would the gate that actually runs let me in?"*,
and only ACLs and tags are behind that gate.

### What closing it needs

A kernel query — something like `SYS_FS_CHECK_ACCESS(path, want)` returning the
verdict `check_path_access` would give. Nothing in libc can compute it: ACL
tables and file-tag tables live in the kernel and are keyed by the calling
process's credentials.

**Not filed as a request yet, deliberately.** Both mechanisms are inert until
configured — `check_path_access` returns immediately while both tables are empty
— so on every system we currently boot, `access()` is correct. Lane A already
has `b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md` open, which
is severe and active; adding a second request for a gap that bites only a
configured system would dilute that. Worth folding into a kernel-side change
that touches this area anyway.
