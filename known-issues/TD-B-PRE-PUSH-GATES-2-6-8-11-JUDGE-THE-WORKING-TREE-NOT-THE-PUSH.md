## TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH

**Filed:** 2026-09-02 by Lane B, out of the gate-7 fix (`dcdd711fe`).
**Status:** open. Not blocking; every gate still fails safe in the common case.

**In short:** the push hook is supposed to answer "is the code I am about to
publish OK?" Seven of its eleven gates actually answer "is the code on my disk
right now OK?" Those are the same question until you commit something and then
keep editing, at which point the hook can wave through a bad commit — or block a
good one — and its own error text will confidently tell you it did neither.

### What is actually wrong

Every gate names the files it cares about from the pushed commit range
(`git log … HEAD --not --remotes=origin`). Only two of them originally then
*read* from that range; four more have been converted since, and the rest still
read the disk:

| Gate | Reads | How |
|---|---|---|
| 1 private-file | pushed tip | `git cat-file -e "$local_sha:$path"` |
| 9 request-deletion | pushed tip | checker takes `--head "$sha"` — the *waiver* half only from 2026-09-04, see step 14 |
| 7 rustfmt | pushed tip | mirror of pushed blobs — **fixed 2026-09-02** |
| 2 unreachable-command | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-02** |
| 3 raced-global | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-02** |
| 4 argv-utf8 | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-02** |
| 6 host-errmsg | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-03** |
| 11 doc-links | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-03** |
| 5 getopt-table | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-04** |
| 8 quote-names | pushed tip | `--head "$sha"` via the `Tree` seam — **fixed 2026-09-02** |

**Every gate in the table now reads the pushed tip.** The column above was
itself stale in both directions on 2026-09-04: it listed gate 8 as reading the
working tree two days after `f129bd5e0` and `bfe200154` converted it, and step 2
below said "one to go: gate 8" on the strength of that row. A table maintained
by hand alongside the thing it describes will do this; what saves it is that
step 3's `HEAD_GATES` is executable and cannot go stale the same way, which is
why the backfill described there is worth more than this row is.

The *conversion* half of this debt is therefore closed, and so, as of
2026-09-04, is step 4's — **every converted gate now has behavioural cases**,
checker-level and end-to-end (91 of them, across eight gates).

The paragraph that stood here said gates 8 and 9 were "wired correctly and
asserted to be wired correctly, but nothing has ever pushed a fixture past
either one, so there is no evidence that the flag changes what they *decide*.
A checker can accept `--head`, be called with it correctly, and ignore it."
Both were then covered on the same day and **both came back red on the first
run** — gate 8 over a missing baseline (step 13), gate 9 over a waiver read
from the wrong tree (step 14). Neither defect was predicted; both were found by
asking the gate a question with the commit and the worktree disagreeing. Two of
two is a small sample and still the whole argument for step 4.

Gate 7 is not on that list any more, and it is the proof the rest matter: it had
exactly this defect and it published two unformatted commits (`861f4d80e`,
`09a436956`) to `origin/lane-b` on 2026-09-02. Nobody predicted it; it was found
by diffing a published blob against its own rustfmt output.

Two failure shapes, both real:

- **False pass.** Commit a violation, fix it in the worktree, push without
  committing the fix. The gate reads the fix and approves the violation, which
  is then on origin forever. This is the one that happened.
- **False fail.** Commit clean code, start editing the same file, push. The gate
  reads your unfinished edit and blocks a commit that is fine. Every one of
  these gates prints some version of "this is never complaining about someone
  else's code" — which is true, and beside the point, because it is complaining
  about work you have not committed.

### Why gate 7's fix does not just get copied

Gate 7's unit of work is a **file**, so its fix was to materialise each pushed
blob into a temp mirror at its real relative path (~0.7 s per file, nothing for
files nobody touched) and hand rustfmt that. Files whose submodules are not in
the push get a one-byte stub, which is sound because *rustfmt's verdict on a
file does not depend on its children.*

That last clause is what does not generalise:

- **Gate 11 (doc-links) resolves names across a whole crate.** A link in `a.rs`
  is satisfied by a definition in `b.rs`. Stub out `b.rs` and the checker
  reports every link in the crate as dead — not a false fail at the margin, a
  gate that fails everything. It needs the *whole pushed crate*, not a file.
- **Gates 2–6 and 8** are per-file in principle, but each checker opens files
  itself and there is no seam to hand it bytes.

Whole-tree materialisation was measured on this machine before gate 7 was
written, and it is not viable as a per-push cost:

| Approach | Measured |
|---|---|
| `git archive HEAD` (whole tree) | 86 s, 204 MB |
| `git archive HEAD -- userspace/zip/src` (80 KB of output) | 23 s — archive walks the whole tree regardless of pathspec |
| `git archive HEAD -- posix/src` | 11 s |
| `cp -al posix/src <tmp>` | 53 s, and **fails outright** on this filesystem |
| `git cat-file blob` per file | ~0.7 s each |
| `git ls-tree -r` per directory | 1.5–4 s |

So per-crate mirroring for gate 11 costs ~0.7 s × the crate's file count:
`userspace/coreutils/src` is 124 files (~90 s), `posix/src` is 2305 (~27 min).
Unacceptable at the push boundary.

### What the proper fix looks like

Give the checkers the seam gate 9's checker already has.
`check-requests-not-deleted.py` takes `--head <sha>` and asks git about the
pushed commit rather than about the disk, and `test-pre-push-gates.py` has a
test asserting it is passed (`gate 9 passes --head so it judges the pushed
commit`) — so the pattern, the precedent and even the regression test all
exist. The work is:

