## 630. The shellcheck gate stops the build on a finding but waves it through when the tool is missing

**Date:** 2026-08-29
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** We now run a shell-script linter over `scripts/` before the
kernel build, and refuse to build if it complains. But not every one of the
three lanes has the linter installed — it is a third-party program, not part of
the toolchain — and a lane that lacks it would otherwise be unable to build the
kernel at all. So when the tool is absent the check prints how to install it and
carries on. That means the check is *mandatory where it can run and advisory
where it cannot*, which is a weaker promise than a gate normally makes, and the
reason it is written down here rather than just done.

**Context.** `scripts/shellcheck-all.sh` existed for two days referenced by
nothing, and was unrunnable in lane A because the binary was never installed
there. Wiring it into `boot-test.sh` raised two choices that both have a real
case on either side.

**Decision 1 — gate at `error`, not `warning`.**

*For `error`:* the tree already has zero findings at that floor, so the gate is a
clean-tree test with no baseline file. It can only ever fire on something the
change in hand introduced, which makes a failure unambiguous and actionable and
means nothing has to be paid down before it can be turned on.

*For `warning`:* it catches strictly more, including SC2086 (unquoted expansion)
— which is the class that produced the `D:\visual` stray-file incident, i.e. the
single most expensive shell bug this project has had. Gating at `error` does
**not** catch that class. This is the genuine cost of the choice and it should be
stated plainly rather than buried.

*Why `error` won anyway:* `warning` has 48 findings today, ~35 of them one false
positive (SC2209 on `DIFF_PROG=<name>`, where the tool cannot tell a bare command
name from a forgotten `$(...)`) in **lane B's and lane C's** differential
harnesses. Turning it on would require either 34 cross-lane edits I may not make,
or a baseline file — converting a clean-tree test into a ratchet whose baseline
then drifts and whose entries nobody revisits. A gate that must be introduced
alongside a 48-line suppression list starts its life being ignored.

*How to reverse:* raise the floor to `warning` once the tree is clean at that
floor. That is the right end state and the only thing standing in its way is
cross-lane ownership, not disagreement.

*Correction, 2026-08-29 — the condition above originally read "once lane B
quotes the `DIFF_PROG` assignments", and that was wrong.* It named one of the
two blockers, so it would have read as satisfied while the reversal still broke
the build — the same failure shape as the `design-decisions.md` insertion rule
corrected in §631: a condition that looks met when it is not is worse than one
nobody has met, because it invites the action it is supposed to gate. Measured
today, `warning` has **44 findings across 38 of 78 scripts**, and they are in
three groups, not one:

| Group | Count | Whose | State |
|---|---|---|---|
| SC2209 on `DIFF_PROG=<name>` | 37, one per `*-diff.sh` | lane B | outstanding, and **growing** — every new harness copies the idiom |
| SC2191/SC2258/SC2010/SC2034 in `gen-chmod-fixture.sh`, `paste-diff.sh`, `split-diff.sh` | 7 | lane B | outstanding, and **not** the same false positive — these need reading, not quoting |
| SC2154/SC2054 ×2 in `boot-test.sh`, SC2034 ×3 in `test-boot-lock.sh` | 6 | lane A | **done** — annotated with `# shellcheck disable=` plus the reason each is a false positive |

Lane A's six were the half of the blocker the original condition did not
mention, and they are cleared: `_floor_val` is assigned through an `eval`
shellcheck cannot follow (and its suggested "fix" would compare the floor's
*name* against the digits, silently disabling the very check that loop
performs); the two SC2054s are QEMU's own `,` property separator inside a single
`-device` argument; the three SC2034s are inputs to a lock region sourced from a
file extracted at run time. None was a real defect, but each had to be looked at
to know that, which is the work the condition was hiding.

So the accurate condition is: **44 findings to go, 37 of them one mechanical
edit and 7 of them genuine reading, all of them lane B's** — the paragraph
above said "lane B's and lane C's", and that was wrong too: every one of the 37
flagged scripts is a differential harness for `userspace/**`, and lane C owns
none of them. Filed as
`requests/a-b-shellcheck-floor-the-remaining-findings-are-all-yours.md` — whose
name deliberately carries no number, because the count moves: it was 43 across
76 scripts when the request was drafted and 44 across 78 an hour later, when
`xargs-diff.sh` merged from `main` carrying the same unquoted idiom as its 36
predecessors.

***Reversed the same day: the floor is `warning` as of 2026-08-29.*** Lane B
cleared all 44 within hours of the request and replied in
`requests/b-a-shellcheck-is-clean-at-warning-raise-the-floor.md`; the sweep
reports `78 script(s), 0 with findings at severity warning`, verified in lane
A's own worktree before the flip. Decision 1 above is therefore superseded, and
it is worth being precise about what that means: **it was not wrong, it was
temporary and said so.** The condition it set — clean at `warning`, no
suppression list, no baseline file — is exactly the condition that was met, so
the gate is still a clean-tree test and still has no baseline to drift. What
changed is only which severity the tree happens to be clean at.

