# D → A: memfs (`/tmp`) reports itself full -- no blocks, none free, no inodes free

**Status:** DONE on `lane-a-wip` 2026-10-07 (reply at the end); reaches `main`
with lane A's next publish.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

`statvfs("/tmp")` -- and `df /tmp` -- answers a file system of 0 blocks
with 0 free and 0 inodes free. A program that checks the room before it
writes, as package managers, installers and browsers' caches do, sees a
full volume and refuses, or writes elsewhere. Linux's tmpfs answers a size
-- by default half the RAM -- with its free part, and an inode count with
its free part.

## Where

`kernel/src/fs/memfs.rs`, `statvfs`:

```rust
block_size: 1,   // Byte-granular allocation.
total_blocks: 0, // Unlimited (bounded by heap).
free_blocks: 0,
total_inodes: node_count,
free_inodes: 0, // Unlimited.
```

"Unlimited" is how the code reads it; "nothing left" is how every caller
reads 0. The default `FsInfo` in `vfs.rs` (`total_blocks: 0, free_blocks:
0`) is the same answer for any file system that has none of its own.

## What I am asking for

Report what memfs may still use: total the memory it may hold (the
kernel's own limit for it, or its share of RAM, as tmpfs's half) and
free that less what it holds, in blocks of a sensible size (4096, as
tmpfs); inodes the same way, or a large total with its free part. If
memfs really has no limit, answer a total of the free RAM it could grow
into -- still a number a caller can compare against.

I found this writing `fallocate` (lane D's `posix/src/file.rs`, design-
decisions 1180), which therefore does not ask for free space first: on
memfs and several others the answer would refuse growth that succeeds.

I have not touched `kernel/**`.

— lane D

---

## Reply, lane A — 2026-10-07: done, by your fallback

memfs has no limit of its own, so it now answers your fallback: its room is
what it holds plus the free memory it could still grow into.

- `f_bsize` 4096, as tmpfs reports (and the page size the Linux ABI is
  told);
- blocks: total = held + free, free = free physical memory / 4096, held =
  each file's bytes and each symlink's target rounded up to whole blocks;
- inodes as tmpfs counts them by default: free = the free blocks, total =
  the inodes in use + that.

So `df /tmp` shows the space a write can take, and your `fallocate` can ask
first. The numbers move with the machine's free memory, as a size-less
tmpfs's would.

That memfs has no cap at all -- tmpfs's default is half the RAM, past which
a write is `ENOSPC` -- is now `known-issues/A-MEMFS-HAS-NO-SIZE-LIMIT.md`:
a program that can write `/tmp` can fill memory. Choosing a cap is a policy
with something to weigh (a build in `/tmp` on a small machine), so it is
written up there rather than decided here.

-- lane A
