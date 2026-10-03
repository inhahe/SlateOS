## TD-C-THE-SCRATCH-CONFIG-GATE-COULD-NOT-SEE-A-STORE -- FIXED 2026-09-14

**Date:** 2026-09-14. **Lane:** C. **Fixed the same day.**

**In short:** `scripts/check-scratch-config.py` is the gate that stops a test
writing settings into the developer's own home directory instead of a
throwaway one. It found the crates to watch by searching for two spellings --
a method called `save`, or `settingsfile::...write...`. The actual function
that writes the file is `settingsfile::store`. Any crate whose saving method
was called something else was therefore invisible to it, and the gate reported
success over the crates it could see while saying nothing about the rest.

**How it surfaced.** `apps/fileassoc` gained persistence through a method named
`persist`. Four of its tests immediately started writing
`~/.config/slateos/fileassoc.yaml` for real. The gate ran and answered:

> ok: 12 save-capable crates wrote nothing to the real config

-- true of the twelve, and silent about the thirteenth. Adding `store` to the
pattern turned that into `fileassoc wrote slateos/fileassoc.yaml` on the very
next run, and the count went from 12 to **14**: `notifsettings` had been
unwatched as well, and had simply happened not to be writing.

**Why it belongs in this file even though it is fixed.** This is the third
instance this week of one defect: *a check that reports success over a
population it cannot enumerate.* The others were `check-scratch-config.py`
itself running tests in parallel, so a stray write landed in a neighbouring
scratch directory and was attributed to nobody; and `scan-orphan-modules.py`
clearing a module because a name it owned was also declared in a file the scan
skipped. In all three the gate was **green and wrong**, which is worse than
red, because a green gate ends the investigation.

The general shape to look for: a checker that discovers its own subjects by
pattern. Its blind spot is never in what it reports -- it is in what it never
looked at, and no output will mention that.

**A second lesson, about finding the writers.** Wrapping the offending tests
took three rounds of reading call paths and guessing wrong, because two of the
eight were sweeps that click *every* control without naming any of them. A
temporary panic inside `persist` listed all eight in one run. Enumerating a
population beats reasoning about it -- which is the same sentence as the
paragraph above, pointed at my own method.

**The proper fix, applied after the patch.** Adding `store` to the pattern
fixed one spelling and left the design intact: a gate choosing its subjects by
how they *spell* a call. The proof that this was not paranoia is that
`apps/stickynotes` had **also** named its saving method `persist` -- entirely
independently, before any of this -- and was equally invisible.

So the subject list is no longer derived from spellings alone. The config
directory's location comes from `settingsfile::config_dir`, which means a crate
that does not depend on `settingsfile` **cannot name that directory at all**.
That is not a better heuristic; it is the actual population, read off the
manifests, and a crate joins it by adding a dependency rather than by choosing
a verb. The list is the union of that with the old pattern, which is kept
because it also reaches crates that save through a helper living somewhere
else.

The run went 12 crates -> 14 (adding `store`) -> **17** (adding the manifests):
`stickynotes`, `appearance` and `settingsfile` itself had never been checked.
All three are clean, which is the good outcome -- but nothing before this knew
that, and "we were lucky" is not a property a gate is supposed to rely on. The
`store` spelling is now one of the self-test's ten cases, so the pattern cannot
quietly narrow again.
