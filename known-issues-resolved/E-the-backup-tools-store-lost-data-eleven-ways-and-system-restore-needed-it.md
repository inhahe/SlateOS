### [E] The backup tool's store lost data eleven ways, and System Restore needed it -- 2026-09-27
**Status:** FIXED (lane E, 2026-09-27) -- the engine moved out of `apps/backup` into `apps/snapstore`, which the backup tool and (next) System Restore both use.

**In short:** the backup tool's store -- the copies it keeps and the lists of
what each backup holds -- could lose or misstate data in eleven ways, found
when System Restore needed a real store to keep restore points in. A backup
could be silently incomplete and still say "Backup complete"; two backups
taken in one second overwrote each other's list; restoring a backup could
bring back files deleted before it was taken; and a private file restored
after being deleted came back readable by everyone. All eleven are fixed,
each with a test that fails without its fix.

| What went wrong | What a user saw |
|---|---|
| A file, folder or link it could not read was a warning on stderr and was left out | "Backup complete", exit 0, and the file was not in the backup |
| Backup ids were `<seconds>-<kind>`, never checked | two backups in one second shared a directory; the second's list replaced the first's |
| A file was hashed, then copied into the store under that hash | a file changed between the two was stored under another content's name, and handed back to every later backup that deduplicated against it |
| An incremental listed only what changed, and restore rebuilt the rest by walking back to a full one, adding and never removing | restoring an incremental brought back every file deleted since the full backup; `diff` against one listed every unchanged file as deleted |
| An incremental's parent was the newest backup of *any* source | a store of two folders compared each with the other |
| A record (`meta.json`) that would not parse was skipped when listing | `prune` then treated every blob only that backup named as an orphan and deleted it |
| `prune` collected orphans with no regard to what was being written | a backup running beside a prune could have its new blobs deleted before its list named them |
| Modes were not recorded | a private file deleted and restored came back with the default mode |
| "Keep monthly" counted thirty-day blocks | the first and last of one January were two "months", December and January one |
| `schedules.json` that would not parse read as "no schedules" | adding one schedule wiped every other |
| Arguments were read with `env::args()` | `backup create --source` on a folder whose name is not UTF-8 panicked before reading anything |

And one in the store's JSON reader: a number read as a count was `n as u64`,
so -5 read as 0 and a mode of 420.7 as 420, and the check that refuses a
corrupt mode never refused anything.

**Where.** `apps/snapstore/src/`: `lib.rs` (`Store::capture`, `restore`,
`files`, `list`, `collect_garbage`, `claim_dir`, `valid_id`,
`ensure_real_parents`), `store.rs` (`ContentStore::ingest` -- hash the source,
skip the copy if the store has it, otherwise hash the *staged copy* and name
the blob by that), `scan.rs` (`walk`, which returns what it could not read),
`manifest.rs` (version 3: complete listings, modes, folders, unread paths,
the capture's exclusions), `retention.rs` (calendar months; a complete
snapshot does not hold its chain), `json.rs` (`as_u64`). The command line is
`apps/backup/src/main.rs`, now its arguments and its messages. Every existing
store reads: versions 1 and 2 of both files are still read, and an old
incremental is still rebuilt from its chain.

**New with it: a restore that makes a folder match a snapshot**
(`RestoreOptions::mirror`) -- it removes what the snapshot does not have,
never what the capture could not read or excluded, and never writes through
a link. System Restore is its first user; the command line does not offer it.

**Tests:** `apps/snapstore` 70 unit, 19 store and 1 routing test (the store's
writes all go through `safeio`), run on Windows and on Linux under WSL (the
link and mode tests are Unix-only); `apps/backup` 21. Mutation:
`apps/snapstore/mutate.py`, 20 rows. The harness learnt to read failures from
a crate's `tests/` binaries, which it scored as crashes
(`scripts/mutation_harness.py`, `failed_tests`; `scripts/test-mutation_harness.py`).
