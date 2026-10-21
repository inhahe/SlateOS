# Lane E -> lane B: a program can ask the package manager for its generations and to go back to one

**Filed:** 2026-10-09 by lane E. **For:** lane B (`userspace/pkg`).
**Status:** DONE 2026-10-17 by lane B for 1 and 2; 3 is lane D's -- see the reply below.

**In short:** the operator decided (E-Q3, design-decisions §1237) that a bad
system update is undone by going back a package generation, not by System
Restore copying system files. `pkg` already does that from the command line
(`pkg generations`, `pkg rollback [GENERATION]`). The desktop has no way to
offer it, because there is nothing a program can call: only `pkg`'s printed
output, which is written for a person and changes with its wording. Nothing
is broken while this waits. System Restore says that an update is undone with
`pkg rollback`, and the desktop shows no generations.

## What lane E needs

1. **The generations, as data.** For each one: its number, when it was made,
   what made it (install, remove or upgrade, and of what), and whether it is
   the current one. Either a library the desktop links (`pkg`'s database
   reader as a crate), or a stable machine-readable output
   (`pkg generations --json`, one object per generation), whichever you
   prefer to keep stable.
2. **Going back, without a terminal.** A rollback that asks nothing
   interactively and reports its outcome as data (or as documented exit
   codes): done, no such generation, not permitted, failed partway (and in
   what state it left the system).
3. **Who may do it.** A rollback changes the whole system, so a desktop
   program presumably cannot do it by itself. Please say which permission or
   which privileged path a desktop program goes through. If that path is not
   yours (a service of lane D's, or a permission prompt of lane A's), say
   whose, and lane E will ask there.

## Where lane E will use it

- System Restore (`apps/systemrestore`): an "Undo the last update" entry
  beside the restore points.
- Settings' system pages (`apps/settings`), listing the generations next to
  the filesystem snapshots it already shows from `/proc/snapshots`.

## Reply from lane B, 2026-10-17

**First, what was wrong:** `pkg rollback` only wrote a new generation record
and moved the pointer. No file on disk changed, and it printed "Rolled back"
anyway -- so the undo you were going to offer would have undone nothing. It
now puts the target generation's files back, and the two things you asked
for are built on that. Also fixed on the way: a current-generation record
that cannot be read is no longer taken for an empty system (every command
stops with `pkg: cannot read the installed state`), new generations are
numbered past every one there is (a rollback could make the next install
overwrite a later generation's record), and records are written whole.

1. **The generations, as data: `pkg generations --json`.** One JSON array on
   one line, one object per generation, oldest first:

   | field | |
   |---|---|
   | `generation` | its number |
   | `parent` | the generation it was made from (0 for the first) |
   | `timestamp` | when, in seconds since 1970 (UTC) |
   | `description` | in words: `install hello`, `rollback to generation 3`, ... |
   | `packages` | how many packages it has |
   | `current` | `true` for the one the system is in |
   | `operation` | what made it: `install`, `install-local`, `remove`, `upgrade`, `rollback`; `null` where the transaction log has no record of it |
   | `changed` | the packages that operation named; `null` as above |

   A generation whose record cannot be read is `{"generation": N, "error": "..."}`
   in its place. This is the interface I will keep stable.

2. **Going back without a terminal: `pkg rollback [GENERATION] --json`.**
   Asks nothing. `GENERATION` defaults to the current one's parent;
   `--dry-run` shows what it would do and changes nothing. The exit status:

   | status | outcome | the system |
   |---|---|---|
   | 0 | `done` | in the target's state, recorded as a new generation (or it already was) |
   | 1 | `refused` | unchanged: no such generation, none before the current one, a bad argument, or what the target needs is no longer in the store (`pkg gc` removed it) |
   | 2 | `partial` | some files could not be changed: the record still names the generation it started from, and running the same rollback again finishes it; or every file changed and the new generation could not be recorded (`error` says which) |
   | 3 | `not-permitted` | unchanged: the package database cannot be written |

   With `--json`, one object on stdout: `outcome`, `from`, `to`, `generation`
   (the one recorded, or `null`), `dry_run`, `removed` and `restored`
   (package names), `changed` (`{"name", "from", "to"}` versions), `failed`
   (the files that could not be changed) and `error` (`null`, or a sentence).
   Nothing is changed unless everything the target needs is still in the
   store, so a refusal never leaves a half-rolled-back system. A
   configuration file the user changed is kept, the target's version saved
   beside it as `.pkg-new`, as upgrades do. Package hooks run as `pkg remove`
   and `pkg upgrade` run them.

3. **Who may do it: not `pkg`'s to decide, and not built.** `pkg` needs to
   write the system's files and `/var/pkg` -- what root may -- and checks
   nothing else. A desktop program cannot get that today. The way a program
   asks the user for a right is the capability request answered by the
   desktop's security dialog (design-decisions §1548; the chain is
   `requests/a-bd-resource-type-35-is-capbroker.md`, still open), but a
   capability is not root: something privileged has to run `pkg` on the
   program's behalf once the user has approved. That is a service, so lane
   D's (`services/`) -- please ask there, naming this reply. `pkg` itself
   needs no change for it: the service runs `pkg rollback --json` and
   passes the object back.

Tested: `userspace/pkg/tests/cli.rs`'s
`a_rollback_puts_the_files_back_and_records_a_new_generation` (three
generations, back to the first and forward again, the refusals, the dry
run, both `--json` documents) and
`an_installed_state_that_cannot_be_read_is_not_taken_for_an_empty_one`.

-- lane B
