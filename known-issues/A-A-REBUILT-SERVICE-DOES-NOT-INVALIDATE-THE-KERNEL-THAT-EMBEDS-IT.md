## A-A-REBUILT-SERVICE-DOES-NOT-INVALIDATE-THE-KERNEL-THAT-EMBEDS-IT (lane A's tree, found by lane B 2026-09-07)

**In short:** the kernel compiles six small ring-3 programs *into itself* by
reading their compiled output off disk. There is a script that builds them and
it works well. What is missing is the other half: nothing tells cargo the kernel
*depends* on those files, so rebuilding one of them does not rebuild the kernel.
The kernel keeps the old copy and says nothing. One of the six is the init
process.

**This entry replaces a wrong one.** Its first two versions claimed "nothing
builds them" and counted first one artifact, then three. All of that was wrong,
and the retraction is below because how it was wrong is more useful than the
finding.

### What is actually true

`kernel/src/main.rs` and `kernel/src/container.rs` carry nine `include_bytes!`
sites naming **six** distinct artifacts:

| Artifact | Path references |
|---|---|
| `services/hello` | 7 |
| `services/netstack` | 3 |
| `services/udpget` | 2 |
| `services/init`, `services/httpget`, `services/ticker` | 1 each |

**`scripts/bootstrap-worktree.sh` builds all six**, and is a good piece of work:
it *derives* the list by grepping the `include_bytes!` calls rather than
hardcoding it, so it cannot fall out of step with the kernel; it explains in its
header why these services cannot simply be workspace members (they are
`no_std`/`no_main` with their own linker script, static relocation and large code
model, and cargo's config hierarchy is CWD-based, so the only reliable way to get
a service's flags is to invoke cargo from inside its directory); `--check`
reports what is missing with an exit status `boot-test.sh` reads; and it is
safe to re-run. On a fresh or freshly-migrated worktree, running it once fixes
the loud failure entirely.

### The finding that survives

`kernel/build.rs` emits `cargo:rerun-if-changed` for exactly three things --
`linker.ld`, `ada/{f}` and `ada/prebuilt/stamp.txt` -- and contains **no
reference to any `services/` path**. Bootstrap is a one-time provisioning step;
it does not create a dependency edge. So after provisioning:

    edit services/init -> cargo build --release in services/init -> rebuild kernel
    -> the kernel still embeds the previous init.

Lane C, checking independently in its own tree, confirmed this from the file
rather than from the shape of the bug, and put the consequence better than I
did: a missing file fails loudly and immediately, but a *stale embed* produces a
kernel that builds clean, boots, and runs last week's binary -- so the symptom is
"my change to that program did nothing", which is indistinguishable from a change
that genuinely does nothing. Someone debugs the program, not the build. And it is
worse on a long-lived machine than a fresh one, which inverts the usual order in
which people suspect their toolchain.

**The fix does not need a new builder,** and an earlier draft of this entry said
it did -- lane C's prescription, which lane C then withdrew for the same reason
the rest of this entry was wrong: it assumed no build step existed. One does, and
`bootstrap-worktree.sh` is better than the thing that draft proposed writing,
because it derives its list from the `include_bytes!` calls and so cannot drift
from the kernel, which a hand-maintained `build.rs` list would not give.

What is wanted is the missing half only, in `kernel/build.rs`, **in this order**:

1. derive the artifact list by scanning `kernel/src` for `include_bytes!` of
   `services/*/target/*/release/*` -- the same derivation
   `bootstrap-worktree.sh` uses, so the two halves of one invariant cannot
   disagree when a seventh service is added. Fail closed if the scan finds
   none: zero means the scanner broke, not that the kernel embeds nothing.
2. check each artifact exists; if any is missing, **hard-fail naming
   `scripts/bootstrap-worktree.sh`**.
3. only then emit `cargo:rerun-if-changed` for each.

**The order is load-bearing, and lane A measured why.** On cargo 1.95.0, a
`rerun-if-changed` naming a path that does *not* exist makes the build script
re-run on every build: a counter in `build.rs` gave 1, 2, 3 across three no-op
builds for a missing path, against 1, 1, 1 for an existing one (and 2 after
touching it). So emitting the directives before the existence check would trade
this quiet bug for another one, whose only symptom is that the kernel never
caches.

A `rerun-if-changed` line *alone* is still not sufficient, but the reason is
narrower than the earlier draft claimed. Not that cargo must be the thing that
builds the artifact -- lane A's position, which is the better one, is that a
fresh clone *should* fail, and what was wrong was failing as fifteen
`include_bytes!` errors blaming the kernel rather than one error naming the
script. Step 2 fixes that without pretending the artifact appeared by itself.
What remains knowingly accepted is that `rerun-if-changed` declares invalidation,
not production: driving the service builds from `build.rs` would re-couple what
the root `Cargo.toml` deliberately excluded -- it keeps these crates outside the
`build-std` blast radius -- and would hide that coupling somewhere the exclusion
is not stated.

**Owner:** `kernel/**`, so lane A -- specifically the *live* lane A session, which
during this exchange was not the one that answered to the name.

### The retraction, which is the useful part

This entry was wrong twice before being right, and each time in the same way:
**I reported the first place I looked as though it were the whole picture.**

| Pass | Claim | Method | Wrong because |
|---|---|---|---|
| 1 (me) | one artifact | grepped `container.rs` | stopped at one file |
| 2 (lane A) | two artifacts | grepped `main.rs`, found `init` | stopped at one more |
| 3 (me) | three artifacts | `grep kernel/src/*.rs` | glob does not recurse |
| 4 (lane C) | six embed sites | `grep -c` on `container.rs` | counted one file's sites, not artifacts |
| 5 (me) | six artifacts, and a script already builds them | `grep -r` all of `kernel/src`, then **read the script** | -- |

Four sessions, five passes, one afternoon, on a defect whose full extent one
recursive grep would have given at any point. And the thing that actually
corrected it was not a better grep: it was reading `bootstrap-worktree.sh`, which
had documented the whole problem, its cause, and why the obvious fix does not
work -- before any of us started. The tool that finds a fact and the document
that already contains it are different instruments, and I reached for the first
four times before the second.

The five passes were not five different mistakes. Every one was a command that
answered a narrower question than the one being asked, and whose answer was then
reported at the width of the question: `grep -c` on one file, a glob that does
not recurse, a `head -3`, a process scan whose regex matched the scanning command
itself. None of them was wrong; each was asked something small and answered it
exactly.

Three habits come out of it, all cheap and all general.

**State which tree you measured, in the same sentence as the measurement**
(lane C's). All of today's errors -- this one, and separately a lockscreen bug
reported from a worktree 54 commits behind -- were a session reading its own
worktree and reporting it as the state of the project. That is what worktrees do:
the isolation that stops three lanes clobbering each other also stops them seeing
each other.

**When a tool exists for the exact problem you are diagnosing, its documentation
is a primary source about the problem, not just instructions for the tool**
(also lane C's, and the sharpest of the three). I reached for `grep` four times
before reading `bootstrap-worktree.sh`. Lane C did something harder to notice: it
*ran* the script, used its runtime and the artifact's byte count as evidence in
three separate messages, and never read its header -- so the disproof of what it
was about to recommend was in its own terminal output. A script is a text that
happens to run, and both of us treated it as only the second thing.

**Derive, do not enumerate.** The one artefact in this story that never went
wrong is the script's list of services, precisely because it is computed from the
kernel's own source at every run. Four sessions counted by hand and four got a
different number; the derivation has been right the whole time and unattended.
That is why `build.rs` should scan rather than hold a list of six.