> **Correction, 2026-09-04 (step 14).** Two things in that paragraph were
> wrong, and both mattered. The script is `check-requests-not-deleted.py`, not
> `check-request-deletion.py` — a name typed from memory into the document that
> nominates it as the model. And the precedent was only half a precedent: gate
> 9 read its *deletions* from the commit and its *waivers* off the disk, so the
> pattern being copied here had the very defect this task exists to remove. It
> was found by writing gate 9's own behavioural cases, months of pushes after
> the paragraph above called it the model to follow. A precedent nobody has
> tested is a claim, not a precedent.

1. ~~A shared helper — `scripts/gittree.py` — exposing `list(rev, pathspec)` and
   `read(rev, path)` over one long-lived `git cat-file --batch`, so N files cost
   one process rather than N × 0.7 s. This is the piece that makes the whole
   thing affordable and does not exist yet.~~ **Done 2026-09-02.** It exists,
   with `GitTree.read` / `read_many` / `list_paths` for the Python checkers and
   a `materialise` subcommand for the shell hook, which cannot import Python.
   Gate 7 uses it: 2568 files in 2m11s rather than ~15 minutes of `git cat-file`
   plus a `sed` each. `scripts/test-gittree.py` covers it (26 assertions), and
   `test-pre-push-fmt-gate.py` now runs every case twice — once batched, once
   over the per-file fallback — because the batched path shipped a `\r\n`
   defect that refused *every clean file in a push* and that only the batched
   run could see. Steps 2–4 are untouched.
   **Extended 2026-09-02** with the `Tree` seam the checkers are actually
   written against: `WorkTree` (the disk) and `RevTree` (a revision) behind one
   interface, chosen by `open_tree(root, head)`. The measurement that made
   step 2 possible at all is in `RevTree`'s docstring: 2,305 blobs of
   `posix/src` in **46 s** over the shared `git cat-file --batch`, against the
   ~27 minutes recorded in the table above — so gate 11 joins the conversion
   rather than being carved out of it. 97 assertions, and 12 mutants all
   caught, including both halves of the original bug.
2. Convert each checker's file access to go through it, with `--head <sha>`
   selecting git and its absence keeping today's filesystem walk (the checkers
   are also run by hand and by the boot test, where the working tree is right).
   **In progress.** Gate 2's `multicall-aliases.py` converted 2026-09-02;
   output on the working tree is byte-identical to before in both its modes.
   Gate 3's `raced-globals.py` converted 2026-09-02 the same way — the
   pre-conversion script was checked out *in place* and its `--check`, `--all`
   and `--selftest` output diffed byte-for-byte against the converted one, and
   then `--check --head HEAD` was diffed against the clean working-tree run:
   two independent code paths agreeing over ~14k files. It is also **faster**,
   not a tax: 6m18s before, 4m55s on the disk after, 3m49s against a revision.
   Gate 4's `argv-utf8.py` converted 2026-09-02 (step 8): `--check` and
   `--check --head HEAD` agree byte-for-byte on the real tree, `--selftest`
   still passes every rule it kept, and an unreadable revision exits 2.
   Gate 6's `host-errmsg.py` converted 2026-09-03 (step 9): both of its inputs
   — the `.rs` corpus and the ratchet baseline — go through the seam, `--check`
   on the real tree still exits 0, and `--selftest` reports 9/9.
   Gate 11's `check-doc-links.py` converted 2026-09-03 (step 10): all four of
   its tree-derived inputs — the crate list, each crate's unit list, the
   library's definition set, and the `Cargo.toml` dependency list — go through
   the seam; `rust_files()` is gone rather than left as a second, unseamed way
   to reach the disk. `--selftest` reports 57/57, `--check` on the working tree
   and `--check --head HEAD` both exit 0 on the real tree, and an unopenable
   revision exits 2.
   Gate 5's `getopt-ambiguity-check.py` converted 2026-09-04 (step 11): it is
   the only checker here whose comparison has just *one* tree in it — the other
   side is the live host's GNU utilities — so `--head` selects our half and the
   measurement stays a measurement.
   **Done — all eight are converted.** This line read "one to go: gate 8" until
   2026-09-04, when checking `git log` on the file rather than trusting the note
   showed gate 8 had been converted on 2026-09-02 by `f129bd5e0`/`bfe200154`.
   The note was written from the gate table above, which was stale, so the two
   agreed with each other and were both wrong — the reason the fix for *that* is
   step 3's executable table rather than a more carefully maintained prose one.
3. Pass `--head` from the hook, and extend `test-pre-push-gates.py`'s existing
   assertion to all eight gates instead of just gate 9. **In progress.** The
   assertion is now a table (`HEAD_GATES`) rather than a gate-9 special case,
   and it checks *every* non-selftest invocation of each listed checker rather
   than the file merely containing the flag somewhere. Gates 2 and 9 are in it.
   A second assertion pairs with it: a gate looping over `$pushed_shas` must
   also guard on that list being non-empty, because a loop over nothing runs
   the checker zero times while `note_gate` has already reported the gate as
   having run. Gates 3 and 4 joined both assertions 2026-09-02, gate 6
   2026-09-03, gates 8 and 11 backfilled 2026-09-04 and gate 5 on conversion the
   same day. The backfill is the point: 8 and 11 were wired with `--head "$sha"`
   on 2026-09-02 and 2026-09-03 and simply never added to `HEAD_GATES`, so for
   two days the assertion covered five of the seven converted gates and said
   nothing about the other two — an unlisted gate is not failed, it is never
   asked about, which reads exactly like a gate that passed. They were found by
   grepping the hook for `--head` and diffing that against the table by hand,
   which is the comparison the table exists to make unnecessary. The table now
   carries that history in a comment, above the rule that the commit adding
   `--head` to a gate adds its checker to the table.
