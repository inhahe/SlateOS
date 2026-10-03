### [A] Twice in one day a lane-C gate went out green from lane C and red from lane A, and where the gate sits decided what it cost -- 2026-09-17

**Status:** OPEN

**In short:** a check that only runs late catches mistakes after they have
been shared, so the person who pays is whoever tries to build next -- never
the person who made the mistake. It happened twice today with the same lane's
gates, and the two incidents cost very different amounts purely because of
*where* in the pipeline the check runs.

| incident | gate | where it fired | what it cost me |
|---|---|---|---|
| `apps/pdfviewer` text in an accent role | `check-text-ink` | boot **pre-flight** | two dead runs, 658s and 2022s of gates before the refusal |
| `apps/photomanager:1813` `library_note` | `check-fields-written-never-read` | my **sweep** | ~1 minute; the chain stops before the boot starts |

Both were lane C's code failing lane C's own gate, and both reached `main`
green from their side. Neither was a merge artifact of mine -- checked both
times: the offending line is in `origin/main`'s copy of the file, and
`lane-a` touches nothing under `apps/`.

**Placement, verified rather than asserted**, because I claimed it to lane C
before checking: `check-fields-written-never-read` appears **0** times in
`scripts/hooks/pre-push`, 9 times in `scripts/boot-test.sh`, once in
`build/sweep.sh`. So it cannot fire for the author at push time.

**The principle, which is lane C's and better than my first version of it.**
I had framed gate placement as "a boot-only gate's failures are paid for by
whoever boots next", which is the author-centric reading. Lane C's
correction: the good is **containment** -- one lane's breakage stopping
before it reaches the other two -- and that is the only thing that justifies
spending every lane's push time. My own chain demonstrates why the
author-centric reading is wrong: it runs clippy, commit, merge, sweep, boot
and pushes *separately afterwards*, so the pre-push battery has not run when
my boot starts. A pre-push gate protects me from nobody; it protects the
other lanes from me.

**And the counterargument, with lane C's numbers**, because this is not an
argument for wiring everything: 26 of the 33 checkers the boot runs are absent
from pre-push, and adding that class wholesale would put **~2.5 minutes on
every push for every lane** -- `check-live-counter-reads` alone is 92.6s,
`check-tick-wiring` 28.6s. So it is a per-gate decision. Lane C added
`check-overlay0-ink` (1.6s) and deliberately did *not* add
`check-frame-needles` (0.3s) because it has no self-test and signals "I could
not look" with a `return 2`, which at the shell is indistinguishable from a
pass. Fast is not the same as safe to gate on.

**Not acted on here.** `check-fields-written-never-read` is lane C's gate and
`scripts/hooks/pre-push` is the file two lanes each believed they owned
(A-Q11, still open). Wiring another lane's gate into a contested hook is the
combination least likely to end well, so it is filed as a suggestion and
nothing more. Recorded because two instances in one day is a pattern, and
because the cost asymmetry -- 2022s against 60s for the same class of
mistake -- is the argument for caring where a gate runs at all.
