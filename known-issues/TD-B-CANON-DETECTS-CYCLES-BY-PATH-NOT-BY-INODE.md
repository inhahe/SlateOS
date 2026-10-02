## TD-B-CANON-DETECTS-CYCLES-BY-PATH-NOT-BY-INODE (lane B, 2026-08-22) — OPEN, cannot bite on SlateOS today

**In short:** A *symlink* is a file whose contents are the name of another file,
so following one can go in circles — `a` points at `b`, `b` points back at `a`.
`readlink -f` and `realpath` have to notice that and stop, rather than following
the circle forever. Ours notices by remembering the **names** it has already
walked through; the program we copied the algorithm from remembers the
**identity numbers** the filesystem gives each directory instead. Those two are
the same answer everywhere except on a filesystem where one directory has been
made to appear at two different names at once — a "bind mount", which SlateOS
does not have and has no plans to add. If that ever changes, a circle built out
of the two names would be followed 100,000 times before a safety counter cut it
off, instead of being spotted immediately. Nothing is broken now; this is a note
about what would have to change first.

### Where

`userspace/coreutils/src/canon.rs` — `canonicalize()`, the `seen` set:

```rust
let mut seen: HashSet<(Vec<u8>, Vec<u8>)> = HashSet::new();
...
} else if !seen.insert((parent, comp)) || expansions > MAX_EXPANSIONS {
```

`parent` is the canonical path accumulated so far; gnulib's
`canonicalize_filename_mode` (`lib/canonicalize.c`) instead stores
`(st_dev, st_ino, component)` obtained from a `stat` of the parent.

### Why ours is different

Two reasons, both about what the code is allowed to assume.

1. **[`Fs`] has three methods on purpose.** `cwd`, `read_link` and `dir_check`
   are the whole filesystem interface the algorithm needs, which is what lets
   the entire component walk be tested against a `BTreeMap` fake with no
   temporary directories and no host-specific behaviour. Adding a fourth method
   that returns a `(device, inode)` pair would drag a concept into the trait
   that the SlateOS VFS does not yet expose to userspace at all — there is no
   `stat` field carrying a device number today — so the trait would have to
   describe something that cannot be implemented.

2. **Inode identity is not available on the host either.** The host build (the
   one all these tests run under) is Windows, where `std::fs::Metadata` exposes
   no `st_ino`. So the inode-keyed version would be dead code on the host and
   untestable on the target, which is the worst of both.

### What breaks, precisely

Only a directory reachable under two distinct canonical paths. Concretely, with
`/x` and `/y` naming the same directory, and `/x/l -> /y/l`:

- gnulib: the second visit to the directory has the same `(dev, ino)` and the
  same component `l`, so the loop is detected on the second hop and `ELOOP` is
  reported.
- ours: `/x` and `/y` are different byte strings, so the pair never repeats.
  The walk alternates forever until `expansions > MAX_EXPANSIONS` (100,000)
  trips, and *then* reports `ELOOP` — the same answer, after a bounded amount
  of pointless work.

Note the failure mode is a slow correct answer, not a hang and not a wrong
answer. That is what makes this tech debt rather than a bug.

### What the correct fix looks like

When the SlateOS VFS exposes a stable per-file identity to userspace — it will
need one for `ls -i`, for `du`'s hard-link deduplication, and for `cp -a`'s
"same file" refusal, so this is coming regardless — add a fourth method:

```rust
fn identity(&self, path: &[u8]) -> io::Result<(u64, u64)>;
```

with a default implementation returning `Err(Unsupported)`, key `seen` on
`(identity_or_path, comp)`, and keep `MAX_EXPANSIONS` as the backstop for
implementations that decline. The fake `Fs` in `canon.rs`'s tests can then
hand out synthetic identities, and the bind-mount case above becomes a unit
test rather than a paragraph.

**Trigger:** the first of (a) a `dev`/`ino` pair reaching userspace through the
SlateOS `stat` ABI, or (b) bind mounts appearing in the VFS. Neither exists as
of 2026-08-22.
