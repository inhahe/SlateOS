## B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED — FIXED 2026-09-03

**In short:** `mv dir /other/filesystem/` fails. Moving a directory between two
filesystems means copying the whole subtree and then removing the original, and
this `mv` does not do it — it refuses with "moving a directory across filesystems
is not implemented by this mv" rather than doing it wrong. On a machine with a
single filesystem nothing is affected; on one with `/home` on its own volume this
is the ordinary thing a user does with `mv`.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `copy_across_devices`'s directory
arm, and the refusal `move_one` hoists above the destination-clearing. The
hoisting is deliberate and must survive the fix: GNU clears the destination first
and then copies, which would become clear-then-refuse here and destroy a file for
a move that never happened.

**What GNU does.** `copy.c`'s `copy_internal` recurses through `readdir`,
recreating each entry and then `rmdir`-ing the source directory once its contents
are gone, with the parent's mode set last so that a directory that was not
writable during the copy still ends up with the mode it had.

**The proper fix.** A recursive fallback. `cp.rs` already has the whole of it —
the `fts` walk, the `fileid`-keyed hard-link table that `preserve_links` needs,
and `fsattr`'s preserve ordering — which is the reason `cp`'s source-side
attribute readers were moved into `fsattr` in the first place. The work is to
share the walk rather than to write a second one, and it should follow
`B-MVS-CROSS-DEVICE-FALLBACK-THROWS-AWAY-THE-TIMES-AND-THE-OWNER`, because a
recursive copy that does not preserve attributes would only multiply that defect
by the number of files in the tree.

**Both prerequisites are now closed** (times/owner, and
`B-MVS-CROSS-DEVICE-FALLBACK-DROPS-EXTENDED-ATTRIBUTES` on 2026-09-01), so
nothing gates this but the work itself.

**"Share the walk" is measured to be bigger than it sounds — read this before
starting.** `copy_tree` (`cp.rs:2709`) is only a `make_dir` plus a loop over
`copy_entry`, and it looks liftable on its own. It is not: `copy_entry`
(`cp.rs:2863`) calls `stat_destination`, `overwrite_allowed`,
`remove_destination_first` and `place_entity`, reads `job.flags.follow_walked()`
and consults `job.copied` — and `place_entity` is the whole per-entity dispatch,
which reaches the file copy, the symlink recreation, the preserve tail and back
into `copy_tree`. Lifting the walk therefore means lifting cp's copy *core*, not
a walk. `cp.rs` is 8411 lines; the flag coupling is the shallow part (33
`job.flags.` sites).

That is still the right shape, because it is GNU's: there is one
`copy_internal`, parameterised by `struct cp_options`, and `mv` is that engine
with `move_mode = true` and `preserve_*` forced on (`mv.c:119-146`). Two
independent walks would be two places for the same bug.

**Do it in stages that each leave the tree green**, because a half-extracted
engine is a red `main` for all three lanes:

1. Leaf helpers that are already pure (`read_dir_fastread`, `make_dir`,
   `create_dir_with_mode`, `ModeDebt`) into a library module.
2. The per-file copy and its preserve tail — the `copy_reg` equivalent — which
   is where `mv`'s existing `preserve_onto_file` should end up being the same
   code rather than a parallel one.

   **This stage is not a pure move, and knowingly so.** The two copy *bodies*
   it merges are each half-right in opposite directions, so there is no
   behaviour to preserve — only a correct one to arrive at. `mv` uses
   `io::copy`, which gets the `copy_file_range` offload and loses the failing
   side; `cp` uses a plain 64 KiB read/write loop, which keeps the two
   sentences and never offloads at all. GNU's `sparse_copy` does both and has a
   **third** sentence, `error copying SRC to DST` (`copy.c:376`), for the
   offload failure that names neither end — which is why it never needs to tell
   read from write there. The unified body is GNU's, and it closes
   `B-MVS-CROSS-DEVICE-COPY-CANNOT-TELL-A-READ-FAILURE-FROM-A-WRITE-FAILURE`
   (see that entry for the errno list the fallback is entered on).

   So the "harness numbers must not move" rule below is suspended **for this
   stage only, and only for cases that reach a copy failure or measure copy
   throughput**. Everything else must still be byte-identical, and the
   exception must be spent deliberately: any case that moves has to be one this
   note predicts, not one discovered afterwards and rationalised.