4. Behavioural coverage, per `test-pre-push-fmt-gate.py`: for each gate, the
   false-pass and false-fail cases specifically. **Baseline cases are worthless
   here** — committed-clean-passes and committed-dirty-is-refused are green
   against the broken code, which is exactly why this survived in gate 7.
   **In progress**, in `scripts/test-checkers-honour-head.py`: every case
   builds a repository whose commit and worktree *disagree*, runs the checker
   twice against it — with and without `--head` — and requires the two verdicts
   to differ. Gate 2 has 7 cases; a first draft had 5 and four mutants survived
   it, because asserting that the *dispatch* comes from the revision leaves the
   checker free to answer "does anything produce this name?" from the disk, and
   that half decides the verdict just as completely. Whatever the next checker
   reads, every input it reads must be made to differ.
   **Extended 2026-09-02** past the checker to the hook. The seven cases above
   all invoke the checker directly, which leaves the hook↔checker seam untested
   — and *both* defects these gates have actually suffered lived in that seam
   (gate 11 handed its scope to argv and died of a length limit; gate 7
   enumerated the commit and read the disk). Three cases now push for real,
   through the real hook in `.git/hooks/`, against a commit and a worktree that
   disagree. Verified by mutating the wiring: dropping `--head "$sha"`, pinning
   it to `--head HEAD`, iterating the `$pushed_shas` loop over nothing,
   discarding the checker's exit status, and skipping the gate outright are all
   caught. Two traps found while writing them, both recorded in the suite: an
   empty directory is a legitimate `WorkTree`/`RevTree` divergence (git has no
   empty directories), and a fixture whose *directory name* contains the alias
   makes `"<alias>" in output` true while the gate has skipped — `_push` now
   redacts the fixture's paths so no case can pass that way.
   **Gates 8 and 9 covered 2026-09-04** (steps 13 and 14), which completes the
   set: **every converted gate now has behavioural cases, checker-level and
   end-to-end.** The per-gate floors in `test-checkers-honour-head.py` are the
   live record of which gates are covered, and they are executable, so unlike
   the hand-maintained table in step 2 they cannot quietly go stale; the count
   is 91 across eight gates.

5. **The path scope itself had the same defect, one level up.** Six gates decide
   whether to run at all through the hook's `touches()` helper, which asked
   `git rev-list HEAD --not --remotes=origin -- <paths>`. That is a question
   about the branch that happens to be checked out. `git push origin feature`
   while standing on `main` is an ordinary thing to do, and in that push HEAD's
   commits are all already on the remote — so `touches` found nothing under
   `userspace/`, every gate scoped by it skipped itself, and `feature`'s
   contents went out unjudged. Silently: the tally printed `skipped`, which is
   also what an honest documentation-only push prints, so the two are
   indistinguishable from the output. **Fixed 2026-09-02**: the helper now
   scopes by `$pushed_shas`, the same list the converted gates judge. Pinned by
   `test-checkers-honour-head.py`'s off-branch push case and by
   `test-pre-push-gates.py` →
   `test_the_path_scope_is_taken_from_the_push_not_from_head`, which also
   forbids unioning HEAD back in (`rev-list A B` is the union, so mentioning
   `$pushed_shas` is not on its own enough). Found by mutation testing, not by
   review: `--head HEAD` survived every end-to-end case, and the reason it did
   was that no case could push a branch it was not standing on — because
   `touches` would have skipped the gate.

6. **Gate 3 converted, 2026-09-02.** `raced-globals.py` now takes `--head` and
   reads *four* inputs through the seam, each of which decides its verdict on
   its own: the `.rs` source, the baseline that forgives, the `Cargo.toml` that
   says whether the crate's tests can run at all, and the set of files that
   exist. The manifest is the dangerous one — a crate with no test target is
   silenced *entirely*, races and all, so an uncommitted `test = false` read off
   the disk would drop a committed race on the floor without mentioning it.
   Twelve gate-3 cases and four end-to-end pushes in
   `test-checkers-honour-head.py` (23 cases, 68 assertions overall), and the
   conversion was mutation-verified with sixteen mutants: every way of reading
   one of those four inputs off the disk, each of the five short-circuiting
   probes inside `crate_has_test_target` separately (they cannot share a
   fixture — the first that finds a target answers and the rest are never
   reached), exit 1 instead of 2 on an unreadable revision, the checker's skip
   list not reaching the seam, and three wiring mutants. Two things the run
   found that review had not:

   * **A `for sha in $pushed_shas` loop cannot be distinguished from
     `git rev-parse HEAD` by any case that pushes the branch it is standing
     on** — which was every case at first. This is the same blind spot that hid
     the `touches` defect in step 5, and it is per-gate: gate 2's off-branch
     case says nothing about gate 3's loop. Every converted gate therefore needs
     its own off-branch push case, and gate 3 now has one.
   * **Gate 3's `[ -n "${pushed_shas# }" ]` guard is currently unobservable.**
     Now that `touches` is scoped by `$pushed_shas`, an empty list already makes
     `touches` false and skips the gate, so removing the guard changes no
     outcome and no behavioural case can kill that mutant. It is kept rather
     than deleted as dead code, and the reasoning is recorded in the suite: the
     redundancy is exactly one edit deep, since `touches` was HEAD-scoped until
     this same day and under that spelling a branch-deletion push reached the
     loop with nothing in it. It stays pinned statically by
     `test-pre-push-gates.py`. The push shape that would exercise it — deleting
     a remote branch — now has an end-to-end case anyway, asserting the two
     things that must hold however they are delivered: the deletion is allowed
     however broken the working tree is, and the tally reports the gate
     `skipped` rather than `ran`.