The flip matters more than one word suggests: until today the gate **could not
have caught its own founding incident.** SC2086 — the unquoted expansion that
word-splits on a space, which is what created the stray `D:\visual` file — is
severity `warning`, so at `error` the gate was blind to the precise bug class it
was written for. It is now not.

Lane B also did the part that stops the backlog regrowing, which the request
asked for as the one thing worth doing above all the others: `diff-wsl.sh`'s
"Using it" header — the block every new harness is copied from — showed the
unquoted `DIFF_PROG=cat`, and now shows it quoted. All 50 harnesses were
quoted, not just the 37 the tool flagged, so the set is uniform and a reader
cannot tell which ones shellcheck happened to recognise as command names.

*A trap recorded here because it is a repository-wide hazard, not a lane-B
one:* **a comment whose first word is `shellcheck` is parsed as a directive.**
Lane B wrote one line of English beginning `# shellcheck cannot tell ...` into
`diff-wsl.sh`; it failed to parse (SC1073/SC1072), and because all 50 harnesses
*source* that file under `-x`, every one of them then reported SC1094 and
**silently lost its `-x` suppressions**. The count went from 44 to 227 across 50
files nobody had touched. Two things follow. `diff-wsl.sh` is a blast radius: a
parse error in it degrades every dependant at once and does so quietly. And
`shellcheck -S warning diff-wsl.sh` alone would *not* have found it, because the
directive error is severity `error` — only the whole sweep shows it. That is the
fourth instance in this entry of the same shape: a check whose answer depends on
how it is invoked.

*One more measurement worth writing down, because it nearly produced a fourth
wrong statement in this entry.* `shellcheck-all.sh` runs `shellcheck -x` from
inside `scripts/`, and `-x` is load-bearing: it follows the harnesses'
`# shellcheck source=diff-wsl.sh` directives and thereby suppresses ~3 findings
per harness (SC2034 on `DIFF_PROG`/`DIFF_NEED`, SC2154 on `bindir`) that a bare
`shellcheck scripts/paste-diff.sh` from the repo root still reports. Run from
the wrong directory the relative `source=` cannot resolve and `-x` appears to
do nothing — which is what lane A saw, and briefly recorded as "`-x` does not
help, the hypothesis is dead". It was the cwd, not the flag. This is the same
defect shape a third time: **a check whose answer depends on how it is invoked
will be believed on whichever invocation you happened to run.** The number to
trust is the one `shellcheck-all.sh` prints, because that is the invocation the
gate uses.

**Decision 2 — a missing tool skips rather than fails.**

*For skipping:* the alternative is that installing a third-party binary becomes a
precondition for building the kernel. A lane that pulls `main`, runs the boot
test and is told it cannot build until it downloads something from GitHub has
been handed a worse problem than the one the gate solves. The skip is loud and
prints platform-specific install instructions, so it is a prompt rather than
silence.

*Against skipping:* it reproduces in miniature exactly the defect being fixed. A
check that silently does nothing in some environments is how `shellcheck-all.sh`
came to sit unrun for two days in the first place, and a lane could stay in the
skip state indefinitely without any consequence.

*Why skip won:* the failure modes are asymmetric. A false skip costs delayed
detection of a lint finding. A false hard-fail costs a lane its ability to build
and boot-test the kernel, which is the thing every other gate in the file is
protecting. Given the tool is a single static binary with no root requirement,
the install prompt is very likely to be acted on; and the day all three lanes
have it, the skip path is dead code that costs nothing to leave in.

*How to reverse:* change the `rc -eq 2` arm from `return 0` to `exit 1`. Worth
doing once it is confirmed all three lanes have the binary — at that point the
skip protects nobody and only hides a regression in the tool's installation.

**REVERSED 2026-08-29 (lane A, Claude autonomous), on the trigger above.** The
precondition was *verified rather than assumed*: the tool is present in all
three lane worktrees, and `shellcheck-all.sh warning` exits 0 with zero findings
over 79/80/79 scripts in each. So no lane is blocked by the change, and from
here an exit 2 means the binary was **removed** — which must stop the build
rather than be waved through.

What tipped it was not just the precondition being met but what the intervening
weeks showed: this is the first of **five** gates since found blind (the D1
rustfmt-wrapped chain, D1's `from_str_radix`, the wording gate's wrapped
literals, and D4's brace miscount are the others), and the only one whose
blindness was *designed in on purpose*. Every one of the other four was
discovered after it had already cost something. Leaving a deliberate blind spot
in place, in a file whose other gates now all fail closed, was the least
defensible of the five.

Small confirmation that the reversal has teeth: the very first run under it
failed the build, on a comment in the new arm that opened with the tool's own
name — a `# shellcheck …` line is parsed as a *directive*, and an unparseable
directive is itself an error (SC1072/SC1073). Under the old arm that would have
depended on the tool being installed to be noticed at all.

**Where it is:** `check_shellcheck` in `scripts/boot-test.sh`, immediately after
`check_orphan_modules`. Fulfils
`requests/b-a-two-cd-calls-ignore-failure-in-shared-scripts.md`, which is also
where the install note for the Windows build is recorded.
