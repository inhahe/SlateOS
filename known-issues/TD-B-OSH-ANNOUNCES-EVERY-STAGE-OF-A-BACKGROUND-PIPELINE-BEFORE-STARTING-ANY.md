## `TD-B-OSH-ANNOUNCES-EVERY-STAGE-OF-A-BACKGROUND-PIPELINE-BEFORE-STARTING-ANY` (lane B, 2026-08-26) — **open**, tech debt

**In short:** with a debugger's DEBUG trap armed, bash tells the trap about a
background pipeline's stages *one at a time, starting each before it mentions
the next*, while osh mentions all of them first and only then starts any. Every
status, every `${PIPESTATUS[@]}` and every verdict comes out the same; the only
thing that differs is the order in which a stage's own output can interleave
with the trap's, and even in bash that order is a race.

**Where:** `userspace/oils/src/interp.rs`, `Shell::announce_async` (the loop over
`ao.first.commands`) and its caller in the item loop, which hands the whole job
to `Shell::exec_background` once the loop has finished.

**Reproduce** (bash 5.2.21 / glibc 2.39, `osh` built from this tree):

```sh
shopt -s extdebug
c(){ echo "F<$BASH_COMMAND>" >&2; [ "$BASH_COMMAND" != "$W" ] || return 2; }
f(){ { exit 3; } | exit 4 | exit 5 & echo after; }
W="exit 5"; trap c DEBUG; f
```

bash prints `F<exit 4>`, `F<exit 3>`, `F<exit 5>`; osh prints `F<exit 4>`,
`F<exit 5>`, `F<exit 3>`. The `F<exit 3>` is the *group* stage's own child
announcing its body — a group stage is not announced by the owning shell, so the
only shell that can name `exit 3` is the child running it. bash has already
forked that child before it announces `exit 5`, so the child's line can land in
the middle; osh forks nothing until the loop is over, so it always lands after.

**Why it is only an ordering difference:** the child is concurrent with the
parent either way, so its position in the stream is a race in bash too — replace
the group with `{ sleep 2; exit 3; }` and bash reorders to `F<exit 4>`,
`F<sleep 2>`, `F<exit 5>`, `F<exit 3>`. Nothing a script can read depends on it:
the answer, the array and which stages ran are identical, which is what
`tests/corpus/debug-trap-verdict-takes-a-pipeline-stage.sh` checks. No corpus
section asserts the interleaving, deliberately — a racy expectation would be a
flaky test.

**Proper fix:** interleave announcement and start, the way `Shell::exec_pipeline`
would have to as well. That means `announce_async` cannot decide the whole
`skips` vector up front and hand it to a clone; the job's stages would have to be
forked by the announcing shell as it goes, which is a different division of
labour between the parent and the `&` job's clone than osh currently has. Worth
doing only if a real observable is found that depends on it.
