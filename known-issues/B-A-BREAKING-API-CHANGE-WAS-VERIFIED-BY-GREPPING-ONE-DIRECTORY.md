## B-A-BREAKING-API-CHANGE-WAS-VERIFIED-BY-GREPPING-ONE-DIRECTORY (lane B, 2026-09-07)

**In short:** I removed two things from a shared library, converted every
program that used them, and broke the build for all three lanes anyway --
because the way I found "every program that used them" was to search *one*
directory, and two of the callers live somewhere else. This is a note about
the method, not about the two callers, which are fixed.

**What happened.** `5264cba7a` carried out `design-decisions.md` §353 item 3:
`authlib`'s `/etc/shadow` store is deleted rather than kept as a fallback. That
removed the whole `authlib::shadow` module and the second parameter of
`Authenticator::with_stores` -- a hard, compile-breaking API change. To find the
callers I ran, in effect:

```
grep -l authlib userspace/*/Cargo.toml
```

That returned nine crates, all of which I converted and tested. It also silently
excluded every crate outside `userspace/`. Two exist:

| Crate | Lane | How it broke |
|---|---|---|
| `init/loginmgr` | B (mine) | two-argument `with_stores`, twice |
| `apps/lockscreen` | C | two-argument `with_stores`, **and** `authlib::shadow::lookup`, which no longer exists at all -- fixed by lane C in `d9f1f540e` |

`main` did not build for a day. The boot test builds the whole workspace, so it
blocked all three lanes, not just mine.

**Why the usual safety nets did not catch it.**

- `cargo test -p <crate>` over the crates I had touched cannot see a caller I
  did not know about. That is the whole point of the list being wrong.
- `cargo test --workspace` *would* have caught it. I started one and abandoned
  it as too slow on this machine (it was ~15% through after 15 minutes), then
  substituted "every crate that depends on `userdb`" -- reasoning that
  `userdb`'s change was purely additive so the blast radius was bounded. That
  reasoning was sound for `userdb` and I applied it to the wrong change:
  `authlib`'s change was *subtractive*, and a subtractive API change has a blast
  radius of "every caller in the tree", which is exactly the thing I had not
  enumerated.
- The pre-push gates are lint- and text-based; none of them compiles the
  workspace.

**The fix, as a habit:** when a change *removes or narrows* a public item, the
caller list must come from the whole tree, not a subdirectory:

```
grep -rl '<crate-name>' --include=Cargo.toml .
```

and the verification must be a whole-workspace **`cargo check`** -- not
`cargo test`. `check` skips building and linking test binaries and skips running
them, which is where nearly all of `cargo test --workspace`'s time goes on this
tree; it is the cheapest thing that still sees every caller. Measured on this
machine: see the timing note at the end of this entry.

**Distinguish the two shapes of change**, because the cheap verification is only
valid for one of them:

- *Additive* (a new function, a new field, a new variant on a `#[non_exhaustive]`
  enum): existing callers cannot break. Testing the crate and its known
  dependents is enough.
- *Subtractive or narrowing* (removing an item, removing a parameter, changing a
  type, renaming): every caller in the tree is a candidate. Enumerate from the
  whole tree and `cargo check --workspace` before merging to `main`.

**A grep would not have been enough, which is the sharper point.** Lane C, which
owns `apps/`, made this observation and it is the one worth keeping: *nothing
builds `apps/**` at all*. The boot test targets `x86_64-unknown-none`, where
`apps/*` are not in `default-members`; each lane builds only what it touched;
there is no CI. `apps/*` **is** a workspace member, so a host-target
`cargo check --workspace` sees it -- but nobody has a reason to run one. So the
grep being narrow is how *I* missed it, and the absence of any build covering
`apps/` is why nothing else caught it either. Two independent holes lined up.

