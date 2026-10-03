## TD-C-A-CLEANUP-THAT-REMOVES-A-DISTINCTION -- METHOD 2026-09-16

**In short:** five times in one day, the obvious tidy-up would have deleted
something the system actually knew, and in four of the five it looked identical
to deleting something redundant. The only thing that told them apart was
measuring first. This is written down because the instinct that produced each
one was correct — there really were four copies of the same idea, and three of
them really did have to go.

**Date:** 2026-09-16. **Lane:** C.

**The five, and what measuring found:**

| the tidy-up | what it would have removed | how it showed up |
|---|---|---|
| derive `apps/filesearch` from the toolkit's table | 30 working search facets — it knew `psd`, `raw`, `m4v`, `tex` and the shell dialects, and the table did not | diffing the two lists before editing either |
| delete `backup_settings.rs` for duplicating `apps/backup` | nothing, in the end — but it *looked* the richer of the two, so the risk read backwards | reading the live program instead of counting the dead one's types |
| derive `apps/diskanalyzer`'s colours | the colour of every Windows binary on the disk | the same diff, which came back as exactly four extensions |
| "fix" the toolkit's `default_app` names | the fix would have preserved a duplicate of `apps/fileassoc`'s job | asking who reads the field: nobody, and every value was wrong |
| bulk-import the 30 missing extensions | it would have *answered* the open question of whether this OS classifies formats it cannot run | grouping the 30 before adding any |

**The two shapes worth recognising.**

1. **A dead model can afford vocabulary a live one has not earned.**
   `backup_settings` had `RetentionPolicy::Tiered`, which `apps/backup` appeared
   to lack — until reading it showed `keep_last`, `keep_daily`, `keep_weekly`
   and `keep_monthly` as independent numbers, of which `Tiered` is one frozen
   preset. The dead copy had more *names*; the live one had more *expressiveness*,
   because its names were parameters and it was constrained by having to work.
   **Do not size two models by counting their types.**

2. **"Finishing the job" can answer a question nobody asked.** Five foreign
   executables were missing from the toolkit's table. Adding them alongside the
   obviously-missing formats would have been one edit — and would have decided,
   silently and by acting, that this OS classifies file types it cannot open.
   That decision may well be right. It had not been made, and a cleanup is the
   worst possible place to make one, because nothing in the diff looks like a
   decision.

**The cheap defence, which worked every time:** diff the two lists *before*
editing either. It is one script and under a minute, it turns "these look
redundant" into a number, and in three of the five cases the number was the
whole answer.

**The expensive failure it avoids** is not a broken build. Every one of these
cleanups would have compiled, passed its tests, and shipped: a search with
fewer filters, a treemap with grey where teal used to be, a policy quietly
adopted. `TD-C-A-CORRECT-TOOL-ANSWERING-A-NARROWER-QUESTION` records the same
shape one level down — a true answer to a slightly narrower question — and this
is that at the scale of a refactor.
