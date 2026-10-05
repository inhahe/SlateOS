### B-A-A-LAYOUT-SWEEP-IS-VOIDED-BY-ANY-COMMIT-MADE-WHILE-IT-RUNS, AND ITS PER-ARM GUARD CANNOT SEE THIS (lane A, 2026-08-19) -- FIXED 2026-08-19 in `020e014d0` + `ff74de728` + `fc40b5f15` + `f376cb8ff`; see "Resolution" at the end of this entry

**In short:** a layout sweep builds and boots the same kernel six times at six
different code placements, and the six results are only comparable to each
other. `bench-history.py` decides which runs are comparable by asking whether
they were recorded at the same git commit. That is not the same question. The
WHPX sweep running when this was written committed nothing to `kernel/` at
all -- the six arms are byte-identical builds -- but three *documentation*
commits landed while it ran, so the arms recorded three different commit
hashes and the tool filed them as three unrelated experiments of one arm each.
Six of a minimum three are needed to form a band; one is not. The sweep would
have produced nothing, ~70 minutes later, with no error anywhere.

**Where:** `scripts/bench-history.py` -- `layout_arms()` line 3108, the group
key `(record["commit"], record.get("accel"))`. Guard hole:
`scripts/layout-sweep.py` -- `check_arm_counts()`.

**Evidence.** Mid-sweep, with three arms recorded:

```
('b36a244bb', None)              pads= [0, 1024, 1536, 2048, 2560, 3072]   <- the TCG sweep, intact
('943b3f21b', 'Hyper-V/WHPX')    pads= [0]
('fb6605fc0', 'Hyper-V/WHPX')    pads= [1024]
('493b6aac5', 'Hyper-V/WHPX')    pads= [1536]
```

and the arms really are one experiment -- the `kernel/` tree object is
identical across every commit involved:

```
943b3f21b  kernel-tree=6876f9eb3b843727242e8f260f225df3b62f28ec
fb6605fc0  kernel-tree=6876f9eb3b843727242e8f260f225df3b62f28ec
493b6aac5  kernel-tree=6876f9eb3b843727242e8f260f225df3b62f28ec
3fff02a30  kernel-tree=6876f9eb3b843727242e8f260f225df3b62f28ec

$ git diff --name-only 943b3f21b 3fff02a30
known-issues.md
open-questions.md
```

Nothing that can affect any build changed across the whole span. The arms are
sound; only the key that groups them is wrong.

**Why the guard did not catch it -- the third hole of the same shape.**
`check_arm_counts()` exists because two earlier sweeps were voided silently,
and its docstring says so. It printed, for every arm:

```
[layout-sweep] confirmed: the pad=0 record is accepted as an arm by bench-history.py
[layout-sweep] confirmed: the pad=1024 record is accepted as an arm by bench-history.py
[layout-sweep] confirmed: the pad=1536 record is accepted as an arm by bench-history.py
```

All three statements are true. `layout_arm_rejection()` is a predicate on a
*single record* -- is this row dirty, is the host loaded, is the profile
right -- and every arm passed it. But whether a sweep yields a band is a
property of the *group*, and no per-record predicate can see a group. The
guard was built to answer "will this row be kept?" and the question that
matters is "will this row land beside the previous one?".

**Why this is not a data problem and must not be fixed as one.** The recorded
`commit` values are correct: that genuinely was HEAD at each boot. Nothing in
the log is false, so `design-decisions.md` §238 (an append-only measurement
log may be corrected in place, but only for facts about the *harness*) does
not apply and the rows must not be touched. The defect is that `commit` is
being used as a *proxy* for "the kernel source that produced this binary", and
that proxy silently stopped holding the moment anything outside the build
inputs became committable during a run. This is the same class of defect as
`CANARY_MIN_RESOLVABLE` surviving the `CENTI` change: an inference resting on
a justification that quietly became false, with no test able to notice because
the justification was never expressed as code.

**The fix, in three parts.**

