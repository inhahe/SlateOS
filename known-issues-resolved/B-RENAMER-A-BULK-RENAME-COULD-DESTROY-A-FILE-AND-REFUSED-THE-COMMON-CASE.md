## B-RENAMER-A-BULK-RENAME-COULD-DESTROY-A-FILE-AND-REFUSED-THE-COMMON-CASE (lane C, 2026-08-16) — FIXED

**In short:** The bulk renamer had two opposite problems at once. It **refused**
the most ordinary bulk rename there is — shifting a numbered run, `1.jpg` to
`2.jpg`, `2.jpg` to `3.jpg` and so on — reporting a conflict for every file,
because it treated a name another file is *moving out of* as a name that is
taken. And it would have **destroyed** a file in the renames it did allow,
because it performed them in list order: told to swap two names, whichever move
ran first would write over the other file. It also never updated the stored
location of a renamed file, so a second rename in the same session acted on a
file that was no longer there. Nothing here has reached a real filesystem yet
(the rename is still in-memory), which is the only reason no data was lost.

**Where:** `apps/renamer/src/main.rs` — `detect_conflicts`, `execute_rename`,
`undo`, and the path rewriting inside them.

**What was wrong**

1. **The path was never updated.** `execute_rename` did:

   ```rust
   file.original_name = file.new_name.clone();
   file.original_path = file.original_path.replace(&file.original_name, &file.new_name);
   ```

   The assignment happens *first*, so by the time `replace` runs both arguments
   are the new name and the call is a no-op. `original_path` kept naming the
   old file forever.

   Fixing only the order would have left a second bug behind it. `str::replace`
   substitutes **every** occurrence, and a path is not just a filename: a file
   called `Nirvana` inside a folder called `Nirvana` becomes
   `/music/Nevermind/Nevermind` — a path under a directory that does not exist.
   A folder that merely *contains* the name is enough (`/report-archive/report`
   → `/summary-archive/summary`). The replacement, `replace_file_name`, is
   structural: it splits at the final separator and substitutes only what
   follows it.

2. **The conflict check asked the wrong question, twice.** It compared each
   file's new name against every *other* file's **original** name:

   ```rust
   if i != j && orig.eq_ignore_ascii_case(&file.new_name) { file.conflict = true; }
   ```

   - Comparing against *original* names means a name that another file is
     **vacating** counts as occupied. Shifting a numbered sequence — the single
     most common bulk rename — was flagged as one conflict per file and refused
     outright. That is not a collision; it is an **ordering constraint**.
   - `eq_ignore_ascii_case` is wrong on this OS specifically. Slate OS has a
     case-sensitive filesystem (`design.txt`), so `photo.jpg` and `PHOTO.jpg`
     are two different files and renaming one to the other's case variant is a
     legitimate rename. Being ASCII-only, it was not even *consistently*
     case-insensitive for the names where a user might have expected it.

   It now computes the name each file will **end up** with (its new name if
   selected, its current name otherwise), counts those, and flags a file only
   when its final name is shared — and compares exactly.

3. **Nothing ordered the renames.** This became load-bearing the moment (2) was
   fixed: permitting shifts and swaps means the *order* of the moves decides
   whether a file survives. `a.txt → b.txt` and `b.txt → a.txt` performed in
   list order loses a file whichever one runs first, and a shift performed
   front-to-back overwrites each file with the one behind it.

**The general shape.** A batch of renames is a graph, not a list. Each rename
is an edge; a destination that is currently occupied by another *source* is a
dependency, not a conflict. The distinction the old code could not draw is
between a name that is occupied **permanently** (a real collision — refuse it)
and one that is occupied **for now** (do it later). Only a true cycle has no
valid order at all, and a cycle needs exactly one extra move to break.

**The fix — `rename_plan`**

Repeatedly emit every pending rename whose destination is currently free,
updating the occupied set as it goes. When a full pass emits nothing, everything
left is in a cycle: park one source under an unused temporary name
(`.renamer-tmp-N`, chosen so it collides with nothing in the directory, *including
a real file that happens to be called that*) and re-queue the remainder of that
rename against the temporary. Renames where the name does not change are
dropped up front — otherwise they are permanently "blocked" by themselves and
get mistaken for a cycle.

The output is an ordered `Vec<RenameStep>` that can be handed straight to
`fs::rename` when this is wired to a real filesystem, which is the point:
whoever wires it cannot reintroduce the clobber, because the ordering is not
their responsibility any more. `execute_rename`, `undo` and `redo` all go
through it — undoing a batch is itself a batch, and reversing a swap by walking
the stored pairs in order clobbers exactly as the forward direction does.

**Tests** (15 new, 83 pass), including `no_step_in_any_plan_overwrites_a_file`,
which simulates the plan against a model directory and asserts the invariant
that actually matters at every step — the source is present and the destination
is free — across a forward shift, a backward shift, a two-cycle, a three-cycle,
two independent cycles with a bystander, and a cycle with a tail.

**Verified by injection.** Each of the three original behaviours was put back in
turn: the unordered plan reddens 10 new tests, the path bug 4, the old conflict
check 5. **No pre-existing test fails under any of them**, and the new tests
that stay green under each injection are the happy-path ones —
`two_files_renamed_to_the_same_name_are_flagged` and
`colliding_with_a_file_that_is_not_being_renamed_is_flagged` pass with the old
conflict check in place, because a genuine duplicate is the one case it got
right. That is why all three shipped.

**Still open in this program**

- **The rename is still entirely in-memory.** `execute_rename` mutates the file
  list and never calls `fs::rename`, so `rename_plan`'s ordering guarantee is
  currently unexercised against a real filesystem. When it is wired up, the
  plan must be executed step-by-step with the failure of any step aborting the
  rest — a half-applied plan that has parked a file under `.renamer-tmp-N` and
  then stopped leaves that file under a hidden name with nothing pointing at
  it. The plan should be journalled, or the temporary parking undone on
  failure. Not fixed; nothing to fix until the filesystem call exists.
