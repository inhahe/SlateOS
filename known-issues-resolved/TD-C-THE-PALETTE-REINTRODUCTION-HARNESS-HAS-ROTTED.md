## `TD-C-THE-PALETTE-REINTRODUCTION-HARNESS-HAS-ROTTED` (lane C, 2026-09-27) -- **FIXED 2026-09-27**

**Status:** FIXED 2026-09-27, all three parts of the proper fix below.
`--check` reports **1,451 defects, 0 stale, 0 ambiguous, 0 no-op**. (1) Every
test that only stale entries proved is proven by a live one again. (2) Rather
than retiring the remaining ~270 stale entries, each was **re-derived** at the
site that now makes the same decision: most had rotted through a handful of
tree-wide changes -- boxes became surfaces (`push_surface`; since §829 a card
is outlined, not filled, so a frozen fill is the same call with that member of
its paint replaced), glyphs became themed icons, hue text became inked
(§837), captions moved from overlay0 to subtext0, the palette moved into the
toolkit (§838), and the default shortcuts were cut back (§1416). Seven were
retired instead, each leaving a `# RETIRED` record: duplicates of an edit
another entry makes, or no-ops the design had since absorbed. Retiring was
the cheaper option and the plan below chose it; re-deriving keeps what those
entries uniquely proved, which is that a fixture reaches a branch (a
hover-only button, an empty-list caption, a switcher edge case) -- retirement
would have dropped exactly that. (3) `scripts/test-reintro-palette.py` holds
the invariant on every boot (the boot test runs every `scripts/test-*.py`):
the tree must check clean, and six controls prove `check()` still reports each
kind of rot. Mutation-tested: a blind `check()`, a real rename in the tree
and a crash on a deleted file each fail it.

**Still to do, and tracked here until done:** a re-derived defect is checked
to *apply*, not yet to be *caught*. Each batch is being run through the
harness itself (`python scripts/reintro-palette.py <labels>`, ~3.5 min a
defect, in a scratch worktree); a declaration a run shows to be wrong
(`MISSING` or `UNDECLARED`) is corrected in the same pass. First results: the
Run dialog's five and the widget layer's and the OSD's first batches.

**In short:** `scripts/reintro-palette.py` proves the palette-conversion tests
are real by putting each old colour back and checking a test fails. No gate
runs it, and it has rotted unseen: its `--check` mode stopped on a file that no
longer exists. Found while deleting the dead launcher. Nine files it named are
gone (settings pages moved to the Settings program, `blur.rs`, `a11y.rs`, the
launcher's dialog), and their 400 entries were removed on 2026-09-27, since
they can never apply. What is left: **1,459 defects, of which 308 no longer
match the code they break and 3 match it ambiguously** (`--check`, 2026-09-27).

**First repair pass, 2026-09-27: 296 stale.** Thirteen had only moved -- the
palette went from `appearance` into the toolkit (design-decisions §838) while
its tests stayed -- and now target `gui/toolkit/src/palette.rs`; the real sweep
re-proved all thirteen. It also found **defect B had become a no-op**: the
light palette's text inks are recomputed from `ink_sources` by the legibility
floor, so copying the dark value into the `subtext1:` field changed nothing.
B now copies it into `overlay0`, a role nothing recomputes, and is caught
again. Four entries declared a test fewer than catch them; declared now. Of the
296 left, a search of `gui/` finds 265 whose text no longer exists anywhere
(the code was rewritten, and each needs its defect re-derived by hand) and the
rest matching only generic lines in unrelated files.

**Where:** `scripts/reintro-palette.py`; the stale entries are listed by
`python scripts/reintro-palette.py --check`.

**Triaged, 2026-09-27 (later): what the stale entries still prove.** Counted
per target file and per declared test, against the live entries:

- **Every file but one still has live entries** proving its colour sweep (the
  `every_colour_..._comes_from_its_palette` test each module carries) catches a
  colour put back. That sweep checks *every* colour the module draws, so a
  stale entry whose only declared test is the sweep adds nothing the live ones
  do not already prove. The exception is `gui/desktop/src/run_dialog.rs`:
  **five stale, none live**.
- **31 tests are proven to bite only by stale entries** -- behaviour tests, not
  sweeps: in `gui/desktop/src/lib.rs` eleven start-menu, power-menu and
  window-switcher tests; `context_ext.rs`'s
  `a_hovered_extensions_icon_follows_the_accent`; `osd.rs`'s
  `every_pair_this_module_uses_to_tell_things_apart_stays_apart` and
  `volume_icon_levels`; three in `widgets.rs`; three in `run_dialog.rs`; five
  palette tests in `gui/appearance`; and one or two each in `calendar.rs`,
  `datetime_settings.rs`, `login_screen.rs`, `startup_settings.rs` and
  `wallpaper.rs`. Listed by the triage's script; rerun it to regenerate.
- The work since the first pass added no stale entry: against `main` at
  `08d0ef08d`, thirteen entries changed status, all from stale to live (the
  retargets above).

**Proper fix, now concrete:** (1) re-derive one entry for each of the 31 tests
above -- a defect written against the code as it now reads, run through the
harness to confirm the named test catches it -- and at least one for
`run_dialog.rs`'s sweep; (2) retire the remaining ~270 stale entries, each
leaving a one-line `# RETIRED 2026-..: stale; <file>'s sweep is proven by N
live entries` in its place, which is the file's own convention and keeps the
record; (3) wire `--check` (seconds, no build) into the boot test's tooling
suites, so the next rename that strands an entry fails a gate rather than going
unnoticed for weeks. (1) is the part with value; (2) and (3) are what keep it.