**How it was actually found:** not by me and not by the grep. Lane C was
stripping blanket `#![allow(dead_code)]` out of `apps/`, which made clippy look
at the crate for the first time, and the errors fell out. It fixed
`apps/lockscreen` in `d9f1f540e` and merged it in `97219f95c` before I had
finished writing the request asking it to. My `init/loginmgr` half was the only
part still outstanding by then.

**Timing, measured rather than assumed (2026-09-07):** lane C measured all 143
`apps/` crates at **58 seconds** warm. My own `cargo check --workspace` run on
this machine hit a separate obstacle worth recording: it fails on
`kernel/src/container.rs`'s `include_bytes!` of
`services/hello/target/x86_64-unknown-none/release/hello`, a *build artifact*
that the D:->E: migration deliberately did not copy. So a whole-workspace check
is not clean out of the box on a fresh tree -- it needs `services/hello` built
for the bare-metal target first, or that crate excluded. Anyone proposing the
check as a standing gate has to handle that, or the gate fails for a reason
unrelated to the change under test, which is the fastest way to get a gate
ignored.

**Open question:** whether to make a host-target `cargo check --workspace` a
pre-merge gate is `open-questions.md` -> **C-Q11**, raised by lane C. The real
objection is that it lets one lane's red crate block another lane's merge --
which is exactly what happened here, in both directions.

**Measured for C-Q11 (2026-09-07, lane B, `E:/…/os-lane-b` at `b9b7c61df`,
machine idle, workspace green, target `x86_64-pc-windows-gnu`):**

| Run | Elapsed | What it is |
|---|---|---|
| full check, kernel never checked in this tree | **139 s** | honest cold cost; *not* the gate's number, since nobody merges from a cold tree |
| immediate re-run, nothing changed | **15 s** | the no-op cost |
| after touching one lane-B source file | **14 s** | inherits the previous run's cache -- **this is the gate's realistic cost** |

Cold-to-warm is 139 -> 15, so quoting 14 s as a from-cold figure is off by two
minutes.

**Contended, same tree, load = lane A's boot test in its gate phase** (hundreds
of short-lived Python processes reading the whole tree -- the load a merge
collides with more often than a build): no-op **28 s**, one lane-B file touched
**18 s**. So contention roughly doubles it and it stays under half a minute.

**Scoped vs whole, both warm-incremental, both under that same contention** --
the comparison the scoping question actually needs, and which nobody had taken:

| | one file touched | no-op |
|---|---|---|
| scoped: 158 `apps/`+`gui/` packages as `-p` flags | **8 s** | **7 s** |
| whole: `--workspace` | **15 s** | **16 s** |

**This inverted two predictions, including both of the ones in this file's
earlier drafts.** Lane C expected the enumerated form to lose, on the reasoning
that 158 `-p` flags make cargo do resolution work proportional to the set named;
lane B claimed the whole workspace was *cheaper* than the subset, having compared
lane C's **cold-scoped** 39 s against a **warm-incremental** whole-workspace 14 s
-- two different kinds of measurement, which is the same category error this file
documents elsewhere. Scoped wins about 2:1 once both are warm-incremental.

The reason is plain once looked at: `--workspace` is not "the apps plus the
kernel", it is *every member*, including `userspace/*` -- some thousands of
crates. Both mental models had it as apps + kernel. Neither party read the
manifest before reasoning about the difference.

**The case the gate actually fires in** -- lane C's objection to every number
above, and the right one: a gate runs after `git merge origin/main`, so the
interesting day is the one where a *shared crate moved* and everything
downstream re-checks. All the figures above are one-file-touched or no-op,
which is the cheap day. Measured, same tree, same contention, by touching each
shared crate and re-checking the whole workspace:

| Shared crate touched | Crates naming it in a manifest | `cargo check --workspace` |
|---|---|---|
| `authlib` | 12 | **19 s** |
| `posix` | 13 direct | **22 s** |
| `guitk` | 144 | **26 s** |
| `quoting` | **773** | **49 s** |