7. **The seam itself was pruning the wrong set, found while starting gate 4**
   (`gittree.py`, 2026-09-02). `_PRUNE` was `("target", ".git")` — but
   `.gitignore` also carries `**/target-*/`, the alternate cargo build dirs
   (`target-lint`, `target-test`, `target-hl2`, `target-probe`) that a lane
   creates whenever it needs a second build that will not fight the first for
   cargo's lock. Those are gitignored, so a `RevTree` can never list one, and
   they are on the disk while a build is running, so a `WorkTree` listed all of
   them. That is the seam breaking its own central promise — and in the worst
   available way, because whether it happens depends on **whether another lane
   is running clippy at that moment**. A gate that judges a commit one way at
   10:00 and the other way at 10:05, with no commit in between, is a gate
   nobody can act on.

   It was not hypothetical for gate 4: `argv-utf8.py` prunes `target-*` by hand
   today, so moving it onto the seam would have *lost* a rule it already had.
   Gate 3 shipped earlier the same day with the defect latent — `raced-globals`
   walks the whole repo through `files_under("", prune=SKIP_DIRS)` and
   `SKIP_DIRS` has no `target-*` either.

   The fix mirrors `.gitignore` rather than enumerating names, for the reason
   `.gitignore`'s own comment gives: "Neither the count nor the names are
   principled … so match the shape instead of enumerating the instances."
   `_PRUNE_DIR_PREFIX = ("target-",)`, applied through a single `_prune_dir`
   so the walk (which prunes by directory name) and the filter (which prunes by
   path) cannot drift apart.

   **That pattern's trailing slash is load-bearing, and getting it wrong would
   have been worse than the bug.** It matches directories only, so a
   tracked file `posix/src/target-arch.rs` is source and must survive. A rule
   that pruned the prefix wherever it appeared would hide such a file from
   *both* sides at once — which the seam's headline equality assertion cannot
   see, because both sides would be equally wrong. Hence the split into
   `_pruned` (a file path: the prefix family applies to every component but the
   last) and `_pruned_prefix` (a directory prefix: every component), and a
   `files_under` that answers a file-named prefix by file rules *first*. There
   are no such files today — measured across all 13846 tracked paths — which is
   the argument for fixing the rule while it is cheap, not for skipping it.

   Mutation-verified with ten mutants against `test-gittree.py`; nine caught
   after the suite was strengthened three times. What the run found:

   * **Two survivors were "same answer, different work."** Reverting the walk
     prune to exact names, and reverting `WorkTree`'s prefix guard to file
     rules, both still answer `[]` — the per-file filter drops whatever the
     walk turns up. Only the descent differs, and on this repository a
     `target/` is tens of gigabytes inside a push gate. No assertion about
     return values can see either, so `case_the_walk_never_descends_into_build_output`
     records the directories `os.walk` actually yields. Note a call *count*
     is not enough: `os.walk` is invoked once per `files_under` however deep it
     goes, which is why the existing probe missed this.
   * **One survivor was a missing fixture**, not a missing assertion: nothing
     in the tree had a directory whose name merely *contains* `target-`, so a
     substring spelling of the rule passed everything. `cross-target-tests/` is
     now committed in the fixture for that one mutant.
   * **One survivor is genuinely equivalent and is documented, not removed.**
     `RevTree`'s guard cannot tell `_pruned_prefix` from `_pruned`, because a
     prefix whose last component starts with `target-` is already decided
     before the guard runs — either it names a tracked file and the `_fileset`
     branch answers first, or `__init__` has already dropped everything under
     it from the index. The guard stays: it is the correct call if the index
     filter changes, and the seam's rule is that both implementations spell the
     same thing the same way. The equivalence rests on the index being
     pre-filtered, which the suite asserts separately, so it fails loudly
     rather than silently if that stops holding.

   Behaviour on the real tree is unchanged and that was checked rather than
   assumed: the only gitignored directories present are `target`, `build`,
   `limine` and `toolchain/sysroot`, and no tracked path has a `target-`
   component — so the change is a no-op today and matters only while a lane is
   mid-build, which is exactly when it was unobservable before.