1. **Compute source identity; stop inferring it.** Define
   `src_digest(treeish)` **once**, as a hash over the build-relevant paths of
   a tree -- and give it two callers rather than two implementations:

   - `boot-test.sh` calls it on the *working tree* and records the result in
     each row, so uncommitted state counts and a doc commit does not.
   - `layout_arms()` groups by `(src_digest, accel)`; for a row written before
     the field existed it **derives** the digest from the row's `commit`,
     which git still holds. That is not editing data -- the rows stay exactly
     as written -- it is reading `commit` for what it can actually answer.
     Sound only because `layout_arms()` already drops `dirty` rows, so for
     every row it keeps, `commit` really does identify the source.

   The derivation is what rescues the six arms already recorded: their commits
   differ, but `git rev-parse <c>:kernel` is
   `6876f9eb…` for every one of them, so the correct key groups them and the
   sweep's ~70 minutes are not lost.

   **The exclusion list must be explicit top-level paths, never a glob**, and
   this is the part that is easy to get dangerously wrong. The obvious
   spelling -- exclude `*.md` and `*.txt` -- is *unsafe*: this tree contains
   `kernel/src/fs/declared.txt`, `kernel/src/fs/existing_files.txt` and
   `kernel/ada/prebuilt/stamp.txt`, all inside the kernel build. A recursive
   `*.txt` exclusion would drop them from the digest, and two genuinely
   different kernels would then share a key and merge into one band -- a band
   that wide dismisses every regression, silently.

   That asymmetry decides the whole design. Over-inclusion splits arms that
   belong together and yields "unmeasured", under which a movement stays a
   regression and a human looks at it. Under-inclusion merges arms built from
   different source, and fails in the direction that hides faults. Over-
   inclusion is what we have today and it has now cost a sweep; it is *still*
   the correct direction to err in. So the key is narrowed by an audited list
   of root documents (`*.md` at depth 0, the root `*.txt` design documents,
   `requests/`, `bench/*.jsonl`) and by nothing else. Anything new and
   unrecognised counts, and costs at worst a split.

2. **Make the guard ask the question that matters.** `check_arm_counts()`
   should, from arm two onward, compare this arm's group key against the
   previous arm's and abort the sweep immediately when they differ, naming
   both. Three hours of silence becomes a twenty-minute failure with the
   reason attached -- which is the stated purpose of the guard and the reason
   it exists.

3. **Say it in the sweep's own output.** `layout-sweep.py` should print, at
   start, that committing anything during the sweep will void it under the
   current fallback path -- and stop needing to say so once part 1 lands.

**The obvious interim mitigation does not work, and finding out why sharpened
the fix.** Freezing commits for the rest of the sweep looked like it would let
the remaining arms share a commit and form a three-pad band. It cannot:
`boot-test.sh` captures `BT_HEAD` *before* the build (deliberately -- it must
name the source that was built, not the source at the time the row is
written), so arm 4 was already stamped with the commit that was HEAD ~10
minutes before it recorded. At most two of the six arms can now share a key,
one below `MIN_PADS_FOR_LAYOUT_BAND`. Freezing buys nothing, so it was not
done.

That is what forces the retroactive derivation in part 1 rather than a
record-a-new-field-and-move-on fix, and the derivation is the better design
anyway: it states the rule once, applies it to old and new rows alike, and
does not leave a cutover date before which bands are computed by one rule and
after which by another.

**Re-run the sweep after part 1 lands** regardless -- not to obtain the band,
which the derivation recovers, but to validate part 1 end-to-end on a sweep
that is *deliberately* committed through. A fix for a silent-fragmentation
bug that has never been observed to survive a commit mid-sweep is a fix on
paper.

#### A git-only digest would be under-inclusive — the kernel embeds six untracked binaries

Found while auditing what part 1's digest must cover, and it changes the
design: **a `src_digest` computed from git alone is wrong in the unsafe
direction.** The kernel `include_bytes!`s six ring-3 service binaries, and
none of them is tracked:

```
services/hello/target/x86_64-unknown-none/release/hello    tracked=NO exists=YES
services/init/target/x86_64-unknown-none/release/init      tracked=NO exists=YES
services/ticker/target/x86_64-unknown-none/release/ticker  tracked=NO exists=YES
rootfs.ext4                                                tracked=NO
```

They are build artifacts, gitignored by design (`boot-test.sh` line 1483 has
a whole comment about a fresh checkout not having them). So two arms that
embed *different* `init` binaries would produce different kernels, hash
identically under a tracked-files-only digest, and merge into one band --
which is precisely the merge-two-different-kernels failure the exclusion-list
argument above rules out. Ruling it out via the exclusion list and then
re-admitting it through untracked files would be an odd way to lose.

