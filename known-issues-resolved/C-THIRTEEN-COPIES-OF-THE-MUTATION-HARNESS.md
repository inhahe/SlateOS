## `C-THIRTEEN-COPIES-OF-THE-MUTATION-HARNESS` (lane C, 2026-08-29) -- **fixed** the same day

**What it is.** Every app wired in this campaign carries its own
`apps/<app>/mutate.py`, and there are thirteen of them. Only the `MUTATIONS`
table at the top is genuinely per-app; everything below it -- `run_tests`, the
compile/hang/crash classification, the `[ok]`/`[??]`/`[BAD]` verdicts, the
backup, the `try/finally` restore and the summary -- is the same harness copied
thirteen times.

**Why it matters, concretely.** On 2026-08-29 a machine restart killed a sweep
mid-mutation and left `apps/simon/src/main.rs` holding a live mutation, with the
real program in `main.rs.bak`. The `finally` that exists to prevent exactly this
cannot run when the process is never asked to clean up. The fix -- refuse to
start when a backup survives, print the diff, name the two recoveries and exit 2
-- went into `apps/simon/mutate.py` only. **The other twelve still have the
gap**, and will keep it until each is edited by hand.

That is `known-issues.md` lesson 63 in its own tooling: a rule kept only by
copying is a rule that will be dropped. The failure mode is quiet and it
poisons evidence rather than crashing -- a mutant compiles, so the next run
reads it as `original` and mutates on top of it, and a sweep reporting `[ok]`
throughout then proves nothing at all.

**A second divergence, found the same day.** Simon's second sweep returned 182
`[ok]`, one `[skip]` and two wrong-test verdicts -- and **exited 0**. All
thirteen harnesses print their verdicts and return success unconditionally, so
a run that has not passed is indistinguishable from one that has unless someone
reads every line of the log. Simon's now exits 1 unless every mutation was
caught by the tests named for it, and counts a `[skip]` as a failure: a
survivor is loud, but a stale anchor silently stops applying its mutation, and
does so inside a run that still ends "0 survived". The other twelve still exit
0 regardless. This is the same debt as above and doubles the argument for
extracting the harness -- two independent correctness fixes have now had to be
written into one copy of thirteen.

**The fix as built.** The harness is now `scripts/mutation_harness.py`, exposing
`sweep(src, mutations, crate, timeout=..., only=None)`. Each
`apps/<app>/mutate.py` is its docstring, its `MUTATIONS` table and a two-line
call; the startup guard, the restore, the verdict classification and the exit
code exist once. All thirteen were converted mechanically and each table was
verified to be byte-identical to the one at `HEAD`. The tables stayed per-app:
they are the part that is genuinely different, and the part worth reading.

**It is in `scripts/`, not `apps/`, and that is not cosmetic.** The first
attempt put it at `apps/mutation_harness.py`. `apps/*` is a Cargo workspace
member glob, so the `apps/__pycache__/` directory Python writes on first import
became a workspace member, and *every* `cargo test` in the tree started failing
with "failed to load manifest for workspace member `apps/__pycache__`" before
compiling anything. `__pycache__/` is in `.gitignore`, so the thing that broke
the build was invisible to `git status`. See lesson 66.

**Four faults the extraction itself exposed** -- worth recording, because the
argument for extracting was "thirteen copies hide bugs", and consolidating them
promptly surfaced four more, every one of which produced a *false green*:

1. **The verdict classifier could not tell a dead test run from an absent one.**
   `crashed = compiled and not timed_out and not failed and returncode != 0`
   was meant to catch a mutant that aborts the test process before any test can
   report. But `compiled` was `"could not compile" not in output`, and a
   *manifest* error is not a compile error -- so a cargo that never started
   satisfied every clause. The `apps/__pycache__` break therefore scored all 20
   of sliding's mutations as `[ok] caught -- the harness died` and exited 0,
   having run no test at all. `run_tests` now requires positive evidence that a
   test binary started (`running N tests` in the output), and a mutant that
   compiles but produces no such line **stops the sweep** rather than earning a
   verdict.

2. **No sweep had ever checked that the suite passes on the unmutated program.**
   That is the general form of fault 1 and the cheaper guard: a run whose green
   condition is "the thing I broke got noticed" cannot distinguish "my sabotage
   worked" from "this was already broken", and the more thoroughly the
   environment is broken the more mutations get "caught". `sweep` now runs the
   suite once against the real source first and refuses to start unless it
   compiled, ran and passed.

3. **The per-mutation timeout was being spent on the build.** It exists to catch
   a mutant that loops forever, and is sized for the tests -- sliding allows
   120s. But `cargo test` builds first, and a cold workspace build is slower
   than any suite: sliding's baseline was killed at 120s while still compiling
   dependencies. Unfixed, the first mutation after any cold build would have
   been scored "caught by a hang" -- a false `[ok]`, the worst kind. `sweep` now
   builds with `--no-run` outside the timed window first, so every timed run
   recompiles one crate, which is what the timeout was calibrated against.

4. **A hang verdict is the one verdict that checks nothing, and they cascade.**
   `[ok] caught -- the suite hung` is accepted without ever consulting the
   mutation's `expect` list: there are no test names to read, so the coverage the
   entry was written to prove is not verified at all. Fixing (3) only moved the
   *cold* build out of the timed window; every incremental rebuild was still
   inside it. So when sliding's one genuinely unbounded mutation timed out, the
   runner killed the tree mid-work, the redo landed inside the *next* mutation's
   120s, and the two mutations after it were also scored "caught by a hang" --
   neither of which can loop. Three of twenty verdicts were hangs; only one was
   real. `sweep` now builds every mutant untimed before the timed run, so the
   timed window holds test execution and nothing else, and a compile failure is
   observed at the build step instead of inferred from test output.

   Confirmed rather than assumed: re-running those three with the fix,
   `the scramble walks until it is unsolved, without a bound` still hangs -- it
   is an unbounded loop -- while `the shuffle undoes itself half the time` and
   `a won board goes on playing` now name their owning tests
   (`the_walk_never_undoes_the_move_it_just_made`,
   `a_won_board_ignores_further_moves`).

**Verified** by running a previously-green app's sweep to completion against the
shared harness before trusting the other twelve: sliding, 20 of 20 caught, exit
0, source restored. Note what that number would have been worth on its own --
the run was "20/20 green" both before and after fault 4 was fixed, and the
difference was three verdicts that proved nothing versus one. A sweep's headline
count is not its result; the distribution of *kinds* of verdict is.