8. **Gate 4 converted, 2026-09-02.** `argv-utf8.py` now takes `--head` and reads
   four inputs through the seam, each of which can decide the verdict alone: the
   `.rs` enumeration under `userspace/coreutils`, each file's source text, the
   baseline that forgives, and the ungated survey of everything else under
   `userspace/`. The baseline is the sharp one here — read off the disk it
   silences a finding in a commit that does not contain the silencing line, so
   an uncommitted baseline edit publishes a panicking utility *and* the gate
   stays green on every later push, because by then the edit is committed.

   There is a fifth thing to get right that is not an input: `stale_entries`,
   the direction the ratchet **shrinks** in. Every case that pins `new` is
   satisfied by a checker that gets the new half right and answers the stale
   half from the disk, so this is the half a conversion is likeliest to leave
   behind. It has its own case, asserting `FIXED <key>` on the revision arm and
   no `FIXED` at all on the disk arm.

   The hand-rolled directory walk was **deleted rather than ported**. It carried
   its own copy of the "skip build output" rule, and the copy had already
   drifted — the drift is step 7 above, found because moving gate 4 onto the
   seam would otherwise have *lost* a rule the checker already had.

   **A `--selftest` rule turned out to be in the wrong place, and the fixtures
   are what proved it.** The eighth rule asserted `len(rust_files(disk,
   GATED_REL)) > 50` — a guard against the gate quietly losing its subject, which
   is the exact failure this tool exists to prevent: a clean report produced by
   accident. It had two defects. It asked the **disk** even on a run judging a
   revision; and a threshold of fifty is a claim about *this* checkout, so the
   checker could not be self-tested anywhere else — which is precisely what its
   own new end-to-end fixtures do, and they failed. It now lives in `main()` as
   `_no_corpus(tree)`, asked of whichever tree is under judgement, testing
   non-emptiness rather than a count, and exiting **2**: the gate has lost its
   subject, which is not a finding about anybody's code, and printing gate 4's
   refusal over it would tell an author their utility panics on a legal
   filename when nothing of the sort was observed. The relocation is a real fix
   and not fixture accommodation — a commit that renames `userspace/coreutils`
   away disarms gate 4 *for that commit*, and the old rule would have answered
   from a working tree that still had the directory.

   **One of the new end-to-end cases passed for the wrong reason first, and a
   sub-assertion is the only thing that caught it.** `_push` discriminates
   markers per *gate*, not per *reason*, so the self-test failure above printed
   gate 4's refusal text and the verdict assertion went green while the gate had
   never judged anything. `"tool.rs" in blob` is what failed. That is the
   standing argument for pairing every verdict assertion with a content one: a
   refusal is evidence that *a* gate refused, never evidence of *why*.

   Twelve gate-4 cases — nine direct, three end-to-end, including its own
   off-branch push per step 6 — bringing `test-checkers-honour-head.py` to 35.

   **Mutation-verified with fourteen mutants: thirteen caught behaviourally,
   and the fourteenth is gate 3's documented equivalent.** Each mutant is a way
   of converting the checker to `--head` while still answering some part of the
   question from the disk, which is precisely the defect the conversion removes.
   Every input got one (source text, baseline, file enumeration, ungated
   survey); so did the walk being hand-rolled again and forgetting build output,
   `--head` accepted-and-ignored, both no-verdict exits demoted from 2 to 1, all
   three ways to break the relocated corpus guard (ask the disk, exit 1, drop
   it), the stale half of the ratchet, and three wiring mutants in the hook.

   The harness reports a mutant caught only by `test-pre-push-gates.py` as
   `STATIC-ONLY` rather than as caught, because the static suite matches on the
   hook's *text* and would not notice the same mistake made anywhere else. One
   mutant landed there: **dropping gate 4's `[ -n "${pushed_shas# }" ]` guard**
   — the same result gate 3 got, for the same reason, and it is worth being
   explicit that this is a *derived* equivalence rather than a second
   observation. Since step 5 rescoped `touches` by `$pushed_shas`, an empty list
   already makes `touches` false and skips the gate, so the guard cannot change
   an outcome and no behavioural case can kill it. It is kept, not deleted as
   dead code: the redundancy is exactly one edit deep, and under the spelling
   `touches` had until the morning of the same day, a branch-deletion push
   reached the loop with nothing in it. It stays pinned statically.

   Nothing survived outright, which is a first for these conversions — gate 2's
   first draft lost four mutants and gate 3's found two blind spots. The reason
   is that both of those runs produced rules that were written down (step 6's
   "every converted gate needs its own off-branch push case", step 4's "every
   input it reads must be made to differ"), and gate 4's cases were written to
   those rules before the harness ran rather than after it complained.

9. **Gate 6 converted, 2026-09-03.** `host-errmsg.py` now takes `--head` and
   reads both of its inputs through the seam: the `.rs` corpus under
   `userspace/coreutils` (enumeration *and* source text) and the ratchet
   baseline `scripts/host-errmsg-baseline.txt`. Fewer inputs than gate 4, and
   the conversion is correspondingly smaller — but its **guard covers one more
   of them**, and that is the part worth reading before converting gate 5 or 8.

   A baseline the seam cannot read comes back as an *empty backlog*, not as an
   error. So an unreadable one makes `--check` call every baselined bin NEW and
   refuse the push with a paragraph each, all of them naming bins the author
   did not touch — and on a clean tree the same read calls every baseline line
   stale instead. The refusal is not merely wrong, it is wrong in a way that
   hides its own cause: the sentence it prints is about someone's error
   message, and the actual fault is that a file moved. How *loud* it is scales
   with the ratchet's current length (two lines today, and it only ever
   shrinks); *whether* it happens does not. So `_inputs_missing` asks about both
   inputs, of whichever tree is under judgement, before anything is reported,
   and exits **2** — `run-checker.sh`'s no-verdict arm — rather than 1.

   **Gate 4 has the corpus half of that guard and not the baseline half**
   (`argv-utf8.py`, `_no_corpus`; its `load_baseline` still returns an empty
   set for an unreadable baseline with nothing said). Same defect, four
   baseline lines rather than two. Filed as
   `TD-B-GATE-4-CANNOT-TELL-AN-EMPTY-BACKLOG-FROM-A-MISSING-ONE` rather than
   fixed here — it needs its own honour-head case and its own mutant to be
   worth anything, and neither belongs in a commit about gate 6 — and **fixed
   the same day** in the commit after next, which also renamed gate 4's
   `_no_corpus` to `_inputs_missing` so the two gates spell one rule one way.
   Gate 11 (step 10) gets the **corpus** half and not the baseline half,
   because it has no baseline — it is a pure cross-reference checker with
   nothing to ratchet. See step 10 for why "zero crates in scope" must stay a
   *pass* there while "zero crates in the tree at all" is a no-verdict.
   Gates 5 and 8 get **both** halves when they are converted.

   `needs_baseline` is false for two modes and both exclusions are load-bearing
   in opposite directions: `--write-baseline` *creates* the file and must run
   without it, and `--list` never consults it. Both are pinned by assertions in
   the same case, so neither can be quietly widened to "always" or dropped to
   "never".

   **The `--selftest` rule that was in the wrong place in gate 4 was in the
   wrong place here too, and again the end-to-end fixtures are what proved it**
   — this time by failing four cases outright. Rule 8,
   `gated-tree-is-not-empty`, asserted `len(rust_files(tree, GATED_REL)) > 50`
   against a `WorkTree` of `ROOT`: the disk, on a run that may be judging a
   revision, and a threshold that is a claim about *this* checkout, so the
   checker could not be self-tested in a fixture at all. That is step 8's
   defect verbatim, in a file written before step 8 existed. It is deleted, and
   a comment in its place records both halves and cites `argv-utf8.py`'s
   `_no_corpus` as the precedent, so the next conversion does not rediscover it
   a third time.

   Fourteen gate-6 cases — eleven direct, three end-to-end including its own
   off-branch push per step 6 — bringing `test-checkers-honour-head.py` to 49.
   Two are new shapes rather than translations of gate 4's:

   * **The `--list` case counts *sites*, not files, and its fixture has two
     sites in one file on purpose.** `--list` is the mode no exit code depends
     on and the one the backlog is read from, so nothing else in the suite
     covers it. A one-site fixture would make the site count and the file count
     agree, and the case would then still pass against a `--list` that had
     quietly started counting the disk's files. Mutation confirmed this is the
     assertion that catches exactly that.
   * **The refusal marker is deliberately not the obvious phrase.** "prints the
     host's error text" also appears in the *checker's own FIX advice*, which a
     `--check` run prints on a finding the hook then goes on to allow. Matching
     it would score a fixture as refused on the strength of text from a gate
     that did not refuse it — step 8's "a refusal is evidence that *a* gate
     refused, never evidence of *why*", one level finer. The marker is the
     refusal sentence proper.

   **Mutation-verified with five mutants, all caught:** the baseline read off
   the disk (1 assertion fired), the corpus guard removed (2), the baseline
   guard removed (2), `--list` walking the disk (1), and `--head` accepted and
   ignored (21). The source was restored and re-checked afterwards rather than
   assumed — `--selftest` back to 9/9.

