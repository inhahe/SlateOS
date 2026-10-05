## TD-C-THREE-MORE-SHELL-SETTINGS-MODULES-ARE-REACHED-BY-NOTHING

**Status 2026-09-16: all three triaged. Two deleted, one deliberately kept.**
`default_apps.rs` went after its model was ported to `gui/associations`;
`backup_settings.rs` went because `apps/backup` already implements a superset
of it. `power_settings.rs` is **kept**, and the reason is below -- it is the
one of the three that holds design recorded nowhere else, which is the same
ground on which `remote.rs` was kept in the sibling entry.

**Read that as the rule, not the score.** Three modules with identical
symptoms -- `pub`, compiling, tested, reached by nothing -- split two-to-one on
the only question that matters, which is whether a working implementation of
the same idea exists somewhere else. Two had one; the third does not. A sweep
that deleted all three for looking alike would have been right twice and
destructive once.

**Date:** 2026-09-16. **Lane:** C.
**Where:** `gui/desktop/src/{power_settings,backup_settings,default_apps}.rs`.

**In short:** the desktop shell carries three settings screens that no code
anywhere opens. Together they are about 6,800 lines. They compile, their tests
pass, and nothing reaches them -- `pub` in a library, so the compiler cannot
say so. Found while looking for somewhere to put a screen-lock delay and
noticing that the obvious home already modelled screen-off and sleep timeouts
that nothing persists and nothing honours.

| Module | Lines | Reached by |
|---|---|---|
| `power_settings.rs` | 1855 | nothing -- its one mention elsewhere is inside a doc comment |
| `backup_settings.rs` | 2654 | nothing |
| `default_apps.rs` | 2325 | nothing |

This is the same shape as
`TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`, which named
`snapshots.rs`, `associations.rs` and `remote.rs` and is two-thirds resolved.
These three were not in that entry. (`privacy_settings.rs`, listed alongside
them in an older triage, has since been deleted.)

**Do not simply delete them.** That entry's own conclusion is the rule: two
were removed, and `remote.rs` was *kept* because "both halves of it hold
design that is recorded nowhere else, so deleting it would lose knowledge
rather than remove duplication". The same question has to be asked of each of
these three, and at least one clearly holds something:

* `default_apps.rs` -- **DONE 2026-09-16, precondition met first.** The rule
  above was that porting the category model came before deleting it. It did.
  The model now lives in `gui/associations` as design-decisions 857, and it is
  not a port so much as a grounding: the shell's version carried a
  hand-written list of "the music extensions", and `guitk::filetypes` already
  held `FILE_TYPE_TABLE`, 133 entries of extension to `FileCategory`, which
  `apps/explorer` was already importing. So a category is now a *view* on that
  table, and the toolkit gained `extensions_in()` -- the reverse of the
  `category_from_extension()` it already had -- with a round-trip test that the
  two directions agree.

  Three of the six categories did not survive the move, and their absence is
  the useful part. "Web browser" and "email" have no consumer anywhere in the
  tree -- there is no browser in `apps/` at all, and the only mentions of
  `mailto` were this dead module and `apps/qrcode`, which encodes the string
  into a QR image. "Documents" was dropped because `txt` and `pdf` are both
  filed under `Document` and do not share a program, so the row could only
  impose a wrong association; that is now a test against the table rather than
  a claim in prose.

  Porting also found something the shell module could never have revealed:
  there is **no write side**, and there cannot be one outside
  `apps/fileassoc`. Its `write_into` prunes every entry its registry no longer
  holds, so a second writer's associations are deleted at the next save with no
  error anywhere. A `set_category` was written, compiled, tested against a
  document, and deleted for that reason. What moved is the read side, which now
  has a real consumer: the Settings page reports each kind as Agreed, Mixed or
  Not set.

  Removed: 2,325 lines, 35 public items, 35 tests -- a suite proving the
  behaviour of a panel nobody could open. The shell's test count went 2,907 ->
  2,872, exactly the 35, which is the check that nothing else went with it.