**The same hole is already in `dirty`, today.** `git diff --quiet HEAD -- .`
cannot see untracked files, so `dirty == False` does not mean "the built
kernel is reproducible from this commit" -- it means "the *tracked* files
match HEAD". Rebuild `services/init` between two boots at one commit and both
rows say `dirty: false`, and `layout_arms()` will happily band them across
two different kernels. The existing TCG band rests on this too. It has
probably never bitten, because a sweep runs its arms back-to-back and nothing
rebuilds the services in between -- but "probably never" is the same
guarantee the commit-proxy had until this morning.

**So the digest has two halves**, and the second is free: enumerate the
embedded artifacts by the *same derivation* `bootstrap-worktree.sh` already
uses -- it greps `include_bytes!` out of `kernel/src` (line 106) rather than
keeping a list -- and hash their contents alongside the tracked build inputs.
A service added to the kernel is then covered without anyone remembering,
which is the property that derivation was written for in the first place.
`rootfs.ext4` joins them: every boot attaches it, and what it contains
changes what the suite measures.

One statement of each rule, in one more place than planned.

#### The derivation was validated against the real records before being written

A design that rescues six recorded arms is a claim about data that already
exists, so it can be tested before any of it is implemented.
`build/srcdigest-proto.py` (read-only; safe to run while a sweep holds the
tree) computes the proposed digest over `git ls-tree -r <commit>` with the
exclusion list above, and was run against the arms as recorded. Four
controls, all with a stated failure mode:

| control | result |
|---|---|
| the four WHPX arms recorded so far, across **four different commits** | one digest, `397a6447…` — **groups** |
| the TCG sweep commit `b36a244bb` | `1a167159…` — **discriminates**; a digest that matched here would band two unrelated sweeps |
| a kernel-only commit (`d937ea7bd`, `kernel/src/layout_pad.rs`) vs its parent | digests differ — **sensitive** to the thing that matters |
| a doc-only pair (`5850f706f` vs `5ea44e308`) | identical — **insensitive** to the thing that broke it |

and the exclusion list audited directly: 101 of 13,427 paths excluded, of
which 68 are `requests/`, 31 are root documents and 2 are the harness's own
`bench/*.jsonl`. **Zero** lie under `kernel/`, `toolchain/`, `services/`,
`.cargo/`, or `Cargo.{toml,lock}` — which is the assertion the
`kernel/src/fs/declared.txt` trap made necessary, now checked rather than
reasoned about.

The third and fourth controls are the pair that matters, and they are worth
stating as a single sentence: *a kernel edit changes the key and a
documentation edit does not*, which is exactly what `commit` failed to do and
the entire reason for the change. Neither control is implied by the first
two — a digest could group the arms by being insensitive to everything.

#### Resolution (2026-08-19)

All three parts landed, in four commits, and the six recorded arms were
rescued as the design predicted.

| part | commit | what it does |
|---|---|---|
| 1a | `020e014d0` | `scripts/src_digest.py` — one definition of source identity, with the audited exclusion list and the `include_bytes!` derivation for the untracked artifacts |
| 1b | `fc40b5f15` | `boot-test.sh` computes it over the **working tree** and stamps every row, so uncommitted state counts and a docs commit does not |
| 1c | `ff74de728` | `arm_group_key()` groups by `(src_digest, accel)`, deriving the digest from `commit` for rows written before the field existed |
| 2 | `f376cb8ff` | `check_arm_counts()` now takes the previous arm's `(pad, key)` and aborts the sweep the moment two arms disagree |

**Part 3 was answered by part 1, not skipped.** The planned warning was
"committing anything during this sweep will void it *under the current
fallback path*", and the plan already said it should "stop needing to say so
once part 1 lands." It has. A sweep that commits through itself now records
one digest across all six arms, so there is nothing left to warn about — and
printing a warning about a hazard that no longer exists is worse than
printing nothing, because the next reader will design around it. What replaced
it is part 2's message, which is not a warning about a possibility but a
report of an actual disagreement, naming both keys.

**The rescue was verified, not assumed.** `arm_group_key` on the six recorded
WHPX arms — six different commits, every one a documentation commit — returns
a single key, because `git rev-parse <c>:kernel` is `6876f9eb…` for all six.
The arms band. The ~70 minutes are not lost.

**Two things this fix quietly repaired that were not in the original report.**