10. **Gate 11 converted, 2026-09-03.** `check-doc-links.py` now takes `--head`
    and reads **four** tree-derived inputs through the seam, more than any
    previous conversion: the crate list (`crate_roots`), each crate's unit list
    (`units`, the `src/bin/*.rs` set plus `src/lib.rs`), the definition set
    parsed out of the library's own text (`definitions`), and the dependency
    names in `Cargo.toml` (`dependencies`). Miss any one and the checker still
    runs and still prints a verdict — it just judges a *blend* of two trees,
    which is the failure mode with no symptom. `rust_files()` was deleted
    rather than left converted-but-unused: an unseamed helper that still
    reaches the disk is a loaded gun for the next edit.

    **The hook side is where the real bug was, and a test caught it.** The
    scope list is derived from `$pushed_shas`, and my first draft put that
    expansion inside the pre-existing `IFS='\n'` block that the old
    `[ -d ]`-filtering loop needed. A space-separated list of shas then arrived
    at git as one unsplit word, git matched nothing, the list came out empty,
    and **the gate skipped itself on every push** — the exact class of silent
    self-disablement this whole entry is about, reintroduced by the fix for it.
    `test-pre-push-doclinks-gate.py` failed 2 of 13 within a minute of the
    edit. The loop is gone entirely now (it existed only for the `[ -d ]`
    filter, which is itself a working-tree read and had to go), git's output is
    piped straight into the scope file, and a comment records the trap.

    **The `[ -d ]` filter was a working-tree read hiding inside the hook.** It
    dropped any directory absent from the disk, so a directory added by the
    push and then deleted from the worktree would silently leave the scope and
    its crate would go unscanned. Removing it is safe because the *checker*
    answers that question now: `crates_touching` maps a path to a crate using
    the revision's own crate roots, so a path matching none of them simply
    contributes nothing.

    **Scope is the union across the whole push, not per-sha.** A crate named by
    a later commit is worth scanning at an earlier one too, and a directory
    that does not exist at the sha being scanned matches no crate root there
    and costs nothing. The gate name is per-sha (`doc-links-$sha`) for gate 9's
    reason: a shared name lets a later clean sha delete the kept log of an
    earlier failing one.

    **The `_inputs_missing` rule applies by half here.** Gate 11 has no
    baseline — it is a pure cross-reference checker with nothing to ratchet —
    so only the corpus half is meaningful, and it has to be spelled carefully:
    `crate_roots(tree) == []` (no lane-B crates in the tree *at all*) is a
    no-verdict and exits 2, but "zero crates **in scope**" must stay a **pass**,
    because a push touching only `kernel/` legitimately maps to no lane-B crate
    and refusing it would be a false fail on every lane-A and lane-C push.

    Eleven behavioural cases in `test-checkers-honour-head.py` (three of them
    end-to-end through the hook, including an off-branch push per step 6's
    rule), plus the pre-existing 13-case `test-pre-push-doclinks-gate.py`,
    which now copies `gittree.py` into its fixture repos alongside the checker.
    The suite's floors moved with them — 61 cases overall, 11 for gate 11 — so
    deleting a case is as loud as deleting the wiring it covers.

