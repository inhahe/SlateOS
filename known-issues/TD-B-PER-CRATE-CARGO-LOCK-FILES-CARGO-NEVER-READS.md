## TD-B-PER-CRATE-CARGO-LOCK-FILES-CARGO-NEVER-READS (lane B, 2026-09-26) — lane B's fixed; 148 in other lanes' trees

**In short:** a crate that is a member of the workspace is built with the
workspace root's `Cargo.lock`; a `Cargo.lock` inside the crate's own
directory is never read. The tree carried 340 such files, left from builds
before the crates joined the workspace, and they had rotted as unread files
do -- `userspace/crond/Cargo.lock` still named its package `crond2`. Nothing
builds differently because of them; the harm is to whoever reads one as the
crate's dependency set.

**Lane B's 192** (`userspace/`, `init/`) are deleted (74fde4474).

**Still present, each lane's to delete:** `apps/` 135 (lane E), `gui/` 8
(lanes C, F), `net/` 2, `kernel/` 1 (lane A), `posix/` 1, `toolchain/` 1
(lane D). Every one belongs to a workspace member. To list them:

```sh
cargo metadata --no-deps --format-version 1   # the members' manifest paths
git ls-files '*Cargo.lock'                     # minus the root's
```

and delete a lock file only when its directory is a member's. **Not dead,
and not to be deleted:** `netipc/`, `netproto/`, `netring/`, `tzrules/` and
six under `services/` (`hello`, `httpget`, `init`, `netstack`, `ticker`,
`udpget`) are outside the workspace, so cargo does read their lock files.