1. *`dirty` stopped being a reason to drop a well-identified arm.* The entry's
   own postscript established that `dirty` cannot see untracked files, so it
   never meant what `layout_arms` used it for. Now `dirty` and `commit` are
   consulted **only** when a row carries no `src_digest`. For a row that does,
   the digest answers the identity question directly — including the embedded
   service binaries `dirty` was blind to — so the commit hash is provenance
   rather than identity. That removes the *second* silent way to void a
   three-hour sweep (the harness's own `bench/history.jsonl` write is what
   made later arms dirty), which was a separate voided sweep, not this one.
2. *The class of failure, not just this instance.* `arm_group_key` fixes the
   specific cause — a docs commit is no longer a new identity. It does not
   make silent fragmentation impossible: a real edit landing mid-sweep, a
   rebuilt service binary, a regenerated `rootfs.ext4`, or an accelerator that
   fell back to TCG on one run all still split a sweep in exactly the same
   way. Part 2 catches every one of those without enumerating any of them,
   because comparing consecutive keys asks the question directly instead of
   listing its causes. That is why part 2 was worth doing after part 1 rather
   than being made redundant by it.

**Still outstanding, deliberately: the end-to-end re-run.** The entry's last
paragraph asked for a sweep that *deliberately* commits through itself, on
the grounds that "a fix for a silent-fragmentation bug that has never been
observed to survive a commit mid-sweep is a fix on paper." That is still true
and the re-run has not happened. It is not being held open as a bug, because
the mechanism is covered from both ends by tests that do not need three hours:
`scripts/test-src-digest.py` exercises the digest's grouping and
discrimination against the real recorded rows, and the four controls tabulated
above were run against `bench/history.jsonl` as it actually exists. What the
re-run would add is confidence that the *live* recorder path writes the field
the parser reads, which the next benchmark boot demonstrates for free — so it
is tracked as a validation step on the next sweep rather than as an open
defect.


#### VALIDATION (2026-08-19, same day) -- the end-to-end re-run happened, both parts held, and it found a third cause

The paragraph immediately above says the re-run "has not happened" and is
"tracked as a validation step on the next sweep". It happened that afternoon.
Recording the outcome here rather than editing that paragraph, because what it
predicted and what it got are different in an instructive way.

**Part 1 held.** Three commits were made *deliberately* while the sweep ran --
`b3ee55d25` (design-decisions §240), `0d9da98c6` (a Q54 amendment) and
`a8a62afba` (a known-issues entry), all root `*.md`. The tracked half of the
digest was byte-identical across all three and the commit preceding them:
`tracked:11d8ab60978a1b56` at `41581ca30`, `b3ee55d25`, `0d9da98c6` and
`a8a62afba`. A documentation commit landing mid-sweep is no longer a new
identity. That is the exact claim that could only be made on paper before.

**Part 2 held, and earned its place.** The sweep still fragmented -- for a cause
neither part 1 nor the four controls could have covered -- and part 2 caught it
on the second arm and named it:

```
pad=0:    ('full:ace827eb370bec40', 'Hyper-V/WHPX')
pad=1024: ('full:f75959ab7b96d782', 'Hyper-V/WHPX')
```

Same accelerator; the source digest alone moved. It stopped after ~24 minutes
rather than spending ~72 on a band that could not form. Note that part 2's own
message already lists "a regenerated `rootfs.ext4`" among the causes it exists
to catch, and that is precisely what it caught -- written before anyone knew it
was happening.

**The third cause: the boot test was rewriting `rootfs.ext4`,** which is an
input to the digest's artifact half. Measured `f62e019d` -> `b2ecc74d` across
one boot on an idle tree. Fixed in `218028ced` by attaching the image with
`snapshot=on`. Full write-up in
`B-A-THE-BOOT-TEST-REWRITES-ROOTFS.EXT4, WHICH-IS-AN-INPUT-TO-THE-IDENTITY-OF-WHAT-IT-JUST-BOOTED`.

**What this says about the reasoning in the paragraph above.** That paragraph
argued the re-run was low-value because the mechanism was "covered from both
ends by tests that do not need three hours". The tests it cites are sound and
all still pass; they simply could not see this, because every one of them
reasons about `bench/history.jsonl` *as recorded* and the defect was in what the
act of booting does to the tree. The re-run was not confirming a fix -- it was
the only thing in the project capable of observing that testing the system
changes the system. The general lesson, which is worth more than the bug: a test
that only inspects recorded output cannot detect that the recorder has side
effects.
