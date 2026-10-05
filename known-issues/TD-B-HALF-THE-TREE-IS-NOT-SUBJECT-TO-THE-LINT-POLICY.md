## TD-B-HALF-THE-TREE-IS-NOT-SUBJECT-TO-THE-LINT-POLICY (lane B, 2026-09-10) — ratcheted, 128 open

**In short:** CLAUDE.md requires `#![deny(clippy::all, clippy::pedantic)]` in
every crate plus five defensive lints in non-test code. **134 of 256 crates**
under `userspace/`, `services/` and `init/` are subject to none of it. For
those, `clippy::all` is *warn* rather than *deny*, `pedantic` is off entirely,
and `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` and
`arithmetic_side_effects` are all off — so "clippy clean" means something much
weaker for half the tree than the other half, and nothing in the build says
which kind of clean you got.

**Including crates I reported clean today.** `crond`, `crontab`, `cpio` and
`at` are all on the list. Those reports were true and much less informative
than they sounded.

**How it was found, which is the repeatable part.** Not by looking for it. Gate
12 compiles the unix-gated half of a changed crate by building for linux; it
had been running `cargo test`, needing a cross-linker this host lacks, so it
had never compiled anything. Fixing that produced a sweep of all 57 crates with
a unix arm — all clean — whose output carried, crate after crate:

    warning: missing `[lints]` to inherit `[workspace.lints]`

The compile was the question. This was the answer to a larger one nobody had
asked. Third time this week the useful finding came from a check aimed
elsewhere.

**Why a ratchet and not a fix.** Adding `[lints] workspace = true` to one
2,700-line crate (`userspace/crond`) produced **147 warnings**: 68 unwraps, 42
arithmetic side-effects, 33 indexing and slicing panics. Across 134 crates that
is a programme, not a commit. `scripts/check-workspace-lints.py` with
`scripts/workspace-lints-baseline.txt` records who is exempt today and refuses
a *new* one; pre-push gate 21 runs it per pushed sha.

**Proven able to refuse before it was wired**, by stripping `[lints]` from
`userspace/ar` and confirming `--check` exits 1, then restoring and confirming
0 and a clean tree. A ratchet that has never been observed to refuse is
indistinguishable from one with nothing to say.

**The work, when someone does it:** pick a crate, add the two lines, fix what
it reports. The count may only fall.

**134 -> 129.** `uname` and `tee` were done on 2026-09-11 (`userspace/tee` was
then deleted the same day as a duplicate -- see the pair log below, and the
note there about what that says for this programme) (the other three came
off earlier). Both were small — 2 and 8 non-test warnings — and both turned out
to have **no tests at all**, which the count had no way to show. So the job is
not one thing but two: satisfy the lints, and leave behind something that
proves the crate still does what it did. They have 3 and 6 tests now.

Two findings worth more than the warnings:

* **`uname` indexed `&args[1..]`.** A program can be started with an EMPTY
  argument vector — `execve` takes it from the caller and nothing requires a
  program name in it — so that slice panicked before a single option was read.
  Reachable by a caller rather than by a user, which is why no amount of
  command-line testing would have found it.
* **`tee` sliced `&buf[..n]`** on the result of `Read::read`. That one cannot
  panic, because `Read` promises `n <= buf.len()`; it is now checked anyway,
  because a reader that broke the promise would have `tee` copy stale buffer
  bytes it never read into every output file, and stopping is better than that.

**`expand` (2026-09-11), and the interesting part is that it had no bug.** 24
warnings, and a full behavioural diff against GNU coreutils 9.4 — six tab-stop
values including `0`, `-1` and one too large for `usize` — matched byte for
byte on stdout and on exit code. So the lints bought no defect here, and the
work was still worth doing for the one structural change they prompted:

`TabStops::Regular` now holds a `NonZeroUsize` instead of a `usize`. The parser
already rejected 0, and `next_tab_stop`'s `col / interval` four functions away
had to trust that. The type carries it now, so the division cannot panic
however the value arrived — and `col / *interval` uses
`impl Div<NonZeroUsize> for usize`, which is documented as never panicking, so
the proof is the type's rather than a comment's.

That is the shape worth repeating: the lint could not be satisfied honestly
without making the invariant explicit, and making it explicit removed the need
for the invariant to be remembered.

10 tests added — it had none, like the two before it. Three crates in and all
three had no tests at all, which is starting to look less like a coincidence
than like what "not subject to the lint policy" selected for.

**The house style, measured before following it:** of 50 covered crates only 9
use a crate-level `#![allow(clippy::arithmetic_side_effects, ...)]`. The other
41 fixed the warnings. So the blanket is for genuinely bounded, pervasive
arithmetic — `ar`'s archive offsets, with its rationale written down — and not
the default answer.

**A worked example, with the real number.** `userspace/at` (1,900 lines) was
put through it on 2026-09-10. With the test module exempted the way the covered
crates do it — `#[cfg(test)] #[allow(clippy::unwrap_used, ...)]` — the
NON-TEST count was **102**: 55 arithmetic side-effects, 44 indexing/slicing
panics, 3 others. Spread across the file, not concentrated, so there is no
cheap subset. Budget accordingly: this is a crate-sized job each, not a sweep.

The enablement was then reverted and `at` stays on this list, because 99
visible warnings on one crate while 133 are silent is noise without a plan.
What was kept is what the lints *found*: `day_of_week` indexed
`T[(m - 1) as usize]` with no range guard, so month 0 became `usize::MAX` and
panicked — and `month` is a `u32` straight off a parsed timespec. Both calendar
helpers now delegate to `civildate`, which computes rather than indexes.

**The lints are an instrument, and using one without keeping it installed is a
legitimate outcome.** The panic was the deliverable.
