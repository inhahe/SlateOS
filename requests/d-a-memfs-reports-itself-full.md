# D → A: memfs (`/tmp`) reports itself full -- no blocks, none free, no inodes free

**Status:** open — for lane A. Nothing for lane D to do: the C library
passes the kernel's numbers through `statvfs`/`statfs` as they come.

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