3. Opening the destination — `cp`'s `create_destination` against `mv`'s, the
   one place the two programs genuinely differ rather than merely duplicate.
   (This stage was always intended and always described in the progress note
   below, but was missing from this list until 2026-09-02; the list said
   "3. the walk", the prose said "stage 3 is the destination open, then the
   walk, then stage 4", and the two disagreed by one for two days.)
4. The walk and `place_entity`, with `CpFlags` becoming the engine's options
   struct.
5. `mv`'s `copy_across_devices` directory arm drives the engine, then `rmdir`s.

**Progress: stages 1, 2, 3 and 4 have landed.** `userspace/coreutils/src/copy.rs`
holds the leaf helpers, the shared preserve tail (`preserve_attributes` and
everything it calls), the shared byte copy (`copy_bytes`), the shared
destination open (`open_destination`) and now the walk itself, all of which
both programs call. The copy-body defect stage 2 predicted was real and is
closed; see `design-decisions.md` §745, which supersedes §741.

Stage 3 unified the two `create_destination` functions behind one `copy::Dest`
describing what is at the name — `New`, or `Exists(Clobber)` — and deleted 335
lines from the two binaries. Folding `cp -f`'s unlink *inside* `Exists` is what
makes the type honest: the unlink is only reachable when something is in fact
there, so two independent booleans would have spelled a fourth combination the
code must then remember to ignore. Both harnesses stayed byte-identical across
the move: cp 581/0/30 and mv 360/0/11, before and after.

Stage 4 moved the walk — `copy_tree`, `copy_entry`, `place_entity` and the
whole per-entity dispatch under them — out of `cp.rs` and into `copy.rs`: 1,310
lines deleted from the binary against 1,506 added to the library, the ~200-line
difference being `Opts`, `Run`, `Deref` and their docs, which did not exist
before. `CpFlags`
did **not** become the engine's options struct as the list above says; it
narrows into a new `copy::Opts` through `CpFlags::opts`, because a dozen of its
fields (`-r`'s own meaning, `-T`, the backup *type*, the interactive mode) are
`cp` command-line concepts the engine has no use for and `mv` cannot supply.
The three choices inside the move that had a real argument on both sides are
`design-decisions.md` §750.

Two things the stage turned up rather than caused, both fixed in it:

- `cp.rs` named `fsattr::set_times`, `set_xattr` and `get_xattr` in three
  `#[cfg(unix)]` test helpers while importing only `fsattr::{Link, On}`. It
  compiled here for a bad reason — nothing on this Windows host ever
  type-checks the `cfg(unix)` half — and is written up separately as
  `B-CP.RS'S-UNIX-ONLY-TESTS-NAME-A-MODULE-THE-FILE-NEVER-IMPORTS`.
- Four `mv.c` line citations were off by the width of `cp_option_init`'s
  SELinux fields: `mv.c:139` and `mv.c:140` are `preserve_security_context`
  and `set_security_context`, not the two unlink knobs (which are 127 and
  128). One of them carried a wrong *claim* as well — that a move sets
  `unlink_dest_before_opening`, which is the reason its destination is always
  `Dest::New`. It sets it false. Upstream's general unlink is guarded by
  `! x->move_mode` in as many words (`copy.c:2571`); what actually clears a
  move's destination is the EXDEV fallback's own unlink, so that a
  cross-device `mv` "acts as if it were really using the rename syscall"
  (`copy.c:2870`), setting `new_dst` on the spot (`copy.c:2892`). A citation
  exists to be checked, and these three would have failed the check.

What remains is **stage 5**: `mv`'s `copy_across_devices` grows a directory arm
that drives the engine and `rmdir`s behind it, which is what actually closes
this entry. `overwrite_allowed`'s `bool` return has to widen to a three-valued
verdict first — `Interactive::AlwaysSkip` is a skip that *succeeds*, and a
directory arm is the first caller that can tell the difference.

Each stage is certifiable the same way the `fsattr` moves in this chain were:
`scripts/cp-diff.sh` and `scripts/mv-diff.sh` must stay byte-identical across
it. A stage that moves code without changing behaviour and *does* move those
numbers has a bug in the move.

The numbers themselves are deliberately **not** written down here any more.
They were — "557 passed, 0 differed, 33 differ on purpose" and "341/0/11" —
and within a day of being written both were wrong, because the harnesses gain
cases faster than this entry gets re-read: cp-diff is 572/0/30 and mv-diff
357/0/11 as of 2026-09-01, having moved twice that day for reasons that had
nothing to do with this work. A stale target number is worse than none, since
the natural reading of a mismatch is that the stage broke something. Take the
baseline by *running both harnesses immediately before starting a stage*, and
compare against that.

**How it is caught.** `scripts/mv-diff.sh` §22, one `xfail_case` naming this
entry, whose fixture carries a setgid bit and a subdirectory so that the case
keeps measuring something after the refusal is lifted.

**FIXED 2026-09-03 — stage 5, which is what closes this.**
`copy_across_devices` grew a directory arm of eleven lines, because the engine
extracted by stages 1–4 already does all of it: `copy::stat_destination`
followed by `copy::place_entity`, the same call `cp -r` makes for a directory
operand. There is no second walk, which was the whole point of doing the
extraction first.

Three things had to join it, none of them the walk:

- **`copy::Opts::move_mode`** — GNU's `x->move_mode` (`copy.h:169`). The *only*
  thing it decides inside the engine is what `-v` prints, and the asymmetry is
  upstream's rather than ours: a moved **file** is announced `copied 'FAR/d/f'
  -> 'g/f'`, naming both ends (`copy.c:2887`), while a moved **directory** is
  announced `created directory 'g'`, naming only the end that was made
  (`copy.c:2988`). A copy says `'d' -> 'g'` for both.
- **`clear_destination` learned `AT_REMOVEDIR`** — `rmdir` when the *source* is
  a directory, `unlink` otherwise, which is GNU reading the source's kind to
  decide what to do to the destination (`copy.c:2875`). Measured, the
  consequence is that `mv -T FAR/d existing-empty-dir` succeeds and
  `mv -T FAR/d existing-non-empty-dir` fails with
  `unable to remove target: Directory not empty` — plain `rmdir` semantics, and
  the sentence this `mv` already had.
- **`remove_source` became `rm -r`** — depth-first, one `mv: cannot remove X`
  per entry that will not go, and `-v`'s `removed X` / `removed directory X`
  printed *by the walk* rather than by `move_one`, because a directory yields
  one such line per entry and only the walk knows what it removed. A failed
  child silently skips its ancestors, which is `mark_ancestor_dirs`
  (`remove.c:431`) plus `prompt`'s `fts_number` check (`remove.c:206`): the
  `ENOTEMPTY` they would earn says nothing the child's diagnostic did not.

**`Failed` had to become an enum**, and that is the one design point worth
reading twice. Every other step of the fallback fails with *one* sentence, which
is why `Failed` carried it; a tree copy fails at as many entries as went wrong,
and the engine has already reported each one where it happened. `Failed::Reported`
is the variant that carries nothing on purpose — the caller's signal to stop and
to put a `-b` backup back, and nothing else. GNU has the same silence:
`copy_internal` reports and returns false, and `do_move` adds nothing on top of
it (`mv.c:186`).

**What the numbers did.** §22's `xfail_case` is a `run_case` now, taking
`scripts/mv-diff.sh` from 360/0/11 to **361 passed, 0 differed, 10 differ on
purpose**; `scripts/cp-diff.sh` stayed byte-identical at **581/0/30**, which is
the certification that the engine change was confined to `move_mode`.

**One gap the fix opened, logged rather than fixed**:
`B-MV-NEVER-WARNS-ABOUT-A-TWICE-NAMED-SOURCE-DIRECTORY`.
