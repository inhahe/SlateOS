## TD-ONE-CARGO-TARGET-DIRECTORY-PER-AWKWARD-MOMENT (lane C, 2026-08-20) — **cause fixed, one tree still to reclaim**

**Not a code defect. Read this if a build fails with `No space left on
device`, or with anything stranger — see the companion entry
TD-A-FULL-PAGE-FILE-MAKES-RUSTC-REPORT-A-SOURCE-ERROR for what a full volume
looks like when it does *not* name the disk.**

On 2026-08-20 the D: volume reached **1.2 MB free of 1.9 TB**, mid-way through
the whole-workspace gate `CLAUDE.md` requires before a merge. That run printed:

```
= note: ld: final link failed: No space left on device
error: could not compile `envoy-cli` (bin "envoy-cli" test)
rustc-LLVM ERROR: IO failure on output stream: No space left on device
error: could not compile `posix` (lib test)
```

so this one at least accused the right thing. `git commit` had already failed
a minute earlier with `fatal: sha1 file '…/index.lock' write error. Out of
diskspace`, which is how it was noticed at all.

### Cause: a workaround taken eleven times

Cargo serialises concurrent invocations sharing a `CARGO_TARGET_DIR` behind a
build lock, so a foreground `cargo clippy` started next to a background
`cargo test --workspace` waits — for the ~30 minutes the workspace build
takes. The obvious dodge is to hand the second run a target directory of its
own. Lane C had taken that dodge, across several sessions, eleven times:

| tree | | tree | |
|---|---|---|---|
| `target-cred` | `target-hl` | `target-hl2` | `target-hl3` |
| `target-hl4` | `target-lint` | `target-probe` | `target-probe2` |
| `target-systray` | `target-test` | | |

Every one is a full debug build of a ~200-crate workspace. **Deleting the ten
above took the volume from 1.2 MB free to 175 GB free.** None was named in any
document, none was pruned by anything, and each was created to dodge a wait of
minutes at a cost that fell on some later run instead.

That is the shape of the problem: the cost of the dodge is not paid by the run
that takes it. It lands on whichever run happens to be in flight when the
volume fills, as an error that may or may not mention the disk.

### Fix: a fixed per-lane set, and a script that notices when it slips

`scripts/prune-build-trees.py` lists every cargo build tree in the worktree
with its size, and deletes the ones outside the sanctioned set for the lane.
Lane C's set is `target`, `target-c` (long/background runs) and
`target-clippy-c` (the foreground check that would otherwise queue behind
them). Two working directories is enough; a third is a request to justify, not
a default.

It reports by default and only deletes under `--prune`, and it will not delete
a directory unless it carries cargo's own `CACHEDIR.TAG` signature *and* holds
no git-tracked file — so a source directory that happens to be called
`targets/` cannot be caught by it. It borrows `detect_lane` from
`scripts/which-lane.py` rather than reimplementing the mapping, because the
failure mode of getting the lane wrong here is deleting *another* lane's
build trees.

A script that needs its own build directory should take an env-var override
that **defaults into the sanctioned set**, the way
`scripts/reintro-list-hit-tests.py` does with `REINTRO_TARGET_DIR` — not mint
a new name.

### Still outstanding: the default tree is 121.8 GB

After the cleanup, lane C's three sanctioned trees measure:

```
  keep    target                 121.8 GB
  keep    target-c                12.4 GB
  keep    target-clippy-c        772.1 MB
```

`target` — the one used by any `cargo` command that *forgets* to set
`CARGO_TARGET_DIR` — is ten times the size of the tree that does the actual
work, because it has accumulated across sessions and target triples and
nothing has ever pruned it. It is sanctioned, so the script keeps it; that it
reached 121.8 GB is evidence the sanctioned set needs pruning on age too, not
just on membership. Reclaiming it costs one rebuild for whoever next runs a
bare `cargo` command in this worktree, and is worth doing.

**Other lanes are not touched.** `os/target`, `os/target-lint`,
`os-lane-a/target`, and lane B's `target`/`target-check`/`target-test` were
left strictly alone — another agent may have a run in flight in any of them,
and the script only ever prunes the worktree it is invoked from. If the
operator wants the whole drive swept, each lane should run the script in its
own worktree.
