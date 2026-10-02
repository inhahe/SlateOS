## Original entry, retained — its conclusion is superseded by the correction above

**In short:** listing a directory in the in-memory filesystem got three times
slower, and it is my own refactor that did it. The benchmark `vfs_readdir`
ran 14.7–24.3 µs across twelve prior release runs; it now runs 50.2 µs. That
is not host noise — see below — and it is over the 50 µs target the benchmark
is scored against, so every `--bench` run from here on flags it.

**The measurement.** Twelve release runs from `4dd776c46` through `a949c5b38`
put `vfs_readdir` in a band of 14708–24331 ns, median ~16.3 µs. Two runs at
commit `0d6ea0bdc` (kernel sha `80256448cb`) read **50216 ns** and
**49970 ns**.

The second of those two is an **A/A re-run** — `--bench --no-stage`, which
implies `--no-build` and therefore boots the byte-identical ESP image. The
harness recognised the matching `kernel_sha` and scored the pair as the host's
noise floor rather than as movement. The two runs land 0.5 % apart. So the 3×
is a property of the binary, not of the machine that ran it. (That same
replication run absolved six *other* benchmarks — `ipc_channel`, `ipc_pipe`
and four more all flagged together and all fell straight back into range.
Seven simultaneous regressions across unrelated subsystems is the signature of
an environmental outlier; `vfs_readdir` was the one that survived it.)

**The cause is commit `3228012a6`** (the memfs flat-inode-table refactor,
already merged to `main`). It changed

```rust
children: BTreeMap<PathBuf, MemFsNode>     // node owned by its parent
```

to

```rust
children: BTreeMap<PathBuf, u64>          // parent holds inode numbers
inodes:   BTreeMap<u64, MemFsNode>        // one flat table, node stored by value
```

**The mechanism is `BTreeMap`'s inline value storage.** Rust's `BTreeMap`
stores values *in the B-tree nodes themselves*, not behind a pointer. A
`MemFsNode` is ~130 bytes (kind enum + `ino` + `links` + four `Timestamp`s +
`uid`/`gid` + `permissions` + `attributes` + an `xattrs` `Vec`), so each
internal node of `inodes` is multiple kilobytes and a `get` touches several
cache lines per level.

`memfs::readdir` (`kernel/src/fs/memfs.rs:846`) now pays one such lookup per
child:

```rust
children.iter()
    .filter_map(|(name, child_ino)| self.node(*child_ino).ok().map(|n| n.to_dir_entry(name)))
```

where before the refactor it iterated nodes its parent already owned and did
no lookups at all. The benchmark lists `/`, so the cost is per-entry and the
regression scales with directory size.

Ruled out while diagnosing: `readdir` does **not** clone the children map
(`children()` returns `&BTreeMap`, `node()` returns `&MemFsNode`, both shared
borrows); `nlink_of` is O(children) but is reached only from
`metadata`/`lmetadata`, not from `readdir`; `to_dir_entry` is cheap.

**Candidate fixes, in the order they should be measured** — and per
`CLAUDE.md` the breakdown benchmark comes *first*, because the above is a
structural argument and not yet a profile:

1. `BTreeMap<u64, Box<MemFsNode>>` — the B-tree's value array becomes 8-byte
   pointers, so an internal node holds ~16× more keys and the search touches
   far less memory. Cheapest change; costs one indirection per hit and one
   allocation per inode.
2. `Vec<Option<MemFsNode>>` indexed by inode number — O(1) instead of
   O(log n), no search at all. Best asymptotics, but inode numbers must stay
   dense or the vector becomes a leak; needs a free-list for reuse.

**Why the refactor is still right**: the flat table is what makes hard links
representable at all (two names, one node), which is why it happened. The fix
is to the table's *layout*, not to the decision to have one.