So the expensive end -- the widest shared crate in the tree, under contention --
is **49 seconds**, not the two minutes the cold figure suggested. `cargo check`
does no codegen, so a downstream re-check is metadata-only: 773 crates in 49 s
is about 63 ms each.

*Method, and its one hole.* The dependent counts above are **manifest
mentions**, not the set actually re-checked, and the two differ: touching
`posix` re-checked **21** crates against 13 manifest mentions, because the
re-check set is transitive. That the method measures what it claims is
confirmed rather than assumed -- the run's own `Checking <crate>` lines were
counted. But only the *last* run's output survived: the harness wrote every run
to one path and overwrote it, so the same confirmation is not available for the
other three. The timings stand (they scale with dependent count as expected),
the per-run re-check counts do not exist for `authlib`, `guitk` or `quoting`,
and a harness that discards its own evidence is a poor instrument for an entry
about not discarding evidence.

*A second, softer hole, worth naming because it is the same shape.* Every
"contended" figure here is labelled with a load that another session **told**
me it was running, not one this session observed. Had the boot test changed
phase mid-measurement the label would be silently wrong. Lane A's
generalisation covers it exactly: a harness that characterises the machine by
assertion rather than observation is making a claim about the machine, not
about the code.

Two premises were wrong in the discussion that produced this table, both by
reasoning about a set without counting it -- the same failure as the `head -3`
and the one-file `grep -c` recorded elsewhere in this entry:

* *"Downstream of `authlib` is most of those 2,759 `userspace` crates."* It is
  **12**. The blast radius that actually justified the gate was small; what
  makes it matter is that the victims were in another lane, not that there were
  many of them.
* Nobody had identified the widest shared crate. It is not `guitk`, `posix` or
  `authlib` -- it is **`quoting`**, at 773 dependents, and it was in none of the
  three candidate lists.

Member counts, for the record: `userspace/*` 2,759, `apps/*` 143, `services/*`
76, `gui/*` 15, `init/*` 2, `net/*` 2.

**What the numbers do and do not decide.** At 15 s warm-incremental under
contention -- and 49 s in the worst shared-crate case -- cost cannot decide the
policy: the gap between 8 s and 15 s is inside the noise of a merge. Scope should therefore be chosen for **coverage**,
where the two differ sharply -- a gate scoped to `apps/`+`gui/` would have caught
the `guitk::Event` breakage and would *not* have caught the `authlib` one, whose
callers are all under `userspace/`. Whole-workspace is the only scope covering
cross-lane API changes, which is the case that raised the question.

**The run that mattered was the one that failed.** The first attempt returned
`rc=101`: non-exhaustive `match` on `guitk::Event::SettingsChanged` in
`apps/stickynotes` and `apps/explorer` -- the exact two crates, and the exact
failure mode, that prompted C-Q11. Both were already fixed on `origin/main`; the
tree taking the measurement was nine commits behind. So this is a **fourth**
instance of the stale-tree error described above, committed by the person
measuring the cure, during the measurement. Together with lane C's third
instance -- committed by the proposal's author, the same day they wrote the
argument, having just documented two other lanes' -- the pair is stronger
evidence than any timing in the question: two people maximally primed to avoid
it, both failing within hours.

**What a green workspace check does and does not prove.** It catches the crates
that *fail to compile*. Lane C's diagnosis is that the two which broke were the
two matching exhaustively, while roughly 140 others end in a catch-all. Lane A's
refinement narrows that usefully: a catch-all that *forwards* the value
(`Err(e) => report(e)`) still surfaces a new variant as its own text, while one
that *discards* it (`_ => {}`) cannot. The hazard is discarding, not failing to
enumerate. But a gate cannot tell the two apart without reading them -- so a
green check is a floor on correctness, not a proof of it, and every "only N
crates broke" count in this file understates the blast radius, including the
ones written above.
