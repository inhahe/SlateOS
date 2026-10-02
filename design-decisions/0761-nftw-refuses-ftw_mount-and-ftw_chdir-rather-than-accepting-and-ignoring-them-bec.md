## 761. `nftw` refuses `FTW_MOUNT` and `FTW_CHDIR` rather than accepting and ignoring them, because an ignored flag is a wrong answer wearing a success code

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** `nftw` is the C library's "walk a directory tree" call. A caller
passes flags saying how to walk: *don't follow shortcuts*, *don't leave this
disk*, *do it bottom-up*. Ours accepted all of them and acted on exactly one —
so a program that asked not to follow symbolic links followed every one of
them, and a program that asked to stay on one disk wandered onto a network
drive, both while being told they had succeeded. The two flags we cannot
implement
yet now make the call fail with "invalid argument" instead of quietly doing
the wrong thing; the two we can (`FTW_PHYS`, `FTW_DEPTH`) are now actually
obeyed.

### The three ways to handle a flag you have not implemented

| Option | What the caller sees | Why not / why |
|---|---|---|
| Accept and ignore | Success, and a walk that did something else | This was the old behaviour. A ported `rm -rf --one-file-system` deletes across a mount point and reports success. There is no recovery from this because nothing indicates it happened. |
| Accept and approximate | Success, and a walk that is *nearly* right | Requires knowing the real semantics. `FTW_MOUNT` needs `st_dev` comparison against the root — implementable — but `FTW_CHDIR` changes what the callback's `ftwbuf->base` *means* (it becomes a filename relative to the current directory, not an offset into an absolute path). An approximation there hands the callback a name that opens a different file. |
| Refuse with `EINVAL` | An immediate, loud failure at the call site | Chosen. It cannot corrupt anything, the caller finds out at the first call rather than after the damage, and it is trivially reversible: implementing the flag is deleting a bit from one constant. |

`FTW_MOUNT` is the closer call of the two — it is a `st_dev` check away, and
refusing it makes us less useful than glibc for a real workload. It is
refused anyway, on the grounds that shipping a device-comparison written from
memory of the semantics, with no filesystem to test it against (every raw
syscall returns `ENOSYS` on the host — see `posix/src/syscall.rs`), is how a
plausible-but-wrong implementation gets into the tree wearing a passing test
suite. `UNSUPPORTED_NFTW_FLAGS` is one constant precisely so that lifting
either restriction is a one-line change with a test to write.

### Why `FTW_PHYS` being ignored was the serious one

`nftw` destructured its `flags` argument for `FTW_DEPTH` and for nothing else,
and the word `lstat` did not appear anywhere in the file. So the walker always
called `stat` (follow the link) where the caller had asked for `lstat`
(describe the link). Two consequences, and the second is a security bug:

- A symlink to a directory was descended into. A tree with a link to `/` walks
  the whole filesystem, and without cycle detection it does so until it hits
  the depth cap.
- A caller doing a recursive delete under `FTW_PHYS` — which is the *only*
  correct way to write one — was handed `FTW_D` for a symlink and would
  recurse into and empty the target directory rather than unlinking the link.

The defect is invisible at the call site (the flag is accepted), invisible in
the flag-handling code (there is none to be wrong), and *contradicted* by the
module header, which said "symbolic link detection depends on `lstat`" about a
file that never called it. A reader checking whether symlinks were handled
found an answer without reading the code, which is how this survived. The
lesson is narrower than "comments rot": **a doc comment describing a
capability is a claim, and the test for a claim is a test.** `FTW_SL` and
`FTW_SLN` had constants, had their numeric values asserted, and had no
producer — which a coverage-shaped reading of the test file would not reveal,
because the constants *were* tested.

That is also the argument for the structural half of this change. `ftw` and
`nftw` were four functions implementing one loop twice; a walk option honoured
in one copy and dropped in the other is invisible at every call site. They are
now one `Walker` differing only in how an entry is delivered, so an option
cannot be obeyed by one entry point and ignored by the other.

### What else the merge fixed, and what it cost

Three silent losses became reports (see known-issues.md
`B-NFTW-IGNORES-FTW-PHYS-AND-SKIPS-WHAT-IT-CANNOT-WALK`): an unopenable
directory now yields `FTW_DNR` — a flag this module defined, asserted the
numeric value of in a test, and had never once produced — the depth limit
yields `FTW_DNR` with `ENOMEM`, and an over-long child path fails the walk
with `ENAMETOOLONG` rather than being skipped. The `FTW_DNR` case forced
opening the directory *before* announcing it, since `FTW_DNR` replaces `FTW_D`
rather than following it.

The cost is one behaviour change a caller could notice: a walk that used to
return 0 having quietly omitted a subtree may now return -1, or deliver a type
flag the caller has no case for. That is the intended direction — a traversal
that omits files without saying so is worse than one that fails, because the
caller believes it saw everything — but it is a change, and it is why this is
an entry rather than a commit message.

Per-level stack went from ~4 KiB to a few words: the path was a
`[u8; PATH_MAX]` local in the recursive function, so the depth cap of 32 cost
128 KiB of stack while the module's own header cited stack overflow as the
*reason* for the cap. One buffer now lives in the `Walker` and is mutated in
place, the same discipline `crate::fts` uses.
