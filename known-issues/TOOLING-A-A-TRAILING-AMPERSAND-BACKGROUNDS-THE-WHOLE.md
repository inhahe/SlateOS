### [A] TOOLING-A-A-TRAILING-AMPERSAND-BACKGROUNDS-THE-WHOLE-&&-CHAIN-INCLUDING-THE-cd — 2026-08-15 — ⚠️ HIT, recovered

**Status:** OPEN

**What I ran** (intending: background a sampler, then run the benchmark in my
own lane):

```bash
cd "…/os-lane-a" && rm -f build/hostload.tsv && (sampler…) & \
  ./scripts/boot-test.sh --bench > build/bench-rerun2.log 2>&1
```

**What actually happened.** `&` binds *looser* than `&&`, so it terminates the
entire preceding `&&` list. The shell backgrounded `cd … && rm … && (sampler)`
as one unit — **the `cd` included** — and then ran `./scripts/boot-test.sh` in
the foreground from the *tool's default working directory*, which is
`D:\visual studio projects\os`: the *integration checkout*, not my lane. It
started `cargo build -p kernel` there, in the tree another lane was at that
moment pruning.

**Why it was not obvious.** Every symptom pointed somewhere else. The sampler
worked and wrote to the right path (it was inside the backgrounded subshell, so
it *did* get the `cd`), which made the command look like it had worked. The
benchmark's log file was simply *absent* rather than empty or wrong — and an
absent file reads as "never started", which invites you to debug the boot test
rather than the shell. The giveaway was that the log existed under `os/build/`
instead.

**Damage and recovery.** Build output only — no source touched, and
`git status` in `os` showed nothing but the pre-existing
` M .claude/scheduled_tasks.lock` that belongs to another lane. The stray
`cargo`/`rustc` were killed by PID (not by name), and the stray log removed. But
the near-miss is the point: this is precisely the class of accident the
one-worktree-per-lane rule exists to prevent, and the rule did *not* prevent it,
because the rule governs where I *edit* while this bug governs where a command
*runs*.

**Prescription.** Never let `&` be the last operator of a chain that contains a
`cd`. Put the background job in a brace group so the `&` cannot escape it:

```bash
cd "…/os-lane-a" && { sampler… & } && ./scripts/boot-test.sh --bench …
```

Better still, for anything that builds or boots: assert the working directory
first, so a mislanded command fails loudly instead of quietly compiling in
someone else's tree.
