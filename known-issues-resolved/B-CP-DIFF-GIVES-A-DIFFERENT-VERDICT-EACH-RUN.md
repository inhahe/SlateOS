## B-CP-DIFF-GIVES-A-DIFFERENT-VERDICT-EACH-RUN (lane B, 2026-09-12)

**Status: FIXED 2026-09-12.** The harness now checks the ordering premise it
rests on instead of assuming it, and reports a third verdict, `UNSTABLE`, when
it fails. Four consecutive runs since: 581 passed, 0 differed, 0 unstable, the
same three numbers every time — against 0, 1, 1, 1, 3 and 4 differences before.

**The cause was the inode allocator, not either program.** Both `cp`s sort a
directory by inode, so the comparison is only meaningful if the two private
copies allocate in the same relative order. They nearly always do. Across 611
cases that each create and delete a tree, a freed inode need not be handed back
in the order it was freed, and then they do not.

Measured before concluding: two sibling trees built by the same command
sequence agreed on inode order 3000 times out of 3000 under light churn, and
raw `readdir` order agreed 2000 out of 2000 — which also showed that readdir
here returns *name* order, not the creation order the `mktree` comment asserts.
The sort happens afterwards; the premise is about allocation, not enumeration.

**The verdict gates attribution, not the case, and the order of those two
tests is the whole fix.** A first draft asked about inode order first, and
reported `UNSTABLE` for five single-file copies — `cp --preserve= file.txt
new.txt` among them — whose outcome cannot depend on a walk order at all. Five
cases that had agreed perfectly stopped counting as evidence. If the two sides
agree, the premise did not bite whatever the allocator did; it is only when
they disagree that "could this be the fixture rather than the program?" is a
question worth asking.

**`DIFF_CP_SKEW=1` proves the branch fires**, because otherwise `0 UNSTABLE`
and `that branch is unreachable` print the same thing. It recreates one fixture
entry on the GNU side so the two sides genuinely enumerate differently: 92
UNSTABLE, 0 differences among the recursive cases. Two things had to be right
for that mode to work at all, and neither was on the first attempt:

* **The decoy.** Deleting `a.txt` and writing it straight back changes nothing
  — ext4 returns the just-freed inode to the next allocation, so the order is
  identical. A decoy file must take that slot first.
* **The name.** It was `CP_DIFF_SKEW`, and the preamble forwards only `DIFF_*`
  names across the WSL boundary, so it never arrived. The mode reported `0
  UNSTABLE` and looked like a working test finding nothing — the same shape as
  the `DIFF_PKG` forwarding bug fixed earlier in this tree.

Both failures printed a plausible green. That is the argument for the mode
existing at all.

The description below is kept in the tense it was written in.

**What.** `scripts/cp-diff.sh` is not deterministic. Three consecutive runs, no
edits between them, no rebuild:

| run | verdict | case(s) |
|---|---|---|
| 1 | 580 passed, **3 differed** | `-rLv treelink dst`, `-riv tree dst [y,n]`, `-riv tree dst [y,y]` |
| 2 | 580 passed, **1 differed** | `-rHv tree dst` — a case that passed in run 1 |
| 3 | 581 passed, **0 differed** | — |

**Why it matters more than one wrong verdict.** A green run is a sample, not a
proof, and this harness is used as evidence — it certified two `cp` bug fixes
(`B-CP-COPYING-A-FILE-ONTO-ITSELF-EMPTIED-IT`,
`B-CP-R-COULD-NOT-REPLACE-AN-EXISTING-SYMLINK`). Run 3 above would have been
reported as a clean pass by anyone who ran it once. It is also how this was
found: an aggregate run reported `cp: 1 differed` and the obvious first
hypothesis — that a change of mine had broken it — was wrong.

**What differs.** Only the ORDER of the `-v` lines. In every case seen the
resulting trees, file contents, hard links and xattrs were byte-identical —
with one exception that matters: `-riv tree dst` fed `y` then `n`. There the
prompts arrive in a different order, so the `n` lands on a different file, and
the trees genuinely diverge. **A case whose answers are positional is unstable
by construction if the order is.**

**What is NOT the cause.** Neither binary is nondeterministic on its own.
Built a fresh fixture and ran `cp -rLv treelink dst` five times per side:
both produced creation order (`sub`, `a.txt`, `link`, `todir`), identically,
ten runs out of ten. So this is not our `cp` walking a directory differently
from GNU's in general.

**Where it comes from.** Each side gets its own copy of the fixture tree —
correct, because `cp` writes, and the rule is *give the subject a private copy
only when it writes*. But the two copies are two different directories, and
both programs walk a directory in inode order (the harness says so itself at
the `-rv` group, and relies on it). Two directories built by the same sequence
USUALLY allocate inodes in the same order. When they do not, the two sides
enumerate differently and the `-v` lines come out in a different order.

**Why the obvious fix is wrong.** Sorting the `-v` output before comparing
would make this green — and would destroy the property the `-rv` group exists
to test. Its comment is explicit: `tree` is created in the order `sub`,
`a.txt`, `link`, and both programs name them in that order and in neither name
order nor readdir order, *so these cases certify `read_dir_fastread`'s sort and
would go red without it*. Sorting the comparison would certify nothing and
still print `passed`. **A flaky check must not be silenced by removing the
thing it checks.**

**Proper fix, in the order to try it.**

1. Make the two copies enumerate identically rather than usually-identically.
   The fixture is built once and copied per case; if the copy is made by
   replaying the same ordered creation sequence into each side rather than by
   walking the prototype, both sides allocate in the same order by
   construction. This keeps every order-sensitive assertion intact.
2. Change `-riv tree dst [y,n]` to uniform answers. A mixed answer sequence
   asserts *which* file was skipped, which is only meaningful if the prompt
   order is pinned. Split it into two cases, all-`y` and all-`n`.
3. Only if 1 is impossible: compare the `-v` output as a set for the cases
   where order is not the property under test, and keep it ordered for the
   `-rv` group that certifies the sort. Two comparison modes in one harness is
   a cost, but it is smaller than a check that cannot be trusted.

**Five more runs, 2026-09-12 — and they refute the hypothesis above.**

| run | differed | case(s) |
|---|---|---|
| 1 | 0 | — |
| 2 | 1 | `-rfv tree dst` |
| 3 | 1 | `-rbv tree dst` |
| 4 | 4 | `-rv tree dir`, `-rLv tree dst`, `-riv [y,y,y]`, `-riv [y,n]` |
| 5 | 1 | `-rLv treelink dst` |

Eight runs in total have now produced verdicts of 0, 1, 1, 1, 3 and 4, and the
cases move between runs. **I predicted the flakes would land only on the
symlink-following and interactive cases, and run 4 includes `cp -rv tree dir`**
— a plain recursive case from the very group whose comment says those cases
"certify `read_dir_fastread`'s sort and would go red without it". So it is not
a specific traversal path: it is directory enumeration itself, and the sort
certification is measuring something that is only usually true.

That also settles the fix order. Option 3 (compare `-v` output as a set for
some cases, ordered for the `-rv` group) is now the *worst* option rather than
the fallback, because the `-rv` group is exactly where a flake was observed.
Option 1 — make the two copies enumerate identically by construction — is the
only one that repairs what the assertions claim.
