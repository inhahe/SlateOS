## §277 — A staleness check that can only fail is retired, not repaired: the content stamp already answers its question, and better

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the boot test used to print a red warning saying "the compiled
test programs in the disk image are out of date, so treat their results as
covering old code." Since 2026-08-21 that warning has printed on *every* run,
including runs where the programs were freshly built moments earlier — the
check behind it lost the files it was watching and started reporting "I cannot
tell" on every invocation, which the boot test rendered as "they are stale."
The choice was whether to repair that check or delete it. It is deleted, and
the one boot-test slot that genuinely needed an answer now calls a different
check that was already in the tree and answers the question more completely.

### What the check was for

Every ring-3 test program in `services/ctest-*` links `toolchain/sysroot/lib/
libc.a`, which is built from `posix/`. `libc.a` is a build artifact, not
tracked in git, so a `git merge` that brings in new `posix/` commits leaves it
behind without saying anything. From that moment the whole shelf of test
programs exercises a libc that is not in the tree — and every content check
stays green, because the programs and their recorded inputs still agree with
each other. They agree about a stale input.

That had happened four times, each time discovered by a different lane one to
three days later than the lane that caused it. `scripts/stamp-ancestry.py` was
lane A's answer: for each committed artifact it read a committed `.stamp` file
recording the commit it was built at, then walked git history for later commits
touching the sources that artifact declared. History, not content — the
argument being that a content check "cannot see past its own recorded inputs".

### Why it stopped working

Lane B's §355 (2026-08-21) stopped committing the 70 compiled fixtures and the
`.stamp` files that dated them; they are gitignored and built on demand now.
`stamp-ancestry.py` knew exactly one family, matched by
`:(glob)services/ctest-*/*.stamp`. `git ls-files` matches nothing for it.

The script has a deliberate and correct branch for that case — refuse to report
*clean* for a family it cannot see, because "could not verify" must never
render as "fine". With one family and no stamps, that branch became its only
reachable outcome. Lane B filed
`requests/b-a-stamp-ancestry-now-only-ever-errors.md` naming both consequences:
the warning's text says "the posix/ commits named above" while naming nothing
(the script errored before computing a list), and it asserts staleness that is
false — the fixtures at that moment are current *by construction*, because the
only thing that builds them refuses to build against a stale libc.

### The decision

Delete `scripts/stamp-ancestry.py`. In `scripts/boot-test.sh`, replace both
call sites with `ctest-fixtures.py sysroot-check`.

`sysroot-check` asks the same question by content: it hashes the 2312 sources
`libc.a` is built from and compares them against `toolchain/sysroot/
.sysroot.stamp`, written by `build-sysroot.ps1` on every run. That is a direct
answer to "is `libc.a` behind the tree", which is the question the reader
actually has. The premise that made history necessary — "a content check cannot
see past its own recorded inputs" — was true of the *fixture* stamps, which
record `libc.a`'s hash and so can only confirm the fixtures match whatever libc
is on disk. It is not true of a stamp that records the libc's own *sources*.

### Alternatives considered

| Option | Why not |
|---|---|
| Repoint the family at the ELFs | They are gitignored too; there is nothing tracked left to date them by. Lane B ranked this last for the same reason. |
| Keep the script, drop its only family | Leaves a script with zero families whose correct behaviour is to succeed silently — i.e. a file that exists to do nothing. |
| Delete both call sites outright (lane B's suggestion 1) | Nearly right, but see below: it would have left a real hole. |
| Keep the warning, suppress it when it cannot verify | This is how a check that reports nothing gets to keep looking like a check. If it cannot answer, it should not be called. |

### Why the passing-path slot was rewired rather than removed

Lane B's suggestion was to delete the script *and* its call site, on the
grounds that the rootfs script now rebuilds anything missing or out of date, so
the fixtures are current by construction. That is true **at the moment the
image is packed** and not afterwards. The sequence that still bites:

1. Build the fixtures and pack `rootfs.ext4` while `libc.a` is fresh.
2. `git merge origin/main`, which `CLAUDE.md` requires at the start of every
   task, brings in new `posix/` commits.
3. Nothing on disk changed, so `image-check` passes: the image genuinely does
   match the ELFs in the tree.
4. Every ELF in it nonetheless links a libc that is no longer in the tree.

That is a green boot test whose Path-Z rungs covered a system this tree cannot
build — the silent version of the failure, and the one the long comment in
`boot-test.sh` was right to say is worse than the loud one. So the slot keeps a
warning; only its evidence changed. It remains a warning and not a failure for
the reason already recorded there: repairing it means rebuilding `services/**`,
which is lane B's tree, and failing would block every lane-A boot test on a
repair lane A must not make.

The replacement is also strictly stronger in one respect nobody asked for: a
content stamp sees **uncommitted** `posix/` edits. A history walk cannot see
those at all, so the original check would have called a working tree with
half-finished libc changes perfectly clean.

### The cost, stated plainly

We lose the ability to say *which commit, and therefore which lane*, invalidated
the fixtures — a history walk could name `d5a23c2f9` and its author; a content
stamp can only name the files whose bytes moved. That was a genuine convenience
in the cross-lane case, since the lane that trips over the staleness is never
the lane that caused it.

It is worth losing anyway. `git log --oneline <stamp-date>..HEAD -- posix/`
recovers the commit list in one command once you know `posix/` is what moved,
and knowing *that* is the part the reader could not previously get without
hashing `libc.a` across four worktrees by hand. The attribution was the cheap
half of the answer, and it was being paid for with a permanent false alarm.

### Generalisable lesson

**A detector whose "I cannot tell" branch has become unconditional is no longer
expressing doubt about the run; it is expressing a fact about itself.** The
rule it was following — never render *could not verify* as *fine* — is right,
and this is not a case against it. But that rule is about a check that
*sometimes* cannot see; applied to one that *never* can, it produces a banner
that fires every time, and a banner that fires every time is read as
decoration. The failure is not in the branch, it is in leaving the check wired
up after the thing it watched stopped existing. When a check's subject is
removed, the check goes with it in the same change — otherwise its output
outlives its meaning, and readers learn to skip the region of the log it prints
in, which costs more than the check ever bought.

`scripts/boot-history.py` carries a rider to this effect next to the rule it
shares, since it is the other place in the tree that reasons about
unvalidatable evidence.