11. **Gate 5 converted, 2026-09-04.** `getopt-ambiguity-check.py` now takes
    `--head` and reads both of its tree-derived inputs through the seam: the
    enumeration of `userspace/coreutils/src/bin` and each table's source text.

    **It is the odd one out, and worth saying why.** Every other checker in this
    entry compares a tree against a *file* — a baseline, a manifest, another
    module's text — so both sides of the comparison move together when `--head`
    selects a revision. This one compares a tree against **the machine**: it
    reads GNU's own option table out of `<util> --=x` over WSL. Only our side
    has a revision to read, and that asymmetry is the whole content of the
    conversion. It also bounds what the flag can break: a wrong answer here is
    always "we judged the wrong copy of *our* table", never a mismeasurement of
    GNU's.

    **The scope had the same defect as the content, and it outlived it.** Gate 5
    is scoped by utility *name* rather than by path — each name is a WSL round
    trip, ~2 s against ~35 s for a full sweep — so it never went through
    `touches()` and kept its own private copy of `git rev-list HEAD --not
    --remotes`. Step 5 fixed `touches()` on 2026-09-02 and this copy went on
    asking about HEAD for two more days. Here the consequence is worse than a
    wrong answer, because an empty scope is this gate's **skip** condition:
    `git push origin feature` while standing on `main` produced an empty list,
    which read as "this push rewrites no table", so the gate stood down and
    said so in the tally — while being correctly wired to `--head` and pointed
    at exactly the right commit. **Reading the right revision does not help when
    the list of things to read in it came from somewhere else.** The derivation
    is now `getopt_scope_for <sha>`, called per pushed ref inside the same loop
    that judges it, and pinned by `test_gate_5_derives_its_own_scope_from_the_push_too`
    (the helper must name `"$1"` and must not mention HEAD at all).

    Two structural consequences of making the scope per-ref rather than
    per-push. The `--selftest` run moved *inside* the loop behind a
    `getopt_ran` flag, so it runs once when there is anything to judge and not
    at all when there is not; and `note_gate` moved *after* the loop, reporting
    what ran rather than predicting it. The prediction was a second place the
    tally could disagree with reality, which is the failure the
    `[ -n "${pushed_shas# }" ]` guard exists to prevent — a tally derived from
    the run cannot drift from it.

    Seven behavioural cases in `test-checkers-honour-head.py`, three of them
    end-to-end through the real hook including the off-branch push step 6
    requires. The off-branch case is the one that earns its keep: with the
    scope reverted to HEAD it reports `allowed` for a branch carrying a table
    with `--version` deleted, which is the defect above, observed rather than
    argued. `yes` is the fixture utility because its table is two entries that
    have not moved in decades, so a red case means the seam broke rather than
    that a distribution shipped a different coreutils; a host with no GNU
    userland skips the group loudly instead of passing it vacuously.

    Floors moved to 68 overall and 7 for gate 5. **Gates 8 and 9 are
    deliberately absent from that per-gate table**, and a comment there says so:
    both are wired with `--head` and both are in `HEAD_GATES`, but no case has
    ever pushed a fixture past either, so nothing shows the flag changes what
    they decide. A checker can accept `--head`, be called with it correctly, and
    ignore it. Adding a floor for them would make the table look complete while
    guarding nothing.

12. **Gate 8 could not tell a clean tree from an empty one, 2026-09-04.** Found
    while going to *convert* gate 8 and discovering it had been converted two
    days earlier (see step 2). With the conversion already done, the useful
    question became whether the converted gate was correct, and it was not, in a
    way the conversion had made reachable: `--head` can now be pointed at any
    revision, including one where lane B's zone does not exist.

    `survey()` returned only the findings, so an empty result had two causes
    that were indistinguishable from outside — a tree with no defects, and a
    tree with no *sources*. Measured on a commit that renamed `userspace/` away,
    against a baseline that still listed a file in it:

    ```
    fixed: userspace/coreutils/src/bin/cut.rs 1 -> 0
    ok -- 0 known sites in 0 files (1 improved)
    ```

    exit 0. Note what that is worse than: it is not merely a bad push passing.
    The gate reported the disappearance of its own subject as a *burn-down*, and
    the wording (`run --write-baseline to record it`) invites the reader to
    discard every site the ratchet was guarding. A ratchet that offers to forget
    its own backlog is the worst available failure for a ratchet.

    The fix is `Survey(found, scanned)` — the count of files actually read comes
    back from the same walk that produced the findings, because deriving it from
    a second walk would be a second answer to one question, which is the exact
    shape of drift the `Tree` seam was introduced to remove. `_no_corpus` then
    refuses on both the `--head` and the working-tree path, with **exit 2**: 1 is
    `run-checker.sh`'s "the checker found something in your code", and printing
    gate 8's refusal for an empty tree would tell an author their diagnostics
    leak file names when nothing whatever was observed.

    Deliberately not a heuristic threshold. Gate 4's equivalent originally
    asserted `> 50` files, which is a claim about the size of *this* checkout and
    made the rule unrunnable anywhere else; this asks for one file, which is a
    claim about the gate having a subject and holds in a three-file fixture.
    That is what let the five new self-test cases exercise it for real rather
    than assert it in the abstract.

    Two of those cases are worth naming. The **control** — a fixture tree that
    has a source and no violations must *not* trip the guard — carries as much
    weight as the experiment, because the failure it excludes is the one that
    gets a gate switched off: a gate that refuses on every host. And the last
    case pins, on purpose, that `check()` on its own still reads an emptied
    corpus as `1 improved` and exits 0; if that is ever fixed inside `check`, the
    case fails and its comment says to delete itself *and* `_no_corpus` together
    rather than leave two answers standing.

    End to end on a real revision rather than only a fixture: this repository's
    own root commit (`7527d40a0`) predates all four of `ROOTS`, and
    `--check --head 7527d40a0` now exits 2 with the refusal where it would have
    exited 0 with a pass.

    This is a defect in *correctness*, not coverage — it does not close step 4's
    gap. Gate 8 still has no case proving `--head` changes what it decides.

