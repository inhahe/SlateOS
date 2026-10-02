## 732. An empty relative name in the `*at` family is `ENOENT`, and `AT_EMPTY_PATH` reaches the descriptor directly rather than through a synthesised path

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous) · **Lane:** B

**In short:** Every `*at` call — `unlinkat`, `fchownat`, `scandirat` and a
dozen more — builds the full path by gluing the directory's name and the
caller's name together with a `/`. Given an *empty* name that produces
`"/some/dir/"`, which is a perfectly valid path naming the directory itself.
So `unlinkat(fd, "", AT_REMOVEDIR)` deleted the caller's directory and reported
success, and `fchownat(fd, "", uid, gid, 0)` re-owned it. The fix is to refuse
an empty name with `ENOENT` before the join happens — and, for the five calls
that take the `AT_EMPTY_PATH` flag (which asks to operate on the descriptor
*itself*, with no name at all), to serve them from the descriptor rather than
from any path.

**Terms used below.** *`*at` call*: a variant like `unlinkat` that takes a
directory descriptor plus a name relative to it, instead of one absolute path.
*`AT_EMPTY_PATH`*: a flag meaning "the name is empty on purpose — act on the
descriptor". *TOCTOU*: a race where a name is checked and then used, and
something swaps what the name refers to in between.

### The problem

`resolve_dirfd_path` / `build_at_path` cannot represent "no name at all". They
take a directory path and a component and return `dir + "/" + component`. With
an empty component that is `dir + "/"`, and every filesystem in existence
treats a trailing slash as naming the directory. The failure mode is therefore
not an error — it is a *success on the wrong object*, which is the worst shape
a bug can have. Fifteen call sites across `posix/src/file.rs`,
`posix/src/unistd.rs` and `posix/src/dirent.rs` had it.

Separately, five of those calls advertise `AT_EMPTY_PATH` and did nothing with
it: `fstatat(file_fd, "", &st, AT_EMPTY_PATH)` — the documented way to `fstat`
a descriptor through the `*at` interface — joined its way to `ENOTDIR`.

### Decision 1: refuse, rather than quietly mean the directory

The alternative was to make the empty name mean the directory, since that is
what the code already did and it is occasionally a convenience. Rejected: it is
not what Linux does, and more importantly it makes a *destructive* call
(`unlinkat`, `renameat`) succeed on an object the caller did not name. A caller
that means the directory can say `"."`. The one-character difference between
`""` and `"."` is exactly the difference between an uninitialised buffer and an
intentional argument, and only one of the two should destroy a directory tree.

### Decision 2: `AT_EMPTY_PATH` goes to the descriptor, not to `/proc/self/fd/N`

glibc's older `fexecve` and several ports synthesise a path — `/proc/self/fd/N`
— and hand that to the path-based call. That would have been a small change
here, since the path route already exists. Rejected for three reasons:

- It needs a working `/proc/self/fd`, which is a filesystem dependency for what
  is otherwise a pure fd operation, and it fails on any process without `/proc`
  mounted — including early boot and anything in a namespace.
- It reintroduces the TOCTOU that `AT_EMPTY_PATH` exists to avoid: the whole
  point of naming a descriptor is that nothing can be swapped underneath it,
  and resolving a path — any path — gives that back.
- It cannot serve the kinds that have no name. A pipe, a socket and an epoll fd
  are all legal arguments to `fstatat(…, "", …, AT_EMPTY_PATH)` on Linux, and
  none of them has a filesystem path to synthesise.

So each call dispatches to its own fd-based sibling: `fstatat` → `fstat`,
`fchmodat` → `fchmod`, `fchownat` → `fchown`, and `faccessat` → an `fstat`
used as a descriptor-validity probe (we have no permission model for `access`,
so validity is the only question it can answer). When `dirfd` is `AT_FDCWD`
there is no descriptor to use, so the calls take the path route against `"."` —
which is what `AT_FDCWD` means.

`statx` is the odd one: it reads the raw 80-byte kernel record directly (for
birth time, which `struct stat` has nowhere to put), and had no fd-side way to
get one. Rather than drop birth time on the flagged route, `stat_fd_raw` was
added beside the existing `stat_path_raw`. It returns three-valued —
`Some(0)` record obtained, `Some(-1)` failed, `None` *this descriptor has no
record* — because a pipe is not an error, it is a thing with no stat record,
and `statx` then falls back to `fstat` and omits `STATX_BTIME` from the result
mask rather than reporting a birth time of zero.

### Decision 3: the orderings are the measured ones, not the plausible ones

Where the empty-path check goes relative to the other checks is observable, and
three of the four orderings I first wrote down from reasoning were wrong. They
were fixed by running the real calls on Linux 6.6 (`~/atprobe/`), the same
method as §731:

| I assumed | Linux actually does |
|---|---|
| a closed `dirfd` outranks the empty name | `fstatat(999, "", &st, 0)` is `ENOENT`, not `EBADF` |
| `utimensat`'s `tv_nsec` validation runs first | `utimensat(fd, "", {tv_nsec: 2e9}, 0)` is `ENOENT`, not `EINVAL` |
| `statx`'s NULL-buffer check runs first | `statx(fd, "", 0, …, NULL)` is `ENOENT`; `statx(fd, "", AT_EMPTY_PATH, …, NULL)` is `EFAULT` |
| `readlinkat` needs `AT_EMPTY_PATH` to mean anything | it takes no flags at all, and reads the link body whenever `dirfd` is itself a symlink |

The `statx` row is why the check is *split* across the buffer test rather than
placed on one side of it — the unflagged refusal above, the flagged route
below. That looks like untidiness and is not: the two orders genuinely differ.

The `readlinkat` row we cannot implement, because `open` does not implement
`O_PATH` and so no descriptor in this libc ever refers to a symlink rather than
its target. `ENOENT` is therefore correct for every fd a caller can actually
hold; the gap is recorded as
`B-READLINKAT-CANNOT-READ-A-SYMLINK-FD-BECAUSE-O_PATH-IS-UNIMPLEMENTED`.

### Verification

By mutation, not by the tests merely passing. Neutering `reject_empty_path` to
a no-op fails three tests; routing the `AT_EMPTY_PATH` branch back through the
textual join fails exactly the two descriptor-route tests and nothing else.
The `HandleKind::Pipe` case is the load-bearing one in a host build: `fstat`
answers a pipe from the handle kind alone with no syscall, so it is the only
kind that can succeed off-target — and it is precisely the case a synthesised
path could never have served.
