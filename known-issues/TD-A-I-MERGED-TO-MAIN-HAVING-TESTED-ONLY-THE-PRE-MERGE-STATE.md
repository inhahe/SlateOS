## TD-A-I-MERGED-TO-MAIN-HAVING-TESTED-ONLY-THE-PRE-MERGE-STATE (lane A, 2026-09-11) — **process miss, remediated; a cheap tool is proposed**

**In short:** before merging work up to the shared branch you are supposed to pull
everyone else's work into your own branch first and test the combination. I tested
my branch, and while I was working the shared branch moved, so what I pushed was a
combination nobody had tested. It turned out to be safe — but I only established
that *after* pushing, and "it was fine" is not the same as "I knew it would be".

### What happened

`CLAUDE.md` → "Before merging: `git fetch origin && git merge origin/main`, run the
full test suite, only then merge up." I did the test suite and the merge-up, in that
order, and skipped the fetch-and-merge. `main` had advanced from `69c35d73f` to
`12a7a84fa` — eleven commits from lane B — while the boot test ran.

It was safe for a reason I could have checked in ten seconds and checked only
afterwards: **every one of those eleven commits lands in `scripts/` or
`userspace/`, and none in `kernel/`.** So the kernel in merged `main` is
byte-identical to the kernel I boot-tested, and the merge could not have invalidated
the run. I then did the verification properly in the other order — merged
`origin/main` into `lane-a`, confirmed `scripts/rustlex.py` (which lane B and I both
rewrote on the same day) passes its own suite with both changes, re-ran the five
gates of mine that consume it, and boot-tested the combined state.

### The part that is worth more than the apology

**The check I performed by hand is mechanical, and nothing performs it.** "Do the
commits I am about to merge touch anything my test suite actually covers?" is a
`git diff --name-only` and a glob intersection. The lanes are partitioned by
directory precisely so that most cross-lane merges are independent — which means
most of the time the honest answer is "no, your run still stands", and the rule's
full cost (re-merge plus a ~25-minute boot test, every time another lane lands
anything) is not warranted.

So the useful form is advisory rather than blocking:

> `origin/main` is 11 commits ahead of what you merged. They touch `scripts/**`
> (7 files) and `userspace/**` (3 files). **None touch `kernel/**`, so your boot
> test still covers the merged kernel.**

versus the case that matters:

> … **4 touch `kernel/**`. Your boot test does not cover them.**

A blocking gate here risks a three-lane deadlock — each lane re-merging and
re-testing because the others keep landing work — and a deadlock is how a gate gets
bypassed. An advisory that states the one fact I had to derive by hand costs nothing
and would have been right at the moment I needed it.

### A second limit, found in the same run and easy to miss

The remediating boot test printed `WARNING: fixtures are behind the tree (boot test
still valid)`. That is accurate and narrower than it sounds: `rootfs.ext4` still
holds the *previous* userspace binaries, so lane B's `userspace/cgroup` and
`userspace/oils` changes were **not** exercised by the run I used to verify "the
combination". What the run actually verifies is the merged kernel plus the merged
gates — which is the part in my lane, and is what I needed — but it is not the whole
merge, and recording it as "combination verified" would have overstated it.

This cuts the same way as the advisory above: the thing to know is *which* parts of
a merge your test covers, and the harness already knows enough to say so.