13. **Gate 8 given behavioural coverage, 2026-09-04 — and the first run was
    red.** Fourteen cases in `test-checkers-honour-head.py`, eleven at the
    checker and three through the real hook, each input made to differ between
    the commit and the worktree separately: the `.rs` enumeration, each file's
    text, the baseline's entries, the baseline's *counts*, the `fixed:`
    direction, the `target/` skip, both routes to exit 2, and an off-branch
    push. The count case is the one that is particular to this gate — its
    ratchet's granularity is a digit per file rather than a named finding, so a
    case that only swaps entries in and out never reaches the value that
    actually decides the verdict.

    Two of the fourteen failed on the first run, and what they found is the
    point of the exercise. `read_baseline_from` returned `{}` for a baseline
    that was **not in the tree**, under a comment calling that "the safe
    direction — it can only over-report". Over-reporting is not the safe
    direction. A commit that moved `quote-names-baseline.txt` would have been
    refused with gate 8's whole refusal over every site the real file forgives —
    1798 diagnostics across 777 files nobody touched — which is the false
    accusation `scripts/run-checker.sh` exists to argue is the worst thing a
    gate can do, and the reliable way to get a gate bypassed. On a clean tree
    the same read calls every entry stale instead, printing "the backlog is
    fixed" over a commit that fixed nothing. Gates 4 and 6 both carry this
    guard; gate 8 shipped its conversion with the corpus half and not this half.

    Fixed by making absence `None` rather than `{}` — the live baseline is
    legitimately *empty of entries*, so the two states must be distinguishable —
    and adding `_no_baseline` beside `_no_corpus`, exit 2 for the same reason.
    `check()`'s `baseline` argument became required in the same change: its old
    `None` default meant "read it yourself", which would now be the same
    sentinel as "there is nothing there" at a single call site. The guard sits
    after `--write-baseline` (the mode whose job is to create the file) and
    after the plain `report` path (which does not consult the ratchet and exists
    to survey trees nobody has ratcheted yet).

    **The lesson is about the two kinds of assertion, and it is the reason this
    is written down rather than just fixed.** Gate 8 was wired on 2026-09-02,
    listed in `HEAD_GATES`, and green for two days. That assertion is about the
    *shape* of the invocation and it was telling the truth the whole time. It
    simply cannot see a checker that takes `--head`, is handed the right sha,
    and then answers from somewhere else — or, as here, one that reads the right
    tree and mishandles what it finds. A summary that says "gate 8: wired and
    asserted" reads as covered and is not. The floors table in
    `test-checkers-honour-head.py` now carries this history where the next
    person to add a gate will read it.

    Gate 9 was the only converted gate left with no behavioural case at all, and
    the same caveat applied to it verbatim. Step 14 is what happened when it was
    finally asked.

14. **Gate 9 covered, 2026-09-04 — and its `--head` conversion turned out to
    have carried only one of its two inputs.** Nine checker-level cases and
    three end-to-end, written on the same day and for the same reason as gate
    8's. One came back red on the first run:

    ```
    FAIL  gate 9: an uncommitted waiver does not excuse a committed deletion
            got : 0
            want: 1
    ```

    `check-requests-not-deleted.py` asks one question — "does this tree delete a
    request it does not waive?" — and it has two inputs. `deleted_since` took
    `head` in the 2026-09-02 conversion and read the deletions from the commit
    being pushed. `load_allowlist` did not, and went on reading
    `requests/.deletions-allowed` off the disk through a module-level
    `ALLOWLIST` path bound at import time. So for two days the gate read its
    deletions from the commit and its *permissions* from the working tree.

    **That is a worse hole than the one `--head` was added to close.** The
    staged-restore false negative at least leaves the file present somewhere; a
    disk-only waiver leaves nothing at all. Write the basename into
    `.deletions-allowed`, push the deletion, `git checkout` the waiver away, and
    the request is gone from shared history with no record that an override was
    ever claimed — which defeats the exact property the escape hatch is *for*,
    since "a deliberate, reviewable act" is a claim about something published.
    The hook already argued the point and enforced the wrong half of it: gate 9's
    `touches` scope includes `requests/.deletions-allowed` so that "editing the
    waiver list must be the push that re-verifies it". Re-verifying it against
    the working tree's copy is not that.

    Fixed by giving `load_allowlist` the same `head` parameter its sibling has,
    reading the blob through `gittree.GitTree.read` — *not* `open_tree`: this is
    one small file, and `RevTree` would build a `git ls-tree -r` index of all
    13,821 paths (~3.8 s) on every push to answer about one path that is usually
    absent. `GitTree.read` is the same seam without the index and already spells
    absence as `None`. The parse was split into `_parse_allowlist` so a waiver
    cannot mean one thing on the disk and another in a commit, and the
    module-level `ALLOWLIST` path became a repo-relative `ALLOWLIST_REL`
    constant — a `ROOT /` path can only ever name the disk, and it was also the
    second global the self-test had to repoint, which is one more way for a
    fixture to read the real repository.

    Note the asymmetry with gate 8, since it is deliberate: an absent allowlist
    waives nothing and so fails *toward strict*, whereas an absent
    `quote-names-baseline.txt` would forgive nothing and therefore accuse
    everything. That is why gate 8 refuses to return a verdict without its file
    (step 13) and gate 9 is content to carry on without its own.

    Four self-test cases pin the provenance: an uncommitted waiver is invisible
    to `--head`; a committed one is not (the control, without which a reader
    returning `{}` for every revision would pass while refusing every legitimate
    sweep); a waiver is not backdated onto the commit before it; and a tree with
    no allowlist waives nothing.

    Gate 9's end-to-end cases are worth their cost for a reason unique to it: it
    is the only gate in the hook that calls its checker **once per pushed sha**,
    so it has a failure mode no other gate's case can see — a `$pushed_shas`
    loop that iterates zero times prints nothing, refuses nothing, and still
    reports the gate as having run.

### Why it is not done yet

It is eight checkers, a new shared module and a per-gate behavioural suite, on
the one file all three lanes push through — a scope that deserves its own task
rather than being tacked onto a one-gate fix, and a blast radius (a mistake
blocks every lane's pushes) that argues for doing it deliberately. Gate 7 was
split out and landed alone because it had a *proven* escape, not merely a
possible one.

**If it is never done:** the gates keep working correctly whenever the worktree
matches the commit, which is most pushes. The exposure is bounded and does not
grow with time. It bites exactly when someone commits, tidies, and pushes
without committing the tidy — which is a common enough sequence that it has
already cost this project two published-unformatted commits.