* `backup_settings.rs` -- **DONE 2026-09-16.** 2,654 lines, 48 public items,
  36 tests, persisting nothing: no `settingsfile` call, no write, no reference
  but the `pub mod` line. It modelled backup types, frequencies, day-of-week,
  targets, sources, exclude rules, retention policies, status and history.

  `apps/backup` already implements all of it *and runs*: retention, exclusions,
  incremental backups and pruning are live there. The one thing that looked
  like unique knowledge was the shell's `RetentionPolicy::Tiered` -- keep daily
  for 7 days, weekly for 4 weeks, monthly for 12 months -- and on reading the
  app it turned out to be the weaker model: `apps/backup` takes `keep_last`,
  `keep_daily`, `keep_weekly` and `keep_monthly` as *independent numbers*, so
  the shell's variant is one frozen preset over what the app already computes.

  **The general trap, worth keeping:** on the first survey the dead module read
  as the *richer* of the two, because it had more named variants. It could
  afford them. A model that never runs is not constrained by having to work, so
  vocabulary accumulates in it and reads as sophistication. The live model was
  more expressive with fewer names, because its names were parameters. Do not
  size these two by counting their types.

  Preserved rather than kept: the preset numbers 7/4/12 are a defensible
  default someone chose, recorded here, which is one line rather than 2,654.

  The deletion also broke a reference the compiler could not see:
  `input_method.rs` cited this module's `InProgress => blue` as the precedent
  for a status colour collapsing onto the accent. The trap was worth keeping
  and the pointer was not, so the comment now states it without the citation.
  This is the blind spot `scripts/check-unused-exports.py` documents -- prose
  counts as a mention -- seen from the other side.

  Test count: 2,872 -> 2,836, exactly the 36 removed.

* `power_settings.rs` models screen-off and sleep timeouts in minutes with
  "0 = never". Nothing persists them and nothing honours them, so they are an
  echoed setting in waiting.

  **Half of it is now unblocked and half is not, which is worth separating.**
  The compositor grew an idle watch on 2026-09-16, and it is a *map* rather
  than one slot precisely so a second watcher can claim its own delay -- a
  screen dimmer beside the lock screen was the example in its doc. So the
  *timing* half exists. What does not is any way to turn a display off:
  `Present` pushes pixels and has no power control, and the compositor has no
  blanking or DPMS of any kind. Backing that on real hardware is lane A's DRM,
  so the screen-off timeout is a joint task rather than a lane C one.

  The sleep timeout is further still: suspending the machine is not a graphics
  operation at all.

  The lock delay took the route this one would take -- `gui/desktop`'s
  `idle_lock` reads a `session` group, claims a watch, and acts. A screen-off
  delay would be the same three steps once something can act.
* `backup_settings.rs` holds a retention *policy* vocabulary -- `KeepAll`,
  `KeepCount(n)`, `KeepDays(n)`, `Tiered` -- where `apps/backup` takes concrete
  counts (`PruneOptions::keep_daily`, `keep_weekly`) and computes from them. The
  translation between the two exists in exactly one place, and it is a doc
  comment: "Tiered: keep daily for 7 days, weekly for 4 weeks, monthly for 12
  months". That is a decision about what the word means, recorded in prose on a
  module nothing opens, and `apps/backup` has no monthly tier to honour the
  third clause of it. Deleting the module deletes the decision; porting it means
  choosing whether the app grows a monthly tier or the policy loses one.

**Why it matters beyond the line count.** A settings screen nobody can open is
not merely dead code: it is a design that looks decided. Somebody adding a
screen-lock delay would reasonably put it next to the sleep timeout in
`power_settings.rs` and inherit a setting that changes nothing -- which is the
defect this lane spent 2026-09-16 removing from eight other pages.
