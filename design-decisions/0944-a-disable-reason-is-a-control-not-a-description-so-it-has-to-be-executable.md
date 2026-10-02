## 944. A disable reason is a control, not a description, so it has to be executable

**Date:** 2026-09-15 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; **Found by lane B**, whose `check-stale-blockers.py` pass
put the population in front of a reader for the first time

**In short:** several self-tests in this kernel are switched off, and each one
has a comment saying why. That comment is the only thing keeping it off. One
of mine had been wrong for six days -- it named a change another lane had
already landed, while the real reason had moved on three times and was written
down 7000 lines away in a different file. A teammate read the wrong copy,
believed it, and almost sent me a confidently incorrect request. Nothing
catches this, because a comment is never run.

### Why this is not just another 938

938 and 943 are about stale **descriptions**: a reader forms a wrong belief,
and the artifact itself is inert. This is a stale **control**. The sentence is
not describing why the rung is off -- it *is* why the rung is off. Three
things follow that the description cases do not have:

1. **The cost is a test that does not run**, and it accrues daily rather than
   at the moment somebody misreads it.
2. **It is self-sealing.** The rung is off, so the subsystem it covers
   accumulates no evidence, so nothing ever contradicts the sentence. A stale
   description can at least be caught by the thing it describes misbehaving.
3. **It resists the usual remedy.** `known-issues.md` entries get stamped
   `FIXED` on the day they are fixed, and ours are. A comment cannot be
   stamped, because nothing indexes it.

### The evidence, all measured today

| Site | Says | Actually |
|---|---|---|
| `spawn.rs:9683` | ctest-pty is off while lane B routes PtySlave reads through 872/873 | that landed 2026-09-09 in `f83bcb2ed`; four disable cycles ago |
| `main.rs:2710` | the live reason, accurate and current | correct -- and 7000 lines away from the other copy |
| `spawn.rs:9682` | the fixture avoids `alarm`/`setitimer`, known-broken | first half true; `B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING` is stamped **FIXED 2026-09-12** |

The sharp observation is lane B's. Five copies of the timer fact exist outside
`known-issues.md`, and **only the copies rotted**. The entry itself was
stamped correctly and on time. Discipline worked exactly where there was an
index and failed everywhere there was not. So the remedy is not more diligence
about updating copies; it is not having copies. A load-bearing fact gets one
home, and every other site points at it rather than restating it.

### A date is not a content check

My own re-enable condition read: *re-enable when `sysroot-check` passes AND
`services/ctest-pty/ctest-pty.elf` is newer than `6e19f88a1`*. Lane B declined
to settle it that way, and was right twice over. The ELF is gitignored, so it
is a per-worktree build artifact with no canonical instance -- there is no
"the" ELF, and their timestamp said nothing about mine. And a rebuild
refreshes a timestamp whether or not the source changed, which is precisely
how the stale binary came to look staged in the first place. They grepped both
binaries for the `child_verdict` symbol instead. **A condition phrased over
provenance can pass for the wrong reason; phrase it over content.**

They also declined to claim more than the instrument supports: the two builds
are not byte-identical despite identical size and identical source, so the
fixture is not reproducible and "same size, same symbol" is the strongest
available claim. That is the right shape for a verdict -- say what you
measured, not what you would like it to mean.

### What this changes

- Re-enable conditions stop being prose. Where one can be expressed as a
  command, it is written as a command a check can run.
- A disable reason lives at the call site, because the call site is what
  stopped running. The definition gets a pointer, not a second copy of the
  reason. (Lane B's own gate caught them collapsing those two into one
  finding; they are two, and that pairing is what made this findable at all.)
- The population of switched-off tests is printed on every push by lane B's
  pass, which deliberately does **not** judge whether a reason still holds --
  that needs prose understanding. Putting the list in front of a human each
  time is the whole mechanism, and it was enough: it found this in one run.

### Rejected: expire the comments automatically

Tempting to make every disable reason carry a date and go red after N days. It
would have caught all three of these. Rejected because it converts a correct
long-lived reason into recurring noise, and the failure mode of noise on this
project is documented one section up: lane B's first version of the same pass
printed 41 unchanging lines, which is how the five real findings underneath
get skipped.

### Amended the same day: executable is necessary, not sufficient

Lane B fixed the staging gap, and the fix uncovered a second layer that
changes what this entry asks for. The harness's synthetic repositories had
been skipping gate 42 on this line:

```sh
[ -f "$toi" ] || skip_toi=1
```

That is precisely what this entry asks for. It is a disable condition that
is *executed* rather than written in prose, re-evaluated every run, and
incapable of going stale. **And it hid the gate anyway, because it disabled
silently.**

Lane B sharpened the diagnosis after reading the above, and the sharpening
is the point: **the condition was never wrong for a moment.** `[ -f ]` is
correct and was correct on every run. So the failure is not staleness at
all -- it is that **a skip produces the same output as a pass**. The
distinction was in fact recorded, by `note_gate test-order "$skip_toi"`,
into a channel nobody reads at verdict time. Which makes this 942 one level
up: the information existed, and the verdict did not carry it.

So executable is necessary and not sufficient. The qualifier: **an
executable skip must announce itself where the verdict is read.** 942 is a
check that ran and proved nothing; this is a check that never ran and said
nothing. Both render green.

The detail worth keeping is that **supplying a missing file activated a
gate**. Staging one dependency turned a skip into a run, and the run then
failed for the *tree* rather than for the checker. The fixtures had been
passing gate 42 by not having it -- and so would any tree missing that
file. It is the cleanest case in this document of a green verdict
describing an absence.

Lane B's two refusals are recorded because the reasoning generalises. They
declined to key the replacement guard on the graded crates existing (the
fixture creates `posix/`, so the condition is true there and does not
discriminate) and on *one graded crate missing* -- which would have silently
disabled the gate on the day somebody deleted a graded crate, which is
exactly when it should be loudest. **A guard that weakens precisely when the
thing it guards is damaged is worse than no guard**, because it converts a
loud failure into a quiet pass.
